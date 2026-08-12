//! ASP-side subscription continuous-delivery support.
//!
//! A subscription task (`jobType == 1`) stays in `accepted` for its whole active life; the
//! ASP delivers continuously via A2A and NEVER runs an on-chain submit. Closing/settlement
//! is backend-automatic. This module provides:
//!   - the `subStatus` enum + a `SubscriptionDetail` parsed from `GET /subscribe/{subId}`
//!     (`subId == jobId`) and the Active/Ended liveness classification used by `deliver`;
//!   - `subscribe-active` — the resident script's fan-out list (`GET /subscribe/my`);
//!   - `subscribe-agree-refund` / `subscribe-dispute` — the two `sub_user_reject` outcomes;
//!   - the outbound sent-marker (jobId × deliveryId) that makes signal delivery idempotent.
//!
//! NOTE on `subStatus`: the authoritative Subscribe API doc §1.1 (aligned with contract
//! `SubStatus`) defines valid codes: -1 (Init), 1 (Active), 3 (Rejected), 4 (Disputed),
//! 6 (Completed), 7 (Closed), 9 (Failed).

use anyhow::{bail, Result};
use serde::Serialize;
use serde_json::Value;
use std::time::Duration;

use crate::audit;
use crate::commands::agent_commerce::task::common::network::task_api_client::TaskApiClient;
use crate::commands::agent_commerce::task::signing;
use crate::commands::agent_commerce::task::user::subscription_ops::select_subscription_agent_id;

/// `jobType` value that marks a task as a subscription (Subscribe API doc §1.3).
pub const JOB_TYPE_SUBSCRIBE: i64 = 1;

/// Buffer window after `subEndTime` during which the service is still usable while awaiting
/// renewal (Subscribe API doc §3.1: `subBufferEndTime = subEndTime + 1 day`). Used to derive
/// the buffer end when the backend omits `subBufferEndTime` but supplies `subEndTime`.
const BUFFER_WINDOW_SECS: i64 = 86_400;

/// `GET /priapi/v1/aieco/task/subscribe/my` — the ASP's own subscription list.
const SUBSCRIBE_MY_PATH: &str = "/priapi/v1/aieco/task/subscribe/my";

/// Subscription status, mirroring the backend `subStatus` enum (Subscribe API doc §1.1,
/// aligned with the contract `SubStatus`).
/// Only `Active` (1) keeps a subscription in the continuous-delivery phase; every other
/// status ends it (a signal-bearing delivery is then rejected as `subscriptionExpired`, and
/// closing/settlement is backend-automatic — a subscription never runs an ASP submit).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SubStatus {
    /// -1 INIT — DB record created, not yet on-chain (transient; treated as not-live).
    Init,
    /// 0 NONE — internal placeholder for an absent/missing status field (not a backend code).
    None,
    /// 1 Active — subscription live (includes the trial period, distinguished by trialType).
    Active,
    /// 3 Rejected — user rejected, awaiting ASP reaction (1-day window).
    Rejected,
    /// 4 Disputed — ASP raised arbitration.
    Disputed,
    /// 6 Completed — terminal.
    Completed,
    /// 7 Closed — terminal (trial cancel / expiry close / on-chain-fail void).
    Closed,
    /// 9 Failed — refunded (terminal: ASP agreed refund / user won arbitration / auto-refund).
    Failed,
    /// Any code this build does not recognize — treated as not-live (fail safe).
    Unknown(i64),
}

impl SubStatus {
    pub fn from_int(code: i64) -> Self {
        match code {
            -1 => SubStatus::Init,
             0 => SubStatus::None,
             1 => SubStatus::Active,
             3 => SubStatus::Rejected,
             4 => SubStatus::Disputed,
             6 => SubStatus::Completed,
             7 => SubStatus::Closed,
             9 => SubStatus::Failed,
            other => SubStatus::Unknown(other),
        }
    }

    /// True only for `Active` (1) — the sole status that keeps the subscription in the
    /// continuous-delivery (skip-submit) phase.
    pub fn is_active(self) -> bool {
        matches!(self, SubStatus::Active)
    }

