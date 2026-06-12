//! Write-side agent commands:
//! - `agent create`     → pre-transaction/createAgent + broadcast
//! - `agent update`     → pre-transaction/updateAgent + broadcast
//! - `agent activate` / `agent deactivate` → agent-status (no broadcast)
//! - `agent upload`     → pre-transaction/upload-picture (multipart)
//! - `agent feedback-submit` → pre-transaction/createComment + broadcast
//! - `agent xmtp-sign`  → pre-transaction/sign-msg (no broadcast)
//!
//! Each `*_impl` builds the request body / multipart, posts, and (for
//! broadcast-bearing commands) threads into `signing::sign_and_broadcast_agent_transaction`.
//! Shared helpers live in `utils.rs`; wire signing + Erc8004Payload assembly
//! lives in `signing.rs`.

use std::fs;
use std::time::Duration;

use anyhow::{anyhow, bail, Context as _, Result};
use serde_json::{json, Value};
use uuid::Uuid;

use crate::commands::agentic_wallet::auth::{ensure_tokens_refreshed, format_api_error};
use crate::commands::Context;
use crate::output;
use crate::wallet_api::WalletApiClient;

use super::args::{
    ActivateArgs, AgentStatusArgs, ConsentArgs, CreateArgs, FeedbackSubmitArgs, PrecheckArgs,
    UpdateArgs, UploadArgs, XmtpSignArgs,
};
use super::validate;
use super::models::{AgentCard, XLAYER_CHAIN_INDEX, XLAYER_CHAIN_INDEX_NUM};
use super::signing::{
    build_erc8004_overlay, load_agent_signing_session, load_session_cert, load_signing_seed,
    sign_and_broadcast_agent_transaction, sign_key_uuid,
};
use super::socket::{open_identity_subscription, IdentitySubscription};
use super::utils::{
    build_precheck, collect_owned_agents, ensure_provider_has_service, identity_ws_url,
    normalize_bcp47, normalize_role, normalize_singleton_object, parse_agent_unsigned,
    parse_services, parse_stars_arg, reconstruct_post_url_for_log, redact_token_for_debug,
    require_non_empty, trim_or_empty, wallet_client,
};

const PUSH_WAIT_TIMEOUT: Duration = Duration::from_secs(30);

/// Per-page size for the post-broadcast agent-list pagination loop. 100 is
/// well above any real wallet's agent count and minimizes round trips; the
/// loop relies on the response's `total` to know when to stop.
const AGENT_LIST_PAGE_SIZE: usize = 100;
/// Safety cap so a buggy `total` cannot trap us in an infinite paging loop
/// — 100 × 20 = 2 000 agents, ample headroom over real-world counts.
const AGENT_LIST_MAX_PAGES: usize = 20;

// ─── Public command entry points ──────────────────────────────────────────

pub async fn create(args: CreateArgs, ctx: &Context) -> Result<()> {
    output::success(create_impl(&args, ctx).await?);
    Ok(())
}

pub async fn precheck(args: PrecheckArgs, ctx: &Context) -> Result<()> {
    output::success(precheck_impl(&args, ctx).await?);
    Ok(())
}

pub async fn update(args: UpdateArgs, ctx: &Context) -> Result<()> {
    output::success(update_impl(&args, ctx).await?);
    Ok(())
}

pub async fn activate(args: ActivateArgs, ctx: &Context) -> Result<()> {
    output::success(activate_impl(&args, ctx).await?);
    Ok(())
}

pub async fn deactivate(args: AgentStatusArgs, ctx: &Context) -> Result<()> {
    output::success(deactivate_impl(&args, ctx).await?);
    Ok(())
}

pub async fn upload(args: UploadArgs, ctx: &Context) -> Result<()> {
    output::success(upload_impl(&args, ctx).await?);
    Ok(())
}

pub async fn feedback_submit(args: FeedbackSubmitArgs, ctx: &Context) -> Result<()> {
    output::success(feedback_submit_impl(&args, ctx).await?);
    Ok(())
}

pub async fn xmtp_sign(args: XmtpSignArgs, ctx: &Context) -> Result<()> {
    output::success(xmtp_sign_impl(&args, ctx).await?);
    Ok(())
}

// ─── `agent create` ───────────────────────────────────────────────────────

