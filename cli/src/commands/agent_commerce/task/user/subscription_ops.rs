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
use crate::commands::agent_commerce::task::common;
use crate::commands::agent_commerce::task::common::network::task_api_client::TaskApiClient;
use crate::commands::agent_commerce::task::common::okx_a2a;
use crate::commands::agent_commerce::task::common::query as common_query;
use crate::commands::agent_commerce::task::common::state_machine::SubStatus;
use crate::commands::agent_commerce::task::common::subscription_identity::select_subscription_agent_id;
use crate::commands::agent_commerce::task::common::{AGENT_ROLE_ASP, AGENT_ROLE_USER};
use crate::commands::agent_commerce::task::signing;
use crate::commands::agentic_wallet::auth::ensure_tokens_refreshed;

// ── active subscription: ensure XMTP session consent with the provider ──
//
// Subscription deliverables arrive as P2P `[intent:deliver]` XMTP messages, which the
// buyer's a2a daemon holds at `consent=0` until the buyer has an established (allowed)
// session with the provider. One-shot tasks open that session during negotiation; the
// subscribe flow has no negotiation. Establish the session for every active subscription.

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

#[derive(Clone, Copy)]
enum SubscriptionMutation {
    Cancel,
    Reject,
}

fn should_watch_after_subscription_mutation(mutation: SubscriptionMutation) -> bool {
    // Cancel-result events carry only the subscription job id, not an immutable
    // operation id. Starting a new watch could mistake stale backlog for this
    // cancellation. Rejection keeps its existing scoped watch behavior.
    matches!(mutation, SubscriptionMutation::Reject)
}

fn print_watch_after_subscription_mutation(mutation: SubscriptionMutation, sub_id: &str) {
    if !super::content::is_cli_mode() || !should_watch_after_subscription_mutation(mutation) {
        return;
    }
    println!();
    println!("{}", super::content::scoped_watch_handoff(sub_id));
}

// ── subscribe-cancel ────────────────────────────────────────────────────

pub async fn handle_subscribe_cancel(client: &mut TaskApiClient, sub_id: &str) -> Result<()> {
    ensure_tokens_refreshed().await?;
    let (user_agent_id, _) = resolve_user_agent().await?;
    let user_agent_id = select_subscription_agent_id(&user_agent_id, "")?;
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

    // A card created by a pre-upgrade delivery must not resurface from the
    // outstanding queue after cancellation. This clears only the retired
    // delivery-time mode selector and leaves unrelated decisions untouched.
    let _ = okx_a2a::mark_retired_autotrade_mode_decisions_handled(sub_id);

    if super::content::is_cli_mode() {
        println!();
        println!("{}", super::content::scoped_watch_handoff(sub_id));
    }

    Ok(())
}

// ── start-autorenew ─────────────────────────────────────────────────────