    pub fn code(self) -> i64 {
        match self {
            SubStatus::Init => -1,
            SubStatus::None =>  0,
            SubStatus::Active =>  1,
            SubStatus::Rejected =>  3,
            SubStatus::Disputed =>  4,
            SubStatus::Completed =>  6,
            SubStatus::Closed =>  7,
            SubStatus::Failed =>  9,
            SubStatus::Unknown(c) => c,
        }
    }
}

/// How a `deliver` call on this task should behave.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Routing {
    /// Not a subscription task (jobType != 1) — run the existing one-shot pipeline
    /// unchanged (send + on-chain submit).
    NotSubscription,
    /// Live subscription — continuous delivery: send, but SKIP the on-chain submit.
    Active,
    /// Subscription has ended (terminal status, or Active but past the buffer window) —
    /// `deliver` short-circuits to `subscriptionExpired` and does NOT send or submit;
    /// closing/settlement is backend-automatic (a subscription never runs an ASP submit).
    Ended,
}

/// Read an integer field the backend may serialize as a JSON number or a string.
fn as_i64(v: &Value, key: &str) -> Option<i64> {
    let f = v.get(key)?;
    f.as_i64().or_else(|| f.as_str().and_then(|s| s.trim().parse::<i64>().ok()))
}

/// Read a field that may be a JSON string or number, as an owned string.
fn as_string(v: &Value, key: &str) -> Option<String> {
    let f = v.get(key)?;
    f.as_str()
        .map(|s| s.to_string())
        .or_else(|| f.as_i64().map(|n| n.to_string()))
}

/// The subscription fields `deliver` / `subscribe-active` need, parsed out of a
/// subscription-detail JSON object (`GET /subscribe/{subId}` or a `/subscribe/my` list item).
/// On these endpoints `status` carries the subStatus enum (§1.1); we also accept `subStatus`
/// as an alias. Absent fields default to a not-live reading (fail safe).
#[derive(Debug, Clone)]
pub struct SubscriptionDetail {
    pub job_type: i64,
    pub status: SubStatus,
    pub copy_trade: bool,
    /// Subscription end (unix seconds).
    pub sub_end_time: Option<i64>,
    /// Buffer-window end (unix seconds); service is still usable until this while awaiting renewal.
    pub sub_buffer_end_time: Option<i64>,
}

impl SubscriptionDetail {
    pub fn from_json(v: &Value) -> Self {
        let job_type = as_i64(v, "jobType").unwrap_or(0);
        let status_code = as_i64(v, "status")
            .or_else(|| as_i64(v, "subStatus"))
            .unwrap_or(SubStatus::None.code());
        let copy_trade = as_i64(v, "copyTrade").map(|n| n == 1).unwrap_or(false);
        SubscriptionDetail {
            job_type,
            status: SubStatus::from_int(status_code),
            copy_trade,
            sub_end_time: as_i64(v, "subEndTime"),
            sub_buffer_end_time: as_i64(v, "subBufferEndTime"),
        }
    }

    /// Whether this detail describes a subscription task at all.
    pub fn is_subscription(&self) -> bool {
        self.job_type == JOB_TYPE_SUBSCRIBE
    }

    /// Effective buffer-window end (unix seconds): the backend-supplied `subBufferEndTime`,
    /// or `subEndTime + 1 day` when only `subEndTime` is present, or `None` when neither is known.
    fn effective_buffer_end(&self) -> Option<i64> {
        self.sub_buffer_end_time
            .or_else(|| self.sub_end_time.map(|e| e + BUFFER_WINDOW_SECS))
    }

    fn past_buffer(&self, now_secs: i64) -> bool {
        self.effective_buffer_end()
            .map(|end| now_secs >= end)
            .unwrap_or(false)
    }

    /// Active/Ended classification for a KNOWN subscription (caller already established
    /// jobType == 1 from the task detail). A subscription is live only when its status is
    /// `Active` AND we are not past the buffer window; anything else ends it. Never returns
    /// `NotSubscription` — that is decided by the caller from `jobType`, so this does not
    /// depend on `jobType` being present in the `/subscribe` response.
    pub fn liveness(&self, now_secs: i64) -> Routing {
        if self.status.is_active() && !self.past_buffer(now_secs) {
            Routing::Active
        } else {
            Routing::Ended
        }
    }
}