async fn create_impl(args: &CreateArgs, ctx: &Context) -> Result<Value> {
    let access_token = ensure_tokens_refreshed().await?;
    let mut client = wallet_client(ctx)?;
    // --address removed from CLI; broadcast uses the default XLayer address (current account).
    let signing_session = load_agent_signing_session(None)?;
    let from_addr = signing_session.addr_info.address.clone();
    let key_uuid = Uuid::new_v4().to_string();
    let session_signature = sign_key_uuid(&key_uuid, &signing_session.signing_seed)?;
    let normalized_role = normalize_role(require_non_empty(args.role.as_deref(), "--role")?)?;
    // Description requirements differ by role:
    //   - provider: core searchable field → required.
    //   - requester / evaluator: optional; omitted = on-chain ProfileDescription:"",
    //     rendered by the skill as "(not set)". See references/register.md.
    let profile_description = if normalized_role == "provider" {
        require_non_empty(args.description.as_deref(), "--description")?.to_string()
    } else {
        trim_or_empty(args.description.as_deref())
    };
    let card = AgentCard {
        role: normalized_role.clone(),
        name: require_non_empty(args.name.as_deref(), "--name")?.to_string(),
        profile_picture: trim_or_empty(args.picture.as_deref()),
        profile_description,
        communication_address: None,
        services: parse_services(args.service.as_deref())?,
    };
    ensure_provider_has_service(&card)?;
    let body = json!({
        "chainIndex": XLAYER_CHAIN_INDEX_NUM,
        "fromAddr": from_addr,
        "keyUuid": key_uuid.clone(),
        "sessionSignature": session_signature,
        "sessionCert": &signing_session.session_cert,
        "cardJson": serde_json::to_string(&card).context("failed to serialize cardJson")?,
    });
    eprintln!(
        "[agent-identity] create request: url={} access_token_len={} access_token_prefix={} body={}",
        reconstruct_post_url_for_log(
            ctx,
            "/priapi/v5/wallet/agentic/pre-transaction/create-agent",
        ),
        access_token.len(),
        redact_token_for_debug(&access_token),
        serde_json::to_string(&body).unwrap_or_else(|_| "<serialize failed>".to_string())
    );
    let response = client
        .post_authed(
            "/priapi/v5/wallet/agentic/pre-transaction/create-agent",
            &access_token,
            &body,
        )
        .await?;
    eprintln!(
        "[agent-identity] create response: {}",
        serde_json::to_string(&response)
            .unwrap_or_else(|_| "<serialize failed>".to_string())
    );
    let unsigned = parse_agent_unsigned(response)?;
    // erc8004Msg fields: communicationAddress comes from the pre-transaction response;
    // role / keyUuid are held by the client for this create. sessionSignature is no
    // longer included in erc8004Msg (it stays in the request body).
    let communication_address = unsigned
        .extra_data
        .get("communicationAddress")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string();
    let overlay = build_erc8004_overlay(&[
        ("communicationAddress", &communication_address),
        ("role", &normalized_role),
        ("keyUuid", &key_uuid),
    ]);

    // Open the wallet-agentic-identity subscription before broadcast; any failure
    // degrades gracefully and does not block the broadcast.
    // identity_ws_url() defaults to WS_URL_PROD; override with OKX_AGENTIC_WS_URL.
    // push-platform login uses the wallet address as the "token" value (not a JWT).
    let subscription =
        match open_identity_subscription(&from_addr, &identity_ws_url()).await {
        Ok(s) => Some(s),
        Err(e) => {
            eprintln!(
                "[agent-identity] ws subscribe failed, falling through to broadcast-only: {e:#}"
            );
            None
        }
    };

    let tx_hash = sign_and_broadcast_agent_transaction(
        &access_token,
        &unsigned,
        overlay,
        &signing_session,
    )
    .await?;

    let push = wait_for_identity_push(subscription, &tx_hash).await;
    let agent_list = fetch_agent_list(&mut client, &access_token).await;
    let new_agent_id = compute_new_agent_id(
        push.as_ref(),
        agent_list.as_ref(),
        &from_addr,
        args.known_agent_ids.as_deref(),
    );
    Ok(assemble_identity_envelope(
        tx_hash,
        push,
        agent_list,
        new_agent_id,
    ))
}

// ─── `agent consent` ──────────────────────────────────────────────────────

/// Standalone first-time-creation terms consent (legal module). Decoupled
/// from `create`: the skill calls this BEFORE collecting any identity info.
///
/// Two-step. Step 1 (no `consentKey` / `agreed`): backend issues a one-time
/// `consentKey` + `terms` for the client to display. Step 2 (`consentKey` +
/// `agreed`): backend finalizes the user's accept/decline decision.
///
/// Returning users (the address already owns an agent) or a disabled feature
/// flag get an empty `data: []`, which we normalize to `consent: null` /
/// `required: false` so the skill can skip straight to identity Q&A.
/// No signing, no broadcast — see API doc `pre-transaction/agent-consent`.
async fn consent_impl(args: &ConsentArgs, ctx: &Context) -> Result<Value> {
    let access_token = ensure_tokens_refreshed().await?;
    let mut client = wallet_client(ctx)?;
    // No `--address` flag (same as create): consent is scoped to the current
    // selected XLayer wallet. We only need its address for `fromAddr`.
    let signing_session = load_agent_signing_session(None)?;
    let from_addr = signing_session.addr_info.address.clone();

    // chainIndex must be a numeric String per the agent-consent contract.
    let mut body = json!({
        "chainIndex": XLAYER_CHAIN_INDEX,
        "fromAddr": from_addr,
    });
    if let Some(consent_key) = &args.consent_key {
        body["consentKey"] = json!(consent_key);
    }
    if let Some(agreed) = args.agreed {
        body["agreed"] = json!(agreed);
    }

    eprintln!(
        "[agent-identity] consent request: url={} access_token_len={} access_token_prefix={} body={}",
        reconstruct_post_url_for_log(
            ctx,
            "/priapi/v5/wallet/agentic/pre-transaction/agent-consent",
        ),
        access_token.len(),
        redact_token_for_debug(&access_token),
        serde_json::to_string(&body).unwrap_or_else(|_| "<serialize failed>".to_string()),
    );

    let result = client
        .post_authed(
            "/priapi/v5/wallet/agentic/pre-transaction/agent-consent",
            &access_token,
            &body,
        )
        .await;

    match &result {
        Ok(data) => eprintln!(
            "[agent-identity] consent response: {}",
            serde_json::to_string(data)
                .unwrap_or_else(|_| "<serialize failed>".to_string())
        ),
        Err(e) => eprintln!("[agent-identity] consent response err: {:#}", e),
    }

    let response = result.map_err(format_api_error)?;
    // `data` is a list; only `consent` is meaningful here. Step 1 → first item
    // carries a non-null `consent` { consentKey, terms }. Step 2 /
    // existing-agent / flag-off → empty list.
    let consent = response
        .as_array()
        .and_then(|a| a.first())
        .and_then(|item| item.get("consent"))
        .filter(|c| !c.is_null())
        .cloned();
    Ok(json!({
        "required": consent.is_some(),
        "consent": consent.unwrap_or(Value::Null),
    }))
}

// ─── `agent precheck` (unified registration entry) ─────────────────────────
//
// Folds first-time consent + per-wallet uniqueness into ONE command (see the
// registration flow diagram). `--role` is REQUIRED; `--consent-key` optional.
// Flow: fetch agent list → if the wallet HAS agents (⇒ already consented),
// return the uniqueness verdict; if it has NO agents, run the consent gate
// first. Always returns `{ canCreate, role, reason?, consent?, existingSameRole,
// providerCount, knownAgentIds }`:
//   • has agents          → build_precheck verdict (reason when false)
//   • no agents + key      → submit agreement → canCreate:true verdict
//   • no agents + agreed   → canCreate:true verdict
//   • no agents + !agreed  → { canCreate:false, role, reason, consent:{key,terms} }
// A present `--consent-key` submits the agreement (agreed=true). Decline is
// skill-side (show terms, user declines → terminate; user agrees → re-invoke
// with `--consent-key`).

