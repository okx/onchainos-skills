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

use crate::audit;
use crate::commands::agentic_wallet::auth::ensure_tokens_refreshed;
use crate::commands::agent_commerce::task::common::network::task_api_client::TaskApiClient;
use crate::commands::agent_commerce::task::common::okx_a2a;
use crate::commands::agent_commerce::task::common::query as common_query;
use crate::commands::agent_commerce::task::common::state_machine::SubStatus;
use crate::commands::agent_commerce::task::common::{AGENT_ROLE_USER, AGENT_ROLE_ASP};
use crate::commands::agent_commerce::task::signing;
use super::create::resolve_user_agent;
use super::create_subscribe::SUBSCRIBE_API_PREFIX;

// ── copy-trade subscription: ensure XMTP session consent with the provider ──
//
// Copy-trade signals arrive as P2P `[intent:deliver]` XMTP messages, which the buyer's
// a2a daemon holds at `consent=0` until the buyer has an established (allowed) session
// with the provider. One-shot tasks open that session during negotiation; the subscribe
// flow has no negotiation, so consent is never granted and every signal is held → the
// three-way consent card never fires. We establish the session (idempotently, per device)
// so held/future signals are dispatched. Gated to `copyTrade` subscriptions; best-effort.

/// `<onchainos_home>/subscription/consent/<jobId>` — per-device "already established"
/// marker. `None` if `job_id` fails the path-safety charset check.
fn consent_marker_path(job_id: &str) -> Option<std::path::PathBuf> {
    if job_id.is_empty()
        || !job_id.chars().all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
    {
        return None;
    }
    let home = crate::home::onchainos_home().ok()?;
    Some(home.join("subscription").join("consent").join(job_id))
}

/// Idempotently ensure the buyer has a consented XMTP session with `provider_agent_id`
/// for this copy-trade subscription. No-op unless `is_copy_trade`. Safe to call from any
/// device and repeatedly (a per-device marker avoids re-sending). Never fails the caller.
pub(crate) fn ensure_subscription_consent(
    job_id: &str,
    my_agent_id: &str,
    provider_agent_id: &str,
    is_copy_trade: bool,
) {
    if !is_copy_trade || my_agent_id.is_empty() {
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
            "[SUB_CONSENT] copy-trade subscription session established.",
        );
        let _ = crate::home::write_secure(&marker, b"1");
    }
}

// ── subscribe-cancel ────────────────────────────────────────────────────

pub async fn handle_subscribe_cancel(
    client: &mut TaskApiClient,
    sub_id: &str,
) -> Result<()> {
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
        client, &resp["uopData"], &account_id, &address, sub_id, biz_type, &user_agent_id, None,
    ).await?;

    audit::log("cli", "user/subscribe_cancel", true, Duration::default(),
        Some(vec![format!("subId={sub_id}"), format!("txHash={tx_hash}")]), None);

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

