//! Subscription lifecycle management + read-only display commands.
//!
//! Management (on-chain):
//! - `subscribe-cancel`       — unified cancel (trial cancel + close auto-renew)
//! - `start-autorenew`        — enable auto-renew (on-chain, needs terms + termsSig)
//! - `subscribe-reject`       — user rejects delivery (reason in bizContext)
//! - `subscribe-detail`       — show subscription detail
//!
//! Display (read-only):
//! - `my-subscriptions`       — list the logged-in agent's AI-service subscriptions
//!   (buyer or provider view).

use anyhow::{anyhow, bail, Result};
use clap::ValueEnum;
use serde::{Deserialize, Serialize};
use std::time::Duration;

use super::create::resolve_user_agent;
use super::create_subscribe::SUBSCRIBE_API_PREFIX;
use crate::audit;
use crate::commands::agent_commerce::task::common::network::task_api_client::TaskApiClient;
use crate::commands::agent_commerce::task::common::okx_a2a;
use crate::commands::agent_commerce::task::common::query as common_query;
use crate::commands::agent_commerce::task::common::state_machine::SubStatus;
use crate::commands::agent_commerce::task::common::{AGENT_ROLE_ASP, AGENT_ROLE_USER};
use crate::commands::agent_commerce::task::signing;
use crate::commands::agentic_wallet::auth::ensure_tokens_refreshed;

// ── active subscription: ensure XMTP session consent with the provider ──
//
// Subscription deliverables arrive as P2P `[intent:deliver]` XMTP messages, which the
// buyer's a2a daemon holds at `consent=0` until the buyer has an established (allowed)
// session with the provider. One-shot tasks open that session during negotiation; the
// subscribe flow has no negotiation. Establish the session for every active subscription
// so delivery transport is independent of the optional `copyTrade` capability marker.

/// `<onchainos_home>/subscription/consent/<jobId>` — per-device "already established"
/// marker. `None` if `job_id` fails the path-safety charset check.
fn consent_marker_path(job_id: &str) -> Option<std::path::PathBuf> {
    if job_id.is_empty()
        || !job_id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
    {
        return None;
    }
    let home = crate::home::onchainos_home().ok()?;
    Some(home.join("subscription").join("consent").join(job_id))
}

fn should_ensure_subscription_session(status: i64) -> bool {
    status == SubStatus::Active.code()
}

/// Idempotently ensure the buyer has a consented XMTP session with `provider_agent_id`.
/// Safe to call from any device and repeatedly (a per-device marker avoids re-sending).
/// Never fails the caller.
pub(crate) fn ensure_subscription_session(
    job_id: &str,
    my_agent_id: &str,
    provider_agent_id: &str,
) {
    if my_agent_id.is_empty() {
        return;
    }
    if provider_agent_id.is_empty() || provider_agent_id == "?" {
        return;
    }
    let Some(marker) = consent_marker_path(job_id) else {
        return;
    };
    if marker.exists() {
        return; // already established on this device
    }
    // Establish the group (idempotent) + send one marker message → grants the XMTP "allow"
    // so the provider's `[intent:deliver]` signals are dispatched instead of held.
    if okx_a2a::session_create(job_id, my_agent_id, provider_agent_id).is_ok() {
        let _ = okx_a2a::session_send(
            job_id,
            Some(provider_agent_id),
            "[SUB_CONSENT] subscription session established.",
        );
        let _ = crate::home::write_secure(&marker, b"1");
    }
}

// ── subscribe-cancel ────────────────────────────────────────────────────

pub async fn handle_subscribe_cancel(client: &mut TaskApiClient, sub_id: &str) -> Result<()> {
    ensure_tokens_refreshed().await?;
    let (user_agent_id, _) = resolve_user_agent().await?;
    let (account_id, address) = signing::resolve_wallet_by_agent_id(&user_agent_id).await?;

    let resp = client
        .post_with_identity(
            &format!("{SUBSCRIBE_API_PREFIX}/{sub_id}/cancel"),
            &serde_json::json!({}),
            &user_agent_id,
        )
        .await
        .map_err(|e| anyhow::anyhow!("subscribe-cancel failed: {e}"))?;

    let biz_type = signing::extract_biz_type(&resp);
    let tx_hash = signing::sign_uop_and_broadcast(
        client,
        &resp["uopData"],
        &account_id,
        &address,
        sub_id,
        biz_type,
        &user_agent_id,
        None,
    )
    .await?;

    audit::log(
        "cli",
        "user/subscribe_cancel",
        true,
        Duration::default(),
        Some(vec![format!("subId={sub_id}"), format!("txHash={tx_hash}")]),
        None,
    );

    println!("✓ Subscription cancel in progress (transaction broadcast)");
    println!("  subId:  {sub_id}");
    println!("  txHash: {tx_hash}");

    if super::content::is_cli_mode() {
        println!();
        println!("[Watch] 🛑 Mandatory next steps. End the turn after Step 2. Do NOT ask the user whether to watch — it is required to receive the next event.");
        println!();
        println!("**Step 1 (MANDATORY GATE) — Read `skills/okx-ai/references/watch-core.md` now.** If you have NOT read it in this session, you cannot proceed to Step 2 — Step 2's invocation, dispatch rules, and re-arm semantics live ONLY in that file. Skipping this step is a protocol violation.");
        println!();
        println!("**Step 2 — Execute the watch per `skills/okx-ai/references/watch-core.md` §Run watch, scoped to job-id `{sub_id}`.** Then dispatch each returned item per §Dispatch by `kind` and re-enter per §Re-enter after processing. (Do NOT guess the bash invocation — read watch-core.md first.)");
        println!();
        println!("⏭ Skip `detect_watch_support` — this `[Watch]` block is only emitted on supported platforms.");
    }

    Ok(())
}

// ── start-autorenew ─────────────────────────────────────────────────────

pub async fn handle_start_autorenew(client: &mut TaskApiClient, sub_id: &str) -> Result<()> {
    ensure_tokens_refreshed().await?;
    let (user_agent_id, _) = resolve_user_agent().await?;
    let (account_id, address) = signing::resolve_wallet_by_agent_id(&user_agent_id).await?;

    // Step 1: providerConfirmStatus to get terms (with existing subId)
    let confirm_resp = client
        .post_with_identity(
            &format!("{SUBSCRIBE_API_PREFIX}/providerConfirmStatus"),
            &serde_json::json!({ "subId": sub_id, "autoRenew": 1 }),
            &user_agent_id,
        )
        .await
        .map_err(|e| anyhow::anyhow!("providerConfirmStatus failed: {e}"))?;

    if confirm_resp.is_null() || confirm_resp.as_object().map_or(true, |o| o.is_empty()) {
        bail!("providerConfirmStatus returned empty terms");
    }

    let typed_data = &confirm_resp["typedData"];
    if typed_data.is_null() || typed_data.as_object().map_or(true, |o| o.is_empty()) {
        bail!("providerConfirmStatus response missing typedData");
    }

    // Step 2: EIP-712 sign terms (sign the typedData sub-object, not the full response)
    let terms_sig = signing::sign_typed_data(typed_data, &address).await?;

    // Step 3: POST startAutoRenew with terms + termsSig
    let mut terms_for_renew = confirm_resp.clone();
    if let Some(obj) = terms_for_renew.as_object_mut() {
        obj.remove("typedData");
    }

    let resp = client
        .post_with_identity(
            &format!("{SUBSCRIBE_API_PREFIX}/{sub_id}/startAutoRenew"),
            &serde_json::json!({
                "terms": terms_for_renew,
                "termsSig": terms_sig,
            }),
            &user_agent_id,
        )
        .await
        .map_err(|e| anyhow::anyhow!("start-autorenew failed: {e}"))?;

    let biz_type = signing::extract_biz_type(&resp);
    let tx_hash = signing::sign_uop_and_broadcast(
        client,
        &resp["uopData"],
        &account_id,
        &address,
        sub_id,
        biz_type,
        &user_agent_id,
        None,
    )
    .await?;

    audit::log(
        "cli",
        "user/start_autorenew",
        true,
        Duration::default(),
        Some(vec![format!("subId={sub_id}"), format!("txHash={tx_hash}")]),
        None,
    );

    println!("✓ Auto-renew enable in progress (transaction broadcast)");
    println!("  subId:  {sub_id}");
    println!("  txHash: {tx_hash}");
    Ok(())
}