/// Fetch the JWT-scoped agent list for the current wallet (chainIndex only).
/// (Distinct from the paginated post-broadcast `fetch_agent_list` below — this
/// one is a single page for the precheck uniqueness scan.)
async fn fetch_wallet_agents(ctx: &Context) -> Result<Value> {
    let access_token = ensure_tokens_refreshed().await?;
    let mut client = wallet_client(ctx)?;
    let data = client
        .get_authed(
            "/priapi/v5/wallet/agentic/agent/agent-list",
            &access_token,
            &[("chainIndex", XLAYER_CHAIN_INDEX)],
        )
        .await
        .map_err(format_api_error)?;
    Ok(normalize_singleton_object(data))
}

async fn precheck_impl(args: &PrecheckArgs, ctx: &Context) -> Result<Value> {
    // `--role` is required (same handling as `agent create`): missing → bail
    // `missing required parameter`, unrecognized → `invalid value for --role`.
    let role_key = normalize_role(require_non_empty(args.role.as_deref(), "--role")?)?;
    // Current signing wallet → scopes uniqueness to this XLayer address.
    let from_addr = load_agent_signing_session(None)?.addr_info.address;

    // Fetch the wallet's agent list to determine whether any agents exist.
    let agents = fetch_wallet_agents(ctx).await?;
    let has_agents = !collect_owned_agents(&agents, &from_addr).is_empty();

    // Consent gate — ONLY on the no-agents branch (a non-empty agent list proves
    // consent was already given). `--consent-key` present → submit the agreement;
    // absent → check consent status, returning terms (canCreate:false) if not yet
    // accepted so the skill can run the blocking legal-confirmation step.
    if !has_agents {
        if args.consent_key.is_some() {
            consent_impl(
                &ConsentArgs { consent_key: args.consent_key.clone(), agreed: Some(true) },
                ctx,
            )
            .await?; // submit consent (agreed=true)
        } else {
            let c = consent_impl(&ConsentArgs { consent_key: None, agreed: None }, ctx).await?; // query consent status
            if c.get("required").and_then(Value::as_bool).unwrap_or(false) {
                // Legal terms not yet accepted → block with terms; skill confirms with the
                // user, then re-invokes carrying `--consent-key`.
                return Ok(json!({
                    "canCreate": false,
                    "role": role_key,
                    "reason": "You must accept the legal terms before registering an Agent.",
                    "consent": c.get("consent").cloned().unwrap_or(Value::Null),
                }));
            }
        }
    }

    // Check whether an agent with the same role already exists → canCreate (build_precheck handles the empty list
    // too: a brand-new wallet yields canCreate:true; reason is added when false).
    // The verdict carries `existingSameRole` (the same-role agents the skill lists
    // for the provider update-vs-new choice) — no separate full agentList needed.
    Ok(build_precheck(&agents, &from_addr, &role_key))
}

// ─── `agent update` ───────────────────────────────────────────────────────

async fn update_impl(args: &UpdateArgs, ctx: &Context) -> Result<Value> {
    let access_token = ensure_tokens_refreshed().await?;
    let mut client = wallet_client(ctx)?;
    let agent_id = require_non_empty(args.agent_id.as_deref(), "--agent-id")?;
    let signing_session = load_agent_signing_session(None)?;

    // Per spec: update may not change role or CommunicationAddress — they are
    // excluded from cardJson. Only fields provided by the user this call are
    // written; the server decides whether to retain previous values for omitted
    // fields. The CLI does not pre-fetch /agent/agent-list for back-fill; the
    // skill guides the user to supply any needed current values.
    //
    // agentId goes inside cardJson (the backend identifies the target via
    // cardJson.AgentId), not at the request body top level. AgentId stays
    // PascalCase; name / image / services use the new lowercase schema;
    // ProfileDescription is also kept PascalCase as it was not renamed in spec.
    let mut card = serde_json::Map::new();
    card.insert("AgentId".into(), json!(agent_id));
    let name = trim_or_empty(args.name.as_deref());
    if !name.is_empty() {
        card.insert("name".into(), json!(name));
    }
    let description = trim_or_empty(args.description.as_deref());
    if !description.is_empty() {
        card.insert("ProfileDescription".into(), json!(description));
    }
    let picture = trim_or_empty(args.picture.as_deref());
    if !picture.is_empty() {
        card.insert("image".into(), json!(picture));
    }
    if args.service.is_some() {
        let services = parse_services(args.service.as_deref())?;
        card.insert(
            "services".into(),
            serde_json::to_value(&services).context("failed to serialize services list")?,
        );
    }

    let body = json!({
        "chainIndex": XLAYER_CHAIN_INDEX_NUM,
        "sessionCert": &signing_session.session_cert,
        "cardJson": serde_json::to_string(&Value::Object(card))
            .context("failed to serialize cardJson")?,
    });
    eprintln!(
        "[agent-identity] update request: url={} access_token_len={} access_token_prefix={} body={}",
        reconstruct_post_url_for_log(
            ctx,
            "/priapi/v5/wallet/agentic/pre-transaction/update-agent",
        ),
        access_token.len(),
        redact_token_for_debug(&access_token),
        serde_json::to_string(&body).unwrap_or_else(|_| "<serialize failed>".to_string()),
    );
    let update_result = client
        .post_authed(
            "/priapi/v5/wallet/agentic/pre-transaction/update-agent",
            &access_token,
            &body,
        )
        .await;
    match &update_result {
        Ok(data) => eprintln!(
            "[agent-identity] update response: {}",
            serde_json::to_string(data)
                .unwrap_or_else(|_| "<serialize failed>".to_string())
        ),
        Err(e) => eprintln!("[agent-identity] update response err: {:#}", e),
    }
    let response = update_result?;
    let unsigned = parse_agent_unsigned(response)?;
    // Per spec: update has no erc8004Msg sub-fields (communicationAddress is
    // immutable; role / keyUuid / sessionSignature are absent from the update
    // body), so erc8004Msg is omitted from the broadcast extraData entirely.

    // Open the wallet-agentic-identity subscription before broadcast; any failure
    // degrades gracefully and does not block the broadcast.
    // identity_ws_url() defaults to WS_URL_PROD; override with OKX_AGENTIC_WS_URL.
    // push-platform login uses the wallet address as the "token" value (not a JWT).
    let subscription = match open_identity_subscription(
        &signing_session.addr_info.address,
        &identity_ws_url(),
    )
    .await
    {
        Ok(s) => Some(s),
        Err(e) => {
            eprintln!(
                "[agent-identity] ws subscribe failed, falling through to broadcast-only: {e:#}"
            );
            None
        }
    };

    let tx_hash =
        sign_and_broadcast_agent_transaction(&access_token, &unsigned, None, &signing_session)
            .await?;

    let push = wait_for_identity_push(subscription, &tx_hash).await;
    let agent_list = fetch_agent_list(&mut client, &access_token).await;
    let new_agent_id = compute_new_agent_id(
        push.as_ref(),
        agent_list.as_ref(),
        &signing_session.addr_info.address,
        args.known_agent_ids.as_deref(),
    );
    Ok(assemble_identity_envelope(
        tx_hash,
        push,
        agent_list,
        new_agent_id,
    ))
}