// ── Outbound sent-marker (ASP-side idempotency) ──────────────────────────────
//
// Keyed by jobId × deliveryId — the same key space as the buyer's inbound execution latch
// (`autotrade/latch/…`), so the two ends are idempotent-isomorphic; this marker is the
// ASP-local half under `autotrade/sent/…`. Written write-behind (only after a successful
// send) so the worst case on crash is a re-send that the buyer's latch collapses to a notify,
// never a lost delivery recorded as sent.

fn sent_marker_path(job_id: &str, delivery_id: &str) -> Result<std::path::PathBuf> {
    // ONCHAINOS_HOME-aware (co-located with wallet / audit / autotrade latch), not
    // dirs::home_dir(). delivery_id charset is validated [A-Za-z0-9_-] by the schema before
    // this is ever called, so it is safe as a path component.
    let home = crate::home::onchainos_home()?;
    Ok(home
        .join("autotrade")
        .join("sent")
        .join(job_id)
        .join(delivery_id))
}

/// True if this (jobId, deliveryId) signal was already delivered by a prior successful send.
pub fn is_already_sent(job_id: &str, delivery_id: &str) -> bool {
    sent_marker_path(job_id, delivery_id)
        .map(|p| p.exists())
        .unwrap_or(false)
}

/// Record a successful outbound send (write-behind). Idempotent — an existing marker is fine.
/// Call ONLY after the send succeeded.
pub fn record_sent(job_id: &str, delivery_id: &str) -> Result<()> {
    let path = sent_marker_path(job_id, delivery_id)?;
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    match std::fs::OpenOptions::new().write(true).create_new(true).open(&path) {
        Ok(_) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => Ok(()),
        Err(e) => Err(e.into()),
    }
}

fn subscribe_agree_refund_path(client: &TaskApiClient, sub_id: &str) -> String {
    format!("{}/agreeRefund", client.subscribe_path(sub_id))
}

fn subscribe_asp_claim_path(client: &TaskApiClient, sub_id: &str) -> String {
    format!("{}/aspClaim", client.subscribe_path(sub_id))
}

// ── Debug-only local E2E mock (ONCHAINOS_TEST_MOCK_SUBSCRIPTION=1) ────────────
// Synthesize the subscription lookups so the resident-script flow runs with NO backend or
// credentials. `ONCHAINOS_TEST_MOCK_SUBSTATUS` (default 100=Active) picks the subStatus, so
// the Ended path is testable too. Compiled OUT of release builds (`#[cfg(debug_assertions)]`).

#[cfg(debug_assertions)]
fn mock_substatus() -> SubStatus {
    let code = std::env::var("ONCHAINOS_TEST_MOCK_SUBSTATUS")
        .ok()
        .and_then(|s| s.trim().parse::<i64>().ok())
        .unwrap_or(1); // 1 = Active (default); set e.g. 7 to exercise the Ended path
    SubStatus::from_int(code)
}

#[cfg(debug_assertions)]
fn mock_detail() -> SubscriptionDetail {
    SubscriptionDetail {
        job_type: JOB_TYPE_SUBSCRIBE,
        status: mock_substatus(),
        copy_trade: true,
        sub_end_time: None,
        sub_buffer_end_time: None,
    }
}

#[cfg(debug_assertions)]
fn mock_job_id() -> String {
    std::env::var("ONCHAINOS_TEST_MOCK_JOB_ID")
        .unwrap_or_else(|_| "0xMOCKSUBJOB0001".to_string())
}

/// Fetch the authoritative subscription detail for a job (subId = jobId). Returns `None`
/// when the lookup fails (a one-shot task 404s here); the caller then treats it as one-shot.
pub async fn fetch_detail(
    client: &mut TaskApiClient,
    job_id: &str,
    agent_id: &str,
) -> Option<SubscriptionDetail> {
    #[cfg(debug_assertions)]
    if std::env::var("ONCHAINOS_TEST_MOCK_SUBSCRIPTION").as_deref() == Ok("1") {
        eprintln!("[MOCK] fetch_detail({job_id}) → synthetic subscription (subStatus={})", mock_substatus().code());
        return Some(mock_detail());
    }
    match client.fetch_subscription(job_id, agent_id).await {
        Ok(data) => Some(SubscriptionDetail::from_json(&data)),
        Err(_) => None,
    }
}

// ── `subscribe-active` command ───────────────────────────────────────────────