// ── subscribe-reject ────────────────────────────────────────────────────

/// Direct CLI entry — validates reason, resolves agent, then delegates to inner.
pub async fn handle_subscribe_reject(
    client: &mut TaskApiClient,
    sub_id: &str,
    reason: &str,
) -> Result<()> {
    if reason.is_empty() {
        bail!("--reason is required for subscribe-reject");
    }
    if reason.chars().count() > 2000 {
        bail!("--reason exceeds 2000 characters");
    }

    ensure_tokens_refreshed().await?;
    let (user_agent_id, _) = resolve_user_agent().await?;

    handle_subscribe_reject_inner(client, sub_id, reason, &user_agent_id).await
}

/// Inner implementation — caller has already validated reason and resolved agent_id.
pub(crate) async fn handle_subscribe_reject_inner(
    client: &mut TaskApiClient,
    sub_id: &str,
    reason: &str,
    user_agent_id: &str,
) -> Result<()> {
    let (account_id, address) = signing::resolve_wallet_by_agent_id(user_agent_id).await?;

    let resp = client
        .post_with_identity(
            &format!("{SUBSCRIBE_API_PREFIX}/{sub_id}/reject"),
            &serde_json::json!({}),
            user_agent_id,
        )
        .await
        .map_err(|e| anyhow::anyhow!("subscribe-reject failed: {e}"))?;

    let biz_type = signing::extract_biz_type(&resp);
    let reason_extra = serde_json::json!({ "reason": reason });
    let tx_hash = signing::sign_uop_and_broadcast(
        client,
        &resp["uopData"],
        &account_id,
        &address,
        sub_id,
        biz_type,
        user_agent_id,
        Some(&reason_extra),
    )
    .await?;

    audit::log(
        "cli",
        "user/subscribe_reject",
        true,
        Duration::default(),
        Some(vec![format!("subId={sub_id}"), format!("txHash={tx_hash}")]),
        None,
    );

    println!("✓ Subscription rejection in progress (transaction broadcast)");
    println!("  subId:  {sub_id}");
    println!("  txHash: {tx_hash}");

    if super::content::is_cli_mode() {
        println!();
        println!("[Watch] 🛑 Mandatory next steps. End the turn after Step 2. Do NOT ask the user whether to watch — it is required to receive the next event.");
        println!();
        println!("**Step 1 (MANDATORY GATE) — Read `skills/okx-ai/references/watch-core.md` now.** If you have NOT read it in this session, you cannot proceed to Step 2 — Step 2's invocation, dispatch rules, and re-arm semantics live ONLY in that file. Skipping this step is a protocol violation.");
        println!();
        println!("**Step 2 — Execute the watch per `skills/okx-ai/references/watch-core.md` §Run watch, scoped to job-id `{sub_id}`.** Then dispatch each returned item per §Dispatch by `kind` and re-enter per §Re-enter after processing. (Do NOT guess the bash invocation — read watch-core.md first.)");
        println!();
        println!("⏭ Skip `detect_watch_support` — this `[Watch]` block is only emitted on supported platforms.");
    }

    Ok(())
}

// ── subscribe-detail ────────────────────────────────────────────────────

pub async fn handle_subscribe_detail(
    client: &mut TaskApiClient,
    sub_id: &str,
    format: &str,
) -> Result<()> {
    ensure_tokens_refreshed().await?;

    let agent_id = match signing::resolve_agent_id_by_role(AGENT_ROLE_USER).await {
        Ok(id) => id,
        Err(_) => signing::resolve_agent_id_by_role(AGENT_ROLE_ASP)
            .await
            .unwrap_or_default(),
    };

    let json_mode = format.eq_ignore_ascii_case("json");

    let resp = client
        .get_with_identity(&format!("{SUBSCRIBE_API_PREFIX}/{sub_id}"), &agent_id)
        .await
        .map_err(|e| anyhow::anyhow!("subscribe-detail failed: {e}"))?;
    let is_buyer =
        !agent_id.is_empty() && resp["buyerAgentId"].as_str() == Some(agent_id.as_str());

    // Checking an active subscription on a fresh device establishes the provider
    // session (drains any held deliverables). Runs before the json early-return so
    // both modes benefit. Only when the logged-in agent is this subscription's buyer.
    if is_buyer && should_ensure_subscription_session(resp["status"].as_i64().unwrap_or(-1)) {
        ensure_subscription_session(
            sub_id,
            &agent_id,
            resp["providerAgentId"].as_str().unwrap_or(""),
        );
    }

    if json_mode {
        let enriched =
            enrich_subscription_detail(resp, crate::device::id::get_cached_device_id(), is_buyer);
        crate::output::success(enriched);
        return Ok(());
    }

    let title = resp["title"].as_str().unwrap_or("?");
    let status = resp["status"].as_i64().unwrap_or(-1);
    let trial_type = resp["trialType"].as_i64().unwrap_or(0);
    let auto_renew = resp["autoRenew"].as_i64().unwrap_or(0);
    let copy_trade = resp["copyTrade"].as_i64().unwrap_or(0);
    let period_index = resp["periodIndex"].as_u64().unwrap_or(0);
    let buyer = resp["buyerAgentId"].as_str().unwrap_or("?");
    let provider = resp["providerAgentId"].as_str().unwrap_or("?");
    let amount = resp["serviceTokenAmount"].as_str().unwrap_or("?");
    let sub_start = resp["subStartTime"].as_i64();
    let sub_end = resp["subEndTime"].as_i64();

    let sub_status = SubStatus::from_code(status);
    let status_label = if sub_status == SubStatus::Active && trial_type == 1 {
        "Active (Trial)"
    } else {
        sub_status.as_str()
    };

    println!("Subscription Detail: {title}");
    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    println!("  subId:     {}", resp["jobId"].as_str().unwrap_or(sub_id));
    println!("  status:    {status_label}");
    println!("  buyer:     #{buyer}");
    println!("  provider:  #{provider}");
    println!("  fee:       {amount}/month");
    println!("  period:    {period_index}");
    println!("  autoRenew: {auto_renew}");
    println!("  copyTrade: {copy_trade}");
    if let (Some(start), Some(end)) = (sub_start, sub_end) {
        println!("  current:   {start} ~ {end}");
    }
    if trial_type == 1 {
        let (t_start, t_end) = trial_window(&resp);
        println!("  trial:     {t_start} ~ {t_end}");
    }

    // Raw-state lines for the human view. A caller reading this instead of
    // `--format json` must not be able to mistake "field absent" for a real
    // value: an absent offline flag would otherwise read as the server default,
    // and an absent device list read as empty then written back wholesale would
    // wipe every other device's receipt. Only the raw state is printed here — the
    // joined, named device table stays JSON-only.
    let offline_line = match resp["offlineReceiveFlag"].as_i64() {
        Some(1) => "1 (discard)".to_string(),
        Some(0) => "0 (keep — default)".to_string(),
        Some(n) => format!("{n} (keep — default)"),
        None => "missing (keep — default)".to_string(),
    };
    println!("  offline:   {offline_line}");

    let devices = normalize_optional_str_array(resp.get(FIELD_DEVICE_LIST));
    let devices_line = format_devices_for_human(
        devices.as_deref(),
        crate::device::id::get_cached_device_id(),
    );
    println!("  devices:   {devices_line}");
    Ok(())
}