// ─── `agent activate` / `agent deactivate` ────────────────────────────────

/// Unified activation — fully self-contained:
///   Step 0: GET agent info (role + name + description) → role guard
///   Step 1: POST agent-status (status=1)
///   Step 2: if approvalStatus ∈ {1,5} → GET service-list → validate-listing → POST submit-approval
///
/// Return-structure contract (all branches):
///   blockType:1 + reason + agentRole   → not a provider; agent-status never called
///   blockType:2 + reason + validation  → QA failed; agent-status already ran
///   activate [+ validation + submitApproval] → normal path
async fn activate_impl(args: &ActivateArgs, ctx: &Context) -> Result<Value> {
    let agent_id = require_non_empty(args.agent_id.as_deref(), "--agent-id")?;

    // ── Step 0: fetch agent info + role guard ─────────────────────────────
    let agent_info = fetch_agent_info_by_id(agent_id, ctx).await?;
    if let Some(ref info) = agent_info {
        if info.role != "provider" {
            return Ok(json!({
                "blockType": 1,
                "reason": "only provider agents can be listed; requester and evaluator roles are not supported.",
                "agentRole": info.role,
            }));
        }
    }

    // ── Step 1: agent-status (status=1) ──────────────────────────────────
    let activate_result = agent_status_impl(Some(agent_id), 1, ctx).await?;

    // ── Step 2: QA + submit — only when approvalStatus ∈ {1, 5} ──────────
    // approvalStatus 1 = initial listing QA required
    // approvalStatus 5 = re-listing QA required (treat same as 1 per manage.md)
    let needs_approval = activate_result
        .get("approvalStatus")
        .and_then(|v| v.as_u64())
        .map(|s| s == 1 || s == 5)
        .unwrap_or(false);

    if !needs_approval {
        return Ok(json!({ "activate": activate_result }));
    }

    // ── approvalStatus ∈ {1, 5}: QA then submit ──────────────────────────
    // --force skips validate-listing entirely (used after user acknowledges
    // a prior blockType:2 warning).
    if args.force {
        let submit_result = submit_approval_impl(
            Some(agent_id),
            args.preferred_language.as_deref(),
            ctx,
        )
        .await?;
        return Ok(json!({
            "activate": activate_result,
            "submitApproval": submit_result,
        }));
    }

    // Normal path: fetch services, run validate-listing (pure local).
    let raw_services = fetch_raw_services(agent_id, ctx).await?;
    let service_objs: Vec<Value> = raw_services.iter().map(service_item_to_validate_obj).collect();
    let service_json = if service_objs.is_empty() {
        None
    } else {
        serde_json::to_string(&service_objs).ok()
    };

    let (name_str, desc_str) = match &agent_info {
        Some(info) => (info.name.clone(), info.description.clone()),
        None => (String::new(), String::new()),
    };

    let validation_result = validate::run_validation(
        "provider",
        if name_str.is_empty() { None } else { Some(name_str.as_str()) },
        if desc_str.is_empty() { None } else { Some(desc_str.as_str()) },
        service_json.as_deref(),
    );
    let validation_value = serde_json::to_value(&validation_result)?;

    if !validation_result.pass {
        return Ok(json!({
            "blockType": 2,
            "reason": "listing validation failed — fix findings before activating",
            "validation": validation_value,
        }));
    }

    let submit_result = submit_approval_impl(
        Some(agent_id),
        args.preferred_language.as_deref(),
        ctx,
    )
    .await?;

    Ok(json!({
        "activate": activate_result,
        "validation": validation_value,
        "submitApproval": submit_result,
    }))
}

// ─── Activate helpers ─────────────────────────────────────────────────────────

struct AgentInfo {
    role: String,
    name: String,
    description: String,
}

/// Fetch role + name + description for a single agent.
/// Returns `None` when the agent is not found or the response shape is unexpected.
/// Network / auth failures propagate as `Err`.
async fn fetch_agent_info_by_id(agent_id: &str, ctx: &Context) -> Result<Option<AgentInfo>> {
    let access_token = ensure_tokens_refreshed().await?;
    let mut client = wallet_client(ctx)?;

    eprintln!("[agent-identity] activate info-fetch: agent-id={}", agent_id);

    let raw = client
        .get_authed(
            "/priapi/v5/wallet/agentic/agent/agent-list",
            &access_token,
            &[
                ("chainIndex", XLAYER_CHAIN_INDEX),
                ("agentIdList", agent_id),
            ],
        )
        .await
        .map_err(format_api_error)?;

    eprintln!(
        "[agent-identity] activate info-fetch response: {}",
        serde_json::to_string(&raw).unwrap_or_else(|_| "<serialize failed>".to_string())
    );

    let normalized = normalize_singleton_object(raw);
    let items = match normalized.get("list").and_then(Value::as_array) {
        Some(a) => a.clone(),
        None => return Ok(None),
    };

    for item in &items {
        if let Some(info) = parse_agent_info_row(item) {
            return Ok(Some(info));
        }
        if let Some(rows) = item.get("agentList").and_then(Value::as_array) {
            for row in rows {
                if let Some(info) = parse_agent_info_row(row) {
                    return Ok(Some(info));
                }
            }
        }
    }
    Ok(None)
}

/// Extract `AgentInfo` from a single agent row. Returns `None` when the role
/// field is absent or unrecognized.
fn parse_agent_info_row(row: &Value) -> Option<AgentInfo> {
    let raw_role = match row.get("role")? {
        Value::String(s) if !s.trim().is_empty() => s.trim().to_string(),
        Value::Number(n) => n.to_string(),
        _ => return None,
    };
    let role = normalize_role(&raw_role).ok()?;

    let name = row
        .get("name")
        .and_then(Value::as_str)
        .map(str::trim)
        .unwrap_or("")
        .to_string();

    let description = ["description", "profileDescription"]
        .iter()
        .find_map(|k| row.get(*k).and_then(Value::as_str).map(str::trim))
        .unwrap_or("")
        .to_string();

    Some(AgentInfo { role, name, description })
}

