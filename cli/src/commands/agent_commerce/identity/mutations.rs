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

use super::args::{
    ActivateArgs, AgentStatusArgs, ConsentArgs, CreateArgs, FeedbackSubmitArgs, PrecheckArgs,
    UpdateArgs, UploadArgs, XmtpSignArgs,
};
use super::models::{AgentCard, XLAYER_CHAIN_INDEX, XLAYER_CHAIN_INDEX_NUM};
use super::signing::{
    build_erc8004_overlay, load_agent_signing_session, load_session_cert, load_signing_seed,
    sign_and_broadcast_agent_transaction, sign_key_uuid,
};
use super::socket::{open_identity_subscription, IdentitySubscription};
use super::utils::{
    build_precheck, collect_owned_agents, ensure_provider_has_service, identity_ws_url,
    normalize_bcp47, normalize_role, normalize_role_code, normalize_singleton_object,
    parse_agent_unsigned, parse_services, parse_stars_arg, reconstruct_post_url_for_log,
    redact_token_for_debug, require_non_empty, trim_or_empty, wallet_client,
};

const PUSH_WAIT_TIMEOUT: Duration = Duration::from_secs(30);


// ─── Public command entry points ──────────────────────────────────────────

fn scrub_body_for_log(body: &serde_json::Value) -> serde_json::Value {
    let mut b = body.clone();
    if let Some(obj) = b.as_object_mut() {
        for k in &["sessionCert", "sessionSignature"] {
            if obj.contains_key(*k) {
                obj.insert((*k).to_string(), serde_json::json!("<redacted>"));
            }
        }
    }
    b
}

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
    debug_log!(
        "[agent-identity] create request: url={} access_token_len={} access_token_prefix={} body={}",
        reconstruct_post_url_for_log(
            ctx,
            "/priapi/v5/wallet/agentic/pre-transaction/create-agent",
        ),
        access_token.len(),
        redact_token_for_debug(&access_token),
        serde_json::to_string(&scrub_body_for_log(&body)).unwrap_or_else(|_| "<serialize failed>".to_string())
    );
    let response = client
        .post_authed(
            "/priapi/v5/wallet/agentic/pre-transaction/create-agent",
            &access_token,
            &body,
        )
        .await?;
    debug_log!(
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
            debug_log!(
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
    let new_agent_id = extract_agent_id_from_push(push.as_ref());
    Ok(assemble_identity_envelope(tx_hash, push, new_agent_id))
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

    debug_log!(
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
        Ok(data) => debug_log!(
            "[agent-identity] consent response: {}",
            serde_json::to_string(data)
                .unwrap_or_else(|_| "<serialize failed>".to_string())
        ),
        Err(e) => debug_log!("[agent-identity] consent response err: {:#}", e),
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
// Four-step flow:
//   1. Parse `--role` (+ wallet addr). If `--consent-key` is present, submit the
//      agreement to /agent-consent up-front, then continue.
//   2. Fetch the FULL agent list (no role filter) → does the wallet have ANY agent?
//      a. No agent → consent gate: query /agent-consent status.
//           i.  required:true  → { canCreate:false, role, reason, consent } (block)
//           ii. required:false → canCreate:true (zero agents ⇒ no same-role agent)
//      b. Has agents (⇒ already consented) → step 3.
//   3. Fetch the role-scoped agent list (`role` pushed to the backend).
//      a. same-role agent present → step 4.
//      b. absent → canCreate:true.
//   4. Uniqueness rule (build_precheck): requester/evaluator are single per
//      address, provider is unlimited → final canCreate verdict.
// Always returns `{ canCreate, role, reason?, consent?, existingSameRole,
// providerCount }`. Decline is skill-side (show terms, user declines → terminate;
// user agrees → re-invoke with `--consent-key`).

/// Fetch the agent list for the precheck uniqueness scan. The `agent-list`
/// endpoint is JWT-scoped to the current wallet, so no `ownerAddress` is sent;
/// `role` (as the backend integer code) is pushed so the backend returns the
/// role-scoped slice rather than the full list (mirrors get-my-agents' role filter).
async fn fetch_wallet_agents(ctx: &Context, role: Option<&str>) -> Result<Value> {
    let access_token = ensure_tokens_refreshed().await?;
    let mut client = wallet_client(ctx)?;

    let mut query = vec![("chainIndex".to_string(), XLAYER_CHAIN_INDEX.to_string())];
    if let Some(role_raw) = role.filter(|r| !r.trim().is_empty()) {
        query.push(("role".to_string(), normalize_role_code(role_raw)?));
    }

    let query_refs: Vec<(&str, &str)> =
        query.iter().map(|(k, v)| (k.as_str(), v.as_str())).collect();

    let data = client
        .get_authed(
            "/priapi/v5/wallet/agentic/agent/agent-list",
            &access_token,
            &query_refs,
        )
        .await
        .map_err(format_api_error)?;
    Ok(normalize_singleton_object(data))
}

async fn precheck_impl(args: &PrecheckArgs, ctx: &Context) -> Result<Value> {
    // ── Step 1 — parse inputs ──
    // `--role` is required (same handling as `agent create`): missing → bail
    // `missing required parameter`, unrecognized → `invalid value for --role`.
    let role_key = normalize_role(require_non_empty(args.role.as_deref(), "--role")?)?;
    // Current signing wallet → scopes the scan to this XLayer address (the
    // agent-list endpoint is JWT-scoped, so the wallet is implicit).
    let from_addr = load_agent_signing_session(None)?.addr_info.address;

    // Step 1a — a present `--consent-key` means "the user agreed": submit the
    // agreement (agreed=true) up-front, then continue. Absent → straight to step 2.
    if args.consent_key.as_deref().map(|k| !k.trim().is_empty()).unwrap_or(false) {
        consent_impl(
            &ConsentArgs { consent_key: args.consent_key.clone(), agreed: Some(true) },
            ctx,
        )
        .await?;
    }

    // ── Step 2 — does the wallet have ANY agent? ──
    // Fetch the FULL agent list (no role filter). A non-empty list proves consent
    // was already given.
    let all_agents = fetch_wallet_agents(ctx, None).await?;
    let has_any_agent = !collect_owned_agents(&all_agents, &from_addr).is_empty();

    // Step 2a — no agent at all → consent gate. Query consent status; if the terms
    // are not yet accepted, block with them so the skill runs the legal-confirm
    // step. Otherwise (already consented) the wallet has zero agents, hence no
    // same-role agent → can register directly.
    if !has_any_agent {
        let c = consent_impl(&ConsentArgs { consent_key: None, agreed: None }, ctx).await?;
        if c.get("required").and_then(Value::as_bool).unwrap_or(false) {
            // Step 2a.i — legal terms not yet accepted → block with terms; skill
            // confirms with the user, then re-invokes carrying `--consent-key`.
            return Ok(json!({
                "canCreate": false,
                "role": role_key,
                "reason": "You must accept the legal terms before registering an Agent.",
                "consent": c.get("consent").cloned().unwrap_or(Value::Null),
            }));
        }
        // Step 2a.ii — consent satisfied + zero agents → canCreate:true (the empty
        // list yields no same-role agent).
        return Ok(build_precheck(&all_agents, &from_addr, &role_key));
    }

    // ── Step 2b → Step 3 — wallet has agents (⇒ already consented) ──
    // Fetch the role-scoped slice (`role` pushed to the backend) and let
    // build_precheck (Step 4) apply the per-wallet uniqueness rule:
    // requester/evaluator are single, provider is unlimited. An empty slice (no
    // same-role agent) yields canCreate:true; `existingSameRole` carries the
    // same-role agents the skill lists for the provider update-vs-new choice.
    let role_agents = fetch_wallet_agents(ctx, Some(&role_key)).await?;
    Ok(build_precheck(&role_agents, &from_addr, &role_key))
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
    // cardJson.agentId), not at the request body top level. All cardJson keys
    // use the unified lowercase / camelCase schema (agentId / name / image /
    // profileDescription / services) shared with the services-list response.
    let mut card = serde_json::Map::new();
    card.insert("agentId".into(), json!(agent_id));
    let name = trim_or_empty(args.name.as_deref());
    if !name.is_empty() {
        card.insert("name".into(), json!(name));
    }
    let description = trim_or_empty(args.description.as_deref());
    if !description.is_empty() {
        card.insert("profileDescription".into(), json!(description));
    }
    let picture = trim_or_empty(args.picture.as_deref());
    if !picture.is_empty() {
        card.insert("image".into(), json!(picture));
    }
    // Service: the update payload is a DELTA, not a full snapshot. `services`
    // carries ONLY the entries to change, each tagged with an `operation`:
    //   • `create` — a brand-new service, NO `id` (+ the other fields)
    //   • `update` — change an existing service, carries its `id` (+ fields)
    //   • `delete` — remove an existing service, carries its `id` (+ fields)
    // The backend applies the delta against the current on-chain services, so
    // the CLI no longer fetches the existing list or sends full coverage. When
    // the user changes no service this call, `services` is omitted entirely
    // (omission no longer means "clear all").
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
    debug_log!(
        "[agent-identity] update request: url={} access_token_len={} access_token_prefix={} body={}",
        reconstruct_post_url_for_log(
            ctx,
            "/priapi/v5/wallet/agentic/pre-transaction/update-agent",
        ),
        access_token.len(),
        redact_token_for_debug(&access_token),
        serde_json::to_string(&scrub_body_for_log(&body)).unwrap_or_else(|_| "<serialize failed>".to_string()),
    );
    let update_result = client
        .post_authed(
            "/priapi/v5/wallet/agentic/pre-transaction/update-agent",
            &access_token,
            &body,
        )
        .await;
    match &update_result {
        Ok(data) => debug_log!(
            "[agent-identity] update response: {}",
            serde_json::to_string(data)
                .unwrap_or_else(|_| "<serialize failed>".to_string())
        ),
        Err(e) => debug_log!("[agent-identity] update response err: {:#}", e),
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
            debug_log!(
                "[agent-identity] ws subscribe failed, falling through to broadcast-only: {e:#}"
            );
            None
        }
    };

    let tx_hash =
        sign_and_broadcast_agent_transaction(&access_token, &unsigned, None, &signing_session)
            .await?;

    let push = wait_for_identity_push(subscription, &tx_hash).await;
    let new_agent_id = extract_agent_id_from_push(push.as_ref());
    Ok(assemble_identity_envelope(tx_hash, push, new_agent_id))
}

// ─── `agent activate` / `agent deactivate` ────────────────────────────────

/// Unified activation — fully self-contained:
///   Step 0: GET agent info (role + name + description) → role guard
///   Step 1: POST agent-status (status=1)
///   Step 2: if approvalStatus ∈ {1,5} → POST submit-approval (no QA — listing QA
///           runs only at register/update, never here)
///
/// Return-structure contract (all branches):
///   blockType:1 + reason + agentRole   → not a provider; agent-status never called
///   activate [+ submitApproval] → normal path
async fn activate_impl(args: &ActivateArgs, ctx: &Context) -> Result<Value> {
    let agent_id = require_non_empty(args.agent_id.as_deref(), "--agent-id")?;

    // ── Step 0: fetch agent info + role guard ─────────────────────────────
    let agent_info = fetch_agent_info_by_id(agent_id, ctx).await?;
    if agent_info.is_none() {
        bail!("agent {} not found or not accessible", agent_id);
    }
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
    // Backend returns a single-element array; normalize to object so field
    // access (approvalStatus) works correctly on the next step.
    let activate_result = normalize_singleton_object(
        agent_status_impl(Some(agent_id), 1, ctx).await?,
    );

    // ── Step 2: QA + submit — only when approvalStatus ∈ {1, 5} ──────────
    // approvalStatus 1 = initial listing QA required
    // approvalStatus 5 = re-listing QA required (treat same as 1 per manage.md)
    let needs_approval = activate_result
        .get("approvalStatus")
        .and_then(|v| {
            v.as_u64()
                .or_else(|| v.as_str().and_then(|s| s.parse::<u64>().ok()))
        })
        .map(|s| s == 1 || s == 5)
        .unwrap_or(false);

    if !needs_approval {
        return Ok(json!({ "activate": activate_result }));
    }

    // ── approvalStatus ∈ {1, 5}: submit for approval directly ────────────
    let submit_result = submit_approval_impl(
        Some(agent_id),
        args.preferred_language.as_deref(),
        ctx,
    )
    .await?;

    Ok(json!({
        "activate": activate_result,
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

    debug_log!("[agent-identity] activate info-fetch: agent-id={}", agent_id);

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

    debug_log!(
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

async fn deactivate_impl(args: &AgentStatusArgs, ctx: &Context) -> Result<Value> {
    Ok(normalize_singleton_object(
        agent_status_impl(args.agent_id.as_deref(), 2, ctx).await?,
    ))
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

    debug_log!(
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
        Ok(data) => debug_log!(
            "[agent-identity] agent-status response: {}",
            serde_json::to_string(data)
                .unwrap_or_else(|_| "<serialize failed>".to_string())
        ),
        Err(e) => debug_log!("[agent-identity] agent-status response err: {:#}", e),
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

    debug_log!(
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
        Ok(data) => debug_log!(
            "[agent-identity] submit-approval response: {}",
            serde_json::to_string(data)
                .unwrap_or_else(|_| "<serialize failed>".to_string())
        ),
        Err(e) => debug_log!("[agent-identity] submit-approval response err: {:#}", e),
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
    debug_log!(
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
        Ok(data) => debug_log!(
            "[agent-identity] upload response: {}",
            serde_json::to_string(data)
                .unwrap_or_else(|_| "<serialize failed>".to_string())
        ),
        Err(e) => debug_log!("[agent-identity] upload response err: {:#}", e),
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
    let mut body = json!({
        "chainIndex": XLAYER_CHAIN_INDEX_NUM,
        "sessionCert": &signing_session.session_cert,
        "feedBackAgentId": creator_id,
        "comment": serde_json::to_string(&comment).context("failed to serialize comment")?,
    });
    if !task_id.is_empty() {
        body["taskId"] = json!(task_id);
    }

    debug_log!(
        "[agent-identity] feedback-submit request: url={} access_token_len={} access_token_prefix={} body={}",
        reconstruct_post_url_for_log(
            ctx,
            "/priapi/v5/wallet/agentic/pre-transaction/create-comment",
        ),
        access_token.len(),
        redact_token_for_debug(&access_token),
        serde_json::to_string(&scrub_body_for_log(&body)).unwrap_or_else(|_| "<serialize failed>".to_string()),
    );

    let result = client
        .post_authed(
            "/priapi/v5/wallet/agentic/pre-transaction/create-comment",
            &access_token,
            &body,
        )
        .await;

    match &result {
        Ok(data) => debug_log!(
            "[agent-identity] feedback-submit response: {}",
            serde_json::to_string(data)
                .unwrap_or_else(|_| "<serialize failed>".to_string())
        ),
        Err(e) => debug_log!("[agent-identity] feedback-submit response err: {:#}", e),
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

    debug_log!(
        "[agent-identity] xmtp-sign request: url={} access_token_len={} access_token_prefix={} body={}",
        reconstruct_post_url_for_log(
            ctx,
            "/priapi/v5/wallet/agentic/pre-transaction/sign-msg",
        ),
        access_token.len(),
        redact_token_for_debug(&access_token),
        serde_json::to_string(&scrub_body_for_log(&body)).unwrap_or_else(|_| "<serialize failed>".to_string()),
    );

    let result = client
        .post_authed(
            "/priapi/v5/wallet/agentic/pre-transaction/sign-msg",
            &access_token,
            &body,
        )
        .await;

    match &result {
        Ok(data) => debug_log!(
            "[agent-identity] xmtp-sign response: {}",
            serde_json::to_string(data)
                .unwrap_or_else(|_| "<serialize failed>".to_string())
        ),
        Err(e) => debug_log!("[agent-identity] xmtp-sign response err: {:#}", e),
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
/// emits `txHash` + `newAgentId` only.
async fn wait_for_identity_push(
    subscription: Option<IdentitySubscription>,
    tx_hash: &str,
) -> Option<Value> {
    let sub = subscription?;
    match sub.wait_for_match(tx_hash, PUSH_WAIT_TIMEOUT).await {
        Ok(opt) => opt,
        Err(e) => {
            debug_log!("[agent-identity] ws wait failed: {e:#}");
            None
        }
    }
}


/// Assemble the `{ txHash, agent?, newAgentId }` envelope.
/// `agent` is present only when the WS push arrived in time.
/// `newAgentId` is always present (string id or JSON `null`).
fn assemble_identity_envelope(
    tx_hash: String,
    push: Option<Value>,
    new_agent_id: Option<String>,
) -> Value {
    let mut out = json!({ "txHash": tx_hash });
    if let Some(p) = push {
        out["agent"] = p;
    }
    out["newAgentId"] = match new_agent_id {
        Some(id) => Value::String(id),
        None => Value::Null,
    };
    out
}

/// Extract `agentId` from a WS push payload. Returns `None` when the push
/// is absent or does not carry a usable id.
fn extract_agent_id_from_push(push: Option<&Value>) -> Option<String> {
    let p = push?;
    match p.get("agentId")? {
        Value::String(s) if !s.trim().is_empty() => Some(s.trim().to_string()),
        Value::Number(n) => Some(n.to_string()),
        _ => None,
    }
}

#[cfg(test)]
#[path = "tests/mutations_tests.rs"]
mod tests;