pub async fn handle_start_autorenew(client: &mut TaskApiClient, sub_id: &str) -> Result<()> {
    ensure_tokens_refreshed().await?;
    let (user_agent_id, _) = resolve_user_agent().await?;
    let user_agent_id = select_subscription_agent_id(&user_agent_id, "")?;
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

    if confirm_resp.is_null() || confirm_resp.as_object().is_none_or(|o| o.is_empty()) {
        bail!("providerConfirmStatus returned empty terms");
    }

    let typed_data = &confirm_resp["typedData"];
    if typed_data.is_null() || typed_data.as_object().is_none_or(|o| o.is_empty()) {
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

/// Inner implementation — caller has already validated reason and resolved agent_id.
pub(crate) async fn handle_subscribe_reject_inner(
    client: &mut TaskApiClient,
    sub_id: &str,
    reason: &str,
    user_agent_id: &str,
) -> Result<String> {
    let user_agent_id = select_subscription_agent_id(user_agent_id, "")?;
    let (account_id, address) = signing::resolve_wallet_by_agent_id(&user_agent_id).await?;

    let resp = client
        .post_with_identity(
            &format!("{SUBSCRIBE_API_PREFIX}/{sub_id}/reject"),
            &serde_json::json!({}),
            &user_agent_id,
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
        &user_agent_id,
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

    Ok(tx_hash)
}

/// Disabled legacy CLI entry. Refund owns paid subscription rejection.
pub async fn handle_subscribe_reject(
    client: &mut TaskApiClient,
    sub_id: &str,
    reason: &str,
) -> Result<()> {
    let _ = (client, reason);
    bail!(
        "direct subscribe-reject is disabled by Refund; run `onchainos agent refund-prepare {sub_id} --reason <user-authored-reason>` and execute only the returned confirmed action"
    )
}

// ── subscribe-detail ────────────────────────────────────────────────────

pub async fn handle_subscribe_detail(
    client: &mut TaskApiClient,
    sub_id: &str,
    format: &str,
) -> Result<()> {
    ensure_tokens_refreshed().await?;

    let user_agent_id = signing::resolve_agent_id_by_role(AGENT_ROLE_USER)
        .await
        .unwrap_or_default();
    let asp_agent_id = if user_agent_id.trim().is_empty() {
        signing::resolve_agent_id_by_role(AGENT_ROLE_ASP)
            .await
            .unwrap_or_default()
    } else {
        String::new()
    };
    let agent_id = select_subscription_agent_id(&user_agent_id, &asp_agent_id)?;

    let json_mode = format.eq_ignore_ascii_case("json");

    let resp = fetch_subscribe_detail_for_agent(client, sub_id, &agent_id).await?;
    let is_buyer = !agent_id.is_empty() && resp["buyerAgentId"].as_str() == Some(agent_id.as_str());

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
        let display_facts = resolve_subscription_display_facts(&resp).await?;
        let enriched = enrich_subscription_detail(
            resp,
            crate::device::id::get_cached_device_id(),
            is_buyer,
            &display_facts,
        );
        crate::output::success(enriched);
        return Ok(());
    }

    let title = resp["title"].as_str().unwrap_or("?");
    let status = resp["status"].as_i64().unwrap_or(-1);
    let trial_type = resp["trialType"].as_i64().unwrap_or(0);
    let auto_renew = resp["autoRenew"].as_i64().unwrap_or(0);
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
        status_label(status)
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

/// Fetch a subscription's canonical detail for an already-resolved agent.
///
/// `my-subscriptions` is a compact listing and older backend rows may omit
/// `serviceDescription`. Watch authorization prechecks use this read-only
/// detail query before deciding whether an existing subscription is executable.
pub(crate) async fn fetch_subscribe_detail_for_agent(
    client: &mut TaskApiClient,
    sub_id: &str,
    agent_id: &str,
) -> Result<serde_json::Value> {
    client
        .get_with_identity(&format!("{SUBSCRIBE_API_PREFIX}/{sub_id}"), agent_id)
        .await
        .map_err(|e| anyhow::anyhow!("subscribe-detail failed: {e}"))
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
    let agent_id = select_subscription_agent_id(&agent_id, "")?;
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
    #[serde(skip_deserializing)]
    pub status_label: String,
    #[serde(skip_deserializing)]
    pub status_description: String,
    pub chain_id: i64,
    pub title: String,
    pub description: String,
    pub description_summary: String,
    pub buyer_agent_id: String,
    pub buyer_agent_address: String,
    pub provider_agent_id: String,
    pub provider_agent_address: String,
    pub trial_type: i64,
    #[serde(rename = "trialStartTime", alias = "trailStartTime")]
    pub trail_start_time: Option<i64>,
    #[serde(rename = "trialEndTime", alias = "trailEndTime")]
    pub trail_end_time: Option<i64>,
    pub sub_start_time: Option<i64>,
    pub sub_end_time: Option<i64>,
    pub sub_buffer_end_time: Option<i64>,
    pub auto_renew: i64,
    pub copy_trade: i64,
    pub period_index: Option<i64>,
    pub service_id: String,
    /// Canonical ASP service description when the subscription API includes it.
    /// Older compact-list rows may omit this field; authorization prechecks
    /// recover it from subscription detail or the ASP service catalog instead
    /// of guessing from the user's task description.
    #[serde(skip_serializing_if = "String::is_empty")]
    pub service_description: String,
    pub service_params: String,
    pub service_token_address: String,
    pub service_token_amount: String,
    pub payment_token_address: String,
    pub payment_token_amount: String,
    pub payment_currency_amount: String,
    pub offline_receive_flag: i64,
    pub role: String,
    pub has_feed_back: bool,
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
    #[serde(default)]
    total: u64,
    #[serde(default, rename = "totalNoCondition")]
    total_no_condition: Option<u64>,
    #[serde(default)]
    page: Option<u32>,
    #[serde(default, rename = "pageSize")]
    page_size: Option<u32>,
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

/// Minimal machine-readable view used by subscription creation prechecks.
/// Only non-terminal buyer subscriptions are ever exposed through this type.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ExistingSubscriptionSummary {
    pub(crate) job_id: String,
    pub(crate) service_id: String,
    pub(crate) provider_agent_id: String,
    /// Raw backend state retained for compatibility with internal callers only.
    #[serde(skip_serializing)]
    pub(crate) status_name: String,
    /// CLI-owned business wording for any user-facing duplicate-subscription card.
    pub(crate) status_label: String,
    pub(crate) status_description: String,
    pub(crate) restore_listening_available: bool,
    /// Retained for preparation-time confirmation cards, but deliberately
    /// omitted from the create-subscribe duplicate error contract.
    #[serde(skip_serializing)]
    pub(crate) title: String,
    /// Raw backend status used by task-create-prepare so its decision matches
    /// the write-boundary duplicate check exactly.
    #[serde(skip_serializing)]
    pub(crate) status: i64,
}

fn blocks_duplicate_creation(status: i64) -> bool {
    // Unknown future states fail closed: SubStatus::from_code intentionally
    // maps them to Init, which remains blocking. Expired is safe for duplicate
    // creation even when settlement for the old job is still pending; that
    // settlement continues through the old job's reconciliation flow.
    !matches!(
        SubStatus::from_code(status),
        SubStatus::Completed | SubStatus::Closed | SubStatus::Expired | SubStatus::Failed
    )
}

fn summarize_non_terminal_buyer_subscriptions(
    list: Vec<SubscriptionInfo>,
    buyer_agent_id: &str,
) -> Vec<ExistingSubscriptionSummary> {
    let mut summaries = list
        .into_iter()
        .filter(|item| item.buyer_agent_id == buyer_agent_id)
        .filter(|item| blocks_duplicate_creation(item.status))
        .map(|item| ExistingSubscriptionSummary {
            job_id: item.job_id,
            service_id: item.service_id,
            provider_agent_id: item.provider_agent_id,
            status_name: status_name(item.status),
            status_label: status_label(item.status).to_string(),
            status_description: status_description(item.status).to_string(),
            restore_listening_available: item.status == SubStatus::Active.code(),
            title: item.title,
            status: item.status,
        })
        .collect::<Vec<_>>();

    // Historical duplicate rows can exist. Surface ACTIVE first because it is
    // the only status for which the product may offer "Restore listening".
    summaries.sort_by_key(|item| (!item.restore_listening_available, item.job_id.clone()));
    summaries
}

/// Read all subscriptions that block duplicate creation for an already-resolved buyer.
/// Unlike the user-facing listing, this precheck does not create sessions or
/// alter device routing.
pub(crate) async fn fetch_non_terminal_buyer_subscriptions_for_agent(
    client: &mut TaskApiClient,
    buyer_agent_id: &str,
) -> Result<Vec<ExistingSubscriptionSummary>> {
    let buyer_agent_id = select_subscription_agent_id(buyer_agent_id, "")?;
    let data = client
        .get_with_agent_id(&my_subscriptions_path(), &buyer_agent_id)
        .await
        .map_err(|e| anyhow!("failed to check existing subscriptions: {e}"))?;
    let wrapper: SubscriptionList = serde_json::from_value(data)
        .map_err(|e| anyhow!("failed to parse existing subscriptions: {e}"))?;
    Ok(summarize_non_terminal_buyer_subscriptions(
        wrapper.list,
        &buyer_agent_id,
    ))
}

pub(crate) fn existing_subscription_for_service<'a>(
    subscriptions: &'a [ExistingSubscriptionSummary],
    service_id: &str,
) -> Option<&'a ExistingSubscriptionSummary> {
    subscriptions
        .iter()
        .find(|item| item.service_id == service_id)
}

pub fn status_name(status: i64) -> String {
    match status {
        -1 => "INIT".to_string(),
        0 => "CREATED".to_string(),
        1 => "ACTIVE".to_string(),
        3 => "REJECTED".to_string(),
        4 => "DISPUTED".to_string(),
        6 => "COMPLETED".to_string(),
        7 => "CLOSED".to_string(),
        8 => "EXPIRED".to_string(),
        9 => "FAILED".to_string(),
        n => format!("UNKNOWN_{n}"),
    }
}

pub fn status_label(status: i64) -> &'static str {
    match status {
        -1 => "Initializing",
        0 => "Awaiting ASP acceptance",
        1 => "Active",
        3 => "Awaiting ASP decision",
        4 => "Evaluation in progress",
        6 => "Completed",
        7 => "Closed",
        8 => "Expired",
        9 => "Refund completed",
        _ => "Status unavailable",
    }
}