/// Fetch the raw service items for an agent from GET /agent/services.
/// Tolerates both response shapes:
///   • live backend: array of `{ agentInfo, list:[service…] }` wrappers
///   • legacy: single object with a flat `services` array
async fn fetch_raw_services(agent_id: &str, ctx: &Context) -> Result<Vec<Value>> {
    let access_token = ensure_tokens_refreshed().await?;
    let mut client = wallet_client(ctx)?;

    eprintln!(
        "[agent-identity] activate service-fetch: agent-id={}",
        agent_id
    );

    let raw = client
        .get_authed(
            "/priapi/v5/wallet/agentic/agent/services",
            &access_token,
            &[("agentId", agent_id)],
        )
        .await
        .map_err(format_api_error)?;

    eprintln!(
        "[agent-identity] activate service-fetch response: {}",
        serde_json::to_string(&raw).unwrap_or_else(|_| "<serialize failed>".to_string())
    );

    let mut services: Vec<Value> = Vec::new();
    if let Some(wrappers) = raw.as_array() {
        // Live backend shape: array of wrappers each carrying list:[…]
        for wrapper in wrappers {
            if let Some(items) = wrapper.get("list").and_then(Value::as_array) {
                services.extend(items.iter().cloned());
            }
        }
    } else if let Some(items) = raw.get("services").and_then(Value::as_array) {
        // Legacy shape: { services:[…] }
        services.extend(items.iter().cloned());
    }
    Ok(services)
}

/// Convert a raw service-list item to the JSON object shape that
/// `validate::parse_services_lenient` (and `AgentService` serde) expects:
/// `{ "name", "servicedescription", "servicetype", "fee", "endpoint"? }`.
fn service_item_to_validate_obj(svc: &Value) -> Value {
    let get = |keys: &[&str]| -> String {
        for key in keys {
            if let Some(s) = svc.get(*key).and_then(Value::as_str) {
                let t = s.trim();
                if !t.is_empty() {
                    return t.to_string();
                }
            }
        }
        String::new()
    };

    let name = get(&["serviceName", "ServiceName", "name"]);
    let desc = get(&["serviceDescription", "ServiceDescription", "servicedescription"]);
    let stype = get(&["serviceType", "ServiceType", "servicetype"]);
    let fee = get(&["fee", "Fee"]);
    let endpoint = get(&["endpoint", "Endpoint"]);

    let mut obj = json!({
        "name": name,
        "servicedescription": desc,
        "servicetype": stype,
        "fee": fee,
    });
    if !endpoint.is_empty() {
        obj["endpoint"] = json!(endpoint);
    }
    obj
}

async fn deactivate_impl(args: &AgentStatusArgs, ctx: &Context) -> Result<Value> {
    agent_status_impl(args.agent_id.as_deref(), 2, ctx).await
}

async fn agent_status_impl(agent_id_opt: Option<&str>, status: u32, ctx: &Context) -> Result<Value> {
    let access_token = ensure_tokens_refreshed().await?;
    let mut client = wallet_client(ctx)?;
    let agent_id = require_non_empty(agent_id_opt, "--agent-id")?;
    let body = json!({
        "agentId": agent_id,
        "chainIndex": XLAYER_CHAIN_INDEX,
        "status": status,
    });

    eprintln!(
        "[agent-identity] agent-status request: url={} access_token_len={} access_token_prefix={} body={}",
        reconstruct_post_url_for_log(ctx, "/priapi/v5/wallet/agentic/agent-status"),
        access_token.len(),
        redact_token_for_debug(&access_token),
        serde_json::to_string(&body).unwrap_or_else(|_| "<serialize failed>".to_string()),
    );

    let result = client
        .post_authed(
            "/priapi/v5/wallet/agentic/agent-status",
            &access_token,
            &body,
        )
        .await;

    match &result {
        Ok(data) => eprintln!(
            "[agent-identity] agent-status response: {}",
            serde_json::to_string(data)
                .unwrap_or_else(|_| "<serialize failed>".to_string())
        ),
        Err(e) => eprintln!("[agent-identity] agent-status response err: {:#}", e),
    }

    result.map_err(format_api_error)
}

async fn submit_approval_impl(
    agent_id_opt: Option<&str>,
    preferred_language_opt: Option<&str>,
    ctx: &Context,
) -> Result<Value> {
    let access_token = ensure_tokens_refreshed().await?;
    let mut client = wallet_client(ctx)?;
    let agent_id = require_non_empty(agent_id_opt, "--agent-id")?;
    let mut body = json!({
        "agentId": agent_id,
        "chainIndex": XLAYER_CHAIN_INDEX,
    });
    if let Some(lang) = normalize_bcp47(preferred_language_opt) {
        body["preferredLanguage"] = Value::String(lang);
    }

    eprintln!(
        "[agent-identity] submit-approval request: url={} access_token_len={} access_token_prefix={} body={}",
        reconstruct_post_url_for_log(ctx, "/priapi/v5/wallet/agentic/agent/submit-approval"),
        access_token.len(),
        redact_token_for_debug(&access_token),
        serde_json::to_string(&body).unwrap_or_else(|_| "<serialize failed>".to_string()),
    );

    let result = client
        .post_authed(
            "/priapi/v5/wallet/agentic/agent/submit-approval",
            &access_token,
            &body,
        )
        .await;

    match &result {
        Ok(data) => eprintln!(
            "[agent-identity] submit-approval response: {}",
            serde_json::to_string(data)
                .unwrap_or_else(|_| "<serialize failed>".to_string())
        ),
        Err(e) => eprintln!("[agent-identity] submit-approval response err: {:#}", e),
    }

    result.map_err(format_api_error)
}

// ─── `agent upload` ───────────────────────────────────────────────────────

const MAX_UPLOAD_BYTES: usize = 1024 * 1024; // 1 MB