pub async fn handle_start_autorenew(
    client: &mut TaskApiClient,
    sub_id: &str,
) -> Result<()> {
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
        client, &resp["uopData"], &account_id, &address, sub_id, biz_type, &user_agent_id, None,
    ).await?;

    audit::log("cli", "user/start_autorenew", true, Duration::default(),
        Some(vec![format!("subId={sub_id}"), format!("txHash={tx_hash}")]), None);

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
        client, &resp["uopData"], &account_id, &address, sub_id, biz_type, user_agent_id,
        Some(&reason_extra),
    ).await?;

    audit::log("cli", "user/subscribe_reject", true, Duration::default(),
        Some(vec![format!("subId={sub_id}"), format!("txHash={tx_hash}")]), None);

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
        Err(_) => signing::resolve_agent_id_by_role(AGENT_ROLE_ASP).await.unwrap_or_default(),
    };

    let json_mode = format.eq_ignore_ascii_case("json");

    let resp = client
        .get_with_identity(
            &format!("{SUBSCRIBE_API_PREFIX}/{sub_id}"),
            &agent_id,
        )
        .await
        .map_err(|e| anyhow::anyhow!("subscribe-detail failed: {e}"))?;

    // Checking a subscription's detail on a fresh device establishes the copy-trade
    // provider session (drains any held signals). Runs before the json early-return so
    // both modes benefit. Only when the logged-in agent is this subscription's buyer.
    if resp["buyerAgentId"].as_str().unwrap_or("") == agent_id {
        let is_ct = resp["copyTrade"].as_i64().unwrap_or(0) == 1
            && resp["status"].as_i64().unwrap_or(-1) == 1;
        ensure_subscription_consent(
            sub_id,
            &agent_id,
            resp["providerAgentId"].as_str().unwrap_or(""),
            is_ct,
        );
    }

    if json_mode {
        // Detail must carry the same derived statusName as the list path — skill templates
        // render the status column from {statusName}, never from the raw numeric code.
        let mut enriched = resp;
        if let Some(obj) = enriched.as_object_mut() {
            let code = obj.get("status").and_then(|v| v.as_i64()).unwrap_or(-1);
            obj.insert("statusName".to_string(), serde_json::Value::String(status_name(code)));
        }
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

pub async fn handle_subscribe_cost(
    client: &mut TaskApiClient,
) -> Result<()> {
    ensure_tokens_refreshed().await?;
    let (agent_id, _) = resolve_user_agent().await?;
    let path = format!("{SUBSCRIBE_API_PREFIX}/cost/active");
    let resp = client
        .get_with_identity(&path, &agent_id)
        .await
        .map_err(|e| anyhow!("subscribe-cost failed: {e}"))?;
    audit::log(
        "cli", "user/subscribe_cost", true, Duration::default(),
        Some(vec![format!("agentId={agent_id}")]), None,
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
}

#[derive(Debug, Default, Deserialize)]
struct SubscriptionList {
    #[serde(default)]
    list: Vec<SubscriptionInfo>,
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

pub async fn handle_my_subscriptions(
    client: &mut TaskApiClient,
    role: SubscriptionRole,
    status: Option<i32>,
) -> Result<()> {
    let header_agent = common_query::resolve_agent_id("", role.agent_role()).await;
    let path = my_subscriptions_path();
    let data = client
        .get_with_agent_id(&path, &header_agent)
        .await
        .map_err(|e| anyhow!("failed to fetch subscriptions: {e}"))?;
    let wrapper: SubscriptionList = serde_json::from_value(data)
        .map_err(|e| anyhow!("failed to parse subscription list: {e}"))?;
    let mut list = filter_subscriptions(wrapper.list, role, &header_agent, status);
    for item in &mut list {
        item.status_name = status_name(item.status);
    }
    // Buyer listing their subscriptions on any device establishes the copy-trade provider
    // session for each active copy-trade sub (drains held signals cross-device).
    if matches!(role, SubscriptionRole::Buyer) {
        for item in &list {
            if item.copy_trade == 1 && item.status == 1 {
                ensure_subscription_consent(
                    &item.job_id,
                    &header_agent,
                    &item.provider_agent_id,
                    true,
                );
            }
        }
    }
    crate::output::success(serde_json::json!({ "list": list }));
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
            "test", "subscribe-reject", "sub-789", "--reason", "quality not met",
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
        let cli = TestCli::parse_from([
            "test", "subscribe-detail", "sub-ccc", "--format", "json",
        ]);
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
        assert!(matches!(cli.cmd, super::super::TaskCommand::SubscribeCost {}));
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
        let legacy = json!({ "trailStartTime": 1_700_000_000i64, "trailEndTime": 1_700_600_000i64 });
        assert_eq!(trial_window(&legacy), (1_700_000_000, 1_700_600_000));
        // Neither present → zeros, never an error.
        assert_eq!(trial_window(&json!({})), (0, 0));
    }
}