/// Trial-window timestamps from a query-API response. trial* is the canonical
/// spelling; the query API still serves the legacy trail* misspelling — read
/// new-name-first so the backend's migration is invisible here (mirrors the
/// event-side tolerant read).
fn trial_window(resp: &serde_json::Value) -> (i64, i64) {
    let read = |new_key: &str, legacy_key: &str| {
        resp[new_key]
            .as_i64()
            .or_else(|| resp[legacy_key].as_i64())
            .unwrap_or(0)
    };
    (
        read("trialStartTime", "trailStartTime"),
        read("trialEndTime", "trailEndTime"),
    )
}

// ── subscribe-cost (active subscriptions monthly cost) ─────────────────

pub async fn handle_subscribe_cost(client: &mut TaskApiClient) -> Result<()> {
    ensure_tokens_refreshed().await?;
    let (agent_id, _) = resolve_user_agent().await?;
    let path = format!("{SUBSCRIBE_API_PREFIX}/cost/active");
    let resp = client
        .get_with_identity(&path, &agent_id)
        .await
        .map_err(|e| anyhow!("subscribe-cost failed: {e}"))?;
    audit::log(
        "cli",
        "user/subscribe_cost",
        true,
        Duration::default(),
        Some(vec![format!("agentId={agent_id}")]),
        None,
    );
    crate::output::success(resp);
    Ok(())
}

// ── Subscription display types + my-subscriptions ───────────────────────

/// Subscription viewpoint for `my-subscriptions`.
#[derive(Clone, Copy, Debug, ValueEnum)]
#[value(rename_all = "lower")]
pub enum SubscriptionRole {
    Buyer,
    Provider,
}

impl SubscriptionRole {
    pub fn agent_role(self) -> i64 {
        match self {
            Self::Buyer => AGENT_ROLE_USER,
            Self::Provider => AGENT_ROLE_ASP,
        }
    }
}

#[derive(Debug, Default, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct SubscriptionInfo {
    pub job_id: String,
    pub job_type: i64,
    pub status: i64,
    #[serde(skip_deserializing)]
    pub status_name: String,
    pub chain_id: i64,
    pub title: String,
    pub description: String,
    pub description_summary: String,
    pub buyer_agent_id: String,
    pub buyer_agent_address: String,
    pub provider_agent_id: String,
    pub provider_agent_address: String,
    pub trial_type: i64,
    pub trail_start_time: Option<i64>,
    pub trail_end_time: Option<i64>,
    pub sub_start_time: Option<i64>,
    pub sub_end_time: Option<i64>,
    pub sub_buffer_end_time: Option<i64>,
    pub auto_renew: i64,
    pub copy_trade: i64,
    pub period_index: Option<i64>,
    pub service_id: String,
    pub service_params: String,
    pub service_token_address: String,
    pub service_token_amount: String,
    pub payment_token_address: String,
    pub payment_token_amount: String,
    pub payment_currency_amount: String,
    // ── Device routing (additive) ─────────────────────────────────────────
    // Receive-device list for this subscription. Tri-state on the wire:
    // missing | null | array — all tolerated (Option so an explicit `null` on
    // historical rows does not fail deserialization); non-string array elements
    // are dropped (tolerant), matching subscribe-detail's normalize_str_array.
    // Preserve None on emit: null means default-all routing, while Some([]) means
    // the buyer explicitly disabled delivery to every device.
    #[serde(default, deserialize_with = "de_opt_str_array")]
    pub device_list: Option<Vec<String>>,
    // Sibling additive field from the same backend change; tolerate null the same way.
    #[serde(default, deserialize_with = "de_opt_str_array")]
    pub category_codes: Option<Vec<String>>,
    // Derived on the client after parse (device id lives only on the client).
    // Serialized out, never read from the wire (mirrors `status_name`).
    #[serde(skip_deserializing)]
    pub this_device_receives: bool,
}

#[derive(Debug, Default, Deserialize)]
struct SubscriptionList {
    #[serde(default)]
    list: Vec<SubscriptionInfo>,
}

/// Programmatic form of `my-subscriptions`, shared by the standalone command
/// and the wallet login post-condition. Keeping the resolved buyer agent id
/// alongside the JSON lets the login path fetch the matching device table
/// without resolving identity a second time.
pub(crate) struct MySubscriptionsSnapshot {
    pub(crate) data: serde_json::Value,
    pub(crate) agent_id: String,
    pub(crate) is_empty: bool,
}

pub fn status_name(status: i64) -> String {
    match status {
        -1 => "INIT".to_string(),
        1 => "ACTIVE".to_string(),
        3 => "REJECTED".to_string(),
        4 => "DISPUTED".to_string(),
        6 => "COMPLETED".to_string(),
        7 => "CLOSED".to_string(),
        9 => "FAILED".to_string(),
        n => format!("UNKNOWN_{n}"),
    }
}

/// clap value-parser for `--status`: accepts either a raw backend status code or a
/// case-insensitive status name; the name arm is the inverse of `status_name`.
/// Numeric input is deliberately passed through unvalidated — codes the backend adds
/// later must stay filterable (mirrors the UNKNOWN_<n> tolerance on the render side).
pub fn parse_status_filter(s: &str) -> Result<i32, String> {
    if let Ok(n) = s.parse::<i32>() {
        return Ok(n);
    }
    match s.to_ascii_uppercase().as_str() {
        "INIT" => Ok(-1),
        "ACTIVE" => Ok(1),
        "REJECTED" => Ok(3),
        "DISPUTED" => Ok(4),
        "COMPLETED" => Ok(6),
        "CLOSED" => Ok(7),
        "FAILED" => Ok(9),
        _ => Err(format!(
            "invalid status '{s}': expected a code (-1/1/3/4/6/7/9) or a name \
             (INIT/ACTIVE/REJECTED/DISPUTED/COMPLETED/CLOSED/FAILED)"
        )),
    }
}

fn my_subscriptions_path() -> String {
    format!("{SUBSCRIBE_API_PREFIX}/my")
}

/// Tolerant read of a wire `deviceList` / `categoryCodes` value: an array of
/// strings is collected; null / missing / any non-array shape normalizes to `[]`.
fn normalize_str_array(v: Option<&serde_json::Value>) -> Vec<String> {
    match v.and_then(|v| v.as_array()) {
        Some(arr) => arr
            .iter()
            .filter_map(|x| x.as_str().map(str::to_string))
            .collect(),
        None => Vec::new(),
    }
}