async fn upload_impl(args: &UploadArgs, ctx: &Context) -> Result<Value> {
    let access_token = ensure_tokens_refreshed().await?;
    let client = wallet_client(ctx)?;
    let file = require_non_empty(args.file.as_deref(), "--file")?;
    let bytes = fs::read(file).with_context(|| format!("failed to read file: {file}"))?;
    if bytes.len() > MAX_UPLOAD_BYTES {
        bail!(
            "file size {} bytes exceeds the 1 MB limit — please downscale the image and retry",
            bytes.len()
        );
    }
    let filename = std::path::Path::new(file)
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("upload.bin")
        .to_string();
    let upload_url = reconstruct_post_url_for_log(
        ctx,
        "/priapi/v5/wallet/agentic/pre-transaction/upload-picture",
    );
    eprintln!(
        "[agent-identity] upload request: url={} access_token_len={} access_token_prefix={} file_path={} filename={} bytes_len={}",
        upload_url,
        access_token.len(),
        redact_token_for_debug(&access_token),
        file,
        filename,
        bytes.len(),
    );
    let part = reqwest::multipart::Part::bytes(bytes).file_name(filename);
    let form = reqwest::multipart::Form::new().part("file", part);
    let result = client
        .post_authed_multipart(
            "/priapi/v5/wallet/agentic/pre-transaction/upload-picture",
            &access_token,
            form,
        )
        .await;

    match &result {
        Ok(data) => eprintln!(
            "[agent-identity] upload response: {}",
            serde_json::to_string(data)
                .unwrap_or_else(|_| "<serialize failed>".to_string())
        ),
        Err(e) => eprintln!("[agent-identity] upload response err: {:#}", e),
    }

    let data = result?;

    let url = if let Some(url) = data.get("url").and_then(Value::as_str) {
        url.to_string()
    } else if let Some(first) = data.as_array().and_then(|arr| arr.first()) {
        if let Some(url) = first.get("url").and_then(Value::as_str) {
            url.to_string()
        } else if let Some(url) = first.as_str() {
            url.to_string()
        } else {
            bail!("upload response missing url");
        }
    } else {
        bail!("upload response missing url");
    };
    Ok(json!({ "url": url }))
}

// ─── `agent feedback-submit` ──────────────────────────────────────────────

async fn feedback_submit_impl(args: &FeedbackSubmitArgs, ctx: &Context) -> Result<Value> {
    let access_token = ensure_tokens_refreshed().await?;
    let mut client = wallet_client(ctx)?;
    // --agent-id / --creator-id / --score required; --description / --task-id optional.
    let agent_id = require_non_empty(args.agent_id.as_deref(), "--agent-id")?.to_string();
    let creator_id = require_non_empty(args.creator_id.as_deref(), "--creator-id")?.to_string();
    // --score is 0.00–5.00 stars (up to 2 decimal places, step 0.01); the
    // CLI multiplies by 20 with round-half-up to produce the 0–100 u32
    // wire value. Mapping + format validation live in
    // `utils::parse_stars_arg` (single source of truth). Earlier revisions
    // made stars→score the skill's responsibility, which was fragile —
    // skills are prompt-driven and would occasionally forget the
    // multiplication and corrupt the rating on chain. CLI is the
    // authoritative boundary now.
    let score = parse_stars_arg(
        require_non_empty(args.score.as_deref(), "--score")?,
        "--score",
    )?;
    let feedback_desc = trim_or_empty(args.description.as_deref());
    let task_id = trim_or_empty(args.task_id.as_deref());
    let signing_session = load_agent_signing_session(None)?;

    // Request body: create-comment requires chainIndex + sessionCert +
    // feedBackAgentId + comment (fromAddr no longer included). feedBackAgentId
    // is the reviewer's agent id — same source as extraData.erc8004Msg.feedBackAgentId
    // (both from --creator-id) but placed at the top level of the body, not inside
    // the comment sub-object. Note: the "comment" field inside the comment sub-object
    // is the feedback text (feedbackDesc), sharing its name with the outer body.comment
    // (a serialized JSON string) but at a different nesting level.
    let comment = json!({
        "agentid": agent_id,
        "value": score.to_string(),
        "comment": feedback_desc,
    });
    let body = json!({
        "chainIndex": XLAYER_CHAIN_INDEX_NUM,
        "sessionCert": &signing_session.session_cert,
        "feedBackAgentId": creator_id,
        "comment": serde_json::to_string(&comment).context("failed to serialize comment")?,
    });

    eprintln!(
        "[agent-identity] feedback-submit request: url={} access_token_len={} access_token_prefix={} body={}",
        reconstruct_post_url_for_log(
            ctx,
            "/priapi/v5/wallet/agentic/pre-transaction/create-comment",
        ),
        access_token.len(),
        redact_token_for_debug(&access_token),
        serde_json::to_string(&body).unwrap_or_else(|_| "<serialize failed>".to_string()),
    );

    let result = client
        .post_authed(
            "/priapi/v5/wallet/agentic/pre-transaction/create-comment",
            &access_token,
            &body,
        )
        .await;

    match &result {
        Ok(data) => eprintln!(
            "[agent-identity] feedback-submit response: {}",
            serde_json::to_string(data)
                .unwrap_or_else(|_| "<serialize failed>".to_string())
        ),
        Err(e) => eprintln!("[agent-identity] feedback-submit response err: {:#}", e),
    }

    let response = result?;
    let unsigned = parse_agent_unsigned(response)?;
    // erc8004Msg: feedBackAgentId is required (from --creator-id); taskId is optional.
    // Empty values are filtered by build_erc8004_overlay, so an empty taskId is omitted.
    let overlay = build_erc8004_overlay(&[
        ("taskId", &task_id),
        ("feedBackAgentId", &creator_id),
    ]);
    // --address removed from CLI; broadcast uses the default XLayer address (current account).
    let tx_hash = sign_and_broadcast_agent_transaction(
        &access_token,
        &unsigned,
        overlay,
        &signing_session,
    )
    .await?;
    Ok(json!({ "txHash": tx_hash }))
}

// ─── `agent xmtp-sign` ────────────────────────────────────────────────────