/// One still-live subscription job, as surfaced to the ASP dispatch script.
#[derive(Debug, Serialize)]
pub struct ActiveSubscription {
    #[serde(rename = "jobId")]
    pub job_id: String,
    #[serde(rename = "subEndTime", skip_serializing_if = "Option::is_none")]
    pub sub_end_time: Option<i64>,
    #[serde(rename = "subBufferEndTime", skip_serializing_if = "Option::is_none")]
    pub sub_buffer_end_time: Option<i64>,
    #[serde(rename = "copyTrade")]
    pub copy_trade: bool,
    #[serde(rename = "status")]
    pub status: i64,
}

/// `subscribe-active` — list the caller's subscription jobs still in the continuous-delivery
/// phase (Active, not past the buffer window), as a JSON array. The resident dispatch script
/// calls this every time a signal arrives to get the current fan-out set — pulled fresh each
/// time (never cached) so new subscribers, cancellations, and expiries reflect on the next round.
///
/// Source: `GET /priapi/v1/aieco/task/subscribe/my`. Whether the list is buyer- or ASP-scoped
/// depends on the backend; to avoid buyer-view rows leaking into an ASP fan-out set we
/// additionally keep only items whose `providerAgentId` matches the caller (when present).
pub async fn handle_active(client: &mut TaskApiClient, agent_id: &str) -> Result<()> {
    let validated_agent_id = select_subscription_agent_id("", agent_id)?;
    let agent_id = validated_agent_id.as_str();
    let now_secs = chrono::Local::now().timestamp();

    #[cfg(debug_assertions)]
    if std::env::var("ONCHAINOS_TEST_MOCK_SUBSCRIPTION").as_deref() == Ok("1") {
        let d = mock_detail();
        let active: Vec<ActiveSubscription> = if d.liveness(now_secs) == Routing::Active {
            vec![ActiveSubscription {
                job_id: mock_job_id(),
                sub_end_time: d.sub_end_time,
                sub_buffer_end_time: d.sub_buffer_end_time,
                copy_trade: d.copy_trade,
                status: d.status.code(),
            }]
        } else {
            Vec::new()
        };
        eprintln!("[MOCK] subscribe-active → {} synthetic active job(s)", active.len());
        crate::output::success(active);
        return Ok(());
    }

    let data = client.get_with_identity(SUBSCRIBE_MY_PATH, agent_id).await?;
    // The list may sit at `data.list` or be the array itself, depending on the wrapper.
    let items: Vec<Value> = match data.get("list").and_then(|v| v.as_array()) {
        Some(arr) => arr.clone(),
        None => data.as_array().cloned().unwrap_or_default(),
    };

    let active: Vec<ActiveSubscription> = items
        .iter()
        .filter(|item| {
            // Defensive ASP-scoping: if the item names a providerAgentId, it must be this
            // caller — otherwise a buyer-view row would leak into the ASP fan-out set.
            match as_string(item, "providerAgentId") {
                Some(pid) => pid == agent_id,
                None => true,
            }
        })
        .filter_map(|item| {
            let detail = SubscriptionDetail::from_json(item);
            if detail.liveness(now_secs) != Routing::Active {
                return None;
            }
            let job_id = as_string(item, "jobId")?;
            Some(ActiveSubscription {
                job_id,
                sub_end_time: detail.sub_end_time,
                sub_buffer_end_time: detail.sub_buffer_end_time,
                copy_trade: detail.copy_trade,
                status: detail.status.code(),
            })
        })
        .collect();

    crate::output::success(active);
    Ok(())
}

// ── ASP decision after sub_user_reject ───────────────────────────────────────