/// Tolerant tri-state read for `deviceList`: missing/null stays `None`, while an
/// array (including an explicitly empty one) stays `Some`. Other present shapes
/// retain the old tolerant behavior and normalize to `Some([])`.
fn normalize_optional_str_array(v: Option<&serde_json::Value>) -> Option<Vec<String>> {
    match v {
        None | Some(serde_json::Value::Null) => None,
        Some(value) => Some(normalize_str_array(Some(value))),
    }
}

/// Serde adapter for the struct (`my-subscriptions`) parse path so it tolerates
/// the same shapes `normalize_str_array` does on the raw-Value (`subscribe-detail`)
/// path: `null` → `None`; an array → `Some` with non-string elements dropped (a
/// single non-string element must not fail the whole list parse).
fn de_opt_str_array<'de, D>(de: D) -> std::result::Result<Option<Vec<String>>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let opt = Option::<serde_json::Value>::deserialize(de)?;
    Ok(opt.map(|v| normalize_str_array(Some(&v))))
}

/// Derive whether this device receives the subscription from the backend's
/// three-state routing contract. `None` means the historical/default-all mode,
/// but only for the buyer viewpoint. An explicit list uses membership; therefore
/// an unresolved this-device id is false for explicit lists.
fn device_receives(
    this_device_id: Option<&str>,
    device_list: Option<&[String]>,
    default_all_receives: bool,
) -> bool {
    match device_list {
        None => default_all_receives,
        Some(list) => this_device_id.is_some_and(|id| list.iter().any(|d| d == id)),
    }
}

/// Human-readable raw device routing for `subscribe-detail` without JSON mode.
/// Keep default-all distinct from an explicitly cleared receive list.
fn format_devices_for_human(
    device_list: Option<&[String]>,
    this_device_id: Option<&str>,
) -> String {
    let Some(devices) = device_list else {
        return "all (default — deviceList is not explicitly configured)".to_string();
    };
    if devices.is_empty() {
        return "none (no device receives this subscription)".to_string();
    }
    devices
        .iter()
        .map(|d| {
            let short: String = d.chars().take(8).collect();
            if this_device_id == Some(d.as_str()) {
                format!("{short}(this device)")
            } else {
                short
            }
        })
        .collect::<Vec<_>>()
        .join(", ")
}

// ── Device-routing enrichment seam (shared by both emitters) ──────────────
//
// The subscribe-detail (raw `serde_json::Value`) and my-subscriptions (typed
// struct) paths carry identical wire field names and derive the same
// this-device receipt. These constants + `derive_device_enrichment` are the
// single source both emitters consume, so the two paths cannot drift. The
// tolerant read itself stays single-sourced in `normalize_str_array` (the
// raw path calls it directly; the struct path via the `de_opt_str_array`
// serde adapter, which delegates to it).

/// Wire field name: receive-device list for a subscription.
const FIELD_DEVICE_LIST: &str = "deviceList";
/// Wire field name: subscription category codes.
const FIELD_CATEGORY_CODES: &str = "categoryCodes";
/// Wire field name: derived "this device receives" flag.
const FIELD_THIS_DEVICE_RECEIVES: &str = "thisDeviceReceives";
/// Wire field name: the client-resolved this-device id.
const FIELD_THIS_DEVICE_ID: &str = "thisDeviceId";
/// Wire field name: the readable OS name of the this-device (serialize-out only).
const FIELD_THIS_DEVICE_NAME: &str = "thisDeviceName";

/// The readable OS name of THIS device, serialized out on both subscription
/// emitters so a degraded render has a name for the this-device row without a
/// device-table lookup. Sourced from the cached device-name module (the OS name);
/// serialize-out only — never read from the wire (mirrors `thisDeviceReceives`).
fn this_device_name() -> &'static str {
    crate::device::name::get_cached_device_name()
}

/// Normalized device-routing enrichment for one subscription. `device_list`
/// preserves the backend tri-state; category codes still default to `[]`.
struct DeviceEnrichment {
    device_list: Option<Vec<String>>,
    category_codes: Vec<String>,
    this_device_receives: bool,
}

/// Derive the shared device-routing enrichment from the two (already tolerant-read)
/// arrays and the client's this-device id. Pure: device-list `None` is preserved
/// and means default-all only when `default_all_receives` is true; an explicit
/// array uses membership. Category-code `None` still normalizes to `[]`.
fn derive_device_enrichment(
    device_list: Option<Vec<String>>,
    category_codes: Option<Vec<String>>,
    this_device_id: Option<&str>,
    default_all_receives: bool,
) -> DeviceEnrichment {
    let category_codes = category_codes.unwrap_or_default();
    let this_device_receives =
        device_receives(this_device_id, device_list.as_deref(), default_all_receives);
    DeviceEnrichment {
        device_list,
        category_codes,
        this_device_receives,
    }
}

/// Add the CLI-derived fields to the raw subscribe-detail response. Keeping this
/// pure makes the null/empty/selected contract directly regression-testable.
fn enrich_subscription_detail(
    mut detail: serde_json::Value,
    this_device_id: Option<&str>,
    default_all_receives: bool,
) -> serde_json::Value {
    if let Some(obj) = detail.as_object_mut() {
        let code = obj.get("status").and_then(|v| v.as_i64()).unwrap_or(-1);
        obj.insert(
            "statusName".to_string(),
            serde_json::Value::String(status_name(code)),
        );
        // Preserve deviceList's wire-level tri-state while categoryCodes
        // continues to normalize to []. Default-all receipt is buyer-side only.
        let enrichment = derive_device_enrichment(
            normalize_optional_str_array(obj.get(FIELD_DEVICE_LIST)),
            normalize_optional_str_array(obj.get(FIELD_CATEGORY_CODES)),
            this_device_id,
            default_all_receives,
        );
        obj.insert(
            FIELD_DEVICE_LIST.to_string(),
            serde_json::json!(enrichment.device_list),
        );
        obj.insert(
            FIELD_CATEGORY_CODES.to_string(),
            serde_json::json!(enrichment.category_codes),
        );
        obj.insert(
            FIELD_THIS_DEVICE_RECEIVES.to_string(),
            serde_json::Value::Bool(enrichment.this_device_receives),
        );
        obj.insert(
            FIELD_THIS_DEVICE_ID.to_string(),
            match this_device_id {
                Some(id) => serde_json::Value::String(id.to_string()),
                None => serde_json::Value::Null,
            },
        );
        obj.insert(
            FIELD_THIS_DEVICE_NAME.to_string(),
            serde_json::Value::String(this_device_name().to_string()),
        );
    }
    detail
}

/// Add the same derived fields to a typed my-subscriptions row.
fn enrich_subscription_info(
    item: &mut SubscriptionInfo,
    this_device_id: Option<&str>,
    default_all_receives: bool,
) {
    item.status_name = status_name(item.status);
    let enrichment = derive_device_enrichment(
        item.device_list.take(),
        item.category_codes.take(),
        this_device_id,
        default_all_receives,
    );
    item.device_list = enrichment.device_list;
    item.category_codes = Some(enrichment.category_codes);
    item.this_device_receives = enrichment.this_device_receives;
}