/// `onchainos agent xmtp-sign`: sign keyUuid on the fly with the local signing_seed,
/// then POST message + sessionCert to the backend sign-msg endpoint and return the
/// resulting signature. No broadcast.
async fn xmtp_sign_impl(args: &XmtpSignArgs, ctx: &Context) -> Result<Value> {
    let access_token = ensure_tokens_refreshed().await?;
    let mut client = wallet_client(ctx)?;

    let key_uuid = require_non_empty(args.key_uuid.as_deref(), "--key-uuid")?;
    let message = require_non_empty(args.message.as_deref(), "--message")?;

    let signing_seed = load_signing_seed()?;
    let session_cert = load_session_cert()?;
    // Same signing algorithm as create / update's erc8004Msg: Ed25519 over the
    // raw UTF-8 bytes of keyUuid — see sign_key_uuid.
    let session_signature = sign_key_uuid(key_uuid, &signing_seed)?;

    let body = json!({
        "chainIndex": XLAYER_CHAIN_INDEX,
        "sessionCert": session_cert,
        "sessionSignature": session_signature,
        "signType": "aiagentsign",
        "keyUuid": key_uuid,
        "message": message,
    });

    eprintln!(
        "[agent-identity] xmtp-sign request: url={} access_token_len={} access_token_prefix={} body={}",
        reconstruct_post_url_for_log(
            ctx,
            "/priapi/v5/wallet/agentic/pre-transaction/sign-msg",
        ),
        access_token.len(),
        redact_token_for_debug(&access_token),
        serde_json::to_string(&body).unwrap_or_else(|_| "<serialize failed>".to_string()),
    );

    let result = client
        .post_authed(
            "/priapi/v5/wallet/agentic/pre-transaction/sign-msg",
            &access_token,
            &body,
        )
        .await;

    match &result {
        Ok(data) => eprintln!(
            "[agent-identity] xmtp-sign response: {}",
            serde_json::to_string(data)
                .unwrap_or_else(|_| "<serialize failed>".to_string())
        ),
        Err(e) => eprintln!("[agent-identity] xmtp-sign response err: {:#}", e),
    }

    let data = result.map_err(format_api_error)?;
    let first = data
        .as_array()
        .and_then(|arr| arr.first())
        .cloned()
        .ok_or_else(|| anyhow!("xmtp-sign response is empty"))?;
    if first
        .get("signature")
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
        .is_none()
    {
        bail!("xmtp-sign response missing signature");
    }
    Ok(first)
}

// ─── Post-broadcast finalize helpers (create / update) ────────────────────

/// Drain the WS subscription waiting for the matching push. Any failure
/// (including timeout) collapses to `None`; the surrounding command then
/// emits `txHash` + `agentList` only.
async fn wait_for_identity_push(
    subscription: Option<IdentitySubscription>,
    tx_hash: &str,
) -> Option<Value> {
    let sub = subscription?;
    match sub.wait_for_match(tx_hash, PUSH_WAIT_TIMEOUT).await {
        Ok(opt) => opt,
        Err(e) => {
            eprintln!("[agent-identity] ws wait failed: {e:#}");
            None
        }
    }
}

/// Best-effort fetch of the caller's full XLayer agent list (struct C).
/// Pages through `/agent/agent-list?chainIndex=196` with `pageSize=100`
/// until `total` is satisfied or `AGENT_LIST_MAX_PAGES` is hit. Returns
/// `{ total, list: [...] }`. Note the backend's actual response shape
/// uses the field name `list` (not `items` — earlier docs claimed
/// `items`; that was a pre-existing doc bug confirmed empirically on
/// 2026-05-10).
///
/// Failure modes that short-circuit to `None` so the envelope omits the
/// field rather than emit a misleading partial:
///   - any HTTP error from the client during pagination
///   - page 1 missing or non-numeric `total` field (response shape
///     anomaly — cannot trust the dataset)
///   - any page missing or non-array `list` field (same)
///
/// An empty `list` on any page stops pagination gracefully and returns
/// the aggregated results so far (does not abort to `None`). The
/// legitimate empty case (`total == 0` and page 1 returned `list: []`)
/// returns `Some({total: 0, list: []})`.
async fn fetch_agent_list(client: &mut WalletApiClient, access_token: &str) -> Option<Value> {
    let mut all_items: Vec<Value> = Vec::new();
    let mut total: u64 = 0;
    let mut all_agent_count: u64 = 0;
    let mut page: usize = 1;

    while page <= AGENT_LIST_MAX_PAGES {
        let page_str = page.to_string();
        let page_size_str = AGENT_LIST_PAGE_SIZE.to_string();
        let raw = match client
            .get_authed(
                "/priapi/v5/wallet/agentic/agent/agent-list",
                access_token,
                &[
                    ("chainIndex", XLAYER_CHAIN_INDEX),
                    ("page", &page_str),
                    ("pageSize", &page_size_str),
                ],
            )
            .await
        {
            Ok(v) => v,
            Err(e) => {
                eprintln!("[agent-identity] agent-list page {page} fetch failed: {e:#}");
                return None;
            }
        };

        // Keep the raw response for debug logging on shape-mismatch aborts.
        // Without this, the abort logs only say "shape wrong" with no clue
        // what the backend actually returned — see the post-mortem on the
        // 2026-05-10 test where the abort fired but the actual response shape
        // was unknown.
        let raw_repr = raw.to_string();
        let normalized = normalize_singleton_object(raw);

        // Page 1: total must be present and numeric. Missing/wrong-shape => abort.
        if page == 1 {
            total = match normalized.get("total").and_then(Value::as_u64) {
                Some(t) => t,
                None => {
                    eprintln!(
                        "[agent-identity] agent-list page 1 missing or non-numeric `total` field — abort. raw={raw_repr} normalized={normalized}"
                    );
                    return None;
                }
            };
        }

        // `list` must be an array on every page. Missing/wrong-shape => abort.
        // Backend uses the field name `list` (not `items`); see the doc
        // comment above this function.
        let page_items: Vec<Value> = match normalized.get("list").and_then(Value::as_array) {
            Some(arr) => arr.clone(),
            None => {
                eprintln!(
                    "[agent-identity] agent-list page {page} missing or non-array `list` field — abort. raw={raw_repr} normalized={normalized}"
                );
                return None;
            }
        };
        let page_count = page_items.len();

        // Count agents in this page: each group carries agentList[].
        let page_agent_count: u64 = page_items
            .iter()
            .filter_map(|item| item.get("agentList").and_then(Value::as_array))
            .map(|a| a.len() as u64)
            .sum();
        all_agent_count += page_agent_count;
        all_items.extend(page_items);

        // Done: accumulated agent count satisfies backend's reported total.
        // Also covers the empty case: total == 0, page 1 list == [], 0 >= 0.
        if all_agent_count >= total {
            break;
        }

        // list == [] means the backend has no more data; stop paging.
        // total may be stale — treat an empty page as end-of-data rather
        // than an error so we don't keep incrementing page indefinitely.
        if page_count == 0 {
            eprintln!(
                "[agent-identity] agent-list page {page} returned empty list (total={} agents_accumulated={}) — stopping",
                total,
                all_agent_count,
            );
            break;
        }

        page += 1;
        if page > AGENT_LIST_MAX_PAGES {
            eprintln!(
                "[agent-identity] agent-list paging hit safety cap of {} pages × {} ({} agents accumulated, backend total={})",
                AGENT_LIST_MAX_PAGES,
                AGENT_LIST_PAGE_SIZE,
                all_agent_count,
                total,
            );
        }
    }

    Some(json!({
        "total": total,
        "list": all_items,
    }))
}