/// `subscribe-agree-refund` — the ASP agrees to refund a rejected subscription period
/// (Subscribe API §2.4, `POST /subscribe/{subId}/agreeRefund`; subId == jobId). Precondition
/// (backend-enforced): the caller is the subscription's ASP and status is Rejected within the
/// 1-day window. Fetch uopData → sign → broadcast.
pub async fn handle_agree_refund(
    client: &mut TaskApiClient,
    job_id: &str,
    agent_id: &str,
) -> Result<()> {
    let validated_agent_id = select_subscription_agent_id("", agent_id)?;
    let agent_id = validated_agent_id.as_str();
    let (account_id, address) = signing::resolve_wallet_by_agent_id(agent_id).await?;
    let body = serde_json::json!({});

    let path = subscribe_agree_refund_path(client, job_id);
    let resp = client.post_with_identity(&path, &body, agent_id).await?;

    let tx_hash = signing::sign_uop_and_broadcast(
        client, &resp["uopData"], &account_id, &address,
        job_id, signing::extract_biz_type(&resp), agent_id,
        None,
    ).await?;

    audit::log(
        "cli",
        "ASP/subscribe_agree_refund_submitted",
        true,
        Duration::default(),
        Some(vec![
            format!("jobId={job_id}"),
            format!("agentId={agent_id}"),
            format!("txHash={tx_hash}"),
        ]),
        None,
    );

    println!("✓ Agreed to refund this subscription period, waiting for on-chain confirmation");
    println!("  txHash: {tx_hash}");
    println!();
    println!("⚠️  Next steps are driven by system notifications — do not proactively message the buyer.");
    Ok(())
}

/// `subscribe-asp-claim` — the ASP claims accrued, not-yet-claimed subscription income
/// (Subscribe API §2.9, `POST /subscribe/{subId}/aspClaim`; subId == jobId; broadcast
/// bizType 107 SUB_ASP_CLAIM, extracted from the response like every other subscribe
/// write). The backend's stated trigger is the renewal notification ("renew 的时候告知
/// asp") — the `sub_renew` ASP arm in `flow.rs` routes the session here; the command is
/// also safe to run ad-hoc or from the delivery script (claims everything outstanding
/// for this subscription; nothing claimable ⇒ backend error, ignore). Fetch uopData →
/// sign → broadcast.
pub async fn handle_asp_claim(
    client: &mut TaskApiClient,
    job_id: &str,
    agent_id: &str,
) -> Result<()> {
    let validated_agent_id = select_subscription_agent_id("", agent_id)?;
    let agent_id = validated_agent_id.as_str();
    let (account_id, address) = signing::resolve_wallet_by_agent_id(agent_id).await?;
    let body = serde_json::json!({});

    let path = subscribe_asp_claim_path(client, job_id);
    let resp = client.post_with_identity(&path, &body, agent_id).await?;

    let tx_hash = signing::sign_uop_and_broadcast(
        client, &resp["uopData"], &account_id, &address,
        job_id, signing::extract_biz_type(&resp), agent_id,
        None,
    ).await?;

    audit::log(
        "cli",
        "ASP/subscribe_asp_claim_submitted",
        true,
        Duration::default(),
        Some(vec![
            format!("jobId={job_id}"),
            format!("agentId={agent_id}"),
            format!("txHash={tx_hash}"),
        ]),
        None,
    );

    println!("✓ Claim submitted for accrued subscription income, waiting for on-chain confirmation");
    println!("  txHash: {tx_hash}");
    println!();
    println!("⚠️  This claims your own funds only — no buyer action is involved; do not message the buyer.");
    Ok(())
}

/// Max on-chain dispute reason length (parity with the one-shot `dispute raise`/`confirm`).
const MAX_DISPUTE_REASON_CHARS: usize = 2000;