fn filter_subscriptions(
    list: Vec<SubscriptionInfo>,
    role: SubscriptionRole,
    self_agent_id: &str,
    status: Option<i32>,
) -> Vec<SubscriptionInfo> {
    list.into_iter()
        .filter(|item| match role {
            SubscriptionRole::Buyer => item.buyer_agent_id == self_agent_id,
            SubscriptionRole::Provider => item.provider_agent_id == self_agent_id,
        })
        .filter(|item| status.is_none_or(|s| item.status == i64::from(s)))
        .collect()
}

pub(crate) async fn fetch_my_subscriptions_snapshot(
    client: &mut TaskApiClient,
    role: SubscriptionRole,
    status: Option<i32>,
) -> Result<MySubscriptionsSnapshot> {
    let header_agent = common_query::resolve_agent_id("", role.agent_role()).await;
    fetch_my_subscriptions_snapshot_for_agent(client, role, status, header_agent).await
}

/// Fetch subscriptions for an agent id already resolved by the caller. The
/// post-login new-device flow uses this after its pre-heartbeat device probe so
/// identity resolution and device membership refer to the same buyer.
pub(crate) async fn fetch_my_subscriptions_snapshot_for_agent(
    client: &mut TaskApiClient,
    role: SubscriptionRole,
    status: Option<i32>,
    header_agent: String,
) -> Result<MySubscriptionsSnapshot> {
    let header_agent = header_agent.trim().to_string();
    if header_agent.is_empty() {
        return Err(anyhow!(
            "agenticId is required to fetch subscription snapshot"
        ));
    }

    let path = my_subscriptions_path();
    let data = client
        .get_with_agent_id(&path, &header_agent)
        .await
        .map_err(|e| anyhow!("failed to fetch subscriptions: {e}"))?;
    let wrapper: SubscriptionList = serde_json::from_value(data)
        .map_err(|e| anyhow!("failed to parse subscription list: {e}"))?;
    let mut list = filter_subscriptions(wrapper.list, role, &header_agent, status);
    let this_device_id = crate::device::id::get_cached_device_id();
    for item in &mut list {
        // Preserve deviceList's null/[]/selected tri-state on emit. Default-all
        // receipt applies only to the buyer view; provider devices are not
        // subscription-message receivers. categoryCodes still normalizes to [].
        enrich_subscription_info(
            item,
            this_device_id,
            matches!(role, SubscriptionRole::Buyer),
        );
    }
    // Buyer listing subscriptions on any device establishes the provider session for
    // every active subscription (drains held deliverables cross-device).
    if matches!(role, SubscriptionRole::Buyer) {
        for item in &list {
            if should_ensure_subscription_session(item.status) {
                ensure_subscription_session(
                    &item.job_id,
                    &header_agent,
                    &item.provider_agent_id,
                );
            }
        }
    }
    let is_empty = list.is_empty();
    let mut envelope = serde_json::Map::new();
    envelope.insert("list".to_string(), serde_json::json!(list));
    envelope.insert(
        FIELD_THIS_DEVICE_ID.to_string(),
        serde_json::json!(this_device_id),
    );
    // Readable this-device name for the degraded render's this-device row.
    // Serialize-out only, from the cached device-name module (same source and
    // policy as the subscribe-detail emitter).
    envelope.insert(
        FIELD_THIS_DEVICE_NAME.to_string(),
        serde_json::json!(this_device_name()),
    );
    Ok(MySubscriptionsSnapshot {
        data: serde_json::Value::Object(envelope),
        agent_id: header_agent,
        is_empty,
    })
}