pub fn status_description(status: i64) -> &'static str {
    match status {
        -1 => "The subscription record was created and is awaiting on-chain confirmation.",
        0 => "The subscription is waiting for an ASP to accept it.",
        1 => "The subscription is active.",
        3 => "The buyer rejected the current delivery and is waiting for the ASP's decision.",
        4 => "The refund request is in Evaluation.",
        6 => "The subscription completed without a refund.",
        7 => "The subscription is closed.",
        8 => "The subscription expired.",
        9 => "The refund completed successfully.",
        _ => "The subscription status is currently unavailable.",
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
        "CREATED" => Ok(0),
        "ACTIVE" => Ok(1),
        "REJECTED" => Ok(3),
        "DISPUTED" => Ok(4),
        "COMPLETED" => Ok(6),
        "CLOSED" => Ok(7),
        "EXPIRED" => Ok(8),
        "FAILED" => Ok(9),
        _ => Err(format!(
            "invalid status '{s}': expected a code (-1/0/1/3/4/6/7/8/9) or a name \
             (INIT/CREATED/ACTIVE/REJECTED/DISPUTED/COMPLETED/CLOSED/EXPIRED/FAILED)"
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

#[derive(Debug, Default)]
struct SubscriptionDisplayFacts {
    provider_name: Option<String>,
    token_symbol: Option<String>,
    supports_trial: Option<bool>,
    trial_hours: Option<i64>,
}

fn display_string(value: Option<&serde_json::Value>) -> Option<String> {
    let value = value?;
    value
        .as_str()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
        .or_else(|| value.as_i64().map(|value| value.to_string()))
        .or_else(|| value.as_u64().map(|value| value.to_string()))
}

fn positive_trial_hours(value: Option<&serde_json::Value>) -> Option<i64> {
    let hours = value
        .and_then(|value| {
            value
                .as_i64()
                .or_else(|| value.as_str()?.trim().parse::<i64>().ok())
        })
        .filter(|hours| *hours > 0)?;
    Some(hours)
}

fn catalog_trial_facts(service: Option<&serde_json::Value>) -> (Option<bool>, Option<i64>) {
    let Some(service) = service else {
        return (None, None);
    };
    let trial_hours = positive_trial_hours(service.get("freeTrial")).or_else(|| {
        positive_trial_hours(
            service
                .get("subscriptionInfo")
                .and_then(|value| value.get("freeTrial")),
        )
    });
    let explicit = service
        .get("supportTrial")
        .and_then(serde_json::Value::as_bool)
        .or_else(|| {
            service
                .get("subscriptionInfo")
                .and_then(|value| value.get("supportTrial"))
                .and_then(serde_json::Value::as_bool)
        });
    (explicit.or(Some(trial_hours.is_some())), trial_hours)
}

async fn resolve_subscription_display_facts(
    detail: &serde_json::Value,
) -> Result<SubscriptionDisplayFacts> {
    let provider_agent_id = display_string(detail.get("providerAgentId"));
    let service_id = display_string(detail.get("serviceId"));
    let catalog_service = match (provider_agent_id.as_deref(), service_id.as_deref()) {
        (Some(provider_agent_id), Some(service_id)) => {
            common::find_service(provider_agent_id, service_id).await?
        }
        _ => None,
    };

    let provider_name = ["providerAgentName", "aspAgentName", "providerName"]
        .into_iter()
        .find_map(|key| display_string(detail.get(key)))
        .or_else(|| {
            catalog_service
                .as_ref()
                .and_then(|service| display_string(service.get("providerAgentName")))
        });
    let provider_name = match (provider_name, provider_agent_id.as_deref()) {
        (Some(name), _) => Some(name),
        (None, Some(provider_agent_id)) => {
            common::fetch_agent_profile(provider_agent_id).await.name
        }
        (None, None) => None,
    };

    let token_symbol = ["serviceTokenSymbol", "tokenSymbol", "paymentTokenSymbol"]
        .into_iter()
        .find_map(|key| display_string(detail.get(key)));
    let token_symbol = match token_symbol {
        Some(symbol) => Some(symbol),
        None => {
            let token_address = display_string(detail.get("serviceTokenAddress"));
            match token_address.as_deref() {
                Some(address) => Some(
                    common::util::resolve_token_symbol_by_address(
                        common::XLAYER_CHAIN_INDEX,
                        address,
                    )
                    .await?,
                ),
                None => None,
            }
        }
    };
    let (catalog_supports_trial, catalog_trial_hours) =
        catalog_trial_facts(catalog_service.as_ref());
    let inline_trial_hours = positive_trial_hours(detail.get("freeTrial"));
    let supports_trial = detail
        .get("supportTrial")
        .and_then(serde_json::Value::as_bool)
        .or(catalog_supports_trial)
        .or_else(|| {
            (detail.get("trialType").and_then(serde_json::Value::as_i64) == Some(1)).then_some(true)
        });

    Ok(SubscriptionDisplayFacts {
        provider_name,
        token_symbol,
        supports_trial,
        trial_hours: inline_trial_hours.or(catalog_trial_hours),
    })
}

fn trial_duration_label(hours: i64) -> String {
    if hours % 24 == 0 {
        let days = hours / 24;
        format!("{days}-day")
    } else {
        format!("{hours}-hour")
    }
}

fn subscription_fee_label(amount: Option<&str>, symbol: Option<&str>) -> Option<String> {
    let amount = amount.map(str::trim).filter(|value| !value.is_empty())?;
    if super::refund::is_zero_decimal(amount) {
        return Some("Free".to_string());
    }
    let symbol = symbol.map(str::trim).filter(|value| !value.is_empty())?;
    Some(format!("{amount} {symbol} / month"))
}

fn free_trial_label(
    detail: &serde_json::Value,
    facts: &SubscriptionDisplayFacts,
) -> Option<String> {
    match detail.get("trialType").and_then(serde_json::Value::as_i64) {
        Some(1) => {
            let (trial_start, trial_end) = trial_window(detail);
            let derived_hours = (trial_start > 0 && trial_end > trial_start)
                .then_some((trial_end - trial_start) / 3600)
                .filter(|hours| *hours > 0);
            let hours = facts.trial_hours.or(derived_hours)?;
            let first_charge = common::deadline::format_utc_timestamp(trial_end)?;
            let amount = detail
                .get("serviceTokenAmount")
                .and_then(serde_json::Value::as_str)?;
            let symbol = facts.token_symbol.as_deref()?;
            Some(format!(
                "{} free trial. The first subscription fee of {} {} will be charged at {}.",
                trial_duration_label(hours),
                amount,
                symbol,
                first_charge,
            ))
        }
        Some(0) if facts.supports_trial == Some(true) => Some(
            "You have already used the free trial for this service. The subscription fee is charged directly."
                .to_string(),
        ),
        Some(0) if facts.supports_trial == Some(false) => {
            Some("Free trial is not supported.".to_string())
        }
        _ => None,
    }
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
    display_facts: &SubscriptionDisplayFacts,
) -> serde_json::Value {
    if let Some(obj) = detail.as_object_mut() {
        let code = obj.get("status").and_then(|v| v.as_i64()).unwrap_or(-1);
        obj.insert(
            "statusName".to_string(),
            serde_json::Value::String(status_name(code)),
        );
        obj.insert(
            "statusLabel".to_string(),
            serde_json::Value::String(status_label(code).to_string()),
        );
        obj.insert(
            "statusDescription".to_string(),
            serde_json::Value::String(status_description(code).to_string()),
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
        obj.insert(
            "autoRenewLabel".to_string(),
            serde_json::Value::String(
                match obj.get("autoRenew").and_then(serde_json::Value::as_i64) {
                    Some(1) => "Enabled",
                    Some(0) => "Disabled",
                    _ => "—",
                }
                .to_string(),
            ),
        );
        let billing_period_label =
            if obj.get("trialType").and_then(serde_json::Value::as_i64) == Some(1) {
                "Trial Period".to_string()
            } else {
                obj.get("periodIndex")
                    .and_then(serde_json::Value::as_i64)
                    .filter(|period| *period > 0)
                    .map(|period| format!("Billing Period {period}"))
                    .unwrap_or_else(|| "—".to_string())
            };
        obj.insert(
            "billingPeriodLabel".to_string(),
            serde_json::Value::String(billing_period_label),
        );
        obj.insert(
            "offlineMessageHandlingLabel".to_string(),
            serde_json::Value::String(
                match obj
                    .get("offlineReceiveFlag")
                    .and_then(serde_json::Value::as_i64)
                {
                    Some(1) => "Clear",
                    Some(0) => "Resume delivery when back online",
                    _ => "—",
                }
                .to_string(),
            ),
        );
        obj.insert(
            "receiveOnThisDeviceLabel".to_string(),
            serde_json::Value::String(
                if enrichment.this_device_receives {
                    "Receive"
                } else {
                    "Do not receive"
                }
                .to_string(),
            ),
        );
        obj.insert(
            "providerName".to_string(),
            display_facts
                .provider_name
                .clone()
                .map(serde_json::Value::String)
                .unwrap_or(serde_json::Value::Null),
        );
        let provider_label = display_facts
            .provider_name
            .as_deref()
            .zip(
                obj.get("providerAgentId")
                    .and_then(serde_json::Value::as_str),
            )
            .map(|(name, id)| format!("{name} ({id})"));
        obj.insert(
            "serviceProviderLabel".to_string(),
            provider_label
                .map(serde_json::Value::String)
                .unwrap_or(serde_json::Value::Null),
        );
        obj.insert(
            "feeTokenSymbol".to_string(),
            display_facts
                .token_symbol
                .clone()
                .map(serde_json::Value::String)
                .unwrap_or(serde_json::Value::Null),
        );
        let fee_label = subscription_fee_label(
            obj.get("serviceTokenAmount")
                .and_then(serde_json::Value::as_str),
            display_facts.token_symbol.as_deref(),
        );
        obj.insert(
            "feeLabel".to_string(),
            fee_label
                .map(serde_json::Value::String)
                .unwrap_or(serde_json::Value::Null),
        );
        let trial_label = free_trial_label(&serde_json::Value::Object(obj.clone()), display_facts);
        obj.insert(
            "freeTrialLabel".to_string(),
            trial_label
                .map(serde_json::Value::String)
                .unwrap_or(serde_json::Value::Null),
        );
        let mut missing = Vec::new();
        for (field, key) in [
            ("Service Provider", "serviceProviderLabel"),
            ("Free Trial", "freeTrialLabel"),
            ("Fee", "feeLabel"),
        ] {
            if obj.get(key).is_none_or(serde_json::Value::is_null) {
                missing.push(serde_json::Value::String(field.to_string()));
            }
        }
        obj.insert(
            "displayReady".to_string(),
            serde_json::Value::Bool(missing.is_empty()),
        );
        obj.insert(
            "displayMissingFields".to_string(),
            serde_json::Value::Array(missing),
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
    item.status_label = status_label(item.status).to_string();
    item.status_description = status_description(item.status).to_string();
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

/// Prepare one paginated buyer subscription response for the unified task list.
/// The backend owns grouping and totals; this adapter only preserves the existing
/// buyer filtering and device/status enrichment used by `my-subscriptions`.
pub(crate) fn enrich_buyer_subscription_page(
    data: serde_json::Value,
    agent_id: &str,
) -> Result<serde_json::Value> {
    let wrapper: SubscriptionList = serde_json::from_value(data)
        .map_err(|e| anyhow!("failed to parse subscription page: {e}"))?;
    let this_device_id = crate::device::id::get_cached_device_id();
    let mut list = filter_subscriptions(wrapper.list, SubscriptionRole::Buyer, agent_id, None);
    for item in &mut list {
        enrich_subscription_info(item, this_device_id, true);
    }

    let mut page = serde_json::Map::new();
    page.insert("list".to_string(), serde_json::json!(list));
    page.insert("total".to_string(), serde_json::json!(wrapper.total));
    if let Some(total_no_condition) = wrapper.total_no_condition {
        page.insert(
            "totalNoCondition".to_string(),
            serde_json::json!(total_no_condition),
        );
    }
    if let Some(page_number) = wrapper.page {
        page.insert("page".to_string(), serde_json::json!(page_number));
    }
    if let Some(page_size) = wrapper.page_size {
        page.insert("pageSize".to_string(), serde_json::json!(page_size));
    }
    page.insert(
        FIELD_THIS_DEVICE_ID.to_string(),
        serde_json::json!(this_device_id),
    );
    page.insert(
        FIELD_THIS_DEVICE_NAME.to_string(),
        serde_json::json!(this_device_name()),
    );
    Ok(serde_json::Value::Object(page))
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
    fetch_my_subscriptions_snapshot_for_agent_with_mode(client, role, status, header_agent, true)
        .await
}

pub(crate) async fn fetch_my_subscriptions_snapshot_for_agent_read_only(
    client: &mut TaskApiClient,
    role: SubscriptionRole,
    status: Option<i32>,
    header_agent: String,
) -> Result<MySubscriptionsSnapshot> {
    fetch_my_subscriptions_snapshot_for_agent_with_mode(client, role, status, header_agent, false)
        .await
}

async fn fetch_my_subscriptions_snapshot_for_agent_with_mode(
    client: &mut TaskApiClient,
    role: SubscriptionRole,
    status: Option<i32>,
    header_agent: String,
    establish_sessions: bool,
) -> Result<MySubscriptionsSnapshot> {
    let header_agent = select_subscription_agent_id(&header_agent, "")?;

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
    if establish_sessions && matches!(role, SubscriptionRole::Buyer) {
        for item in &list {
            if should_ensure_subscription_session(item.status) {
                ensure_subscription_session(&item.job_id, &header_agent, &item.provider_agent_id);
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

    #[test]
    fn enrich_buyer_subscription_page_preserves_total_and_device_fields() {
        let data = serde_json::json!({
            "total": 2,
            "totalNoCondition": 7,
            "page": 2,
            "pageSize": 20,
            "list": [{
                "jobId": "sub-1",
                "status": 1,
                "buyerAgentId": "user-1",
                "providerAgentId": "asp-1",
                "deviceList": null,
                "categoryCodes": null
            }]
        });

        let page = enrich_buyer_subscription_page(data, "user-1").unwrap();

        assert_eq!(page["total"], 2);
        assert_eq!(page["totalNoCondition"], 7);
        assert_eq!(page["page"], 2);
        assert_eq!(page["pageSize"], 20);
        assert_eq!(page["list"][0]["statusName"], "ACTIVE");
        assert_eq!(page["list"][0]["statusLabel"], "Active");
        assert_eq!(
            page["list"][0]["statusDescription"],
            "The subscription is active."
        );
        assert!(page["list"][0]["deviceList"].is_null());
        assert_eq!(page["list"][0]["categoryCodes"], serde_json::json!([]));
        assert_eq!(page["list"][0]["thisDeviceReceives"], true);
        assert!(page.get("thisDeviceId").is_some());
        assert!(page.get("thisDeviceName").is_some());
    }

    #[test]
    fn enrich_buyer_subscription_page_filters_other_buyers_without_rewriting_total() {
        let data = serde_json::json!({
            "total": 9,
            "list": [
                {
                    "jobId": "mine",
                    "buyerAgentId": "user-1",
                    "providerAgentId": "asp-1"
                },
                {
                    "jobId": "theirs",
                    "buyerAgentId": "user-2",
                    "providerAgentId": "asp-2"
                }
            ]
        });

        let page = enrich_buyer_subscription_page(data, "user-1").unwrap();

        assert_eq!(page["total"], 9);
        assert_eq!(page["list"].as_array().unwrap().len(), 1);
        assert_eq!(page["list"][0]["jobId"], "mine");
    }
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
        assert!(!should_ensure_subscription_session(
            SubStatus::Closed.code()
        ));
        assert!(!should_ensure_subscription_session(
            SubStatus::Failed.code()
        ));
        assert!(!should_ensure_subscription_session(
            SubStatus::Expired.code()
        ));
    }

    #[test]
    fn duplicate_creation_allows_expired_and_terminal_statuses() {
        for status in [-1, 1, 3, 4, 42] {
            assert!(
                blocks_duplicate_creation(status),
                "status {status} must block duplicate creation"
            );
        }
        for status in [6, 7, 8, 9] {
            assert!(
                !blocks_duplicate_creation(status),
                "non-blocking status {status} must allow a new subscription"
            );
        }
    }

    #[test]
    fn duplicate_summary_prefers_active_and_only_active_can_restore_listening() {
        let row = |job_id: &str, service_id: &str, buyer: &str, status: i64| SubscriptionInfo {
            job_id: job_id.to_string(),
            service_id: service_id.to_string(),
            buyer_agent_id: buyer.to_string(),
            provider_agent_id: "asp-1".to_string(),
            status,
            ..SubscriptionInfo::default()
        };
        let summaries = summarize_non_terminal_buyer_subscriptions(
            vec![
                row("job-rejected", "svc-1", "buyer-1", 3),
                row("job-active", "svc-1", "buyer-1", 1),
                row("job-closed", "svc-1", "buyer-1", 7),
                row("job-other-buyer", "svc-1", "buyer-2", 1),
            ],
            "buyer-1",
        );

        assert_eq!(summaries.len(), 2);
        assert_eq!(summaries[0].job_id, "job-active");
        assert!(summaries[0].restore_listening_available);
        assert_eq!(summaries[1].job_id, "job-rejected");
        assert!(!summaries[1].restore_listening_available);
        assert_eq!(
            existing_subscription_for_service(&summaries, "svc-1")
                .expect("service must be blocked")
                .job_id,
            "job-active"
        );
        assert!(existing_subscription_for_service(&summaries, "svc-other").is_none());
    }

    #[test]
    fn subscribe_cancel_does_not_rearm_scoped_watch() {
        assert!(!should_watch_after_subscription_mutation(
            SubscriptionMutation::Cancel
        ));
        assert!(should_watch_after_subscription_mutation(
            SubscriptionMutation::Reject
        ));
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
            "serviceDescription": "Spot trading signals with executable buy and sell entries",
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
        assert_eq!(info.period_index, Some(1));
        assert_eq!(info.service_id, "svc-1");
        assert_eq!(
            info.service_description,
            "Spot trading signals with executable buy and sell entries"
        );
        assert_eq!(info.service_params, "{\"k\":\"v\"}");
    }

    #[test]
    fn subscription_info_preserves_current_copy_trade_field() {
        let wire = detail_fixture();
        assert!(wire.get("copyTrade").is_some());

        let info: SubscriptionInfo = serde_json::from_value(wire).unwrap();
        let list_output = serde_json::to_value(info).unwrap();
        assert_eq!(list_output["copyTrade"], 0);
    }

    #[test]
    fn current_trial_field_names_are_emitted_with_legacy_input_compatibility() {
        let info: SubscriptionInfo = serde_json::from_value(detail_fixture()).unwrap();
        let output = serde_json::to_value(info).unwrap();
        assert_eq!(output["trialStartTime"], 1_700_000_000i64);
        assert_eq!(output["trialEndTime"], 1_700_600_000i64);
        assert!(output.get("trailStartTime").is_none());
        assert!(output.get("trailEndTime").is_none());
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
        for code in [-1i64, 0, 1, 3, 4, 6, 7, 8, 9] {
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
    fn status_name_covers_all_documented_codes_and_unknown() {
        assert_eq!(status_name(-1), "INIT");
        assert_eq!(status_name(0), "CREATED");
        assert_eq!(status_name(1), "ACTIVE");
        assert_eq!(status_name(3), "REJECTED");
        assert_eq!(status_name(4), "DISPUTED");
        assert_eq!(status_name(6), "COMPLETED");
        assert_eq!(status_name(7), "CLOSED");
        assert_eq!(status_name(8), "EXPIRED");
        assert_eq!(status_name(9), "FAILED");
        assert_eq!(status_name(2), "UNKNOWN_2");
        assert_eq!(status_name(42), "UNKNOWN_42");
    }

    #[test]
    fn subscription_status_display_uses_business_meaning() {
        assert_eq!(status_label(0), "Awaiting ASP acceptance");
        assert_eq!(
            status_description(0),
            "The subscription is waiting for an ASP to accept it."
        );
        assert_eq!(status_label(1), "Active");
        assert_eq!(status_label(3), "Awaiting ASP decision");
        assert_eq!(status_label(4), "Evaluation in progress");
        assert_eq!(status_label(6), "Completed");
        assert_eq!(status_label(7), "Closed");
        assert_eq!(status_label(9), "Refund completed");
        assert_eq!(status_description(9), "The refund completed successfully.");
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

    #[tokio::test]
    async fn subscription_detail_transport_rejects_blank_agentic_id_before_request() {
        let mut client = TaskApiClient::new();

        let result = client.fetch_subscription("subscription-1", "   ").await;
        let error = match result {
            Ok(_) => panic!("blank agenticId must be rejected before subscription detail HTTP"),
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
        let historical = enrich_subscription_detail(
            historical,
            Some("d1"),
            true,
            &SubscriptionDisplayFacts::default(),
        );
        assert_eq!(historical["deviceList"], serde_json::Value::Null);
        assert_eq!(historical["categoryCodes"], json!([]));
        assert_eq!(historical["thisDeviceReceives"], json!(true));

        let mut explicitly_none = detail_fixture();
        explicitly_none["deviceList"] = json!([]);
        let explicitly_none = enrich_subscription_detail(
            explicitly_none,
            Some("d1"),
            true,
            &SubscriptionDisplayFacts::default(),
        );
        assert_eq!(explicitly_none["deviceList"], json!([]));
        assert_eq!(explicitly_none["thisDeviceReceives"], json!(false));

        let mut selected = detail_fixture();
        selected["deviceList"] = json!(["d1"]);
        let selected = enrich_subscription_detail(
            selected,
            Some("d1"),
            true,
            &SubscriptionDisplayFacts::default(),
        );
        assert_eq!(selected["deviceList"], json!(["d1"]));
        assert_eq!(selected["thisDeviceReceives"], json!(true));

        let mut provider = detail_fixture();
        provider["deviceList"] = serde_json::Value::Null;
        let provider = enrich_subscription_detail(
            provider,
            Some("d1"),
            false,
            &SubscriptionDisplayFacts::default(),
        );
        assert_eq!(provider["deviceList"], serde_json::Value::Null);
        assert_eq!(provider["thisDeviceReceives"], json!(false));
    }

    #[test]
    fn detail_json_adds_authoritative_display_labels() {
        let mut detail = detail_fixture();
        detail["trialType"] = json!(0);
        detail["periodIndex"] = json!(2);
        detail["autoRenew"] = json!(1);
        detail["offlineReceiveFlag"] = json!(1);
        detail["deviceList"] = json!(["d1"]);

        let detail = enrich_subscription_detail(
            detail,
            Some("d1"),
            true,
            &SubscriptionDisplayFacts {
                provider_name: Some("Provider".to_string()),
                token_symbol: Some("USDT".to_string()),
                supports_trial: Some(true),
                trial_hours: Some(24),
            },
        );

        assert_eq!(detail["autoRenewLabel"], "Enabled");
        assert_eq!(detail["billingPeriodLabel"], "Billing Period 2");
        assert_eq!(detail["offlineMessageHandlingLabel"], "Clear");
        assert_eq!(detail["receiveOnThisDeviceLabel"], "Receive");
        assert_eq!(detail["serviceProviderLabel"], "Provider (2002)");
        assert_eq!(detail["feeLabel"], "10.500000 USDT / month");
        assert_eq!(
            detail["freeTrialLabel"],
            "You have already used the free trial for this service. The subscription fee is charged directly."
        );
        assert_eq!(detail["displayReady"], true);
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