/// `subscribe-dispute` — the ASP raises arbitration for a rejected subscription period via the
/// backend's single combined endpoint (§2.10 `POST /priapi/v1/aieco/task/{jobId}/dispute/
/// approveAndCreateDispute` — approve + create in one call, NOT the old two-phase
/// dispute raise/confirm). Fetch uopData → sign → broadcast; `reason` rides the broadcast
/// bizContext so the arbitration record carries the ASP's argument.
pub async fn handle_dispute(
    client: &mut TaskApiClient,
    job_id: &str,
    reason: &str,
    agent_id: &str,
) -> Result<()> {
    let validated_agent_id = select_subscription_agent_id("", agent_id)?;
    let agent_id = validated_agent_id.as_str();
    if reason.chars().count() > MAX_DISPUTE_REASON_CHARS {
        bail!("Dispute reason exceeds {MAX_DISPUTE_REASON_CHARS} characters. Please shorten it and try again.");
    }
    let (account_id, address) = signing::resolve_wallet_by_agent_id(agent_id).await?;
    let body = serde_json::json!({});

    // §2.10 combined approve+create (subId == jobId). Path shape is /task/{jobId}/dispute/…,
    // NOT under /subscribe/.
    let path = client.endpoint(job_id, "dispute/approveAndCreateDispute");
    let resp = client.post_with_identity(&path, &body, agent_id).await?;

    // Ride the reason on the broadcast bizContext (mirrors `dispute confirm`); the
    // approveAndCreateDispute request body itself stays empty, matching the one-shot path.
    let reason_json = serde_json::json!({ "reason": reason });
    let tx_hash = signing::sign_uop_and_broadcast(
        client, &resp["uopData"], &account_id, &address,
        job_id, signing::extract_biz_type(&resp), agent_id,
        Some(&reason_json),
    ).await?;

    audit::log(
        "cli",
        "ASP/subscribe_dispute_submitted",
        true,
        Duration::default(),
        Some(vec![
            format!("jobId={job_id}"),
            format!("agentId={agent_id}"),
            format!("txHash={tx_hash}"),
        ]),
        None,
    );

    println!("✓ Subscription dispute raised (approve+create), waiting for on-chain confirmation");
    println!("  txHash: {tx_hash}");
    println!();
    println!("⚠️  Next steps are driven by system notifications — do not proactively message the buyer.");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn detail(job_type: i64, status: i64, buffer_end: Option<i64>) -> SubscriptionDetail {
        SubscriptionDetail {
            job_type,
            status: SubStatus::from_int(status),
            copy_trade: false,
            sub_end_time: None,
            sub_buffer_end_time: buffer_end,
        }
    }

    #[test]
    fn substatus_codes_round_trip() {
        for c in [-1, 0, 1, 3, 4, 6, 7, 9, 999] {
            assert_eq!(SubStatus::from_int(c).code(), c);
        }
        assert!(SubStatus::from_int(1).is_active());
        assert!(!SubStatus::from_int(3).is_active());
        assert!(!SubStatus::from_int(100).is_active());
    }

    #[test]
    fn is_subscription_by_jobtype() {
        assert!(detail(1, 1, None).is_subscription());
        assert!(!detail(0, 1, None).is_subscription());
    }

    #[test]
    fn active_within_buffer_is_active() {
        assert_eq!(detail(1, 1, Some(2000)).liveness(1000), Routing::Active);
        assert_eq!(detail(1, 1, None).liveness(1000), Routing::Active);
    }

    #[test]
    fn active_but_past_buffer_is_ended() {
        assert_eq!(detail(1, 1, Some(500)).liveness(1000), Routing::Ended);
    }

    #[test]
    fn buffer_end_derived_from_sub_end_time_when_missing() {
        let mut d = detail(1, 1, None);
        d.sub_end_time = Some(1000);
        assert_eq!(d.liveness(1000 + BUFFER_WINDOW_SECS + 1), Routing::Ended);
        assert_eq!(d.liveness(1000 + 10), Routing::Active);
    }

    #[test]
    fn terminal_status_is_ended() {
        for status in [3, 4, 6, 7, 9, -1, 100] {
            assert_eq!(detail(1, status, Some(9999)).liveness(1000), Routing::Ended);
        }
    }

    #[test]
    fn from_json_parses_string_serialized_ints() {
        let d = SubscriptionDetail::from_json(&json!({
            "jobType": "1", "status": "1", "copyTrade": "1",
            "subEndTime": "1783868715", "subBufferEndTime": "1786633515"
        }));
        assert!(d.is_subscription());
        assert!(d.status.is_active());
        assert!(d.copy_trade);
        assert_eq!(d.sub_buffer_end_time, Some(1786633515));
    }

    #[test]
    fn from_json_status_alias_and_defaults() {
        let d = SubscriptionDetail::from_json(&json!({"jobType": 1, "subStatus": 1}));
        assert!(d.status.is_active());
        // absent status → None (0) → not-live.
        let d2 = SubscriptionDetail::from_json(&json!({"jobType": 1}));
        assert_eq!(d2.status, SubStatus::None);
        assert_eq!(d2.liveness(1000), Routing::Ended);
    }

    #[test]
    fn sent_marker_path_layout() {
        let p = sent_marker_path("job1", "sig-20260716-42").unwrap();
        let s = p.to_string_lossy();
        assert!(s.ends_with("/autotrade/sent/job1/sig-20260716-42"), "unexpected: {s}");
    }
}