pub async fn handle_my_subscriptions(
    client: &mut TaskApiClient,
    role: SubscriptionRole,
    status: Option<i32>,
) -> Result<()> {
    let snapshot = fetch_my_subscriptions_snapshot(client, role, status).await?;
    crate::output::success(snapshot.data);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::Parser;
    use serde_json::json;

    #[derive(Parser)]
    struct TestCli {
        #[command(subcommand)]
        cmd: super::super::TaskCommand,
    }

    #[test]
    fn subscription_session_is_gated_only_by_active_status() {
        assert!(should_ensure_subscription_session(SubStatus::Active.code()));
        assert!(!should_ensure_subscription_session(SubStatus::Init.code()));
        assert!(!should_ensure_subscription_session(SubStatus::Closed.code()));
        assert!(!should_ensure_subscription_session(SubStatus::Failed.code()));
    }

    #[test]
    fn cli_subscribe_cancel() {
        let cli = TestCli::parse_from(["test", "subscribe-cancel", "sub-123"]);
        match cli.cmd {
            super::super::TaskCommand::SubscribeCancel { sub_id } => {
                assert_eq!(sub_id, "sub-123");
            }
            _ => panic!("expected SubscribeCancel"),
        }
    }

    #[test]
    fn cli_start_autorenew() {
        let cli = TestCli::parse_from(["test", "start-autorenew", "sub-456"]);
        match cli.cmd {
            super::super::TaskCommand::StartAutorenew { sub_id } => {
                assert_eq!(sub_id, "sub-456");
            }
            _ => panic!("expected StartAutorenew"),
        }
    }

    #[test]
    fn cli_subscribe_reject() {
        let cli = TestCli::parse_from([
            "test",
            "subscribe-reject",
            "sub-789",
            "--reason",
            "quality not met",
        ]);
        match cli.cmd {
            super::super::TaskCommand::SubscribeReject { sub_id, reason } => {
                assert_eq!(sub_id, "sub-789");
                assert_eq!(reason, "quality not met");
            }
            _ => panic!("expected SubscribeReject"),
        }
    }

    #[test]
    fn cli_subscribe_detail() {
        let cli = TestCli::parse_from(["test", "subscribe-detail", "sub-ccc", "--format", "json"]);
        match cli.cmd {
            super::super::TaskCommand::SubscribeDetail { sub_id, format } => {
                assert_eq!(sub_id, "sub-ccc");
                assert_eq!(format, "json");
            }
            _ => panic!("expected SubscribeDetail"),
        }
    }

    #[test]
    fn cli_subscribe_cost() {
        let cli = TestCli::parse_from(["test", "subscribe-cost"]);
        assert!(matches!(
            cli.cmd,
            super::super::TaskCommand::SubscribeCost {}
        ));
    }

    fn detail_fixture() -> serde_json::Value {
        json!({
            "jobId": "1234567890",
            "jobType": 1,
            "status": 1,
            "chainId": 196,
            "title": "Alpha signals subscription",
            "description": "Daily alpha signals",
            "descriptionSummary": "alpha signals",
            "buyerAgentId": "1001",
            "buyerAgentAddress": "0xbuyer",
            "providerAgentId": "2002",
            "providerAgentAddress": "0xprovider",
            "trialType": 1,
            "trailStartTime": 1700000000,
            "trailEndTime": 1700600000,
            "subStartTime": 1700600000,
            "subEndTime": 1703192000,
            "subBufferEndTime": 1703278400,
            "autoRenew": 1,
            "copyTrade": 0,
            "periodIndex": 1,
            "serviceId": "svc-1",
            "serviceParams": "{\"k\":\"v\"}",
            "serviceTokenAddress": "0xservice",
            "serviceTokenAmount": "10.500000",
            "paymentTokenAddress": "0xpayment",
            "paymentTokenAmount": "10.500000",
            "paymentCurrencyAmount": "10.50"
        })
    }

    #[test]
    fn detail_json_deserializes_all_fields() {
        let info: SubscriptionInfo = serde_json::from_value(detail_fixture()).unwrap();
        assert_eq!(info.job_id, "1234567890");
        assert_eq!(info.job_type, 1);
        assert_eq!(info.status, 1);
        assert_eq!(info.chain_id, 196);
        assert_eq!(info.title, "Alpha signals subscription");
        assert_eq!(info.buyer_agent_id, "1001");
        assert_eq!(info.provider_agent_id, "2002");
        assert_eq!(info.trial_type, 1);
        assert_eq!(info.sub_start_time, Some(1700600000));
        assert_eq!(info.sub_end_time, Some(1703192000));
        assert_eq!(info.sub_buffer_end_time, Some(1703278400));
        assert_eq!(info.auto_renew, 1);
        assert_eq!(info.copy_trade, 0);
        assert_eq!(info.period_index, Some(1));
        assert_eq!(info.service_id, "svc-1");
        assert_eq!(info.service_params, "{\"k\":\"v\"}");
    }

    #[test]
    fn detail_deserializes_tolerates_lingering_description_summary() {
        // AC-4: after WBW-14172 the backend may still send `descriptionSummary`
        // for a transition period. `SubscriptionInfo` uses container-level
        // `#[serde(default)]` with no `deny_unknown_fields`, so a payload that
        // carries the (now display-unused) field must still deserialize cleanly.
        // The fixture already includes `descriptionSummary`.
        assert!(detail_fixture().get("descriptionSummary").is_some());
        let parsed = serde_json::from_value::<SubscriptionInfo>(detail_fixture());
        assert!(
            parsed.is_ok(),
            "SubscriptionInfo must tolerate a lingering descriptionSummary: {:?}",
            parsed.err()
        );
    }

    #[test]
    fn list_element_deserializes_via_wrapper() {
        let wire = json!({ "list": [ detail_fixture() ] });
        let wrapper: SubscriptionList = serde_json::from_value(wire).unwrap();
        assert_eq!(wrapper.list.len(), 1);
        assert_eq!(wrapper.list[0].job_id, "1234567890");
    }

    #[test]
    fn status_filter_accepts_codes_and_names_and_rejects_garbage() {
        // Name arm is the inverse of status_name for every documented code.
        for code in [-1i64, 1, 3, 4, 6, 7, 9] {
            assert_eq!(parse_status_filter(&status_name(code)), Ok(code as i32));
        }
        assert_eq!(parse_status_filter("1"), Ok(1));
        assert_eq!(parse_status_filter("-1"), Ok(-1));
        assert_eq!(parse_status_filter("active"), Ok(1));
        assert_eq!(parse_status_filter("Closed"), Ok(7));
        // Unknown numeric codes pass through (forward-compat with new backend codes).
        assert_eq!(parse_status_filter("42"), Ok(42));
        let err = parse_status_filter("ACTIV").unwrap_err();
        assert!(err.contains("ACTIVE"), "error lists valid names: {err}");
    }

    #[test]
    fn status_name_covers_all_seven_codes_and_unknown() {
        assert_eq!(status_name(-1), "INIT");
        assert_eq!(status_name(1), "ACTIVE");
        assert_eq!(status_name(3), "REJECTED");
        assert_eq!(status_name(4), "DISPUTED");
        assert_eq!(status_name(6), "COMPLETED");
        assert_eq!(status_name(7), "CLOSED");
        assert_eq!(status_name(9), "FAILED");
        assert_eq!(status_name(2), "UNKNOWN_2");
        assert_eq!(status_name(42), "UNKNOWN_42");
    }

    #[test]
    fn trial_times_present_deserialize_to_some() {
        let info: SubscriptionInfo = serde_json::from_value(detail_fixture()).unwrap();
        assert_eq!(info.trail_start_time, Some(1700000000));
        assert_eq!(info.trail_end_time, Some(1700600000));
    }

    #[test]
    fn non_trial_null_times_deserialize_to_none() {
        let mut wire = detail_fixture();
        wire["trailStartTime"] = serde_json::Value::Null;
        wire["trailEndTime"] = serde_json::Value::Null;
        wire["trialType"] = json!(0);
        let info: SubscriptionInfo = serde_json::from_value(wire).unwrap();
        assert_eq!(info.trail_start_time, None);
        assert_eq!(info.trail_end_time, None);
        assert_eq!(info.trial_type, 0);
    }

    #[test]
    fn init_record_null_sub_times_deserialize_to_none() {
        let mut wire = detail_fixture();
        wire["status"] = json!(-1);
        wire["subStartTime"] = serde_json::Value::Null;
        wire["subEndTime"] = serde_json::Value::Null;
        wire["subBufferEndTime"] = serde_json::Value::Null;
        wire["periodIndex"] = serde_json::Value::Null;

        let info: SubscriptionInfo = serde_json::from_value(wire.clone()).unwrap();
        assert_eq!(info.status, -1);
        assert_eq!(info.sub_start_time, None);
        assert_eq!(info.sub_end_time, None);
        assert_eq!(info.sub_buffer_end_time, None);
        assert_eq!(info.period_index, None);

        let wrapper: SubscriptionList =
            serde_json::from_value(json!({ "list": [ wire, detail_fixture() ] })).unwrap();
        assert_eq!(wrapper.list.len(), 2);
        assert_eq!(wrapper.list[0].sub_start_time, None);
        assert_eq!(wrapper.list[1].sub_start_time, Some(1700600000));
    }

    #[test]
    fn subscription_role_maps_agent_role() {
        assert_eq!(SubscriptionRole::Buyer.agent_role(), AGENT_ROLE_USER);
        assert_eq!(SubscriptionRole::Provider.agent_role(), AGENT_ROLE_ASP);
    }

    #[test]
    fn my_subscriptions_path_has_no_query_string() {
        let path = my_subscriptions_path();
        assert_eq!(path, "/priapi/v1/aieco/task/subscribe/my");
        assert!(!path.contains('?'));
    }

    #[tokio::test]
    async fn my_subscriptions_rejects_blank_agentic_id_before_request() {
        let mut client = TaskApiClient::new();

        let result = fetch_my_subscriptions_snapshot_for_agent(
            &mut client,
            SubscriptionRole::Buyer,
            None,
            "   ".to_string(),
        )
        .await;
        let error = match result {
            Ok(_) => panic!("blank agenticId must be rejected before any HTTP request"),
            Err(error) => error,
        };

        assert!(
            error.to_string().contains("agenticId is required"),
            "unexpected error: {error:#}"
        );
    }

    fn sub(buyer: &str, provider: &str, status: i64) -> SubscriptionInfo {
        let mut s: SubscriptionInfo = serde_json::from_value(detail_fixture()).unwrap();
        s.buyer_agent_id = buyer.to_string();
        s.provider_agent_id = provider.to_string();
        s.status = status;
        s
    }

    #[test]
    fn filter_subscriptions_buyer_and_provider_views_are_client_side() {
        let list = || vec![sub("1001", "2002", 1), sub("3003", "1001", 6)];
        let buyer = filter_subscriptions(list(), SubscriptionRole::Buyer, "1001", None);
        assert_eq!(buyer.len(), 1);
        assert_eq!(buyer[0].provider_agent_id, "2002");
        let provider = filter_subscriptions(list(), SubscriptionRole::Provider, "1001", None);
        assert_eq!(provider.len(), 1);
        assert_eq!(provider[0].buyer_agent_id, "3003");
    }

    #[test]
    fn filter_subscriptions_status_filter_is_client_side() {
        let list = || {
            vec![
                sub("1001", "2002", 1),
                sub("1001", "3003", 6),
                sub("1001", "4004", 1),
            ]
        };
        assert_eq!(
            filter_subscriptions(list(), SubscriptionRole::Buyer, "1001", None).len(),
            3
        );
        let active = filter_subscriptions(list(), SubscriptionRole::Buyer, "1001", Some(1));
        assert_eq!(active.len(), 2);
        assert!(active.iter().all(|s| s.status == 1));
        assert!(filter_subscriptions(list(), SubscriptionRole::Buyer, "1001", Some(99)).is_empty());
    }

    #[test]
    fn empty_or_missing_list_defaults_to_empty_array() {
        let wrapper: SubscriptionList = serde_json::from_value(json!({})).unwrap();
        assert!(wrapper.list.is_empty());
        let wrapper: SubscriptionList = serde_json::from_value(json!({ "list": [] })).unwrap();
        assert!(wrapper.list.is_empty());
        let envelope = json!({ "list": SubscriptionList::default().list });
        assert_eq!(envelope, json!({ "list": [] }));
    }

    #[test]
    fn decimal_amounts_stay_string_and_round_trip() {
        let info: SubscriptionInfo = serde_json::from_value(detail_fixture()).unwrap();
        assert_eq!(info.service_token_amount, "10.500000");
        assert_eq!(info.payment_token_amount, "10.500000");
        assert_eq!(info.payment_currency_amount, "10.50");
        let out = serde_json::to_value(&info).unwrap();
        assert_eq!(out["serviceTokenAmount"], json!("10.500000"));
        assert!(out["serviceTokenAmount"].is_string());
    }

    #[test]
    fn status_name_present_in_serialized_envelope_for_detail_and_list() {
        let mut info: SubscriptionInfo = serde_json::from_value(detail_fixture()).unwrap();
        info.status_name = status_name(info.status);
        let out = serde_json::to_value(&info).unwrap();
        assert_eq!(out["statusName"], json!("ACTIVE"));

        let mut wrapper: SubscriptionList =
            serde_json::from_value(json!({ "list": [ detail_fixture() ] })).unwrap();
        for item in &mut wrapper.list {
            item.status_name = status_name(item.status);
        }
        let envelope = json!({ "list": wrapper.list });
        assert_eq!(envelope["list"][0]["statusName"], json!("ACTIVE"));
    }

    #[test]
    fn trial_window_reads_canonical_spelling_first() {
        // Both spellings present → the canonical trial* value wins.
        let both = json!({
            "trialStartTime": 1_700_000_000i64, "trialEndTime": 1_700_600_000i64,
            "trailStartTime": 1_600_000_000i64, "trailEndTime": 1_600_600_000i64
        });
        assert_eq!(trial_window(&both), (1_700_000_000, 1_700_600_000));
    }

    #[test]
    fn trial_window_falls_back_to_legacy_spelling() {
        // Legacy-only response (today's query API) still yields the window.
        let legacy =
            json!({ "trailStartTime": 1_700_000_000i64, "trailEndTime": 1_700_600_000i64 });
        assert_eq!(trial_window(&legacy), (1_700_000_000, 1_700_600_000));
        // Neither present → zeros, never an error.
        assert_eq!(trial_window(&json!({})), (0, 0));
    }

    // ── Device routing enrichment ────────────────────────────────────────

    #[test]
    fn device_list_tri_state_deserializes_without_error() {
        // missing → None (default)
        let info: SubscriptionInfo = serde_json::from_value(detail_fixture()).unwrap();
        assert!(info.device_list.is_none());
        assert!(info.category_codes.is_none());
        // null → None (Option tolerates explicit null on historical rows)
        let mut w = detail_fixture();
        w["deviceList"] = serde_json::Value::Null;
        w["categoryCodes"] = serde_json::Value::Null;
        let info: SubscriptionInfo = serde_json::from_value(w).unwrap();
        assert!(info.device_list.is_none());
        assert!(info.category_codes.is_none());
        // array → Some
        let mut w = detail_fixture();
        w["deviceList"] = json!(["d1", "d2"]);
        w["categoryCodes"] = json!(["c1"]);
        let info: SubscriptionInfo = serde_json::from_value(w).unwrap();
        assert_eq!(info.device_list.unwrap(), vec!["d1", "d2"]);
        assert_eq!(info.category_codes.unwrap(), vec!["c1"]);
        // populated inside the list wrapper deserializes too
        let mut w = detail_fixture();
        w["deviceList"] = json!(["dX"]);
        let wrapper: SubscriptionList = serde_json::from_value(json!({ "list": [w] })).unwrap();
        assert_eq!(
            wrapper.list[0].device_list.as_deref(),
            Some(&["dX".to_string()][..])
        );
    }

    #[test]
    fn device_receives_respects_tri_state_and_viewpoint() {
        let list = vec!["d1".to_string(), "d2".to_string()];
        assert!(device_receives(Some("d1"), Some(&list), true)); // in
        assert!(!device_receives(Some("d3"), Some(&list), true)); // not in
        assert!(!device_receives(None, Some(&list), true)); // unresolved id + explicit list
        assert!(!device_receives(Some("d1"), Some(&[]), true)); // explicit none
        assert!(device_receives(Some("d1"), None, true)); // buyer + default all
        assert!(device_receives(None, None, true)); // no id needed for default all
        assert!(!device_receives(Some("d1"), None, false)); // provider + null
    }

    #[test]
    fn normalize_str_array_tolerant_of_null_missing_and_non_array() {
        assert_eq!(
            normalize_str_array(Some(&json!(["a", "b"]))),
            vec!["a", "b"]
        );
        assert_eq!(
            normalize_str_array(Some(&serde_json::Value::Null)),
            Vec::<String>::new()
        );
        assert_eq!(normalize_str_array(None), Vec::<String>::new());
        assert_eq!(
            normalize_str_array(Some(&json!("notarray"))),
            Vec::<String>::new()
        );
        // non-string array elements are dropped, not errored.
        assert_eq!(
            normalize_str_array(Some(&json!(["a", 1, null, "b"]))),
            vec!["a", "b"]
        );
    }

    #[test]
    fn normalize_optional_str_array_preserves_null_vs_empty() {
        assert_eq!(normalize_optional_str_array(None), None);
        assert_eq!(
            normalize_optional_str_array(Some(&serde_json::Value::Null)),
            None
        );
        assert_eq!(normalize_optional_str_array(Some(&json!([]))), Some(vec![]));
        assert_eq!(
            normalize_optional_str_array(Some(&json!(["a", 1, "b"]))),
            Some(vec!["a".to_string(), "b".to_string()])
        );
    }

    #[test]
    fn my_subscriptions_struct_parse_tolerates_non_string_array_elements() {
        // The struct (my-subscriptions) parse path must be as tolerant as the
        // raw-Value (subscribe-detail) path: a non-string element drops, and one
        // bad element must NOT fail the whole list parse.
        let mut w = detail_fixture();
        w["deviceList"] = json!(["d1", 2, null, "d2"]);
        w["categoryCodes"] = json!([1, "c1"]);
        let info: SubscriptionInfo = serde_json::from_value(w).unwrap();
        assert_eq!(info.device_list.unwrap(), vec!["d1", "d2"]);
        assert_eq!(info.category_codes.unwrap(), vec!["c1"]);

        let mut w2 = detail_fixture();
        w2["deviceList"] = json!(["dX", 7]);
        let wrapper: SubscriptionList = serde_json::from_value(json!({ "list": [w2] })).unwrap();
        assert_eq!(
            wrapper.list[0].device_list.as_deref(),
            Some(&["dX".to_string()][..])
        );
    }

    #[test]
    fn raw_struct_serialization_preserves_null_device_list() {
        let info: SubscriptionInfo = serde_json::from_value(detail_fixture()).unwrap();
        let out = serde_json::to_value(&info).unwrap();
        assert_eq!(out["thisDeviceReceives"], json!(false));
        assert_eq!(out["deviceList"], serde_json::Value::Null);
    }

    #[test]
    fn derive_device_enrichment_is_the_single_source() {
        // The shared seam both emitters consume: None stays null and means default
        // all for a buyer.
        let none = derive_device_enrichment(None, None, Some("d1"), true);
        assert!(none.device_list.is_none());
        assert!(none.category_codes.is_empty());
        assert!(none.this_device_receives);

        // The same null in a provider view must not claim that provider devices
        // receive the buyer's subscription messages.
        let provider_none = derive_device_enrichment(None, None, Some("d1"), false);
        assert!(provider_none.device_list.is_none());
        assert!(!provider_none.this_device_receives);

        // An explicit empty list means the buyer deliberately disabled all devices.
        let empty = derive_device_enrichment(Some(vec![]), None, Some("d1"), true);
        assert_eq!(empty.device_list, Some(vec![]));
        assert!(!empty.this_device_receives);

        let members = derive_device_enrichment(
            Some(vec!["d1".to_string(), "d2".to_string()]),
            Some(vec!["c1".to_string()]),
            Some("d2"),
            true,
        );
        assert_eq!(
            members.device_list,
            Some(vec!["d1".to_string(), "d2".to_string()])
        );
        assert_eq!(members.category_codes, vec!["c1"]);
        assert!(members.this_device_receives); // this-device in list → true

        // Unresolved this-device id ⇒ false even with a populated list.
        let unresolved = derive_device_enrichment(Some(vec!["d1".to_string()]), None, None, true);
        assert!(!unresolved.this_device_receives);
    }

    #[test]
    fn detail_json_preserves_device_routing_tri_state() {
        let mut historical = detail_fixture();
        historical["deviceList"] = serde_json::Value::Null;
        let historical = enrich_subscription_detail(historical, Some("d1"), true);
        assert_eq!(historical["deviceList"], serde_json::Value::Null);
        assert_eq!(historical["categoryCodes"], json!([]));
        assert_eq!(historical["thisDeviceReceives"], json!(true));

        let mut explicitly_none = detail_fixture();
        explicitly_none["deviceList"] = json!([]);
        let explicitly_none = enrich_subscription_detail(explicitly_none, Some("d1"), true);
        assert_eq!(explicitly_none["deviceList"], json!([]));
        assert_eq!(explicitly_none["thisDeviceReceives"], json!(false));

        let mut selected = detail_fixture();
        selected["deviceList"] = json!(["d1"]);
        let selected = enrich_subscription_detail(selected, Some("d1"), true);
        assert_eq!(selected["deviceList"], json!(["d1"]));
        assert_eq!(selected["thisDeviceReceives"], json!(true));

        let mut provider = detail_fixture();
        provider["deviceList"] = serde_json::Value::Null;
        let provider = enrich_subscription_detail(provider, Some("d1"), false);
        assert_eq!(provider["deviceList"], serde_json::Value::Null);
        assert_eq!(provider["thisDeviceReceives"], json!(false));
    }

    #[test]
    fn list_json_preserves_null_and_derives_buyer_default_all() {
        let mut buyer: SubscriptionInfo = serde_json::from_value(detail_fixture()).unwrap();
        enrich_subscription_info(&mut buyer, Some("d1"), true);
        let buyer = serde_json::to_value(buyer).unwrap();
        assert_eq!(buyer["deviceList"], serde_json::Value::Null);
        assert_eq!(buyer["categoryCodes"], json!([]));
        assert_eq!(buyer["thisDeviceReceives"], json!(true));

        let mut provider: SubscriptionInfo = serde_json::from_value(detail_fixture()).unwrap();
        enrich_subscription_info(&mut provider, Some("d1"), false);
        let provider = serde_json::to_value(provider).unwrap();
        assert_eq!(provider["deviceList"], serde_json::Value::Null);
        assert_eq!(provider["thisDeviceReceives"], json!(false));
    }

    #[test]
    fn human_device_summary_distinguishes_default_all_from_none() {
        assert_eq!(
            format_devices_for_human(None, Some("d1")),
            "all (default — deviceList is not explicitly configured)"
        );
        assert_eq!(
            format_devices_for_human(Some(&[]), Some("d1")),
            "none (no device receives this subscription)"
        );
        assert_eq!(
            format_devices_for_human(
                Some(&["d1-long-id".to_string(), "d2-long-id".to_string()]),
                Some("d1-long-id")
            ),
            "d1-long-(this device), d2-long-"
        );
    }

    #[test]
    fn wire_field_name_constants_match_the_camelcase_struct_fields() {
        // Constants are the single source for the four wire names used by both
        // emitters; keep them aligned with the struct's serde(rename_all) output.
        let mut info: SubscriptionInfo = serde_json::from_value(detail_fixture()).unwrap();
        info.device_list = Some(vec!["d1".to_string()]);
        info.category_codes = Some(vec!["c1".to_string()]);
        let out = serde_json::to_value(&info).unwrap();
        assert!(out.get(FIELD_DEVICE_LIST).is_some());
        assert!(out.get(FIELD_CATEGORY_CODES).is_some());
        assert!(out.get(FIELD_THIS_DEVICE_RECEIVES).is_some());
        assert_eq!(FIELD_THIS_DEVICE_ID, "thisDeviceId");
    }

    #[test]
    fn this_device_name_is_os_name_nonempty_not_id_and_unconditional() {
        let name = this_device_name();
        // Sourced from the cached device-name module (the OS display name), and
        // memoized so a second read is byte-identical.
        assert_eq!(name, crate::device::name::get_cached_device_name());
        // Always non-empty — the module falls back to a placeholder, never "".
        assert!(!name.is_empty());
        // A readable name, never an ellipsized / truncated marker.
        assert!(!name.contains('…') && !name.ends_with("..."));
        // The name column source, a distinct concept from the device id — never
        // the id itself.
        if let Some(id) = crate::device::id::get_cached_device_id() {
            assert_ne!(
                name, id,
                "thisDeviceName must be the readable name, not the id"
            );
        }
        // Emitted unconditionally: independent of whether this device receives and
        // independent of an empty device list. Receipt state varies across these
        // cases; the emitted name does not.
        for (device_list, this_id) in [
            (Some(Vec::<String>::new()), Some("dX")),
            (Some(vec!["dX".to_string()]), Some("dX")),
            (Some(vec!["dOther".to_string()]), Some("dX")),
            (None, Some("dX")),
        ] {
            let enr = derive_device_enrichment(device_list, None, this_id, true);
            let _ = enr.this_device_receives; // may be true or false…
            assert_eq!(this_device_name(), name); // …but the name is the same.
            assert!(!this_device_name().is_empty());
        }
    }
}