/// Assemble the `{ txHash, agent?, agentList?, newAgentId }` envelope.
/// `agent` and `agentList` are independent best-effort segments; either may
/// be missing without affecting the other. `newAgentId` is always present as
/// a top-level key (string id when resolvable, JSON `null` otherwise) so
/// callers can rely on the key existing.
fn assemble_identity_envelope(
    tx_hash: String,
    push: Option<Value>,
    agent_list: Option<Value>,
    new_agent_id: Option<String>,
) -> Value {
    let mut out = json!({ "txHash": tx_hash });
    if let Some(p) = push {
        out["agent"] = p;
    }
    if let Some(list) = agent_list {
        out["agentList"] = list;
    }
    out["newAgentId"] = match new_agent_id {
        Some(id) => Value::String(id),
        None => Value::Null,
    };
    out
}

/// Compute the top-level `newAgentId` for a create / update response.
///
/// Resolution order (additive, never errors):
///   1. WS push present with an `agentId` → use it (stringified). This is the
///      authoritative signal and ignores `--known-agent-ids` entirely.
///   2. WS push absent, `agentList` present, AND `known_agent_ids` provided →
///      double-layer diff: find the wrapper whose `ownerAddress` matches the
///      signing wallet, then within that wrapper's nested `agentList[*]` pick
///      the single agentId NOT in the known set. Exactly one new id → use it;
///      zero / more-than-one / no matching wrapper → `None`.
///   3. Otherwise → `None`.
fn compute_new_agent_id(
    push: Option<&Value>,
    agent_list: Option<&Value>,
    signing_address: &str,
    known_agent_ids: Option<&str>,
) -> Option<String> {
    // Rule 1: WS push wins.
    if let Some(p) = push {
        if let Some(id) = agent_id_to_string(p.get("agentId")) {
            return Some(id);
        }
    }

    // Rule 2: diff against the pre-write snapshot.
    let known_csv = known_agent_ids?;
    let agent_list = agent_list?;
    let known: std::collections::HashSet<String> = parse_known_agent_ids(known_csv);

    diff_new_agent_id(agent_list, signing_address, &known)
}

/// Parse the `--known-agent-ids` CSV into a set of normalized id strings.
/// Whitespace-trimmed, empty entries dropped. Ids are compared as strings
/// (after normalization) so `42` and `"42"` collide regardless of JSON type.
fn parse_known_agent_ids(csv: &str) -> std::collections::HashSet<String> {
    csv.split(',')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
        .collect()
}

/// Normalize an agentId `Value` (string or integer) to its canonical string
/// form for set comparison. `None` for null / missing / unsupported types.
fn agent_id_to_string(value: Option<&Value>) -> Option<String> {
    match value? {
        Value::String(s) if !s.trim().is_empty() => Some(s.trim().to_string()),
        Value::Number(n) => Some(n.to_string()),
        _ => None,
    }
}

/// Diff the post-broadcast agent list against the pre-write `known` snapshot
/// to find the single newly-minted agent id for the signing wallet. Tolerates
/// BOTH envelope shapes the `/agent-list` endpoint has used:
///   • single-layer  `list[*]`              — the live backend today: each row
///     IS an agent and carries its own `ownerAddress`.
///   • double-layer   `list[*].agentList[*]` — older grouped schema: `list[*]`
///     is an accountName wrapper carrying `ownerAddress` + nested `agentList`.
/// Only rows whose owning wallet matches `signing_address` are considered (a
/// row/wrapper with no `ownerAddress` is treated as the caller's own, since the
/// list endpoint is JWT-scoped to the caller). Returns `Some(id)` only when
/// EXACTLY ONE matching agent id is absent from `known`.
fn diff_new_agent_id(
    agent_list: &Value,
    signing_address: &str,
    known: &std::collections::HashSet<String>,
) -> Option<String> {
    let items = agent_list.get("list").and_then(Value::as_array)?;
    let signing_lower = signing_address.trim().to_ascii_lowercase();

    // A row/wrapper belongs to the signing wallet when its `ownerAddress`
    // matches (case-insensitive). Missing `ownerAddress` → treat as owned
    // (the endpoint is JWT-scoped), never as a disqualifier.
    let owner_matches = |node: &Value| -> bool {
        match node.get("ownerAddress").and_then(Value::as_str) {
            Some(addr) => addr.trim().to_ascii_lowercase() == signing_lower,
            None => true,
        }
    };

    let mut candidates: Vec<String> = Vec::new();
    for item in items {
        match item.get("agentList").and_then(Value::as_array) {
            // Double-layer: `item` is an accountName wrapper; diff its rows
            // only when the wrapper belongs to the signing wallet.
            Some(rows) if owner_matches(item) => {
                candidates.extend(rows.iter().filter_map(|r| agent_id_to_string(r.get("agentId"))));
            }
            Some(_) => {}
            // Single-layer: `item` IS the agent row, carrying its ownerAddress.
            None => {
                if owner_matches(item) {
                    if let Some(id) = agent_id_to_string(item.get("agentId")) {
                        candidates.push(id);
                    }
                }
            }
        }
    }

    candidates.retain(|id| !known.contains(id));
    candidates.sort();
    candidates.dedup();

    if candidates.len() == 1 {
        candidates.pop()
    } else {
        None
    }
}

#[cfg(test)]
#[path = "tests/mutations_tests.rs"]
mod tests;
