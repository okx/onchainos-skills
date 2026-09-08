//! Buyer refund orchestration following the Skill/CLI v2 progression contract.
//!
//! `refund-prepare` is read-only. `refund-execute` accepts only a plan produced
//! by prepare, re-reads authoritative state, requires explicit confirmation,
//! and maps to an already-existing lifecycle endpoint. No generic refund API is
//! invented here.

use anyhow::{bail, Context as _, Result};
use clap::ValueEnum;
use fs2::FileExt;
use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};
use sha2::{Digest, Sha256};
use std::fs::{self, File, OpenOptions};
use std::path::PathBuf;

use crate::audit;
use crate::commands::agent_commerce::task::common::{
    self, network::task_api_client::TaskApiClient,
};
use crate::commands::agent_commerce::task::signing;
use crate::commands::agentic_wallet::auth::ensure_tokens_refreshed;
use crate::wallet_api::WalletApiClient;

use super::create_subscribe::SUBSCRIBE_API_PREFIX;

const SCHEMA_VERSION: i64 = 2;
const JOURNAL_REVISION: i64 = 3;
const MAX_REASON_CHARS: usize = 2_000;

fn legacy_journal_revision() -> i64 {
    2
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, ValueEnum)]
#[value(rename_all = "kebab-case")]
pub enum RefundOperation {
    /// Close a zero-price one-time task. This is not a refund.
    CloseZero,
    /// Close a paid, unaccepted one-time task so escrow can be returned.
    DirectRefund,
    /// Reject the delivered/current-period service and await ASP resolution.
    RequestRefund,
    /// Cancel trial-to-paid conversion. This is not a refund.
    CancelTrialConversion,
}

impl RefundOperation {
    fn as_str(self) -> &'static str {
        match self {
            Self::CloseZero => "close-zero",
            Self::DirectRefund => "direct-refund",
            Self::RequestRefund => "request-refund",
            Self::CancelTrialConversion => "cancel-trial-conversion",
        }
    }
}

#[derive(Clone, Debug)]
struct RefundSnapshot {
    job_id: String,
    job_type: i64,
    status: i64,
    title: String,
    buyer_agent_id: String,
    provider_agent_id: Option<String>,
    provider_name: Option<String>,
    service_id: Option<String>,
    service_name: Option<String>,
    revision: Option<String>,
    trial_type: Option<i64>,
    period_index: Option<i64>,
    period_start_time: Option<i64>,
    period_end_time: Option<i64>,
    auto_renew: Option<i64>,
    token_address: Option<String>,
    token_symbol: Option<String>,
    chain_id: Option<i64>,
    payment_mode: Option<i64>,
    original_amount: String,
    response_deadline: Option<i64>,
    requested_at: Option<i64>,
    recorded_refund_reason: Option<String>,
    settlement_confirmed: bool,
    settlement_tx_hash: Option<String>,
    settlement_provenance: Option<RefundSettlementProvenance>,
    settlement_time: Option<i64>,
    dispute_round: Option<i64>,
    dispute_phase: Option<String>,
    dispute_prepare_end_time: Option<i64>,
    dispute_round_end_time: Option<i64>,
    /// Set only after a durable local request-refund receipt is reconciled
    /// against the exact fresh task/payment lifecycle facts.
    refund_request_provenance: bool,
}

pub(crate) struct RefundListItem {
    pub(crate) display: Value,
    pub(crate) deadline: Option<i64>,
    pub(crate) job_type: i64,
    pub(crate) status: i64,
    pub(crate) reason: &'static str,
    /// Completed(6) is an evaluation verdict only when a buyer refund request
    /// was durably reconciled first; otherwise it may be a normal completion.
    pub(crate) refund_request_provenance: bool,
    pub(crate) refund_request_available: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct RefundSettlementProvenance {
    source: &'static str,
    operation: String,
    order_id: String,
    biz_type: i64,
    chain_index: String,
    lifecycle_status: i64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct Plan {
    phase: &'static str,
    decision: &'static str,
    reason: &'static str,
    operation: Option<RefundOperation>,
    action_id: Option<&'static str>,
    recommend_stop: bool,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct PendingRefundMutation {
    schema_version: i64,
    /// Local reconciliation format. This is intentionally separate from the
    /// public Refund V2 payload schema version. Journals written before the
    /// request-provenance upgrade omit this field and deserialize as v2.
    #[serde(default = "legacy_journal_revision")]
    journal_revision: i64,
    job_id: String,
    user_agent_id: String,
    snapshot_id: String,
    operation: String,
    state: String,
    #[serde(default)]
    job_type: Option<i64>,
    #[serde(default)]
    trial_type: Option<i64>,
    #[serde(default)]
    period_index: Option<i64>,
    #[serde(default)]
    period_start_time: Option<i64>,
    #[serde(default)]
    period_end_time: Option<i64>,
    #[serde(default)]
    pkg_id: Option<String>,
    #[serde(default)]
    order_id: Option<String>,
    #[serde(default)]
    order_type: Option<String>,
    #[serde(default)]
    biz_uniq_key: Option<String>,
    #[serde(default)]
    tx_hash: Option<String>,
    #[serde(default)]
    account_id: Option<String>,
    #[serde(default)]
    address: Option<String>,
    #[serde(default)]
    chain_index: Option<String>,
    #[serde(default)]
    biz_type: Option<i64>,
    #[serde(default)]
    original_amount: Option<String>,
    #[serde(default)]
    token_address: Option<String>,
    #[serde(default)]
    token_symbol: Option<String>,
    #[serde(default)]
    provider_agent_id: Option<String>,
    #[serde(default)]
    service_id: Option<String>,
    #[serde(default)]
    service_name: Option<String>,
    #[serde(default)]
    payment_mode: Option<i64>,
    updated_at: i64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum RefundOrderStatus {
    Pending,
    Succeeded(Option<String>),
    Failed,
    Unknown,
}

fn pending_state_path(job_id: &str, user_agent_id: &str) -> Result<PathBuf> {
    let digest = Sha256::digest(format!("{user_agent_id}\0{job_id}").as_bytes());
    Ok(crate::home::onchainos_home()?
        .join("refund-v2")
        .join(format!("{}.json", hex::encode(digest))))
}

fn read_pending_mutation(
    job_id: &str,
    user_agent_id: &str,
) -> Result<Option<PendingRefundMutation>> {
    let path = pending_state_path(job_id, user_agent_id)?;
    match fs::read(&path) {
        Ok(bytes) => {
            let state: PendingRefundMutation =
                serde_json::from_slice(&bytes).with_context(|| {
                    format!("parse Refund V2 reconciliation state {}", path.display())
                })?;
            if state.schema_version != SCHEMA_VERSION
                || !matches!(state.journal_revision, 2 | JOURNAL_REVISION)
                || state.job_id != job_id
                || state.user_agent_id != user_agent_id
            {
                bail!("Refund V2 reconciliation state does not match this task and identity");
            }
            Ok(Some(state))
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error)
            .with_context(|| format!("read Refund V2 reconciliation state {}", path.display())),
    }
}

fn pending_mutation_resolved(state: &PendingRefundMutation, snapshot: &RefundSnapshot) -> bool {
    match state.operation.as_str() {
        // A paid direct refund keeps its operation-scoped broadcast receipt
        // through Closed(7), because that receipt is the only documented way
        // to identify the transaction that this client submitted. Other
        // lifecycle states make the original Created-only write unavailable.
        "direct-refund" => matches!(snapshot.status, 1 | 2 | 3 | 4 | 6 | 8 | 9),
        "close-zero" => matches!(snapshot.status, 1 | 2 | 3 | 4 | 6 | 7 | 8 | 9),
        "request-refund" => matches!(snapshot.status, 3 | 4 | 6 | 7 | 8 | 9),
        "cancel-trial-conversion" => snapshot.status != 1 || snapshot.auto_renew == Some(0),
        // Read-only migration support for journals written by older releases;
        // the operation is no longer exposed or executable.
        "finalize-expired-refund" => true,
        _ => false,
    }
}

fn order_detail_item(data: &Value) -> Option<&Value> {
    match data {
        Value::Array(items) if items.len() == 1 => items.first(),
        Value::Object(_) => Some(data),
        _ => None,
    }
}

fn has_durable_broadcast_receipt(state: &PendingRefundMutation) -> bool {
    [
        state.pkg_id.as_deref(),
        state.order_id.as_deref(),
        state.order_type.as_deref(),
        state.biz_uniq_key.as_deref(),
    ]
    .into_iter()
    .all(|value| value.is_some_and(|value| !value.trim().is_empty()))
}

fn parse_refund_order_status(data: &Value, submitted_tx_hash: Option<&str>) -> RefundOrderStatus {
    let Some(detail) = order_detail_item(data) else {
        return RefundOrderStatus::Unknown;
    };
    let status = scalar_string(detail.get("txStatus"))
        .unwrap_or_default()
        .to_ascii_uppercase();
    match status.as_str() {
        "1" | "2" | "PENDING" => RefundOrderStatus::Pending,
        "3" | "6" | "ERROR" | "FAIL" | "FAILED" | "CANCELLED" => RefundOrderStatus::Failed,
        "4" | "SUCCESS" => {
            let observed = match scalar_string(detail.get("txHash")) {
                Some(value) if valid_tx_hash(&value) => Some(value),
                Some(_) => return RefundOrderStatus::Unknown,
                None => None,
            };
            let submitted = match submitted_tx_hash
                .map(str::trim)
                .filter(|value| !value.is_empty())
            {
                Some(value) if valid_tx_hash(value) => Some(value.to_string()),
                Some(_) => return RefundOrderStatus::Unknown,
                None => None,
            };
            match (observed, submitted) {
                (Some(observed), Some(submitted)) if !observed.eq_ignore_ascii_case(&submitted) => {
                    RefundOrderStatus::Unknown
                }
                (Some(observed), _) => RefundOrderStatus::Succeeded(Some(observed)),
                // SUCCESS is useful optional audit evidence even when the
                // wallet response does not expose a transaction hash. Never
                // substitute the submitted candidate as the confirmed hash.
                (None, _) => RefundOrderStatus::Succeeded(None),
            }
        }
        _ => RefundOrderStatus::Unknown,
    }
}

async fn query_refund_order_status(state: &PendingRefundMutation) -> Result<RefundOrderStatus> {
    let resolved_wallet = if state.account_id.is_none() || state.address.is_none() {
        Some(signing::resolve_wallet_by_agent_id(&state.user_agent_id).await?)
    } else {
        None
    };
    let account_id = state
        .account_id
        .as_deref()
        .or_else(|| resolved_wallet.as_ref().map(|wallet| wallet.0.as_str()))
        .ok_or_else(|| anyhow::anyhow!("Refund V2 journal is missing the broadcast account"))?;
    let address = state
        .address
        .as_deref()
        .or_else(|| resolved_wallet.as_ref().map(|wallet| wallet.1.as_str()))
        .ok_or_else(|| anyhow::anyhow!("Refund V2 journal is missing the broadcast address"))?;
    let access_token = ensure_tokens_refreshed().await?;
    let chain_index = state
        .chain_index
        .as_deref()
        .unwrap_or(common::XLAYER_CHAIN_INDEX);
    let mut query = vec![
        ("accountId", account_id),
        ("chainIndex", chain_index),
        ("address", address),
    ];
    if let Some(order_id) = state.order_id.as_deref() {
        query.push(("orderId", order_id));
    } else if let Some(tx_hash) = state.tx_hash.as_deref() {
        query.push(("txHash", tx_hash));
    } else {
        return Ok(RefundOrderStatus::Unknown);
    }

    let mut client = WalletApiClient::new()?;
    let detail = client
        .get_authed(
            "/priapi/v5/wallet/agentic/order/detail",
            &access_token,
            &query,
        )
        .await
        .context("query Refund V2 broadcast order status")?;
    validate_refund_order_detail_binding(&detail, state)?;
    Ok(parse_refund_order_status(&detail, state.tx_hash.as_deref()))
}

fn validate_refund_order_detail_binding(
    detail: &Value,
    state: &PendingRefundMutation,
) -> Result<()> {
    let Some(detail) = order_detail_item(detail) else {
        return Ok(());
    };
    if let (Some(expected), Some(actual)) = (
        state.order_id.as_deref(),
        scalar_string(detail.get("orderId")),
    ) {
        if expected != actual {
            bail!("wallet order detail returned a mismatched orderId");
        }
    }
    if let (Some(expected), Some(actual)) = (
        state.chain_index.as_deref(),
        scalar_string(detail.get("chainIndex")),
    ) {
        if expected != actual {
            bail!("wallet order detail returned a mismatched chainIndex");
        }
    }
    Ok(())
}

fn direct_refund_provenance_matches(
    snapshot: &RefundSnapshot,
    state: &PendingRefundMutation,
) -> bool {
    let service_matches = match (state.service_id.as_deref(), snapshot.service_id.as_deref()) {
        (Some(recorded), Some(fresh)) => recorded == fresh,
        _ => state
            .service_name
            .as_deref()
            .zip(snapshot.service_name.as_deref())
            .is_some_and(|(recorded, fresh)| recorded == fresh),
    };

    if state.operation != "direct-refund"
        || snapshot.status != 7
        || snapshot.job_type != 0
        || snapshot.payment_mode != Some(1)
        || is_zero_decimal(&snapshot.original_amount)
        || !has_durable_broadcast_receipt(state)
        || !state.biz_type.is_some_and(|value| value > 0)
        || state
            .account_id
            .as_deref()
            .is_none_or(|value| value.trim().is_empty())
        || state
            .address
            .as_deref()
            .is_none_or(|value| value.trim().is_empty())
        || state.chain_index.as_deref() != Some(common::XLAYER_CHAIN_INDEX)
        || !state
            .original_amount
            .as_deref()
            .is_some_and(|value| decimal_equal(value, &snapshot.original_amount))
        || !state
            .token_address
            .as_deref()
            .zip(snapshot.token_address.as_deref())
            .is_some_and(|(expected, actual)| expected.eq_ignore_ascii_case(actual))
        || !state
            .token_symbol
            .as_deref()
            .zip(snapshot.token_symbol.as_deref())
            .is_some_and(|(expected, actual)| expected.eq_ignore_ascii_case(actual))
        || !state
            .provider_agent_id
            .as_deref()
            .zip(snapshot.provider_agent_id.as_deref())
            .is_some_and(|(expected, actual)| expected == actual)
        || !service_matches
        || state.payment_mode != Some(1)
    {
        return false;
    }
    true
}

/// Bind a locally submitted refund request to the exact fresh lifecycle row.
/// Status 9 alone is ambiguous for subscriptions (it is also used for
/// charge/conversion failure), so only a durable request receipt with the
/// original buyer/job/payment facts may disambiguate it. Provider, service,
/// period, symbol, and payment-mode fields are useful cross-checks when both
/// the journal and fresh detail expose them, but their omission or harmless
/// display normalization must not erase a real refund request. The same
/// provenance also binds dispute outcomes for one-time tasks.
fn request_refund_provenance_core_matches(
    snapshot: &RefundSnapshot,
    state: &PendingRefundMutation,
) -> bool {
    let service_matches = match (state.service_id.as_deref(), snapshot.service_id.as_deref()) {
        (Some(recorded), Some(fresh)) => recorded == fresh,
        _ => match (
            state.service_name.as_deref(),
            snapshot.service_name.as_deref(),
        ) {
            (Some(recorded), Some(fresh)) => recorded == fresh,
            _ => true,
        },
    };
    let optional_i64_matches = |recorded: Option<i64>, fresh: Option<i64>| {
        recorded
            .zip(fresh)
            .is_none_or(|(recorded, fresh)| recorded == fresh)
    };
    let optional_exact_matches = |recorded: Option<&str>, fresh: Option<&str>| {
        recorded
            .zip(fresh)
            .is_none_or(|(recorded, fresh)| recorded == fresh)
    };
    let optional_ascii_matches = |recorded: Option<&str>, fresh: Option<&str>| {
        recorded
            .zip(fresh)
            .is_none_or(|(recorded, fresh)| recorded.eq_ignore_ascii_case(fresh))
    };
    let submitted_state = matches!(
        state.state.as_str(),
        "broadcast_submitted" | "refund_request_applied" | "request_provenance_incomplete"
    );
    let legacy_journal = state.journal_revision == legacy_journal_revision();
    let recorded_job_type_matches = state
        .job_type
        .is_some_and(|recorded| recorded == snapshot.job_type)
        || (legacy_journal && state.job_type.is_none());
    let recorded_provider = state
        .provider_agent_id
        .as_deref()
        .is_some_and(|value| !value.trim().is_empty());
    let recorded_service = state
        .service_id
        .as_deref()
        .or(state.service_name.as_deref())
        .is_some_and(|value| !value.trim().is_empty());
    let recorded_token_symbol = state
        .token_symbol
        .as_deref()
        .is_some_and(|value| !value.trim().is_empty());
    let job_type_matches = match snapshot.job_type {
        0 => {
            recorded_job_type_matches
                && snapshot.payment_mode == Some(1)
                && state.payment_mode == Some(1)
        }
        1 => {
            recorded_job_type_matches
                && snapshot.trial_type == Some(0)
                && (state.trial_type == Some(0) || (legacy_journal && state.trial_type.is_none()))
                && (legacy_journal
                    || (state.period_start_time.is_some_and(|value| value > 0)
                        && state.period_end_time.is_some_and(|value| value > 0)
                        && state
                            .period_start_time
                            .zip(state.period_end_time)
                            .is_some_and(|(start, end)| start < end)))
        }
        _ => false,
    };

    matches!(state.journal_revision, 2 | JOURNAL_REVISION)
        && state.operation == "request-refund"
        && submitted_state
        && state.job_id == snapshot.job_id
        && state.user_agent_id == snapshot.buyer_agent_id
        && job_type_matches
        && recorded_provider
        && recorded_service
        && recorded_token_symbol
        && matches!(snapshot.status, 3 | 4 | 6 | 8 | 9)
        && has_durable_broadcast_receipt(state)
        && state.biz_type.is_some_and(|value| value > 0)
        && state
            .account_id
            .as_deref()
            .is_some_and(|value| !value.trim().is_empty())
        && state
            .address
            .as_deref()
            .is_some_and(|value| !value.trim().is_empty())
        && state.chain_index.as_deref() == Some(common::XLAYER_CHAIN_INDEX)
        && state
            .original_amount
            .as_deref()
            .is_some_and(|value| decimal_equal(value, &snapshot.original_amount))
        && validate_decimal(&snapshot.original_amount)
        && !is_zero_decimal(&snapshot.original_amount)
        && state
            .token_address
            .as_deref()
            .zip(snapshot.token_address.as_deref())
            .is_some_and(|(expected, actual)| expected.eq_ignore_ascii_case(actual))
        && optional_ascii_matches(
            state.token_symbol.as_deref(),
            snapshot.token_symbol.as_deref(),
        )
        && optional_exact_matches(
            state.provider_agent_id.as_deref(),
            snapshot.provider_agent_id.as_deref(),
        )
        && service_matches
        && optional_i64_matches(state.period_index, snapshot.period_index)
        && optional_i64_matches(state.period_start_time, snapshot.period_start_time)
        && optional_i64_matches(state.period_end_time, snapshot.period_end_time)
        && optional_i64_matches(state.payment_mode, snapshot.payment_mode)
}

fn request_refund_provenance_matches(
    snapshot: &RefundSnapshot,
    state: &PendingRefundMutation,
) -> bool {
    request_refund_provenance_core_matches(snapshot, state)
        && (state.journal_revision != legacy_journal_revision() || matches!(snapshot.status, 3 | 4))
}

fn can_upgrade_request_refund_journal(snapshot: &RefundSnapshot) -> bool {
    match snapshot.job_type {
        0 => true,
        1 => {
            snapshot.trial_type == Some(0)
                && snapshot.period_start_time.is_some_and(|value| value > 0)
                && snapshot.period_end_time.is_some_and(|value| value > 0)
                && snapshot
                    .period_start_time
                    .zip(snapshot.period_end_time)
                    .is_some_and(|(start, end)| start < end)
        }
        _ => false,
    }
}

fn upgrade_request_refund_journal(
    state: &mut PendingRefundMutation,
    snapshot: &RefundSnapshot,
) -> bool {
    if !can_upgrade_request_refund_journal(snapshot) {
        return false;
    }
    state.journal_revision = JOURNAL_REVISION;
    state.job_type = Some(snapshot.job_type);
    state.trial_type = snapshot.trial_type;
    state.period_index = snapshot.period_index;
    state.period_start_time = snapshot.period_start_time;
    state.period_end_time = snapshot.period_end_time;
    true
}

fn apply_request_refund_provenance(
    snapshot: &mut RefundSnapshot,
    state: &PendingRefundMutation,
) -> bool {
    if !request_refund_provenance_matches(snapshot, state) {
        return false;
    }
    snapshot.refund_request_provenance = true;
    if snapshot.status == 9 {
        snapshot.settlement_confirmed = true;
    }
    true
}

fn apply_legacy_request_refund_provenance_after_order_success(
    snapshot: &mut RefundSnapshot,
    state: &PendingRefundMutation,
) -> bool {
    if state.journal_revision != legacy_journal_revision()
        || !matches!(snapshot.status, 6 | 8 | 9)
        || !request_refund_provenance_core_matches(snapshot, state)
    {
        return false;
    }
    snapshot.refund_request_provenance = true;
    if snapshot.status == 9 {
        snapshot.settlement_confirmed = true;
    }
    true
}

fn apply_confirmed_direct_refund(
    snapshot: &mut RefundSnapshot,
    state: &PendingRefundMutation,
) -> bool {
    if state.state != "confirmed" || !direct_refund_provenance_matches(snapshot, state) {
        return false;
    }
    let Some(tx_hash) = state
        .tx_hash
        .as_deref()
        .filter(|value| valid_tx_hash(value))
    else {
        return false;
    };
    let (Some(order_id), Some(biz_type), Some(chain_index)) = (
        state.order_id.clone(),
        state.biz_type,
        state.chain_index.clone(),
    ) else {
        return false;
    };
    snapshot.settlement_tx_hash = Some(tx_hash.to_string());
    snapshot.settlement_provenance = Some(RefundSettlementProvenance {
        source: "wallet_order_detail",
        operation: state.operation.clone(),
        order_id,
        biz_type,
        chain_index,
        lifecycle_status: snapshot.status,
    });
    true
}

async fn reconcile_pending_mutation_locked(
    snapshot: &mut RefundSnapshot,
) -> Result<Option<PendingRefundMutation>> {
    // Expired(8) is already the backend's authoritative terminal result. It
    // must not read, query, or depend on any local mutation journal, including
    // stale journals written by releases that exposed timeout finalization.
    if snapshot.status == 8 {
        let _ = remove_pending_mutation(&snapshot.job_id, &snapshot.buyer_agent_id);
        return Ok(None);
    }
    let Some(mut state) = read_pending_mutation(&snapshot.job_id, &snapshot.buyer_agent_id)? else {
        return Ok(None);
    };

    if apply_confirmed_direct_refund(snapshot, &state) {
        return Ok(None);
    }

    if state.operation == "request-refund" && matches!(snapshot.status, 3 | 4 | 6 | 8 | 9) {
        // Once the task lifecycle has advanced, the original reject
        // write is unavailable and this is no longer an active replay lock.
        // Retain the receipt as durable intent provenance so later dispute
        // handling stays bound to this refund flow, and subscription polling
        // can distinguish a refund-derived Failed(9) from the same backend
        // status used by ordinary charge failures.
        let legacy_terminal_needs_order_confirmation = state.journal_revision
            == legacy_journal_revision()
            && matches!(snapshot.status, 6 | 8 | 9);
        let matched = if legacy_terminal_needs_order_confirmation {
            matches!(
                query_refund_order_status(&state).await,
                Ok(RefundOrderStatus::Succeeded(_))
            ) && apply_legacy_request_refund_provenance_after_order_success(snapshot, &state)
        } else {
            apply_request_refund_provenance(snapshot, &state)
        };
        let previous_revision = state.journal_revision;
        if matched && previous_revision < JOURNAL_REVISION {
            upgrade_request_refund_journal(&mut state, snapshot);
        }
        let next_state = if matched {
            "refund_request_applied"
        } else {
            "request_provenance_incomplete"
        };
        if state.state != next_state || state.journal_revision != previous_revision {
            state.state = next_state.to_string();
            state.updated_at = chrono::Utc::now().timestamp();
            write_pending_mutation(&state)?;
        }
        return Ok(None);
    }

    if state.operation == "direct-refund" && snapshot.status == 7 {
        if matches!(
            state.state.as_str(),
            "broadcast_failed" | "lifecycle_advanced_without_receipt" | "confirmed_without_hash"
        ) {
            return Ok(None);
        }
        if !has_durable_broadcast_receipt(&state) {
            state.state = "lifecycle_advanced_without_receipt".to_string();
            state.updated_at = chrono::Utc::now().timestamp();
            write_pending_mutation(&state)?;
            return Ok(None);
        }
        if !direct_refund_provenance_matches(snapshot, &state) {
            state.state = "provenance_incomplete".to_string();
            state.updated_at = chrono::Utc::now().timestamp();
            write_pending_mutation(&state)?;
            return Ok(None);
        }

        match query_refund_order_status(&state).await {
            Ok(RefundOrderStatus::Succeeded(tx_hash)) => {
                state.state = if tx_hash.is_some() {
                    "confirmed".to_string()
                } else {
                    "confirmed_without_hash".to_string()
                };
                state.tx_hash = tx_hash;
                state.updated_at = chrono::Utc::now().timestamp();
                write_pending_mutation(&state)?;
                if state.tx_hash.is_some() && !apply_confirmed_direct_refund(snapshot, &state) {
                    bail!(
                        "confirmed Refund V2 order no longer matches its direct-refund provenance"
                    );
                }
                return Ok(None);
            }
            Ok(RefundOrderStatus::Failed) => {
                state.state = "broadcast_failed".to_string();
                state.updated_at = chrono::Utc::now().timestamp();
                write_pending_mutation(&state)?;
                return Ok(None);
            }
            Ok(RefundOrderStatus::Pending | RefundOrderStatus::Unknown) | Err(_) => {
                // Fresh Closed(7) is itself the backend's post-chain-event
                // settlement confirmation for a paid one-time escrow task.
                // Keep the receipt for later Tx Hash enrichment, but do not
                // downgrade the confirmed refund to a blocked state merely
                // because wallet-order projection is slower or unavailable.
                return Ok(None);
            }
        }
    }

    if pending_mutation_resolved(&state, snapshot) {
        // Other operations no longer need their local replay guard once
        // authoritative lifecycle state makes the original write unavailable.
        // Direct-refund Closed(7) and applied subscription request-refund
        // provenance are handled above and retained deliberately.
        let _ = remove_pending_mutation(&snapshot.job_id, &snapshot.buyer_agent_id);
        return Ok(None);
    }
    Ok(Some(state))
}

fn acquire_pending_lock(job_id: &str, user_agent_id: &str) -> Result<File> {
    let state_path = pending_state_path(job_id, user_agent_id)?;
    let root = state_path
        .parent()
        .ok_or_else(|| anyhow::anyhow!("Refund V2 state path has no parent"))?;
    crate::home::ensure_dir_0700(root)?;
    let lock_path = state_path.with_extension("lock");
    let lock = OpenOptions::new()
        .create(true)
        .read(true)
        .write(true)
        .open(&lock_path)
        .with_context(|| format!("open Refund V2 reconciliation lock {}", lock_path.display()))?;
    lock.lock_exclusive()
        .context("lock Refund V2 reconciliation state")?;
    Ok(lock)
}

async fn reconcile_pending_mutation(
    snapshot: &mut RefundSnapshot,
) -> Result<Option<PendingRefundMutation>> {
    // Reconciliation can update or retire the journal. Serialize every such
    // transition with refund-execute so a read-only status check cannot
    // overwrite a receipt that an in-flight execution just persisted.
    let _lock = acquire_pending_lock(&snapshot.job_id, &snapshot.buyer_agent_id)?;
    reconcile_pending_mutation_locked(snapshot).await
}

async fn reconcile_without_downgrading_confirmed_settlement(
    snapshot: &mut RefundSnapshot,
) -> Result<Option<PendingRefundMutation>> {
    match reconcile_pending_mutation(snapshot).await {
        Ok(pending) => Ok(pending),
        // Wallet-order reconciliation only enriches a backend-confirmed
        // lifecycle result with an optional Tx Hash. A corrupt/stale local
        // journal or unavailable wallet query must never turn an already
        // confirmed refund back into an unresolved business outcome.
        Err(_) if snapshot.has_confirmed_settlement() => Ok(None),
        Err(error) => Err(error),
    }
}

fn write_pending_mutation(state: &PendingRefundMutation) -> Result<()> {
    let path = pending_state_path(&state.job_id, &state.user_agent_id)?;
    crate::home::atomic_write(&path, &serde_json::to_vec_pretty(state)?, true)
        .with_context(|| format!("write Refund V2 reconciliation state {}", path.display()))?;

    // This journal is the pre-mutation replay guard, so ordinary atomic rename
    // is not enough: make both the file data and directory entry durable before
    // allowing a remote funds mutation to begin.
    File::open(&path)
        .and_then(|file| file.sync_all())
        .with_context(|| format!("sync Refund V2 reconciliation state {}", path.display()))?;
    #[cfg(unix)]
    File::open(
        path.parent()
            .ok_or_else(|| anyhow::anyhow!("Refund V2 state path has no parent"))?,
    )
    .and_then(|directory| directory.sync_all())
    .with_context(|| {
        format!(
            "sync Refund V2 reconciliation directory for {}",
            path.display()
        )
    })?;
    Ok(())
}

fn remove_pending_mutation(job_id: &str, user_agent_id: &str) -> Result<()> {
    let path = pending_state_path(job_id, user_agent_id)?;
    match fs::remove_file(&path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error)
            .with_context(|| format!("remove Refund V2 reconciliation state {}", path.display())),
    }
}

fn is_definitive_api_rejection(error: &anyhow::Error) -> bool {
    error.chain().any(|cause| {
        cause
            .downcast_ref::<crate::wallet_api::ApiCodeError>()
            .is_some_and(|error| (200..300).contains(&error.http_status))
    })
}

fn mutation_outcome_may_be_unknown(error: &anyhow::Error) -> bool {
    error.chain().any(|cause| {
        let message = cause.to_string();
        message == "broadcast failed" || message.ends_with("result is unknown")
    })
}

fn pending_reconciliation_payload(
    snapshot: &RefundSnapshot,
    reason: Option<&str>,
    plan: &Plan,
    pending: &PendingRefundMutation,
) -> Value {
    let mut payload = snapshot.payload(reason, plan);
    payload["capability"]["clientOperation"] = Value::Null;
    payload["settlement"]["state"] = Value::String(pending.state.clone());
    payload["settlement"]["txHash"] = Value::Null;
    payload["settlement"]["broadcastReceipt"] = json!({
        "pkgId": pending.pkg_id.clone(),
        "orderId": pending.order_id.clone(),
        "orderType": pending.order_type.clone(),
        "bizUniqKey": pending.biz_uniq_key.clone(),
        "bizType": pending.biz_type,
        "txHash": pending.tx_hash.clone(),
    });
    payload["settlement"]["retrySafe"] = Value::Bool(false);
    payload["settlement"]["diagnostic"] =
        Value::String("query_authoritative_state_before_retry".to_string());
    payload
}

impl Plan {
    fn blocked(reason: &'static str) -> Self {
        Self {
            phase: "refund_eligibility",
            decision: "blocked",
            reason,
            operation: None,
            action_id: None,
            recommend_stop: false,
        }
    }

    fn executable(
        phase: &'static str,
        reason: &'static str,
        action_id: &'static str,
        operation: RefundOperation,
    ) -> Self {
        Self {
            phase,
            decision: "requires_user_input",
            reason,
            operation: Some(operation),
            action_id: Some(action_id),
            recommend_stop: false,
        }
    }
}

fn scalar_string(value: Option<&Value>) -> Option<String> {
    let value = value?;
    value
        .as_str()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
        .or_else(|| value.as_i64().map(|value| value.to_string()))
        .or_else(|| value.as_u64().map(|value| value.to_string()))
}

fn first_string(values: &[(&Value, &[&str])]) -> Option<String> {
    for (value, keys) in values {
        for key in *keys {
            if let Some(result) = scalar_string(value.get(*key)) {
                return Some(result);
            }
        }
    }
    None
}

fn scalar_i64(value: Option<&Value>) -> Option<i64> {
    let value = value?;
    value
        .as_i64()
        .or_else(|| value.as_u64().and_then(|value| i64::try_from(value).ok()))
        .or_else(|| value.as_str().and_then(|value| value.trim().parse().ok()))
}

fn first_i64(values: &[(&Value, &[&str])]) -> Option<i64> {
    for (value, keys) in values {
        for key in *keys {
            if let Some(result) = scalar_i64(value.get(*key)) {
                return Some(result);
            }
        }
    }
    None
}

pub(crate) fn validate_decimal(value: &str) -> bool {
    let mut parts = value.split('.');
    let whole = parts.next().unwrap_or_default();
    let fraction = parts.next();
    parts.next().is_none()
        && !whole.is_empty()
        && whole.bytes().all(|byte| byte.is_ascii_digit())
        && fraction
            .is_none_or(|part| !part.is_empty() && part.bytes().all(|byte| byte.is_ascii_digit()))
}

pub(crate) fn is_zero_decimal(value: &str) -> bool {
    validate_decimal(value) && value.bytes().all(|byte| byte == b'0' || byte == b'.')
}

pub(crate) fn valid_tx_hash(value: &str) -> bool {
    let hash = value.trim().strip_prefix("0x").unwrap_or_default();
    hash.len() == 64 && hash.bytes().all(|byte| byte.is_ascii_hexdigit())
}

fn canonical_decimal(value: &str) -> Option<String> {
    if !validate_decimal(value) {
        return None;
    }
    let (whole, fraction) = value.split_once('.').unwrap_or((value, ""));
    let whole = whole.trim_start_matches('0');
    let whole = if whole.is_empty() { "0" } else { whole };
    let fraction = fraction.trim_end_matches('0');
    Some(if fraction.is_empty() {
        whole.to_string()
    } else {
        format!("{whole}.{fraction}")
    })
}

fn decimal_equal(left: &str, right: &str) -> bool {
    canonical_decimal(left)
        .zip(canonical_decimal(right))
        .is_some_and(|(left, right)| left == right)
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct RefundSettlementEvidence {
    pub(crate) provider_name: String,
    pub(crate) provider_agent_id: String,
    pub(crate) service_name: String,
    pub(crate) amount: String,
    pub(crate) token_symbol: String,
    pub(crate) tx_hash: Option<String>,
}

/// Whether fresh authoritative task facts prove refund settlement.
///
/// The backend lifecycle contract defines paid Expired(8), paid one-time escrow
/// Closed(7), and paid Failed(9) as states projected after the corresponding
/// refund result. For a formal subscription, Failed(9) is the documented
/// refunded terminal state.
pub(crate) fn authoritative_refund_settlement_confirmed(
    detail: &common::PreFetchedTaskContext,
    expected_status: i64,
) -> bool {
    if detail.status != Some(expected_status) || !matches!(expected_status, 7 | 8 | 9) {
        return false;
    }
    if expected_status == 8 {
        return validate_decimal(detail.token_amount.trim())
            && !is_zero_decimal(detail.token_amount.trim())
            && match detail.job_type {
                Some(0) => true,
                Some(1) => detail.trial_type == Some(0),
                _ => false,
            };
    }
    let has_positive_payment = validate_decimal(detail.token_amount.trim())
        && !is_zero_decimal(detail.token_amount.trim());
    match expected_status {
        9 => {
            has_positive_payment
                && (detail.job_type == Some(0)
                    || (detail.job_type == Some(1) && detail.trial_type != Some(1)))
        }
        7 => detail.job_type == Some(0) && detail.payment_mode == Some(1) && has_positive_payment,
        _ => false,
    }
}

/// Read the refund result from fresh lifecycle state. The event identifies why
/// the query ran; the latest status carries the settlement meaning.
pub(crate) fn refund_event_settlement_confirmed(
    detail: &common::PreFetchedTaskContext,
    expected_status: i64,
    _event: &str,
) -> bool {
    authoritative_refund_settlement_confirmed(detail, expected_status)
}

/// Verify a refund lifecycle event against fresh authoritative task facts.
/// Caller-supplied event fields may veto an inconsistent notification, but
/// they never create settlement proof or replace authoritative display data.
/// For Expired(8), fresh status and ownership are the complete business result,
/// so caller-supplied event fields cannot veto or reinterpret it.
pub(crate) fn verify_final_refund_event(
    message: Option<&Value>,
    prefetched: Option<&common::PreFetchedTaskContext>,
    expected_status: i64,
    expected_buyer_agent_id: &str,
) -> Result<RefundSettlementEvidence> {
    let empty_message = Value::Null;
    let message = if expected_status == 8 {
        &empty_message
    } else {
        message.unwrap_or(&empty_message)
    };
    let detail =
        prefetched.ok_or_else(|| anyhow::anyhow!("fresh authoritative task detail is missing"))?;
    if !matches!(expected_status, 7 | 8 | 9) || detail.status != Some(expected_status) {
        bail!("fresh task status does not match a refund-capable lifecycle state");
    }
    if expected_status == 7 {
        match detail.job_type {
            Some(0) if detail.payment_mode != Some(1) => {
                bail!("paid close is not proven to be the escrow direct-refund path");
            }
            Some(0) => {}
            Some(1) => {
                bail!("subscription Closed(7) lacks an authoritative refund-cause contract");
            }
            Some(other) => {
                bail!("fresh task detail returned unsupported jobType={other}");
            }
            None => bail!("fresh task detail is missing jobType"),
        }
    }
    if detail.user_agent_id.as_deref() != Some(expected_buyer_agent_id) {
        bail!("fresh task detail is not owned by the current User Agent");
    }
    let event = first_string(&[(message, &["event"])]).unwrap_or_default();
    if !refund_event_settlement_confirmed(detail, expected_status, &event) {
        bail!("fresh task detail does not confirm refund settlement");
    }
    if first_string(&[(message, &["code", "txStatus"])]).is_some_and(|value| {
        matches!(
            value.to_ascii_lowercase().as_str(),
            "failed" | "failure" | "error"
        )
    }) {
        bail!("refund transaction-result notification reports failure");
    }

    let authoritative_tx_hash = detail
        .verified_transaction_hash
        .as_deref()
        .filter(|value| valid_tx_hash(value))
        .map(ToOwned::to_owned);
    // Event payloads are caller-provided routing input. Their generic txHash
    // may identify a request, dispute, or unrelated lifecycle transaction, so
    // it neither creates nor overrides the verified operation proof above.

    let event_provider_id = first_string(&[(message, &["providerAgentId", "aspAgentId"])]);
    let provider_agent_id = detail
        .provider_agent_id
        .clone()
        .unwrap_or_else(|| "unavailable".to_string());
    if let (Some(event), Some(fresh)) = (
        event_provider_id.as_deref(),
        detail.provider_agent_id.as_deref(),
    ) {
        if event != fresh {
            bail!("refund event provider does not match fresh task detail");
        }
    }
    let provider_name = detail
        .provider_name
        .clone()
        .unwrap_or_else(|| "name unavailable".to_string());

    let event_service_id = first_string(&[(message, &["serviceId"])]);
    if let (Some(event), Some(fresh)) = (event_service_id.as_deref(), detail.service_id.as_deref())
    {
        if event != fresh {
            bail!("refund event service does not match fresh task detail");
        }
    }
    let service_name = detail
        .service_name
        .clone()
        .or_else(|| detail.service_id.clone())
        .unwrap_or_else(|| "service unavailable".to_string());
    if let (Some(event), Some(fresh)) = (
        first_string(&[(message, &["serviceName"])]).as_deref(),
        detail.service_name.as_deref(),
    ) {
        if event != fresh {
            bail!("refund event service name does not match fresh task detail");
        }
    }

    let original_amount = detail.token_amount.trim();
    if !validate_decimal(original_amount) || is_zero_decimal(original_amount) {
        bail!("fresh task detail is missing a valid paid original amount");
    }
    if let Some(event_amount) = first_string(&[(
        message,
        &["refundAmount", "paymentTokenAmount", "tokenAmount"],
    )]) {
        if !decimal_equal(&event_amount, original_amount) {
            bail!("refund event amount is not the full original payment");
        }
    }

    let fresh_token_symbol = detail.token_symbol.trim();
    let has_fresh_token_symbol = !fresh_token_symbol.is_empty() && fresh_token_symbol != "?";
    let event_token_symbol = first_string(&[(
        message,
        &["refundTokenSymbol", "paymentTokenSymbol", "tokenSymbol"],
    )]);
    if let (Some(event), true) = (event_token_symbol.as_deref(), has_fresh_token_symbol) {
        if !event.eq_ignore_ascii_case(fresh_token_symbol) {
            bail!("refund event token does not match the original payment token");
        }
    }
    let token_symbol = if has_fresh_token_symbol {
        fresh_token_symbol.to_string()
    } else {
        "token symbol unavailable".to_string()
    };
    let original_token_address = detail
        .token_address
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty());
    if expected_status != 8 && original_token_address.is_none() {
        bail!("fresh task detail is missing the original token address");
    }
    if let (Some(event_token_address), Some(original_token_address)) = (
        first_string(&[(
            message,
            &["refundTokenAddress", "paymentTokenAddress", "tokenAddress"],
        )]),
        original_token_address,
    ) {
        if !event_token_address.eq_ignore_ascii_case(original_token_address) {
            bail!("refund event token address does not match the original payment token");
        }
    }

    let event_buyer_id = first_string(&[(message, &["buyerAgentId", "userAgentId"])]);
    if let (Some(event), Some(fresh)) = (event_buyer_id.as_deref(), detail.user_agent_id.as_deref())
    {
        if event != fresh {
            bail!("refund event buyer does not match fresh task detail");
        }
    }

    Ok(RefundSettlementEvidence {
        provider_name,
        provider_agent_id,
        service_name,
        amount: original_amount.to_string(),
        token_symbol,
        tx_hash: authoritative_tx_hash,
    })
}

fn status_name(job_type: i64, status: i64) -> String {
    if job_type == 1 {
        match status {
            -1 => "init",
            0 => "created",
            1 => "active",
            3 => "rejected",
            4 => "disputed",
            6 => "completed",
            7 => "closed",
            8 => "expired",
            9 => "failed",
            _ => return format!("status_{status}"),
        }
        .to_string()
    } else {
        common::state_machine::Status::from_int(status as i32)
            .as_str()
            .to_string()
    }
}

fn refund_status_label(job_type: i64, status: i64, reason: &str) -> String {
    match reason {
        "refund_confirmed" => return "Refund completed".to_string(),
        "provider_response_pending" => return "Awaiting ASP decision".to_string(),
        "arbitration_in_progress" => return "Refund under evaluation".to_string(),
        "refund_not_approved_or_task_completed" => return "Refund not issued".to_string(),
        "trial_subscription_closed_without_refund" | "task_closed_no_new_refund_action" => {
            return "Closed without refund".to_string();
        }
        "expired_without_refundable_payment"
        | "zero_amount_task_closed"
        | "zero_amount_subscription_not_refundable" => {
            return "No refund required".to_string();
        }
        "refund_settlement_details_incomplete" => {
            return "Refund result unavailable".to_string();
        }
        _ => {}
    }
    if job_type != 1 {
        return super::super::common::query::task_status_label(status).to_string();
    }
    match status {
        -1 => "Initializing",
        0 => "Created",
        1 => "Active",
        3 => "Awaiting refund decision",
        4 => "Evaluation in progress",
        6 => "Completed",
        7 => "Closed",
        8 => "Expired",
        9 => "Refund completed",
        _ => "Status unavailable",
    }
    .to_string()
}

fn refund_status_description(job_type: i64, status: i64, reason: &str) -> String {
    match reason {
        "refund_confirmed" => return "The refund completed successfully.".to_string(),
        "provider_response_pending" => {
            return "The refund request is waiting for the ASP's decision.".to_string();
        }
        "arbitration_in_progress" => {
            return "The refund result will be determined by the Evaluation.".to_string();
        }
        "refund_not_approved_or_task_completed" => {
            return "The task completed without a refund.".to_string();
        }
        "trial_subscription_closed_without_refund" | "task_closed_no_new_refund_action" => {
            return "The task is closed and no refund was issued.".to_string();
        }
        "expired_without_refundable_payment"
        | "zero_amount_task_closed"
        | "zero_amount_subscription_not_refundable" => {
            return "This task has no refundable payment.".to_string();
        }
        "refund_settlement_details_incomplete" => {
            return "The current response does not contain a complete refund result.".to_string();
        }
        _ => {}
    }
    if job_type != 1 {
        return super::super::common::query::task_status_description(status).to_string();
    }
    match status {
        3 => "The refund request is waiting for the ASP's decision.",
        4 => "The refund request is in Evaluation.",
        6 => "The subscription completed without a refund.",
        7 => "The subscription is closed.",
        9 => "The refund completed successfully.",
        _ => "The refund status follows the current subscription state.",
    }
    .to_string()
}

impl RefundSnapshot {
    fn from_details(
        job_id: &str,
        task: &Value,
        subscription: Option<&Value>,
        current_user_agent_id: &str,
    ) -> Result<Self> {
        Self::from_details_with_expected_buyer(
            job_id,
            task,
            subscription,
            Some(current_user_agent_id),
        )
    }

    fn from_details_with_expected_buyer(
        job_id: &str,
        task: &Value,
        subscription: Option<&Value>,
        expected_user_agent_id: Option<&str>,
    ) -> Result<Self> {
        let job_type = scalar_i64(task.get("jobType"))
            .ok_or_else(|| anyhow::anyhow!("task detail is missing jobType"))?;
        if !matches!(job_type, 0 | 1) {
            bail!("task detail returned unsupported jobType={job_type}");
        }

        let mut sources = Vec::new();
        if let Some(subscription) = subscription {
            sources.push((subscription, &["subStatus", "status"][..]));
        }
        sources.push((task, &["subStatus", "status"][..]));
        let status =
            first_i64(&sources).ok_or_else(|| anyhow::anyhow!("task detail is missing status"))?;

        let lookup = |subscription_keys: &[&str], task_keys: &[&str]| {
            let mut values = Vec::new();
            if let Some(subscription) = subscription {
                values.push((subscription, subscription_keys));
            }
            values.push((task, task_keys));
            first_string(&values)
        };
        let lookup_i64 = |subscription_keys: &[&str], task_keys: &[&str]| {
            let mut values = Vec::new();
            if let Some(subscription) = subscription {
                values.push((subscription, subscription_keys));
            }
            values.push((task, task_keys));
            first_i64(&values)
        };

        let buyer_agent_id = lookup(
            &["buyerAgentId", "userAgentId"],
            &["buyerAgentId", "userAgentId"],
        )
        .ok_or_else(|| anyhow::anyhow!("task detail is missing buyerAgentId"))?;
        if expected_user_agent_id.is_some_and(|expected| buyer_agent_id != expected) {
            bail!("the selected task is not owned by the current User Agent");
        }

        let original_amount = lookup(
            &["paymentTokenAmount", "tokenAmount"],
            &["paymentTokenAmount", "tokenAmount"],
        )
        .ok_or_else(|| anyhow::anyhow!("task detail is missing the original token amount"))?;
        if !validate_decimal(&original_amount) {
            bail!("task detail returned an invalid original token amount");
        }

        let trial_type = if job_type == 1 {
            let trial_type = lookup_i64(&["trialType"], &["trialType"])
                .ok_or_else(|| anyhow::anyhow!("subscription detail is missing trialType"))?;
            if !matches!(trial_type, 0 | 1) {
                bail!("subscription detail returned unsupported trialType={trial_type}");
            }
            Some(trial_type)
        } else {
            None
        };
        let token_symbol = lookup(
            &["tokenSymbol", "paymentTokenSymbol"],
            &["tokenSymbol", "paymentTokenSymbol"],
        );
        let token_address = lookup(
            &["paymentTokenAddress", "tokenAddress"],
            &["paymentTokenAddress", "tokenAddress"],
        );
        if status != 8 && !is_zero_decimal(&original_amount) && token_address.is_none() {
            bail!("task detail is missing the original token address");
        }

        let period_index = lookup_i64(&["periodIndex"], &["periodIndex"]);
        let period_start_time = lookup_i64(
            &["subStartTime", "periodStartTime"],
            &["subStartTime", "periodStartTime"],
        );
        let period_end_time = lookup_i64(
            &["subEndTime", "periodEndTime"],
            &["subEndTime", "periodEndTime"],
        );
        // A valid billing window is required only when the buyer is asking to
        // refund an active, formally paid subscription period. The backend's
        // documented status-8 payload may legitimately use equal placeholder
        // timestamps, so parsing must preserve that snapshot even though the
        // write remains blocked until its timeout cause is authoritative.
        let active_formal_subscription = job_type == 1 && status == 1 && trial_type == Some(0);
        if active_formal_subscription
            && (period_index.is_some_and(|value| value < 0)
                || period_start_time.is_some_and(|value| value <= 0)
                || period_end_time.is_some_and(|value| value <= 0)
                || period_start_time
                    .zip(period_end_time)
                    .is_some_and(|(start, end)| start >= end))
        {
            bail!("subscription detail returned an invalid billing period");
        }

        let payment_mode = lookup_i64(&["paymentMode"], &["paymentMode"]);
        let mut snapshot = Self {
            job_id: job_id.to_string(),
            job_type,
            status,
            title: lookup(&["title", "jobTitle"], &["title", "jobTitle"]).unwrap_or_default(),
            buyer_agent_id,
            provider_agent_id: lookup(
                &["providerAgentId", "aspAgentId"],
                &["providerAgentId", "aspAgentId"],
            ),
            provider_name: lookup(
                &["providerAgentName", "aspAgentName", "providerName"],
                &["providerAgentName", "aspAgentName", "providerName"],
            ),
            service_id: lookup(&["serviceId"], &["serviceId"]),
            service_name: lookup(&["serviceName"], &["serviceName"]),
            revision: lookup(
                &["revision", "updatedAt", "updateTime"],
                &["revision", "updatedAt", "updateTime"],
            ),
            trial_type,
            period_index,
            period_start_time,
            period_end_time,
            auto_renew: lookup_i64(&["autoRenew"], &["autoRenew"]),
            token_address,
            token_symbol,
            chain_id: lookup_i64(&["chainId", "chainIndex"], &["chainId", "chainIndex"]),
            payment_mode,
            original_amount,
            response_deadline: lookup_i64(
                // `rejectDeadline` is the absolute ASP-response deadline on a
                // rejected task. Do not read `expireConfig.rejectDeadline`:
                // that nested value is a configured duration (for example
                // 1200), not a Unix timestamp suitable for display.
                &[
                    "rejectDeadline",
                    "rejectWindowEndsAt",
                    "responseDeadline",
                    "expireTime",
                ],
                &[
                    "rejectDeadline",
                    "rejectWindowEndsAt",
                    "responseDeadline",
                    "expireTime",
                ],
            ),
            requested_at: lookup_i64(
                &["refundRequestedAt", "rejectTime"],
                &["refundRequestedAt", "rejectTime"],
            ),
            recorded_refund_reason: lookup(
                &["refundReason", "rejectReason", "userReason"],
                &["refundReason", "rejectReason", "userReason"],
            ),
            // Tx Hash is optional display/audit metadata. The authoritative
            // task status is projected only after the backend consumes the
            // corresponding on-chain event, so it can prove settlement even
            // when the detail response does not expose a transaction hash.
            settlement_confirmed: false,
            settlement_tx_hash: None,
            settlement_provenance: None,
            settlement_time: lookup_i64(&["refundTime", "settledAt"], &["refundTime", "settledAt"]),
            dispute_round: lookup_i64(&["currentRound"], &["currentRound"]),
            dispute_phase: lookup(&["disputePhase"], &["disputePhase"]),
            dispute_prepare_end_time: lookup_i64(&["prepareEndTime"], &["prepareEndTime"]),
            dispute_round_end_time: lookup_i64(&["roundEndTime"], &["roundEndTime"]),
            refund_request_provenance: false,
        };

        // Expired(8) is projected only after any paid timeout refund reaches
        // the buyer. It is therefore authoritative settlement for a positive
        // one-time payment or formal subscription payment; trial and zero
        // expiry are terminal no-funds outcomes. Existing one-time Closed(7)
        // and Failed(9) finality remains unchanged.
        let paid_expired = snapshot.status == 8
            && !is_zero_decimal(&snapshot.original_amount)
            && match snapshot.job_type {
                0 => true,
                1 => snapshot.trial_type == Some(0),
                _ => false,
            };
        let paid_one_time_final = snapshot.job_type == 0
            && !is_zero_decimal(&snapshot.original_amount)
            && (snapshot.status == 9 || (snapshot.status == 7 && snapshot.payment_mode == Some(1)));
        let paid_subscription_refund_final = snapshot.job_type == 1
            && snapshot.trial_type == Some(0)
            && snapshot.status == 9
            && !is_zero_decimal(&snapshot.original_amount);
        if paid_expired || paid_one_time_final || paid_subscription_refund_final {
            snapshot.settlement_confirmed = true;
        }

        Ok(snapshot)
    }

    fn is_subscription(&self) -> bool {
        self.job_type == 1
    }

    fn is_trial(&self) -> bool {
        self.trial_type == Some(1)
    }

    fn refund_reason<'a>(&self, reason: Option<&'a str>) -> Option<&'a str> {
        let request_refund_state = (!self.is_subscription() && self.status == 2)
            || (self.is_subscription() && !self.is_trial() && self.status == 1);
        request_refund_state.then_some(reason).flatten()
    }

    fn has_required_refund_display_details(&self) -> bool {
        self.provider_agent_id.is_some()
            && self
                .token_symbol
                .as_deref()
                .is_some_and(|value| !value.trim().is_empty())
    }

    fn has_confirmed_settlement(&self) -> bool {
        self.settlement_confirmed
    }

    fn settlement_confirmation_source(&self) -> Option<&'static str> {
        if !self.has_confirmed_settlement() {
            None
        } else if self.status != 8 && self.is_subscription() && self.refund_request_provenance {
            Some("backend_onchain_lifecycle_with_local_refund_request")
        } else {
            Some("backend_onchain_lifecycle")
        }
    }

    fn context_id(&self, reason: Option<&str>) -> String {
        let canonical = json!({
            "jobId": self.job_id,
            "jobType": self.job_type,
            "status": self.status,
            "buyerAgentId": self.buyer_agent_id,
            "providerAgentId": self.provider_agent_id,
            "serviceId": self.service_id,
            "revision": self.revision,
            "trialType": self.trial_type,
            "periodIndex": self.period_index,
            "periodStartTime": self.period_start_time,
            "periodEndTime": self.period_end_time,
            "autoRenew": self.auto_renew,
            "tokenAddress": self.token_address,
            "tokenSymbol": self.token_symbol,
            "chainId": self.chain_id,
            "paymentMode": self.payment_mode,
            "originalAmount": self.original_amount,
            "userReason": self.refund_reason(reason),
        });
        let digest = Sha256::digest(serde_json::to_vec(&canonical).unwrap_or_default());
        format!("refundctx_{}", hex::encode(digest))
    }

    fn plan(&self, reason: Option<&str>) -> Plan {
        if self.is_trial() {
            return match self.status {
                1 if self.auto_renew == Some(1) => Plan::executable(
                    "refund_eligibility",
                    "trial_subscription_not_refundable",
                    "cancel_trial_conversion",
                    RefundOperation::CancelTrialConversion,
                ),
                1 if self.auto_renew == Some(0) => Plan {
                    phase: "refund_eligibility",
                    decision: "blocked",
                    reason: "trial_conversion_already_cancelled",
                    operation: None,
                    action_id: None,
                    recommend_stop: false,
                },
                1 => Plan::blocked("trial_conversion_state_unknown"),
                7 => Plan {
                    phase: "refund_resolution",
                    decision: "ready",
                    reason: "trial_subscription_closed_without_refund",
                    operation: None,
                    action_id: None,
                    recommend_stop: true,
                },
                8 => Plan {
                    phase: "refund_resolution",
                    decision: "ready",
                    reason: "expired_without_refundable_payment",
                    operation: None,
                    action_id: None,
                    recommend_stop: true,
                },
                _ => Plan::blocked("trial_subscription_not_refundable"),
            };
        }

        if !self.is_subscription() && is_zero_decimal(&self.original_amount) {
            return match self.status {
                0 => Plan::executable(
                    "refund_confirmation",
                    "zero_amount_close_confirmation_required",
                    "close_zero_price",
                    RefundOperation::CloseZero,
                ),
                7 => Plan {
                    phase: "refund_resolution",
                    decision: "ready",
                    reason: "zero_amount_task_closed",
                    operation: None,
                    action_id: None,
                    recommend_stop: true,
                },
                8 => Plan {
                    phase: "refund_resolution",
                    decision: "ready",
                    reason: "expired_without_refundable_payment",
                    operation: None,
                    action_id: None,
                    recommend_stop: true,
                },
                _ => Plan::blocked("zero_amount_close_contract_required"),
            };
        }

        if self.is_subscription() && is_zero_decimal(&self.original_amount) {
            if self.status == 8 {
                return Plan {
                    phase: "refund_resolution",
                    decision: "ready",
                    reason: "expired_without_refundable_payment",
                    operation: None,
                    action_id: None,
                    recommend_stop: true,
                };
            }
            return Plan::blocked("zero_amount_subscription_not_refundable");
        }

        let requires_refund_display_details = (!self.is_subscription()
            && matches!(self.status, 0 | 2))
            || (self.is_subscription() && self.status == 1);
        if requires_refund_display_details && !self.has_required_refund_display_details() {
            return Plan::blocked("refund_task_details_incomplete");
        }

        match self.status {
            0 if !self.is_subscription() && self.payment_mode == Some(1) => Plan::executable(
                "refund_confirmation",
                "direct_refund_confirmation_required",
                "execute_direct_refund",
                RefundOperation::DirectRefund,
            ),
            0 if !self.is_subscription() => Plan::blocked("direct_refund_funding_not_verified"),
            0 => Plan::blocked("direct_subscription_refund_contract_required"),
            1 if !self.is_subscription() => Plan::blocked("accepted_task_refund_contract_required"),
            1 | 2
                if (self.is_subscription() && self.status == 1)
                    || (!self.is_subscription() && self.status == 2) =>
            {
                if !self.is_subscription() && self.payment_mode != Some(1) {
                    return Plan::blocked("refund_payment_not_verified");
                }
                if self.is_subscription()
                    && (self.period_start_time.is_none() || self.period_end_time.is_none())
                {
                    return Plan::blocked("subscription_period_contract_required");
                }
                match reason {
                    None => Plan {
                        phase: "refund_reason_collection",
                        decision: "requires_user_input",
                        reason: "refund_reason_required",
                        operation: None,
                        action_id: Some("provide_refund_reason"),
                        recommend_stop: false,
                    },
                    Some(reason) if reason.trim().is_empty() => Plan {
                        phase: "refund_reason_collection",
                        decision: "requires_user_input",
                        reason: "refund_reason_required",
                        operation: None,
                        action_id: Some("provide_refund_reason"),
                        recommend_stop: false,
                    },
                    Some(reason) if reason.chars().count() > MAX_REASON_CHARS => Plan {
                        phase: "refund_reason_collection",
                        decision: "requires_user_input",
                        reason: "refund_reason_too_long",
                        operation: None,
                        action_id: Some("provide_refund_reason"),
                        recommend_stop: false,
                    },
                    Some(_) => Plan::executable(
                        "refund_confirmation",
                        "refund_request_confirmation_required",
                        "submit_refund_request",
                        RefundOperation::RequestRefund,
                    ),
                }
            }
            3 => Plan {
                phase: "refund_provider_response",
                decision: "blocked",
                reason: "provider_response_pending",
                operation: None,
                action_id: None,
                recommend_stop: false,
            },
            4 => Plan {
                phase: "refund_arbitration",
                decision: "blocked",
                reason: "arbitration_in_progress",
                operation: None,
                action_id: Some("view_arbitration"),
                recommend_stop: false,
            },
            8 if self.has_confirmed_settlement() => Plan {
                phase: "refund_resolution",
                decision: "ready",
                reason: "refund_confirmed",
                operation: None,
                action_id: None,
                recommend_stop: true,
            },
            8 => Plan::blocked("refund_settlement_details_incomplete"),
            9 if self.has_confirmed_settlement() => Plan {
                phase: "refund_resolution",
                decision: "ready",
                reason: "refund_confirmed",
                operation: None,
                action_id: None,
                recommend_stop: true,
            },
            9 => Plan {
                phase: "refund_resolution",
                decision: "blocked",
                reason: "refund_settlement_details_incomplete",
                operation: None,
                action_id: None,
                recommend_stop: false,
            },
            6 => Plan {
                phase: "refund_resolution",
                decision: "blocked",
                reason: "refund_not_approved_or_task_completed",
                operation: None,
                action_id: None,
                recommend_stop: true,
            },
            7 if !self.is_subscription()
                && self.payment_mode == Some(1)
                && self.has_confirmed_settlement() =>
            {
                Plan {
                    phase: "refund_resolution",
                    decision: "ready",
                    reason: "refund_confirmed",
                    operation: None,
                    action_id: None,
                    recommend_stop: true,
                }
            }
            7 if !self.is_subscription() => Plan {
                phase: "refund_resolution",
                decision: "blocked",
                reason: "refund_settlement_details_incomplete",
                operation: None,
                action_id: None,
                recommend_stop: false,
            },
            7 => Plan {
                phase: "refund_resolution",
                decision: "blocked",
                reason: "task_closed_no_new_refund_action",
                operation: None,
                action_id: None,
                // Subscription Closed(7) proves closure but not whether a
                // refund settled. Keep read-only reconciliation available
                // until an authoritative refund cause/result becomes
                // available for the subscription lifecycle.
                recommend_stop: false,
            },
            _ => Plan::blocked("refund_not_available_for_status"),
        }
    }

    fn refund_scope(&self) -> &'static str {
        if self.is_trial() || is_zero_decimal(&self.original_amount) {
            "none"
        } else if self.is_subscription() {
            "current_subscription_period"
        } else {
            "full_task_payment"
        }
    }

    fn settlement_state(&self) -> &'static str {
        match self.status {
            8 if self.has_confirmed_settlement() => "confirmed",
            8 => "not_required",
            9 if self.has_confirmed_settlement() => "confirmed",
            9 => "details_incomplete",
            7 if !self.is_subscription()
                && self.payment_mode == Some(1)
                && self.has_confirmed_settlement() =>
            {
                "confirmed"
            }
            7 => "details_incomplete",
            6 => "not_refunded",
            3 | 4 => "pending",
            _ => "not_started",
        }
    }

    fn refund_state(&self) -> &'static str {
        match self.status {
            0 => "created",
            1 | 2 => "active",
            3 => "provider_pending",
            4 => "arbitrating",
            8 => "resolved",
            6 => "resolved",
            7 | 9 if self.has_confirmed_settlement() => "resolved",
            7 | 9 => "settlement_unverified",
            _ => "unavailable",
        }
    }

    fn display_timestamp(timestamp: Option<i64>) -> Value {
        timestamp
            .and_then(common::deadline::format_local_timestamp_with_offset)
            .map(Value::String)
            .unwrap_or(Value::Null)
    }

    fn display_current_period(&self) -> Value {
        if !self.is_subscription() {
            return Value::Null;
        }
        let (Some(start_time), Some(end_time)) = (self.period_start_time, self.period_end_time)
        else {
            return Value::Null;
        };
        match (
            common::deadline::format_local_timestamp_with_offset(start_time),
            common::deadline::format_local_timestamp_with_offset(end_time),
        ) {
            (Some(start), Some(end)) => Value::String(format!("{start}–{end}")),
            _ => Value::Null,
        }
    }

    fn display_refund_amount(&self, refundable_amount: &str) -> Value {
        if is_zero_decimal(refundable_amount) {
            return Value::String("No refund required".to_string());
        }
        match self
            .token_symbol
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
        {
            Some(symbol) => Value::String(format!("{refundable_amount} {symbol}")),
            None => Value::Null,
        }
    }

    fn display_payload(&self, reason: Option<&str>, refundable_amount: &str) -> Value {
        let service_name = self
            .service_name
            .as_deref()
            .filter(|value| !value.trim().is_empty())
            .or_else(|| (!self.title.trim().is_empty()).then_some(self.title.as_str()))
            .map(|value| Value::String(value.to_string()))
            .unwrap_or(Value::Null);
        let result_deadline = self.response_deadline.or_else(|| {
            (self.is_subscription() && self.status == 1)
                .then_some(self.period_end_time)
                .flatten()
        });
        json!({
            "serviceName": service_name,
            "jobId": self.job_id,
            "serviceProviderName": self.provider_name,
            "agentId": self.provider_agent_id,
            "taskType": if self.is_subscription() { "Subscription" } else { "One-time" },
            "currentPeriod": self.display_current_period(),
            "refundAmount": self.display_refund_amount(refundable_amount),
            "reasonForRefund": reason.or(self.recorded_refund_reason.as_deref()),
            "requestedAt": Self::display_timestamp(self.requested_at),
            "resultDeadline": Self::display_timestamp(result_deadline),
        })
    }

    fn payload(&self, reason: Option<&str>, plan: &Plan) -> Value {
        let refund_flow_verified = matches!(
            plan.reason,
            "direct_refund_confirmation_required"
                | "refund_reason_required"
                | "refund_reason_too_long"
                | "refund_request_confirmation_required"
                | "provider_response_pending"
                | "arbitration_in_progress"
                | "refund_confirmed"
                | "refund_settlement_details_incomplete"
        );
        let provider_decision_flow = refund_flow_verified
            && ((!self.is_subscription() && matches!(self.status, 2 | 3 | 4))
                || (self.is_subscription() && matches!(self.status, 1 | 3 | 4)));
        let refundable_amount = if refund_flow_verified {
            self.original_amount.as_str()
        } else {
            "0"
        };
        let mut display = self.display_payload(reason, refundable_amount);
        let status_label = refund_status_label(self.job_type, self.status, plan.reason);
        let status_description = refund_status_description(self.job_type, self.status, plan.reason);
        display["statusLabel"] = Value::String(status_label.clone());
        display["statusDescription"] = Value::String(status_description.clone());
        let required_params = match plan.reason {
            "refund_reason_required" | "refund_reason_too_long" => json!(["reason"]),
            _ => json!([]),
        };
        // Tx Hash is optional display/audit metadata, not the source of truth
        // for refund completion. Keep it null for unverified states; a
        // confirmed backend lifecycle result may legitimately have no hash in
        // the current detail response.
        let settlement_tx_hash = if matches!(
            plan.reason,
            "refund_settlement_details_incomplete" | "task_closed_no_new_refund_action"
        ) {
            None
        } else {
            self.settlement_tx_hash.clone()
        };
        let settlement_provenance = (plan.reason == "refund_confirmed")
            .then(|| {
                self.settlement_provenance.as_ref().map(|proof| {
                    json!({
                        "source": proof.source,
                        "operation": proof.operation,
                        "orderId": proof.order_id,
                        "bizType": proof.biz_type,
                        "chainIndex": proof.chain_index,
                        "lifecycleStatus": proof.lifecycle_status,
                    })
                })
            })
            .flatten();
        let subscription = if self.is_subscription() {
            json!({
                "kind": if self.is_trial() { "trial" } else { "formal" },
                "trialType": self.trial_type,
                "periodIndex": self.period_index,
                "periodStartTime": self.period_start_time,
                "periodEndTime": self.period_end_time,
                "autoRenew": self.auto_renew,
            })
        } else {
            Value::Null
        };
        let current_period_label =
            self.period_start_time
                .zip(self.period_end_time)
                .and_then(|(start, end)| {
                    let start = common::deadline::format_utc_timestamp(start)?;
                    let end = common::deadline::format_utc_timestamp(end)?;
                    Some(format!("{start}–{end}"))
                });
        let refund_amount_label = if is_zero_decimal(&self.original_amount) {
            Some("No refund required".to_string())
        } else {
            self.token_symbol
                .as_deref()
                .filter(|symbol| !symbol.trim().is_empty())
                .map(|symbol| format!("{} {symbol}", self.original_amount))
        };
        let service_provider_label = self
            .provider_name
            .as_deref()
            .zip(self.provider_agent_id.as_deref())
            .map(|(name, agent_id)| format!("{name} (Agent ID : {agent_id})"));
        let response_deadline_label = self
            .response_deadline
            .and_then(common::deadline::format_utc_timestamp);
        display["taskTypeLabel"] = Value::String(
            if self.is_subscription() {
                "Subscription"
            } else {
                "One-time"
            }
            .to_string(),
        );
        display["serviceProviderLabel"] = service_provider_label
            .map(Value::String)
            .unwrap_or(Value::Null);
        display["currentPeriodLabel"] = current_period_label
            .map(Value::String)
            .unwrap_or(Value::Null);
        display["refundAmountLabel"] = refund_amount_label
            .map(Value::String)
            .unwrap_or(Value::Null);
        display["responseDeadlineLabel"] = response_deadline_label
            .map(Value::String)
            .unwrap_or(Value::Null);
        json!({
            "schemaVersion": SCHEMA_VERSION,
            "refundContextId": self.context_id(reason),
            "display": display,
            "job": {
                "jobId": self.job_id,
                "jobName": self.title,
                "jobType": if self.is_subscription() { "subscription" } else { "one_time" },
                "rawJobType": self.job_type,
                "refundState": self.refund_state(),
                "rawStatus": self.status,
                "statusName": status_name(self.job_type, self.status),
                "statusLabel": status_label,
                "statusDescription": status_description,
                "buyerAgentId": self.buyer_agent_id,
                "providerAgentId": self.provider_agent_id,
                "providerName": self.provider_name,
                "serviceId": self.service_id,
                "serviceName": self.service_name,
                "revision": self.revision,
            },
            "subscription": subscription,
            "payment": {
                "tokenAddress": self.token_address,
                "tokenSymbol": self.token_symbol,
                "chainId": self.chain_id,
                "paymentMode": self.payment_mode,
                "originalAmount": self.original_amount,
                "refundableAmount": refundable_amount,
                "refundScope": self.refund_scope(),
                "partialRefundSupported": false,
                "prorationSupported": false,
            },
            "input": {
                "requiredParams": required_params,
                "reasonMaxChars": MAX_REASON_CHARS,
            },
            "request": {
                "userReason": self.refund_reason(reason),
                "requestedAt": self.requested_at,
                "providerResponseDeadline": self.response_deadline,
                "providerNotification": {
                    "system": if self.status == 3 { "unknown" } else { "not_requested" },
                    "email": if self.status == 3 { "unknown" } else { "not_requested" },
                }
            },
            "rules": {
                "applies": refund_flow_verified,
                "providerMayAgreeOrDispute": provider_decision_flow,
                "providerTimeoutRefundExpected": provider_decision_flow,
                "refundUsesOriginalToken": refund_flow_verified,
                "fullRefundOnly": refund_flow_verified,
                "partialRefundSupported": false,
                "prorationSupported": false,
                "onchainConfirmationRequired": refund_flow_verified,
            },
            "settlement": {
                "state": self.settlement_state(),
                "cause": Value::Null,
                "txHash": settlement_tx_hash,
                "confirmationSource": self.settlement_confirmation_source(),
                "provenance": settlement_provenance,
                "broadcastReceipt": Value::Null,
                "confirmedAt": self.settlement_time,
                "onchainConfirmationRequired": refund_flow_verified,
            },
            "arbitration": {
                "phase": self.dispute_phase,
                "currentRound": self.dispute_round,
                "prepareEndTime": self.dispute_prepare_end_time,
                "roundEndTime": self.dispute_round_end_time,
                "outcome": if self.status == 6 { Some("not_refunded") } else { None::<&str> },
            },
            "capability": {
                "clientOperation": plan.operation.map(RefundOperation::as_str),
                "usesExistingLifecycleEndpoint": plan.operation.is_some(),
                "backendContractRequired": plan.reason.ends_with("contract_required")
                    || plan.reason.ends_with("contract_ambiguous"),
            }
        })
    }
}

fn base_decision(
    phase: &str,
    decision: &str,
    reason: &str,
    next_action: Value,
    payload: Value,
) -> Value {
    let mut result = Map::new();
    result.insert("phase".to_string(), Value::String(phase.to_string()));
    result.insert("decision".to_string(), Value::String(decision.to_string()));
    result.insert("reason".to_string(), Value::String(reason.to_string()));
    result.insert("nextAction".to_string(), next_action);
    result.insert("payload".to_string(), payload);
    Value::Object(result)
}

fn emit_decision(phase: &str, decision: &str, reason: &str, next_action: Value, payload: Value) {
    crate::output::success(base_decision(phase, decision, reason, next_action, payload));
}

fn action(id: &str, recommend: bool, params: Option<Value>) -> Value {
    let mut value = json!({"id": id, "recommend": recommend});
    if let Some(params) = params {
        value["params"] = params;
    }
    value
}

fn plan_actions(snapshot: &RefundSnapshot, plan: &Plan, reason: Option<&str>) -> Value {
    let mut actions = Vec::new();
    if let (Some(action_id), Some(operation)) = (plan.action_id, plan.operation) {
        let mut params = json!({
            "jobId": snapshot.job_id,
            "refundContextId": snapshot.context_id(reason),
            "operation": operation.as_str(),
            "expectedJobType": snapshot.job_type,
            "expectedStatus": snapshot.status,
            "expectedOriginalAmount": snapshot.original_amount,
        });
        if operation == RefundOperation::RequestRefund {
            if let Some(reason) = snapshot.refund_reason(reason) {
                params["reason"] = Value::String(reason.to_string());
            }
        }
        actions.push(action(action_id, true, Some(params)));
        actions.push(action("stop", false, None));
    } else if let Some(action_id) = plan.action_id {
        actions.push(action(
            action_id,
            true,
            Some(json!({"jobId": snapshot.job_id})),
        ));
        actions.push(action("stop", false, None));
    } else if plan.recommend_stop {
        actions.push(action("stop", true, None));
    } else {
        let view_id = if snapshot.status == 4 {
            "view_arbitration"
        } else {
            "view_refund_status"
        };
        actions.push(action(
            view_id,
            true,
            Some(json!({"jobId": snapshot.job_id})),
        ));
        actions.push(action(
            "watch_task",
            false,
            Some(json!({"jobId": snapshot.job_id})),
        ));
    }
    Value::Array(actions)
}

fn login_block(job_id: &str) {
    emit_decision(
        "login_validation",
        "blocked",
        "login_required",
        json!([action("login", true, Some(json!({"jobId": job_id})))]),
        json!({"schemaVersion": SCHEMA_VERSION}),
    );
}

fn identity_block(job_id: &str) {
    emit_decision(
        "identity_validation",
        "blocked",
        "user_identity_required",
        json!([action(
            "register_user_agent",
            true,
            Some(json!({"jobId": job_id}))
        )]),
        json!({"schemaVersion": SCHEMA_VERSION}),
    );
}

async fn current_user_agent_id(job_id: &str) -> Result<Option<String>> {
    if ensure_tokens_refreshed().await.is_err() {
        login_block(job_id);
        return Ok(None);
    }
    match super::create::resolve_user_agent().await {
        Ok((agent_id, _)) => Ok(Some(agent_id)),
        Err(error) if error.to_string().contains("no user identity") => {
            identity_block(job_id);
            Ok(None)
        }
        Err(error) => Err(error).context("failed to resolve current User Agent"),
    }
}

async fn fetch_snapshot(
    client: &mut TaskApiClient,
    job_id: &str,
    user_agent_id: &str,
) -> Result<RefundSnapshot> {
    let mut snapshot = fetch_snapshot_for_identity(client, job_id, user_agent_id, true).await?;
    if snapshot.provider_name.is_none() {
        if let Some(provider_agent_id) = snapshot.provider_agent_id.as_deref() {
            snapshot.provider_name = common::fetch_agent_profile(provider_agent_id).await.name;
        }
    }
    if snapshot.service_name.is_none() {
        if let (Some(provider_agent_id), Some(service_id)) = (
            snapshot.provider_agent_id.as_deref(),
            snapshot.service_id.as_deref(),
        ) {
            snapshot.service_name = common::find_service(provider_agent_id, service_id)
                .await?
                .and_then(|service| {
                    service
                        .get("serviceName")
                        .and_then(Value::as_str)
                        .map(str::trim)
                        .filter(|value| !value.is_empty())
                        .map(ToOwned::to_owned)
                });
        }
    }
    Ok(snapshot)
}

async fn fetch_snapshot_for_identity(
    client: &mut TaskApiClient,
    job_id: &str,
    caller_agent_id: &str,
    require_buyer_ownership: bool,
) -> Result<RefundSnapshot> {
    let task = client
        .get_with_identity(&client.task_path(job_id), caller_agent_id)
        .await
        .context("failed to fetch authoritative task detail")?;
    let job_type = scalar_i64(task.get("jobType"))
        .ok_or_else(|| anyhow::anyhow!("task detail is missing jobType"))?;
    let subscription = if job_type == 1 {
        Some(
            client
                .get_with_identity(&client.subscribe_path(job_id), caller_agent_id)
                .await
                .context("failed to fetch authoritative subscription detail")?,
        )
    } else {
        None
    };
    if require_buyer_ownership {
        RefundSnapshot::from_details(job_id, &task, subscription.as_ref(), caller_agent_id)
    } else {
        RefundSnapshot::from_details_with_expected_buyer(job_id, &task, subscription.as_ref(), None)
    }
}

pub(crate) async fn fetch_refund_list_item_for_identity(
    client: &mut TaskApiClient,
    job_id: &str,
    caller_agent_id: &str,
    require_buyer_ownership: bool,
) -> Result<RefundListItem> {
    let mut snapshot =
        fetch_snapshot_for_identity(client, job_id, caller_agent_id, require_buyer_ownership)
            .await?;
    if snapshot.provider_name.is_none() {
        if let Some(provider_agent_id) = snapshot.provider_agent_id.as_deref() {
            snapshot.provider_name = common::fetch_agent_profile(provider_agent_id).await.name;
        }
    }
    Ok(refund_list_item(snapshot))
}

fn refund_list_item(snapshot: RefundSnapshot) -> RefundListItem {
    let plan = snapshot.plan(None);
    let refund_request_available = plan.reason == "refund_reason_required";
    let deadline = snapshot.response_deadline.or_else(|| {
        (snapshot.is_subscription() && snapshot.status == 1)
            .then_some(snapshot.period_end_time)
            .flatten()
    });
    let mut display = snapshot.display_payload(None, &snapshot.original_amount);
    display["statusLabel"] = Value::String(refund_status_label(
        snapshot.job_type,
        snapshot.status,
        plan.reason,
    ));
    display["statusDescription"] = Value::String(refund_status_description(
        snapshot.job_type,
        snapshot.status,
        plan.reason,
    ));
    display["requestedRefund"] = display["refundAmount"].clone();
    display["buyerReason"] = display["reasonForRefund"].clone();
    display["responseDeadline"] = display["resultDeadline"].clone();
    RefundListItem {
        display,
        deadline,
        job_type: snapshot.job_type,
        status: snapshot.status,
        reason: plan.reason,
        refund_request_provenance: snapshot.refund_request_provenance,
        refund_request_available,
    }
}

impl From<RefundSnapshot> for common::PreFetchedTaskContext {
    fn from(snapshot: RefundSnapshot) -> Self {
        Self {
            title: snapshot.title,
            description: String::new(),
            job_type: Some(snapshot.job_type),
            trial_type: snapshot.trial_type,
            token_symbol: snapshot.token_symbol.unwrap_or_else(|| "?".to_string()),
            token_amount: snapshot.original_amount,
            payment_mode: snapshot.payment_mode,
            max_budget: None,
            provider_agent_id: snapshot.provider_agent_id,
            provider_name: snapshot.provider_name,
            user_agent_id: Some(snapshot.buyer_agent_id),
            status: Some(snapshot.status),
            deliverable: None,
            service_id: snapshot.service_id,
            service_name: snapshot.service_name,
            service_token_address: None,
            service_token_amount: None,
            service_params: None,
            refund_reason: snapshot.recorded_refund_reason,
            period_start_time: snapshot.period_start_time,
            period_end_time: snapshot.period_end_time,
            user_agent_address: None,
            token_address: snapshot.token_address,
            verified_transaction_hash: snapshot.settlement_tx_hash,
            refund_request_provenance: snapshot.refund_request_provenance,
            expire_time: snapshot.response_deadline,
            test_flag: false,
        }
    }
}

/// Final lifecycle events must use the exact same task/subscription
/// composition and ownership checks as the interactive Refund V2 commands.
pub(crate) async fn fetch_authoritative_refund_context(
    client: &mut TaskApiClient,
    job_id: &str,
    user_agent_id: &str,
) -> Result<common::PreFetchedTaskContext> {
    let mut snapshot = fetch_snapshot(client, job_id, user_agent_id).await?;
    let _ = reconcile_without_downgrading_confirmed_settlement(&mut snapshot).await?;
    Ok(snapshot.into())
}

/// Compose task/subscription detail for provider-side lifecycle freshness.
/// The caller identity is used for both reads, while provider ownership is
/// checked by the caller after composition rather than misapplying the buyer
/// ownership rule.
pub(crate) async fn fetch_authoritative_refund_context_for_provider(
    client: &mut TaskApiClient,
    job_id: &str,
    provider_agent_id: &str,
) -> Result<common::PreFetchedTaskContext> {
    fetch_snapshot_for_identity(client, job_id, provider_agent_id, false)
        .await
        .map(Into::into)
}

pub async fn handle_prepare(
    client: &mut TaskApiClient,
    job_id: &str,
    reason: Option<&str>,
) -> Result<()> {
    if job_id.trim().is_empty() {
        emit_decision(
            "refund_eligibility",
            "requires_user_input",
            "refund_target_required",
            json!([action("resolve_refund_target", true, None)]),
            json!({"schemaVersion": SCHEMA_VERSION}),
        );
        return Ok(());
    }
    let Some(user_agent_id) = current_user_agent_id(job_id).await? else {
        return Ok(());
    };
    let mut snapshot = fetch_snapshot(client, job_id, &user_agent_id).await?;
    let pending = reconcile_without_downgrading_confirmed_settlement(&mut snapshot).await?;
    let plan = snapshot.plan(reason);
    if let Some(pending) = pending {
        let operation = if pending.operation == RefundOperation::RequestRefund.as_str() {
            Some(RefundOperation::RequestRefund)
        } else {
            plan.operation
        };
        emit_decision(
            "refund_reconciliation",
            "blocked",
            "refund_operation_pending_reconciliation",
            reconcile_actions(job_id, operation),
            pending_reconciliation_payload(&snapshot, reason, &plan, &pending),
        );
        return Ok(());
    }
    let actions = plan_actions(&snapshot, &plan, reason);
    let payload = snapshot.payload(reason, &plan);
    emit_decision(plan.phase, plan.decision, plan.reason, actions, payload);
    Ok(())
}

fn strict_broadcast_receipt(mut receipt: Value) -> Result<Value> {
    if !receipt.is_object() {
        bail!("broadcast response did not contain a receipt object");
    }

    // The backend contract guarantees durable broadcast handles even when the
    // transaction has not been published yet. `txHash` is explicitly optional
    // in that state, so these identifiers—not a fabricated/pending hash—prove
    // that the broadcast request was accepted for reconciliation.
    for field in ["pkgId", "orderId", "orderType", "bizUniqKey"] {
        if scalar_string(receipt.get(field)).is_none() {
            bail!("broadcast response is missing {field}");
        }
    }

    let tx_hash = match receipt.get("txHash") {
        None | Some(Value::Null) => None,
        Some(Value::String(value)) if value.trim().is_empty() => None,
        Some(Value::String(value)) if valid_tx_hash(value) => Some(value.trim().to_string()),
        Some(Value::String(_)) => bail!("broadcast response returned an invalid transaction hash"),
        Some(_) => bail!("broadcast response returned a non-string transaction hash"),
    };
    if let Some(object) = receipt.as_object_mut() {
        object.insert(
            "txHash".to_string(),
            tx_hash.map(Value::String).unwrap_or(Value::Null),
        );
    }
    Ok(receipt)
}

fn lifecycle_biz_type(response: &Value, expected_biz_type: Option<i64>) -> Result<i64> {
    let biz_type = scalar_i64(response.get("type"))
        .filter(|value| *value > 0)
        .ok_or_else(|| anyhow::anyhow!("lifecycle endpoint did not return a valid type"))?;
    if let Some(expected) = expected_biz_type {
        if biz_type != expected {
            bail!("lifecycle endpoint returned unexpected type={biz_type}; expected {expected}");
        }
    }
    Ok(biz_type)
}

fn validate_lifecycle_preflight(uop_data: &Value) -> Result<()> {
    match uop_data.get("executeResult") {
        Some(Value::Bool(false)) => {
            let detail = scalar_string(uop_data.get("executeErrorMsg"))
                .unwrap_or_else(|| "no error detail returned".to_string());
            bail!("backend transaction preflight failed: {detail}")
        }
        // Legacy task lifecycle responses never required an explicit `true`.
        // The shared signer has always treated only an explicit boolean false
        // as a backend preflight rejection; preserve that wire contract for
        // Refund V2 because the backend response shape did not change.
        _ => Ok(()),
    }
}

async fn sign_response(
    client: &mut TaskApiClient,
    response: &Value,
    account_id: &str,
    address: &str,
    snapshot: &RefundSnapshot,
    reason: Option<&str>,
    expected_biz_type: Option<i64>,
) -> Result<Value> {
    let uop_data = response
        .get("uopData")
        .filter(|value| !value.is_null())
        .ok_or_else(|| anyhow::anyhow!("lifecycle endpoint did not return uopData"))?;
    let response_job_id = scalar_string(response.get("jobId"))
        .ok_or_else(|| anyhow::anyhow!("lifecycle endpoint did not return jobId"))?;
    if response_job_id != snapshot.job_id {
        bail!("lifecycle endpoint returned a mismatched jobId");
    }
    validate_lifecycle_preflight(uop_data)?;
    let biz_type = lifecycle_biz_type(response, expected_biz_type)?;
    let extra = reason.map(|reason| json!({"reason": reason}));
    let mut receipt = signing::sign_uop_and_broadcast_full(
        client,
        uop_data,
        account_id,
        address,
        &snapshot.job_id,
        biz_type,
        &snapshot.buyer_agent_id,
        extra.as_ref(),
    )
    .await?;
    receipt = strict_broadcast_receipt(receipt).context("broadcast receipt result is unknown")?;
    receipt["bizType"] = json!(biz_type);
    Ok(receipt)
}

async fn execute_regular_reject(
    client: &mut TaskApiClient,
    snapshot: &RefundSnapshot,
    reason: &str,
    account_id: &str,
    address: &str,
) -> Result<Value> {
    let deadline = chrono::Utc::now().timestamp() + 1_800;
    let pre_response = client
        .post_mutation_with_identity(
            &client.endpoint(&snapshot.job_id, "pre-reject"),
            &json!({"deadline": deadline}),
            &snapshot.buyer_agent_id,
        )
        .await
        .context("pre-reject result is unknown")?;
    let typed_data = pre_response
        .get("typedData")
        .filter(|value| !value.is_null())
        .ok_or_else(|| anyhow::anyhow!("pre-reject did not return typedData"))?;
    let nonce = pre_response
        .get("nonce")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| anyhow::anyhow!("pre-reject did not return nonce"))?;
    let signature = signing::sign_typed_data(typed_data, address).await?;
    let response = client
        .post_mutation_with_identity(
            &client.endpoint(&snapshot.job_id, "reject"),
            &json!({
                "signatureData": {
                    "signature": signature,
                    "deadline": deadline,
                    "nonce": nonce,
                }
            }),
            &snapshot.buyer_agent_id,
        )
        .await
        .context("reject result is unknown")?;
    sign_response(
        client,
        &response,
        account_id,
        address,
        snapshot,
        Some(reason),
        None,
    )
    .await
}

async fn execute_operation(
    client: &mut TaskApiClient,
    snapshot: &RefundSnapshot,
    operation: RefundOperation,
    reason: Option<&str>,
    account_id: &str,
    address: &str,
) -> Result<Value> {
    match operation {
        RefundOperation::CloseZero | RefundOperation::DirectRefund => {
            let response = client
                .post_mutation_with_identity(
                    &client.endpoint(&snapshot.job_id, "close"),
                    &json!({}),
                    &snapshot.buyer_agent_id,
                )
                .await
                .context("close result is unknown")?;
            sign_response(client, &response, account_id, address, snapshot, None, None).await
        }
        RefundOperation::CancelTrialConversion => {
            let response = client
                .post_mutation_with_identity(
                    &format!("{SUBSCRIBE_API_PREFIX}/{}/cancel", snapshot.job_id),
                    &json!({}),
                    &snapshot.buyer_agent_id,
                )
                .await
                .context("trial conversion cancellation result is unknown")?;
            sign_response(client, &response, account_id, address, snapshot, None, None).await
        }
        RefundOperation::RequestRefund => {
            let reason = reason.ok_or_else(|| anyhow::anyhow!("refund reason is required"))?;
            if snapshot.is_subscription() {
                let response = client
                    .post_mutation_with_identity(
                        &format!("{SUBSCRIBE_API_PREFIX}/{}/reject", snapshot.job_id),
                        &json!({}),
                        &snapshot.buyer_agent_id,
                    )
                    .await
                    .context("subscription refund request result is unknown")?;
                sign_response(
                    client,
                    &response,
                    account_id,
                    address,
                    snapshot,
                    Some(reason),
                    None,
                )
                .await
            } else {
                execute_regular_reject(client, snapshot, reason, account_id, address).await
            }
        }
    }
}

fn reconcile_actions(job_id: &str, operation: Option<RefundOperation>) -> Value {
    let is_request_refund = operation == Some(RefundOperation::RequestRefund);
    if is_request_refund {
        return Value::Array(vec![action(
            "view_refund_status",
            false,
            Some(json!({"jobId": job_id})),
        )]);
    }

    let follow_up = action("watch_task", false, Some(json!({"jobId": job_id})));
    Value::Array(vec![
        action("view_refund_status", true, Some(json!({"jobId": job_id}))),
        follow_up,
    ])
}

pub async fn handle_execute(
    client: &mut TaskApiClient,
    job_id: &str,
    operation: RefundOperation,
    refund_context_id: &str,
    reason: Option<&str>,
    confirm: bool,
) -> Result<()> {
    let Some(user_agent_id) = current_user_agent_id(job_id).await? else {
        return Ok(());
    };
    let mut snapshot = fetch_snapshot(client, job_id, &user_agent_id).await?;
    let pending = reconcile_without_downgrading_confirmed_settlement(&mut snapshot).await?;
    let plan = snapshot.plan(reason);

    if snapshot.context_id(reason) != refund_context_id {
        emit_decision(
            "refund_eligibility",
            "blocked",
            "refund_context_stale",
            json!([action(
                "prepare_refund",
                true,
                Some(json!({"jobId": job_id}))
            )]),
            snapshot.payload(reason, &plan),
        );
        return Ok(());
    }
    if plan.operation != Some(operation) {
        emit_decision(
            "refund_eligibility",
            "blocked",
            "refund_operation_not_available",
            plan_actions(&snapshot, &plan, reason),
            snapshot.payload(reason, &plan),
        );
        return Ok(());
    }
    if let Some(pending) = pending {
        emit_decision(
            "refund_reconciliation",
            "blocked",
            "refund_operation_pending_reconciliation",
            reconcile_actions(job_id, Some(operation)),
            pending_reconciliation_payload(&snapshot, reason, &plan, &pending),
        );
        return Ok(());
    }
    if !confirm {
        emit_decision(
            "refund_confirmation",
            "requires_user_input",
            "refund_execution_confirmation_required",
            plan_actions(&snapshot, &plan, reason),
            snapshot.payload(reason, &plan),
        );
        return Ok(());
    }

    let (account_id, address) = match signing::resolve_wallet_by_agent_id(&user_agent_id).await {
        Ok(wallet) => wallet,
        Err(error) => {
            audit::log(
                "cli",
                "user/refund_v2_wallet_preflight_failed",
                false,
                std::time::Duration::default(),
                Some(vec![format!("jobId={job_id}")]),
                Some("wallet resolution failed before any refund mutation"),
            );
            let mut payload = snapshot.payload(reason, &plan);
            payload["capability"]["clientOperation"] = Value::Null;
            payload["capability"]["diagnostic"] = Value::String(error.to_string());
            emit_decision(
                "refund_execution",
                "blocked",
                "refund_wallet_preflight_failed",
                json!([action("stop", true, None)]),
                payload,
            );
            return Ok(());
        }
    };

    let _lock = match acquire_pending_lock(job_id, &user_agent_id) {
        Ok(lock) => lock,
        Err(error) => {
            let mut payload = snapshot.payload(reason, &plan);
            payload["capability"]["clientOperation"] = Value::Null;
            payload["capability"]["diagnostic"] = Value::String(error.to_string());
            emit_decision(
                "refund_execution",
                "blocked",
                "refund_reconciliation_guard_unavailable",
                json!([action("stop", true, None)]),
                payload,
            );
            return Ok(());
        }
    };

    // A second read under the per-task execution lock closes the gap between
    // confirmation and mutation, including another local process winning the
    // lock while this command was resolving its wallet.
    let mut snapshot = fetch_snapshot(client, job_id, &user_agent_id).await?;
    let pending = match reconcile_pending_mutation_locked(&mut snapshot).await {
        Ok(pending) => pending,
        Err(_) if snapshot.has_confirmed_settlement() => None,
        Err(error) => return Err(error),
    };
    let plan = snapshot.plan(reason);
    if snapshot.context_id(reason) != refund_context_id || plan.operation != Some(operation) {
        emit_decision(
            "refund_eligibility",
            "blocked",
            "refund_context_stale",
            json!([action(
                "prepare_refund",
                true,
                Some(json!({"jobId": job_id}))
            )]),
            snapshot.payload(reason, &plan),
        );
        return Ok(());
    }
    if let Some(pending) = pending {
        emit_decision(
            "refund_reconciliation",
            "blocked",
            "refund_operation_pending_reconciliation",
            reconcile_actions(job_id, Some(operation)),
            pending_reconciliation_payload(&snapshot, reason, &plan, &pending),
        );
        return Ok(());
    }

    let mut pending = PendingRefundMutation {
        schema_version: SCHEMA_VERSION,
        journal_revision: JOURNAL_REVISION,
        job_id: job_id.to_string(),
        user_agent_id: user_agent_id.clone(),
        snapshot_id: snapshot.context_id(None),
        operation: operation.as_str().to_string(),
        // Persist `unknown` before the first mutation. If the process exits at
        // any point after this write, the next invocation must reconcile
        // authoritative state rather than repeat the operation.
        state: "unknown".to_string(),
        job_type: Some(snapshot.job_type),
        trial_type: snapshot.trial_type,
        period_index: snapshot.period_index,
        period_start_time: snapshot.period_start_time,
        period_end_time: snapshot.period_end_time,
        pkg_id: None,
        order_id: None,
        order_type: None,
        biz_uniq_key: None,
        tx_hash: None,
        account_id: Some(account_id.clone()),
        address: Some(address.clone()),
        // Task UserOperations are broadcast on X Layer by signing.rs. Persist
        // the actual broadcast chain rather than a display-chain field from
        // task detail, so later order reconciliation queries the same domain.
        chain_index: Some(common::XLAYER_CHAIN_INDEX.to_string()),
        biz_type: None,
        original_amount: Some(snapshot.original_amount.clone()),
        token_address: snapshot.token_address.clone(),
        token_symbol: snapshot.token_symbol.clone(),
        provider_agent_id: snapshot.provider_agent_id.clone(),
        service_id: snapshot.service_id.clone(),
        service_name: snapshot.service_name.clone(),
        payment_mode: snapshot.payment_mode,
        updated_at: chrono::Utc::now().timestamp(),
    };
    if let Err(error) = write_pending_mutation(&pending) {
        let mut payload = snapshot.payload(reason, &plan);
        payload["capability"]["clientOperation"] = Value::Null;
        payload["capability"]["diagnostic"] = Value::String(error.to_string());
        emit_decision(
            "refund_execution",
            "blocked",
            "refund_reconciliation_guard_unavailable",
            json!([action("stop", true, None)]),
            payload,
        );
        return Ok(());
    }

    let started = std::time::Instant::now();
    let receipt = match execute_operation(
        client,
        &snapshot,
        operation,
        reason,
        &account_id,
        &address,
    )
    .await
    {
        Ok(receipt) => receipt,
        Err(error) => {
            let definitive_rejection = is_definitive_api_rejection(&error);
            if definitive_rejection || !mutation_outcome_may_be_unknown(&error) {
                if let Err(remove_error) = remove_pending_mutation(job_id, &user_agent_id) {
                    pending.state = "unknown".to_string();
                    pending.updated_at = chrono::Utc::now().timestamp();
                    let _ = write_pending_mutation(&pending);
                    let mut payload =
                        pending_reconciliation_payload(&snapshot, reason, &plan, &pending);
                    payload["capability"]["diagnostic"] = Value::String(format!(
                        "backend rejected the write, but the local reconciliation guard could not be cleared: {remove_error}"
                    ));
                    emit_decision(
                        "refund_reconciliation",
                        "blocked",
                        "refund_operation_pending_reconciliation",
                        reconcile_actions(job_id, Some(operation)),
                        payload,
                    );
                    return Ok(());
                }
                let mut payload = snapshot.payload(reason, &plan);
                payload["capability"]["clientOperation"] = Value::Null;
                payload["capability"]["diagnostic"] = Value::String(error.to_string());
                emit_decision(
                    "refund_execution",
                    "blocked",
                    if definitive_rejection {
                        "refund_write_rejected"
                    } else {
                        "refund_prebroadcast_failed"
                    },
                    json!([action(
                        "prepare_refund",
                        true,
                        Some(json!({"jobId": job_id}))
                    )]),
                    payload,
                );
                return Ok(());
            }
            pending.state = "unknown".to_string();
            pending.updated_at = chrono::Utc::now().timestamp();
            let _ = write_pending_mutation(&pending);
            audit::log(
                "cli",
                "user/refund_v2_outcome_unknown",
                false,
                started.elapsed(),
                Some(vec![
                    format!("jobId={job_id}"),
                    format!("operation={}", operation.as_str()),
                ]),
                Some("mutation or broadcast outcome requires reconciliation"),
            );
            let mut payload = pending_reconciliation_payload(&snapshot, reason, &plan, &pending);
            payload["settlement"]["error"] = Value::String(error.to_string());
            emit_decision(
                "refund_settlement",
                "blocked",
                "refund_outcome_unknown",
                reconcile_actions(job_id, Some(operation)),
                payload,
            );
            return Ok(());
        }
    };

    pending.state = "broadcast_submitted".to_string();
    pending.pkg_id = scalar_string(receipt.get("pkgId"));
    pending.order_id = scalar_string(receipt.get("orderId"));
    pending.order_type = scalar_string(receipt.get("orderType"));
    pending.biz_uniq_key = scalar_string(receipt.get("bizUniqKey"));
    pending.tx_hash = scalar_string(receipt.get("txHash"));
    pending.biz_type = scalar_i64(receipt.get("bizType"));
    pending.updated_at = chrono::Utc::now().timestamp();
    if let Err(error) = write_pending_mutation(&pending) {
        // The broadcast was accepted, but without a durable local copy of its
        // reconciliation handles a later process could see only the pre-write
        // `unknown` marker. Keep this response fail-closed and surface the
        // accepted receipt so an operator can reconcile it; do not report the
        // operation as ready or perform any local lifecycle cleanup.
        audit::log(
            "cli",
            "user/refund_v2_receipt_persist_failed",
            false,
            started.elapsed(),
            Some(vec![
                format!("jobId={job_id}"),
                format!("agentId={user_agent_id}"),
                format!("operation={}", operation.as_str()),
            ]),
            Some("broadcast accepted but reconciliation receipt was not persisted"),
        );
        let mut payload = pending_reconciliation_payload(&snapshot, reason, &plan, &pending);
        payload["settlement"]["broadcastReceipt"] = receipt;
        payload["capability"]["diagnostic"] = Value::String(error.to_string());
        emit_decision(
            "refund_reconciliation",
            "blocked",
            "refund_outcome_unknown",
            reconcile_actions(job_id, Some(operation)),
            payload,
        );
        return Ok(());
    }

    match operation {
        RefundOperation::CloseZero | RefundOperation::DirectRefund => {
            let _ = super::negotiate::cleanup(job_id);
        }
        RefundOperation::CancelTrialConversion => {
            let _ = common::okx_a2a::mark_retired_autotrade_mode_decisions_handled(job_id);
        }
        RefundOperation::RequestRefund => {}
    }

    audit::log(
        "cli",
        "user/refund_v2_broadcast_submitted",
        true,
        started.elapsed(),
        Some(vec![
            format!("jobId={job_id}"),
            format!("agentId={user_agent_id}"),
            format!("operation={}", operation.as_str()),
            format!("reasonLen={}", reason.map(str::len).unwrap_or_default()),
            format!(
                "txHash={}",
                receipt
                    .get("txHash")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
            ),
        ]),
        None,
    );

    let result_reason = match operation {
        RefundOperation::CloseZero => "zero_amount_close_broadcast_submitted",
        RefundOperation::DirectRefund => "refund_broadcast_submitted",
        RefundOperation::RequestRefund => "refund_request_broadcast_submitted",
        RefundOperation::CancelTrialConversion => "trial_conversion_cancel_broadcast_submitted",
    };
    let mut payload = snapshot.payload(reason, &plan);
    payload["settlement"]["state"] = Value::String("broadcast_submitted".to_string());
    // `settlement.txHash` is optional metadata exposed only after wallet-order
    // confirmation. The broadcast candidate remains inside
    // `broadcastReceipt` while its exact wallet order is pending.
    payload["settlement"]["txHash"] = Value::Null;
    payload["settlement"]["broadcastReceipt"] = receipt;
    if operation == RefundOperation::RequestRefund {
        payload["request"]["providerNotification"]["system"] = Value::String("unknown".to_string());
        payload["request"]["providerNotification"]["email"] = Value::String("unknown".to_string());
    }
    emit_decision(
        "refund_settlement",
        "ready",
        result_reason,
        reconcile_actions(job_id, Some(operation)),
        payload,
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn task(job_type: Value, status: Value, amount: &str) -> Value {
        json!({
            "jobType": job_type,
            "status": status,
            "title": "Audit task",
            "buyerAgentId": "buyer-1",
            "providerAgentId": "asp-1",
            "providerAgentName": "Example ASP",
            "serviceId": "svc-1",
            "serviceName": "Audit",
            "tokenAmount": amount,
            "tokenSymbol": "USDT",
            "tokenAddress": "0xtoken",
            "paymentMode": 1,
            "updatedAt": 42,
        })
    }

    fn snapshot(job_type: Value, status: Value, amount: &str) -> RefundSnapshot {
        let task = task(job_type, status, amount);
        let subscription = (scalar_i64(task.get("jobType")) == Some(1)).then(|| {
            json!({
                "status": task["status"].clone(),
                "buyerAgentId": "buyer-1",
                "providerAgentId": "asp-1",
                "paymentTokenAmount": amount,
                "tokenSymbol": "USDT",
                "trialType": 0,
                "subStartTime": 1_700_000_000,
                "subEndTime": 1_700_100_000,
            })
        });
        RefundSnapshot::from_details("job-1", &task, subscription.as_ref(), "buyer-1").unwrap()
    }

    fn confirmed_direct_refund(snapshot: &RefundSnapshot, tx_hash: &str) -> PendingRefundMutation {
        PendingRefundMutation {
            schema_version: SCHEMA_VERSION,
            journal_revision: JOURNAL_REVISION,
            job_id: snapshot.job_id.clone(),
            user_agent_id: snapshot.buyer_agent_id.clone(),
            snapshot_id: snapshot.context_id(None),
            operation: "direct-refund".to_string(),
            state: "confirmed".to_string(),
            job_type: Some(snapshot.job_type),
            trial_type: snapshot.trial_type,
            period_index: snapshot.period_index,
            period_start_time: snapshot.period_start_time,
            period_end_time: snapshot.period_end_time,
            pkg_id: Some("pkg-1".to_string()),
            order_id: Some("order-1".to_string()),
            order_type: Some("AA".to_string()),
            biz_uniq_key: Some("refund-job-1".to_string()),
            tx_hash: Some(tx_hash.to_string()),
            account_id: Some("account-1".to_string()),
            address: Some("0xbuyer".to_string()),
            chain_index: Some("196".to_string()),
            biz_type: Some(200),
            original_amount: Some(snapshot.original_amount.clone()),
            token_address: snapshot.token_address.clone(),
            token_symbol: snapshot.token_symbol.clone(),
            provider_agent_id: snapshot.provider_agent_id.clone(),
            service_id: snapshot.service_id.clone(),
            service_name: snapshot.service_name.clone(),
            payment_mode: snapshot.payment_mode,
            updated_at: 1,
        }
    }

    fn submitted_request_refund(snapshot: &RefundSnapshot) -> PendingRefundMutation {
        let mut state = confirmed_direct_refund(snapshot, &format!("0x{}", "ab".repeat(32)));
        state.operation = "request-refund".to_string();
        state.state = "broadcast_submitted".to_string();
        state.biz_uniq_key = Some(format!("request-{}", snapshot.job_id));
        state
    }

    #[test]
    fn accepts_numeric_strings_for_job_type_and_status() {
        let snapshot = snapshot(json!("1"), json!("1"), "10.00");
        assert!(snapshot.is_subscription());
        assert_eq!(snapshot.status, 1);
        assert_eq!(snapshot.original_amount, "10.00");
    }

    #[test]
    fn undocumented_raw_detail_hash_alias_is_ignored() {
        let task = json!({
            "jobType": 1,
            "status": 1,
            "title": "Task title",
            "buyerAgentId": "stale-buyer",
            "providerAgentId": "asp-1",
            "serviceId": "svc-1",
            "paymentTokenAmount": "99",
            "tokenSymbol": "OLD",
            "tokenAddress": "0xold",
            "paymentMode": 1,
            "refundTxHash": format!("0x{}", "ab".repeat(32)),
        });
        let subscription = json!({
            "status": 9,
            "userAgentId": "buyer-1",
            "tokenAmount": "10",
            "paymentTokenSymbol": "USDT",
            "paymentTokenAddress": "0xtoken",
            "trialType": 0,
        });
        let snapshot =
            RefundSnapshot::from_details("job-1", &task, Some(&subscription), "buyer-1").unwrap();
        let context: common::PreFetchedTaskContext = snapshot.into();
        assert_eq!(context.status, Some(9));
        assert_eq!(context.user_agent_id.as_deref(), Some("buyer-1"));
        assert_eq!(context.token_amount, "10");
        assert_eq!(context.token_symbol, "USDT");
        assert_eq!(context.token_address.as_deref(), Some("0xtoken"));
        assert!(context.verified_transaction_hash.is_none());
    }

    #[test]
    fn provider_context_uses_subscription_first_without_buyer_identity_mischeck() {
        let task = json!({
            "jobType": 1,
            "status": 1,
            "buyerAgentId": "buyer-1",
            "providerAgentId": "asp-1",
            "serviceId": "svc-1",
            "paymentTokenAmount": "99",
            "tokenSymbol": "OLD",
            "tokenAddress": "0xold",
        });
        let subscription = json!({
            "status": 8,
            "userAgentId": "buyer-1",
            "providerAgentId": "asp-1",
            "serviceId": "svc-1",
            "tokenAmount": "10",
            "paymentTokenSymbol": "USDT",
            "paymentTokenAddress": "0xtoken",
            "trialType": 0,
        });
        let snapshot = RefundSnapshot::from_details_with_expected_buyer(
            "job-1",
            &task,
            Some(&subscription),
            None,
        )
        .unwrap();
        let context: common::PreFetchedTaskContext = snapshot.into();
        assert_eq!(context.status, Some(8));
        assert_eq!(context.provider_agent_id.as_deref(), Some("asp-1"));
        assert_eq!(context.user_agent_id.as_deref(), Some("buyer-1"));
        assert_eq!(context.token_amount, "10");
        assert_eq!(context.token_symbol, "USDT");
    }

    #[test]
    fn rejects_wrong_buyer_and_missing_core_fields() {
        let value = task(json!(0), json!(0), "1");
        let error = RefundSnapshot::from_details("job-1", &value, None, "buyer-2")
            .unwrap_err()
            .to_string();
        assert!(error.contains("not owned"));

        let mut missing = value;
        missing.as_object_mut().unwrap().remove("status");
        assert!(RefundSnapshot::from_details("job-1", &missing, None, "buyer-1").is_err());
    }

    #[test]
    fn exact_decimal_zero_does_not_use_float_parsing() {
        assert!(is_zero_decimal("0"));
        assert!(is_zero_decimal("000.000000"));
        assert!(!is_zero_decimal("0.000001"));
        assert!(!is_zero_decimal("0e0"));
        assert!(!is_zero_decimal("-0"));
    }

    #[test]
    fn one_time_matrix_uses_only_proven_lifecycle_operations() {
        assert_eq!(
            snapshot(json!(0), json!(0), "0.00").plan(None).operation,
            Some(RefundOperation::CloseZero)
        );
        assert_eq!(
            snapshot(json!(0), json!(0), "2.5").plan(None).operation,
            Some(RefundOperation::DirectRefund)
        );
        assert_eq!(
            snapshot(json!(0), json!(1), "2.5").plan(Some("late")),
            Plan::blocked("accepted_task_refund_contract_required")
        );
        assert_eq!(
            snapshot(json!(0), json!(2), "2.5")
                .plan(Some("wrong deliverable"))
                .operation,
            Some(RefundOperation::RequestRefund)
        );
    }

    #[test]
    fn active_refund_requires_user_authored_reason() {
        let snapshot = snapshot(json!(0), json!(2), "2.5");
        assert_eq!(snapshot.plan(None).reason, "refund_reason_required");
        assert_eq!(snapshot.plan(Some("   ")).reason, "refund_reason_required");
        assert_eq!(
            snapshot.plan(Some("not what I ordered")).reason,
            "refund_request_confirmation_required"
        );
    }

    #[test]
    fn missing_reason_still_exposes_complete_refund_confirmation_display() {
        let snapshot = snapshot(json!(0), json!(2), "2.5");
        let plan = snapshot.plan(None);
        let payload = snapshot.payload(None, &plan);
        let actions = plan_actions(&snapshot, &plan, None);

        assert_eq!(payload["display"]["serviceName"], json!("Audit"));
        assert_eq!(payload["display"]["jobId"], json!("job-1"));
        assert_eq!(
            payload["display"]["serviceProviderName"],
            json!("Example ASP")
        );
        assert_eq!(payload["display"]["taskType"], json!("One-time"));
        assert_eq!(payload["display"]["refundAmount"], json!("2.5 USDT"));
        assert!(payload["display"]["reasonForRefund"].is_null());
        assert_eq!(actions[0]["id"], json!("provide_refund_reason"));
    }

    #[test]
    fn trial_is_never_classified_as_refundable() {
        let task = task(json!(1), json!(1), "12");
        let subscription = json!({
            "status": 1,
            "buyerAgentId": "buyer-1",
            "paymentTokenAmount": "12",
            "tokenSymbol": "USDT",
            "trialType": 1,
            "autoRenew": 1,
        });
        let snapshot =
            RefundSnapshot::from_details("job-1", &task, Some(&subscription), "buyer-1").unwrap();
        let plan = snapshot.plan(Some("refund"));
        assert_eq!(plan.reason, "trial_subscription_not_refundable");
        assert_eq!(plan.operation, Some(RefundOperation::CancelTrialConversion));
        assert_eq!(snapshot.refund_scope(), "none");

        let mut already_cancelled = snapshot.clone();
        already_cancelled.auto_renew = Some(0);
        assert_eq!(
            already_cancelled.plan(None).reason,
            "trial_conversion_already_cancelled"
        );
        assert_eq!(already_cancelled.plan(None).operation, None);

        let mut unknown_conversion = snapshot;
        unknown_conversion.auto_renew = None;
        assert_eq!(
            unknown_conversion.plan(None).reason,
            "trial_conversion_state_unknown"
        );
        assert_eq!(unknown_conversion.plan(None).operation, None);
    }

    #[test]
    fn formal_subscription_created_fails_closed_without_invented_endpoint() {
        let snapshot = snapshot(json!(1), json!(0), "10");
        let plan = snapshot.plan(None);
        assert_eq!(plan.reason, "direct_subscription_refund_contract_required");
        assert_eq!(plan.operation, None);
    }

    #[test]
    fn created_paid_task_requires_verified_escrow_funding() {
        let mut task = task(json!(0), json!(0), "10");
        task["paymentMode"] = json!(0);
        let snapshot = RefundSnapshot::from_details("job-1", &task, None, "buyer-1").unwrap();
        assert_eq!(
            snapshot.plan(None).reason,
            "direct_refund_funding_not_verified"
        );
        assert_eq!(snapshot.plan(None).operation, None);
    }

    #[test]
    fn unknown_trial_type_and_missing_subscription_period_fail_closed() {
        let task = task(json!(1), json!(1), "10");
        let invalid_trial = json!({
            "status": 1,
            "buyerAgentId": "buyer-1",
            "paymentTokenAmount": "10",
            "tokenSymbol": "USDT",
            "trialType": 2,
        });
        assert!(
            RefundSnapshot::from_details("job-1", &task, Some(&invalid_trial), "buyer-1").is_err()
        );

        let missing_period = json!({
            "status": 1,
            "buyerAgentId": "buyer-1",
            "paymentTokenAmount": "10",
            "tokenSymbol": "USDT",
            "trialType": 0,
        });
        let snapshot =
            RefundSnapshot::from_details("job-1", &task, Some(&missing_period), "buyer-1").unwrap();
        assert_eq!(
            snapshot.plan(Some("service issue")).reason,
            "subscription_period_contract_required"
        );
    }

    #[test]
    fn formal_subscription_refund_is_limited_to_active_status() {
        assert_eq!(
            snapshot(json!(1), json!(1), "10")
                .plan(Some("service issue"))
                .operation,
            Some(RefundOperation::RequestRefund)
        );
        assert_eq!(
            snapshot(json!(1), json!(2), "10")
                .plan(Some("service issue"))
                .operation,
            None
        );
        let zero = snapshot(json!(1), json!(1), "0.00").plan(Some("service issue"));
        assert_eq!(zero.reason, "zero_amount_subscription_not_refundable");
        assert_eq!(zero.operation, None);
    }

    #[test]
    fn pending_arbitration_does_not_repeat_and_expired_is_terminal() {
        for status in [3, 4] {
            let plan = snapshot(json!(1), json!(status), "10").plan(Some("reason"));
            assert_eq!(plan.operation, None);
        }

        let expired_subscription = snapshot(json!(1), json!(8), "10");
        let plan = expired_subscription.plan(None);
        assert_eq!(plan.reason, "refund_confirmed");
        assert_eq!(plan.operation, None);
        assert_eq!(plan.action_id, None);
        let actions = plan_actions(&expired_subscription, &plan, None);
        assert_eq!(actions[0]["id"], "stop");
        let payload = expired_subscription.payload(None, &plan);
        assert_eq!(payload["payment"]["refundableAmount"], "10");
        assert_eq!(payload["settlement"]["state"], "confirmed");
        assert_eq!(
            payload["settlement"]["confirmationSource"],
            "backend_onchain_lifecycle"
        );
        assert_eq!(payload["settlement"]["txHash"], Value::Null);
        assert_eq!(payload["rules"]["providerTimeoutRefundExpected"], false);
        assert_eq!(payload["capability"]["clientOperation"], Value::Null);

        let expired_one_time = snapshot(json!(0), json!(8), "10");
        let plan = expired_one_time.plan(None);
        assert_eq!(plan.reason, "refund_confirmed");
        assert_eq!(plan.operation, None);
        assert_eq!(plan.action_id, None);
        assert_eq!(
            expired_one_time.payload(None, &plan)["capability"]["clientOperation"],
            Value::Null
        );
    }

    #[test]
    fn expired_subscription_equal_period_placeholder_is_refund_final() {
        let task = task(json!(1), json!(8), "10");
        let subscription = json!({
            "status": 8,
            "buyerAgentId": "buyer-1",
            "providerAgentId": "asp-1",
            "serviceId": "svc-1",
            "paymentTokenAmount": "10",
            "tokenSymbol": "USDT",
            "trialType": 0,
            "subStartTime": 1_700_000_000,
            "subEndTime": 1_700_000_000,
        });
        let snapshot =
            RefundSnapshot::from_details("job-1", &task, Some(&subscription), "buyer-1").unwrap();
        let plan = snapshot.plan(None);
        assert_eq!(plan.reason, "refund_confirmed");
        assert_eq!(snapshot.settlement_state(), "confirmed");
        assert_eq!(plan.operation, None);
    }

    #[test]
    fn v2_shape_is_stable_and_context_binds_only_request_refund_reason() {
        let first = snapshot(json!(0), json!(0), "10");
        let second = snapshot(json!(0), json!(2), "10");
        assert_ne!(first.context_id(None), second.context_id(None));
        assert_eq!(
            first.context_id(Some("reason A")),
            first.context_id(Some("reason B"))
        );
        assert_ne!(
            second.context_id(Some("reason A")),
            second.context_id(Some("reason B"))
        );
        let subscription = snapshot(json!(1), json!(1), "10");
        let mut next_period = subscription.clone();
        next_period.period_start_time = next_period.period_start_time.map(|value| value + 100_000);
        next_period.period_end_time = next_period.period_end_time.map(|value| value + 100_000);
        assert_ne!(
            subscription.context_id(Some("same reason")),
            next_period.context_id(Some("same reason"))
        );
        let plan = first.plan(None);
        let direct_actions = plan_actions(&first, &plan, Some("ignored for direct refund"));
        assert!(direct_actions[0]["params"].get("reason").is_none());
        assert_eq!(
            direct_actions[0]["params"]["refundContextId"],
            first.context_id(None)
        );
        let output = base_decision(
            plan.phase,
            plan.decision,
            plan.reason,
            plan_actions(&first, &plan, None),
            first.payload(None, &plan),
        );
        let keys = output
            .as_object()
            .unwrap()
            .keys()
            .cloned()
            .collect::<Vec<_>>();
        assert_eq!(
            keys,
            vec!["decision", "nextAction", "payload", "phase", "reason"]
        );
        assert_eq!(output["payload"]["schemaVersion"], SCHEMA_VERSION);
    }

    #[test]
    fn paid_write_requires_display_details_but_finality_does_not_depend_on_labels() {
        let mut missing_provider_identity = task(json!(0), json!(0), "10");
        missing_provider_identity
            .as_object_mut()
            .unwrap()
            .remove("providerAgentId");
        let snapshot =
            RefundSnapshot::from_details("job-1", &missing_provider_identity, None, "buyer-1")
                .unwrap();
        assert_eq!(snapshot.plan(None).reason, "refund_task_details_incomplete");
        assert_eq!(snapshot.plan(None).operation, None);

        let mut missing_write_token_symbol = task(json!(0), json!(0), "10");
        missing_write_token_symbol
            .as_object_mut()
            .unwrap()
            .remove("tokenSymbol");
        let snapshot =
            RefundSnapshot::from_details("job-1", &missing_write_token_symbol, None, "buyer-1")
                .unwrap();
        assert_eq!(snapshot.plan(None).reason, "refund_task_details_incomplete");
        assert_eq!(snapshot.plan(None).operation, None);

        let mut final_without_service = task(json!(0), json!(9), "10");
        final_without_service
            .as_object_mut()
            .unwrap()
            .remove("serviceName");
        final_without_service
            .as_object_mut()
            .unwrap()
            .remove("serviceId");
        final_without_service
            .as_object_mut()
            .unwrap()
            .remove("tokenSymbol");
        let snapshot =
            RefundSnapshot::from_details("job-1", &final_without_service, None, "buyer-1").unwrap();
        assert_eq!(snapshot.plan(None).reason, "refund_confirmed");
        assert!(snapshot.token_symbol.is_none());
        assert_eq!(snapshot.settlement_state(), "confirmed");
        let confirmed_plan = snapshot.plan(None);
        assert_eq!(
            snapshot.payload(None, &confirmed_plan)["settlement"]["txHash"],
            Value::Null
        );
        assert_eq!(
            snapshot.payload(None, &confirmed_plan)["settlement"]["confirmationSource"],
            "backend_onchain_lifecycle"
        );
        assert!(snapshot.payload(None, &confirmed_plan)["settlement"]["provenance"].is_null());
    }

    #[test]
    fn refund_payload_adds_display_labels_from_authoritative_fields() {
        let mut snapshot = snapshot(json!(1), json!(1), "10");
        snapshot.response_deadline = Some(1_700_200_000);
        let plan = snapshot.plan(Some("service not delivered"));
        let payload = snapshot.payload(Some("service not delivered"), &plan);

        assert_eq!(payload["display"]["taskTypeLabel"], "Subscription");
        assert_eq!(
            payload["display"]["serviceProviderLabel"],
            "Example ASP (Agent ID : asp-1)"
        );
        assert_eq!(
            payload["display"]["currentPeriodLabel"],
            "2023-11-14 22:13 (UTC+00:00)–2023-11-16 02:00 (UTC+00:00)"
        );
        assert_eq!(payload["display"]["refundAmountLabel"], "10 USDT");
        assert_eq!(
            payload["display"]["responseDeadlineLabel"],
            "2023-11-17 05:46 (UTC+00:00)"
        );
    }

    #[test]
    fn final_status_does_not_offer_duplicate_refund() {
        let backend_confirmed = snapshot(json!(0), json!(9), "10").plan(None);
        assert_eq!(backend_confirmed.reason, "refund_confirmed");
        assert_eq!(backend_confirmed.operation, None);
        assert_eq!(
            snapshot(json!(0), json!(9), "10").settlement_state(),
            "confirmed"
        );
        let confirmed_snapshot = snapshot(json!(0), json!(9), "10");
        let confirmed_payload = confirmed_snapshot.payload(None, &confirmed_snapshot.plan(None));
        assert_eq!(confirmed_payload["job"]["statusName"], "failed");
        assert_eq!(confirmed_payload["job"]["statusLabel"], "Refund completed");
        assert_eq!(
            confirmed_payload["display"]["statusDescription"],
            "The refund completed successfully."
        );

        let mut failed_without_payment_mode = task(json!(0), json!(9), "10");
        failed_without_payment_mode
            .as_object_mut()
            .unwrap()
            .remove("paymentMode");
        let failed_without_payment_mode =
            RefundSnapshot::from_details("job-1", &failed_without_payment_mode, None, "buyer-1")
                .unwrap();
        assert_eq!(
            failed_without_payment_mode.plan(None).reason,
            "refund_confirmed"
        );
        assert!(failed_without_payment_mode.settlement_tx_hash.is_none());

        let subscription_refund = snapshot(json!(1), json!(9), "10");
        assert_eq!(subscription_refund.plan(None).reason, "refund_confirmed");
        assert_eq!(subscription_refund.settlement_state(), "confirmed");
        let subscription_payload =
            subscription_refund.payload(None, &subscription_refund.plan(None));
        assert_eq!(
            subscription_payload["display"]["statusLabel"],
            "Refund completed"
        );

        let mut refunded_task = task(json!(0), json!(9), "10");
        refunded_task["refundTxHash"] = json!(format!("0x{}", "ab".repeat(32)));
        let refunded_snapshot =
            RefundSnapshot::from_details("job-1", &refunded_task, None, "buyer-1").unwrap();
        let refunded = refunded_snapshot.plan(None);
        assert_eq!(refunded.reason, "refund_confirmed");
        assert_eq!(refunded.operation, None);
        assert_eq!(refunded_snapshot.settlement_state(), "confirmed");
        assert!(refunded_snapshot.settlement_tx_hash.is_none());

        let mut closed_task = task(json!(0), json!(7), "10");
        closed_task["refundTxHash"] = json!(format!("0x{}", "cd".repeat(32)));
        let mut closed =
            RefundSnapshot::from_details("job-1", &closed_task, None, "buyer-1").unwrap();
        assert_eq!(closed.plan(None).reason, "refund_confirmed");
        assert!(closed.settlement_tx_hash.is_none());
        let proof = confirmed_direct_refund(&closed, &format!("0x{}", "cd".repeat(32)));
        assert!(apply_confirmed_direct_refund(&mut closed, &proof));
        assert_eq!(closed.plan(None).reason, "refund_confirmed");
        assert_eq!(closed.settlement_state(), "confirmed");
        let confirmed_payload = closed.payload(None, &closed.plan(None));
        assert_eq!(
            confirmed_payload["settlement"]["provenance"]["source"],
            "wallet_order_detail"
        );
        assert_eq!(
            confirmed_payload["settlement"]["provenance"]["operation"],
            "direct-refund"
        );
        assert_eq!(
            confirmed_payload["settlement"]["provenance"]["orderId"],
            "order-1"
        );
        assert_eq!(
            confirmed_payload["settlement"]["confirmationSource"],
            "backend_onchain_lifecycle"
        );
        let lost = snapshot(json!(0), json!(6), "10").plan(None);
        assert_eq!(lost.reason, "refund_not_approved_or_task_completed");
        assert_eq!(lost.operation, None);
    }

    #[test]
    fn refund_detail_display_describes_pending_and_terminal_results() {
        for (status, expected_label) in [
            (3, "Awaiting ASP decision"),
            (4, "Refund under evaluation"),
            (6, "Refund not issued"),
            (9, "Refund completed"),
        ] {
            let item = refund_list_item(snapshot(json!(0), json!(status), "10"));
            assert_eq!(
                item.display["statusLabel"], expected_label,
                "unexpected refund label for task status {status}"
            );
            assert!(item.display["statusDescription"]
                .as_str()
                .is_some_and(|value| !value.is_empty()));
        }

        let subscription = refund_list_item(snapshot(json!(1), json!(9), "10"));
        assert_eq!(subscription.reason, "refund_confirmed");
        assert_eq!(subscription.display["statusLabel"], "Refund completed");
        assert_eq!(
            subscription.display["statusDescription"],
            "The refund completed successfully."
        );
    }

    #[test]
    fn receipt_requires_durable_handles_but_allows_an_unpublished_transaction() {
        assert!(strict_broadcast_receipt(json!({})).is_err());
        let base = json!({
            "pkgId": "pkg-1",
            "orderId": "order-1",
            "orderType": "AA",
            "bizUniqKey": "refund-job-1",
        });
        let mut invalid = base.clone();
        invalid["txHash"] = json!("pending");
        assert!(strict_broadcast_receipt(invalid).is_err());

        let without_hash = strict_broadcast_receipt(base.clone()).unwrap();
        assert_eq!(without_hash["txHash"], Value::Null);

        let mut with_hash = base;
        with_hash["txHash"] = json!(format!("0x{}", "ab".repeat(32)));
        assert!(strict_broadcast_receipt(with_hash).is_ok());
    }

    #[test]
    fn wallet_order_status_requires_success_and_keeps_transaction_hash_optional() {
        let hash = format!("0x{}", "ab".repeat(32));
        assert_eq!(
            parse_refund_order_status(&json!([{"txStatus":"2", "txHash":hash}]), None),
            RefundOrderStatus::Pending
        );
        assert_eq!(
            parse_refund_order_status(&json!([{"txStatus":"3", "txHash":hash}]), None),
            RefundOrderStatus::Failed
        );
        assert_eq!(
            parse_refund_order_status(&json!([{"txStatus":"4", "txHash":hash}]), None),
            RefundOrderStatus::Succeeded(Some(hash.clone()))
        );
        assert_eq!(
            parse_refund_order_status(
                &json!([{"txStatus":"4", "txHash":hash}]),
                Some(&format!("0x{}", "cd".repeat(32)))
            ),
            RefundOrderStatus::Unknown
        );
        assert_eq!(
            parse_refund_order_status(&json!([{"txStatus":"4"}]), None),
            RefundOrderStatus::Succeeded(None)
        );
        assert_eq!(
            parse_refund_order_status(&json!([{"txStatus":"4"}]), Some(&hash)),
            RefundOrderStatus::Succeeded(None)
        );
        assert_eq!(
            parse_refund_order_status(
                &json!([{"txStatus":"4", "txHash":"not-a-hash"}]),
                Some(&hash)
            ),
            RefundOrderStatus::Unknown
        );
        assert_eq!(
            parse_refund_order_status(
                &json!([
                    {"txStatus":"4", "txHash":hash},
                    {"txStatus":"4", "txHash":hash}
                ]),
                None
            ),
            RefundOrderStatus::Unknown
        );
    }

    #[test]
    fn direct_refund_confirmation_requires_complete_operation_provenance() {
        let mut closed = snapshot(json!(0), json!(7), "10");
        let hash = format!("0x{}", "ab".repeat(32));
        let proof = confirmed_direct_refund(&closed, &hash);
        assert!(direct_refund_provenance_matches(&closed, &proof));
        assert!(apply_confirmed_direct_refund(&mut closed, &proof));

        let mut missing_type = proof.clone();
        missing_type.biz_type = None;
        assert!(!direct_refund_provenance_matches(&closed, &missing_type));

        let mut missing_account = proof.clone();
        missing_account.account_id = None;
        assert!(!direct_refund_provenance_matches(&closed, &missing_account));

        let mut wrong_chain = proof.clone();
        wrong_chain.chain_index = Some("1".to_string());
        assert!(!direct_refund_provenance_matches(&closed, &wrong_chain));

        let mut wrong_amount = proof.clone();
        wrong_amount.original_amount = Some("11".to_string());
        assert!(!direct_refund_provenance_matches(&closed, &wrong_amount));

        let mut wrong_token = proof.clone();
        wrong_token.token_address = Some("0xother".to_string());
        assert!(!direct_refund_provenance_matches(&closed, &wrong_token));

        let mut missing_symbol = proof.clone();
        missing_symbol.token_symbol = None;
        assert!(!direct_refund_provenance_matches(&closed, &missing_symbol));

        let mut wrong_provider = proof.clone();
        wrong_provider.provider_agent_id = Some("asp-2".to_string());
        assert!(!direct_refund_provenance_matches(&closed, &wrong_provider));

        let mut missing_service = proof.clone();
        missing_service.service_id = None;
        missing_service.service_name = None;
        assert!(!direct_refund_provenance_matches(&closed, &missing_service));

        let mut service_name_fallback = proof.clone();
        service_name_fallback.service_id = None;
        assert!(direct_refund_provenance_matches(
            &closed,
            &service_name_fallback
        ));

        let mut wrong_service = proof.clone();
        wrong_service.service_id = Some("svc-2".to_string());
        assert!(!direct_refund_provenance_matches(&closed, &wrong_service));

        let mut missing_payment_mode = proof;
        missing_payment_mode.payment_mode = None;
        assert!(!direct_refund_provenance_matches(
            &closed,
            &missing_payment_mode
        ));
    }

    #[test]
    fn request_refund_provenance_requires_core_binding_and_vetoes_known_conflicts() {
        let active = snapshot(json!(1), json!(1), "10");
        let mut terminal = active.clone();
        terminal.status = 9;
        let proof = submitted_request_refund(&active);
        assert!(request_refund_provenance_matches(&terminal, &proof));

        let mut unknown = proof.clone();
        unknown.state = "unknown".to_string();
        assert!(!request_refund_provenance_matches(&terminal, &unknown));

        let mut no_receipt = proof.clone();
        no_receipt.order_id = None;
        assert!(!request_refund_provenance_matches(&terminal, &no_receipt));

        let mut wrong_job_type = proof.clone();
        wrong_job_type.job_type = Some(0);
        assert!(!request_refund_provenance_matches(
            &terminal,
            &wrong_job_type
        ));

        let mut wrong_amount = proof.clone();
        wrong_amount.original_amount = Some("11".to_string());
        assert!(!request_refund_provenance_matches(&terminal, &wrong_amount));

        let mut wrong_period = proof.clone();
        wrong_period.period_end_time = wrong_period.period_end_time.map(|value| value + 1);
        assert!(!request_refund_provenance_matches(&terminal, &wrong_period));

        let mut optional_terminal_fields_missing = terminal.clone();
        optional_terminal_fields_missing.provider_agent_id = None;
        optional_terminal_fields_missing.service_id = None;
        optional_terminal_fields_missing.service_name = None;
        optional_terminal_fields_missing.token_symbol = None;
        optional_terminal_fields_missing.period_index = None;
        optional_terminal_fields_missing.period_start_time = None;
        optional_terminal_fields_missing.period_end_time = None;
        optional_terminal_fields_missing.payment_mode = None;
        assert!(request_refund_provenance_matches(
            &optional_terminal_fields_missing,
            &proof
        ));

        let mut incomplete_v3_journal = proof.clone();
        incomplete_v3_journal.provider_agent_id = None;
        incomplete_v3_journal.service_id = None;
        incomplete_v3_journal.service_name = None;
        incomplete_v3_journal.token_symbol = None;
        incomplete_v3_journal.period_index = None;
        incomplete_v3_journal.period_start_time = None;
        incomplete_v3_journal.period_end_time = None;
        incomplete_v3_journal.payment_mode = None;
        assert!(!request_refund_provenance_matches(
            &terminal,
            &incomplete_v3_journal
        ));

        let mut wrong_provider = proof.clone();
        wrong_provider.provider_agent_id = Some("other-asp".to_string());
        assert!(!request_refund_provenance_matches(
            &terminal,
            &wrong_provider
        ));

        let mut wrong_symbol = proof.clone();
        wrong_symbol.token_symbol = Some("DAI".to_string());
        assert!(!request_refund_provenance_matches(&terminal, &wrong_symbol));

        let mut wrong_payment_mode = proof.clone();
        wrong_payment_mode.payment_mode = Some(3);
        assert!(!request_refund_provenance_matches(
            &terminal,
            &wrong_payment_mode
        ));

        let one_time_active = snapshot(json!(0), json!(2), "10");
        let mut one_time_lost = one_time_active.clone();
        one_time_lost.status = 6;
        assert!(request_refund_provenance_matches(
            &one_time_lost,
            &submitted_request_refund(&one_time_active)
        ));
    }

    #[tokio::test(flavor = "current_thread")]
    async fn applied_request_is_durable_provenance_not_an_active_lock() {
        let _lock = crate::home::TEST_ENV_MUTEX.lock().unwrap();
        let previous_home = std::env::var_os("ONCHAINOS_HOME");
        let home = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("target")
            .join("test_tmp")
            .join("refund_v2_request_provenance");
        if home.exists() {
            fs::remove_dir_all(&home).unwrap();
        }
        fs::create_dir_all(&home).unwrap();
        std::env::set_var("ONCHAINOS_HOME", &home);

        let active = snapshot(json!(1), json!(1), "10");
        let submitted = submitted_request_refund(&active);
        write_pending_mutation(&submitted).unwrap();

        let mut provider_pending = active.clone();
        provider_pending.status = 3;
        assert!(reconcile_pending_mutation_locked(&mut provider_pending)
            .await
            .unwrap()
            .is_none());
        assert!(provider_pending.refund_request_provenance);
        let restored = read_pending_mutation(&active.job_id, &active.buyer_agent_id)
            .unwrap()
            .unwrap();
        assert_eq!(restored.state, "refund_request_applied");

        let mut dispute_lost = active.clone();
        dispute_lost.status = 6;
        assert!(reconcile_pending_mutation_locked(&mut dispute_lost)
            .await
            .unwrap()
            .is_none());
        assert!(dispute_lost.refund_request_provenance);
        assert!(!dispute_lost.has_confirmed_settlement());

        let mut terminal = active.clone();
        terminal.status = 9;
        assert!(reconcile_pending_mutation_locked(&mut terminal)
            .await
            .unwrap()
            .is_none());
        assert!(terminal.refund_request_provenance);
        assert!(terminal.has_confirmed_settlement());
        assert_eq!(terminal.plan(None).reason, "refund_confirmed");
        assert!(
            read_pending_mutation(&active.job_id, &active.buyer_agent_id)
                .unwrap()
                .is_some()
        );

        let context: common::PreFetchedTaskContext = terminal.into();
        assert!(context.refund_request_provenance);
        assert!(refund_event_settlement_confirmed(
            &context,
            9,
            "sub_asp_agree"
        ));
        assert!(refund_event_settlement_confirmed(
            &context,
            9,
            "sub_failed_notify"
        ));

        if let Some(previous_home) = previous_home {
            std::env::set_var("ONCHAINOS_HOME", previous_home);
        } else {
            std::env::remove_var("ONCHAINOS_HOME");
        }
        fs::remove_dir_all(&home).unwrap();
    }

    #[tokio::test(flavor = "current_thread")]
    async fn legacy_v2_request_receipt_migrates_and_survives_restart() {
        let _lock = crate::home::TEST_ENV_MUTEX.lock().unwrap();
        let previous_home = std::env::var_os("ONCHAINOS_HOME");
        let home = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("target")
            .join("test_tmp")
            .join("refund_v2_legacy_request_migration");
        if home.exists() {
            fs::remove_dir_all(&home).unwrap();
        }
        fs::create_dir_all(&home).unwrap();
        std::env::set_var("ONCHAINOS_HOME", &home);

        // Literal journal shape written by the already-shipped Refund V2:
        // no journalRevision, jobType, trialType, or billing-period fields.
        let legacy: PendingRefundMutation = serde_json::from_value(json!({
            "schemaVersion": 2,
            "jobId": "job-1",
            "userAgentId": "buyer-1",
            "snapshotId": "refundctx_legacy",
            "operation": "request-refund",
            "state": "broadcast_submitted",
            "pkgId": "pkg-1",
            "orderId": "order-1",
            "orderType": "AA",
            "bizUniqKey": "request-job-1",
            "txHash": null,
            "accountId": "account-1",
            "address": "0xbuyer",
            "chainIndex": "196",
            "bizType": 200,
            "originalAmount": "10.00",
            "tokenAddress": "0xtoken",
            "tokenSymbol": "USDT",
            "providerAgentId": "asp-1",
            "serviceId": "svc-1",
            "serviceName": "Audit",
            "paymentMode": 1,
            "updatedAt": 1
        }))
        .unwrap();
        assert_eq!(legacy.journal_revision, legacy_journal_revision());
        assert!(legacy.job_type.is_none());
        let terminal_before_migration = snapshot(json!(1), json!(9), "10.00");
        assert!(
            !request_refund_provenance_matches(&terminal_before_migration, &legacy),
            "an old receipt must not be upgraded for the first time from an ambiguous terminal status"
        );
        let mut terminal_without_period = terminal_before_migration.clone();
        terminal_without_period.period_start_time = None;
        terminal_without_period.period_end_time = None;
        let mut legacy_without_safe_v3_fields = legacy.clone();
        assert!(apply_legacy_request_refund_provenance_after_order_success(
            &mut terminal_without_period,
            &legacy_without_safe_v3_fields
        ));
        assert!(!upgrade_request_refund_journal(
            &mut legacy_without_safe_v3_fields,
            &terminal_without_period
        ));
        assert_eq!(
            legacy_without_safe_v3_fields.journal_revision,
            legacy_journal_revision(),
            "missing terminal period fields must keep the safely re-checkable legacy journal instead of writing an invalid v3 record"
        );
        write_pending_mutation(&legacy).unwrap();

        // Rejected(3) is the authoritative transition proving that the
        // request was applied. Reconciliation upgrades the old local receipt
        // instead of stranding it as permanently incomplete.
        let mut rejected = snapshot(json!(1), json!(3), "10");
        assert!(reconcile_pending_mutation_locked(&mut rejected)
            .await
            .unwrap()
            .is_none());
        assert!(rejected.refund_request_provenance);
        let migrated = read_pending_mutation("job-1", "buyer-1").unwrap().unwrap();
        assert_eq!(migrated.journal_revision, JOURNAL_REVISION);
        assert_eq!(migrated.state, "refund_request_applied");
        assert_eq!(migrated.job_type, Some(1));
        assert_eq!(migrated.trial_type, Some(0));
        assert_eq!(migrated.period_start_time, Some(1_700_000_000));
        assert_eq!(migrated.period_end_time, Some(1_700_100_000));

        // A new process later sees Failed(9). Optional display fields may be
        // absent from the terminal projection without erasing the durable
        // refund intent or the backend-confirmed settlement.
        let mut terminal = snapshot(json!(1), json!(9), "10.00");
        terminal.provider_agent_id = None;
        terminal.service_id = None;
        terminal.service_name = None;
        terminal.token_symbol = None;
        assert!(reconcile_pending_mutation_locked(&mut terminal)
            .await
            .unwrap()
            .is_none());
        assert!(terminal.refund_request_provenance);
        assert!(terminal.has_confirmed_settlement());
        assert_eq!(terminal.plan(None).reason, "refund_confirmed");

        if let Some(previous_home) = previous_home {
            std::env::set_var("ONCHAINOS_HOME", previous_home);
        } else {
            std::env::remove_var("ONCHAINOS_HOME");
        }
        fs::remove_dir_all(&home).unwrap();
    }

    #[test]
    fn expired_status_is_self_sufficient_refund_finality_for_paid_tasks() {
        for job_type in [0, 1] {
            let expired = snapshot(json!(job_type), json!(8), "10.00");
            assert!(expired.has_confirmed_settlement());
            assert_eq!(expired.plan(None).reason, "refund_confirmed");
            assert_eq!(expired.settlement_state(), "confirmed");
            assert_eq!(
                expired.settlement_confirmation_source(),
                Some("backend_onchain_lifecycle")
            );
            assert!(expired.settlement_tx_hash.is_none());

            let context: common::PreFetchedTaskContext = expired.into();
            assert!(authoritative_refund_settlement_confirmed(&context, 8));
            assert!(verify_final_refund_event(None, Some(&context), 8, "buyer-1").is_ok());
        }
    }

    #[test]
    fn expired_status_does_not_need_token_address_or_escrow_mode() {
        let mut detail = task(json!(0), json!(8), "10");
        detail.as_object_mut().unwrap().remove("tokenAddress");
        detail["paymentMode"] = json!(3);
        let expired = RefundSnapshot::from_details("job-1", &detail, None, "buyer-1").unwrap();
        assert!(expired.has_confirmed_settlement());
        assert_eq!(expired.plan(None).reason, "refund_confirmed");
    }

    #[test]
    fn trial_and_zero_amount_expiry_are_terminal_without_refund_funds() {
        let task = task(json!(1), json!(8), "10");
        let trial = json!({
            "status": 8,
            "buyerAgentId": "buyer-1",
            "paymentTokenAmount": "10",
            "tokenSymbol": "USDT",
            "trialType": 1,
        });
        let trial = RefundSnapshot::from_details("job-1", &task, Some(&trial), "buyer-1").unwrap();
        let trial_plan = trial.plan(None);
        assert!(!trial.has_confirmed_settlement());
        assert_eq!(trial_plan.reason, "expired_without_refundable_payment");
        assert_eq!(trial.settlement_state(), "not_required");
        assert_eq!(plan_actions(&trial, &trial_plan, None)[0]["id"], "stop");

        for job_type in [0, 1] {
            let zero = snapshot(json!(job_type), json!(8), "0");
            let plan = zero.plan(None);
            assert!(!zero.has_confirmed_settlement());
            assert_eq!(plan.reason, "expired_without_refundable_payment");
            assert_eq!(zero.settlement_state(), "not_required");
            assert_eq!(plan_actions(&zero, &plan, None)[0]["id"], "stop");
        }
    }

    #[test]
    fn wallet_order_detail_cannot_cross_order_or_chain_binding() {
        let snapshot = snapshot(json!(0), json!(7), "10");
        let proof = confirmed_direct_refund(&snapshot, &format!("0x{}", "ab".repeat(32)));
        assert!(validate_refund_order_detail_binding(
            &json!([{"orderId":"order-1", "chainIndex":"196"}]),
            &proof
        )
        .is_ok());
        assert!(validate_refund_order_detail_binding(
            &json!([{"orderId":"other", "chainIndex":"196"}]),
            &proof
        )
        .is_err());
        assert!(validate_refund_order_detail_binding(
            &json!([{"orderId":"order-1", "chainIndex":"1"}]),
            &proof
        )
        .is_err());
    }

    #[test]
    fn pending_receipt_hash_is_not_rendered_as_final_settlement_hash() {
        let snapshot = snapshot(json!(0), json!(0), "10");
        let mut pending = confirmed_direct_refund(&snapshot, &format!("0x{}", "ab".repeat(32)));
        pending.state = "broadcast_submitted".to_string();
        let plan = snapshot.plan(None);
        let payload = pending_reconciliation_payload(&snapshot, None, &plan, &pending);
        assert_eq!(payload["settlement"]["txHash"], Value::Null);
        assert_eq!(
            payload["settlement"]["broadcastReceipt"]["txHash"],
            pending.tx_hash.clone().unwrap()
        );
    }

    #[test]
    fn reconciliation_marker_is_durably_written_before_mutation() {
        let _lock = crate::home::TEST_ENV_MUTEX.lock().unwrap();
        let previous_home = std::env::var_os("ONCHAINOS_HOME");
        let home = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("target")
            .join("test_tmp")
            .join("refund_v2_durable_marker");
        if home.exists() {
            fs::remove_dir_all(&home).unwrap();
        }
        fs::create_dir_all(&home).unwrap();
        std::env::set_var("ONCHAINOS_HOME", &home);

        let snapshot = snapshot(json!(0), json!(0), "10");
        let pending = PendingRefundMutation {
            schema_version: SCHEMA_VERSION,
            journal_revision: JOURNAL_REVISION,
            job_id: snapshot.job_id.clone(),
            user_agent_id: snapshot.buyer_agent_id.clone(),
            snapshot_id: snapshot.context_id(None),
            operation: "direct-refund".to_string(),
            state: "unknown".to_string(),
            job_type: Some(snapshot.job_type),
            trial_type: snapshot.trial_type,
            period_index: snapshot.period_index,
            period_start_time: snapshot.period_start_time,
            period_end_time: snapshot.period_end_time,
            pkg_id: None,
            order_id: None,
            order_type: None,
            biz_uniq_key: None,
            tx_hash: None,
            account_id: None,
            address: None,
            chain_index: None,
            biz_type: None,
            original_amount: None,
            token_address: None,
            token_symbol: None,
            provider_agent_id: None,
            service_id: None,
            service_name: None,
            payment_mode: None,
            updated_at: 1,
        };
        write_pending_mutation(&pending).unwrap();
        let restored = read_pending_mutation(&pending.job_id, &pending.user_agent_id)
            .unwrap()
            .unwrap();
        assert_eq!(restored.operation, pending.operation);
        assert_eq!(restored.state, "unknown");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode =
                fs::metadata(pending_state_path(&pending.job_id, &pending.user_agent_id).unwrap())
                    .unwrap()
                    .permissions()
                    .mode()
                    & 0o777;
            assert_eq!(mode, 0o600);
        }

        if let Some(previous_home) = previous_home {
            std::env::set_var("ONCHAINOS_HOME", previous_home);
        } else {
            std::env::remove_var("ONCHAINOS_HOME");
        }
        fs::remove_dir_all(&home).unwrap();
    }

    #[test]
    fn lifecycle_biz_type_requires_a_positive_integer() {
        assert_eq!(
            lifecycle_biz_type(&json!({"type": 999}), None).unwrap(),
            999
        );
        assert!(lifecycle_biz_type(&json!({"type": 0}), None).is_err());
        assert!(lifecycle_biz_type(&json!({"type": -1}), None).is_err());
        assert!(lifecycle_biz_type(&json!({}), None).is_err());
    }

    #[test]
    fn lifecycle_uop_preserves_legacy_explicit_false_preflight_contract() {
        assert!(validate_lifecycle_preflight(&json!({"executeResult": true})).is_ok());
        let rejected = validate_lifecycle_preflight(&json!({
            "executeResult": false,
            "executeErrorMsg": "execution reverted"
        }))
        .unwrap_err()
        .to_string();
        assert!(rejected.contains("backend transaction preflight failed"));
        assert!(rejected.contains("execution reverted"));
        assert!(validate_lifecycle_preflight(&json!({})).is_ok());
        assert!(validate_lifecycle_preflight(&json!({"executeResult": null})).is_ok());
        assert!(validate_lifecycle_preflight(&json!({"executeResult": "true"})).is_ok());
    }

    #[test]
    fn formal_subscription_failed_status_is_the_refunded_terminal_state() {
        let mut detail = common::PreFetchedTaskContext::from_api_response(&json!({
            "jobType": 1,
            "status": 9,
            "buyerAgentId": "buyer-1",
            "providerAgentId": "asp-1",
            "serviceId": "svc-1",
            "paymentTokenAmount": "10.00",
            "paymentTokenSymbol": "USDT",
            "paymentTokenAddress": "0xtoken",
        }));

        let status_query_event = json!({"event": "sub_asp_agree", "code": 0});
        assert!(
            verify_final_refund_event(Some(&status_query_event), Some(&detail), 9, "buyer-1")
                .is_ok(),
            "fresh formal-subscription status 9 proves the documented refund result"
        );
        detail.refund_request_provenance = true;

        for event in [
            "sub_asp_agree",
            "sub_reject_refund_notify",
            "job_auto_refunded",
            "job_refunded",
            "dispute_resolved",
        ] {
            let message = json!({"event": event, "code": 0});
            assert!(
                verify_final_refund_event(Some(&message), Some(&detail), 9, "buyer-1").is_ok(),
                "legacy refund result event {event} must settle a fresh subscription Failed(9)"
            );
        }

        let generic_failure = json!({"event": "sub_failed_notify", "code": 0});
        assert!(
            verify_final_refund_event(Some(&generic_failure), Some(&detail), 9, "buyer-1").is_ok(),
            "fresh formal-subscription status 9 carries the documented refunded meaning"
        );
        assert_eq!(
            RefundSnapshot::from_details(
                "job-1",
                &json!({
                    "jobType": 1,
                    "trialType": 0,
                    "status": 9,
                    "buyerAgentId": "buyer-1",
                    "providerAgentId": "asp-1",
                    "serviceId": "svc-1",
                    "paymentTokenAmount": "10.00",
                    "paymentTokenSymbol": "USDT",
                    "paymentTokenAddress": "0xtoken",
                }),
                None,
                "buyer-1"
            )
            .unwrap()
            .plan(None)
            .reason,
            "refund_confirmed",
            "formal subscription Failed(9) is the documented refunded terminal state"
        );
    }

    #[test]
    fn final_refund_evidence_requires_fresh_full_original_payment() {
        let detail = common::PreFetchedTaskContext::from_api_response(&json!({
            "jobType": 0,
            "status": 9,
            "buyerAgentId": "buyer-1",
            "providerAgentId": "asp-1",
            "providerAgentName": "Alice ASP",
            "serviceId": "svc-1",
            "serviceName": "Audit",
            "paymentTokenAmount": "10.00",
            "paymentTokenSymbol": "USDT",
            "paymentTokenAddress": "0xtoken",
        }));
        let event = json!({
            "buyerAgentId": "buyer-1",
            "providerAgentId": "asp-1",
            "serviceId": "svc-1",
            "refundAmount": "10",
            "tokenSymbol": "usdt",
            "paymentTokenAddress": "0xToken",
            "txHash": format!("0x{}", "ab".repeat(32)),
        });
        let evidence =
            verify_final_refund_event(Some(&event), Some(&detail), 9, "buyer-1").unwrap();
        assert_eq!(evidence.amount, "10.00");
        assert_eq!(evidence.token_symbol, "USDT");
        assert!(evidence.tx_hash.is_none());

        let mut partial = event.clone();
        partial["refundAmount"] = json!("9.99");
        assert!(verify_final_refund_event(Some(&partial), Some(&detail), 9, "buyer-1").is_err());
        assert!(verify_final_refund_event(Some(&partial), None, 9, "buyer-1").is_err());

        let mut unrelated_hash = partial;
        unrelated_hash["refundAmount"] = json!("10");
        unrelated_hash["txHash"] = json!(format!("0x{}", "cd".repeat(32)));
        assert!(
            verify_final_refund_event(Some(&unrelated_hash), Some(&detail), 9, "buyer-1").is_ok()
        );
        assert!(verify_final_refund_event(Some(&event), Some(&detail), 9, "buyer-2").is_err());

        let mut non_escrow_close = common::PreFetchedTaskContext::from_api_response(&json!({
            "jobType": 0,
            "status": 7,
            "buyerAgentId": "buyer-1",
            "providerAgentId": "asp-1",
            "serviceId": "svc-1",
            "serviceName": "Audit",
            "paymentMode": 3,
            "paymentTokenAmount": "10.00",
            "paymentTokenSymbol": "USDT",
            "paymentTokenAddress": "0xtoken",
        }));
        non_escrow_close.verified_transaction_hash = Some(format!("0x{}", "ab".repeat(32)));
        assert!(
            verify_final_refund_event(Some(&event), Some(&non_escrow_close), 7, "buyer-1").is_err()
        );

        let mut subscription_decline = common::PreFetchedTaskContext::from_api_response(&json!({
            "jobType": 1,
            "status": 7,
            "buyerAgentId": "buyer-1",
            "providerAgentId": "asp-1",
            "serviceId": "svc-1",
            "serviceName": "Audit",
            "paymentTokenAmount": "10.00",
            "paymentTokenSymbol": "USDT",
            "paymentTokenAddress": "0xtoken",
        }));
        subscription_decline.verified_transaction_hash = Some(format!("0x{}", "ab".repeat(32)));
        assert!(
            verify_final_refund_event(Some(&event), Some(&subscription_decline), 7, "buyer-1")
                .is_err()
        );
        assert!(
            verify_final_refund_event(Some(&event), Some(&subscription_decline), 7, "buyer-1")
                .is_err()
        );
    }

    #[test]
    fn final_refund_evidence_uses_authoritative_labels_or_unavailable_fallbacks() {
        let base = json!({
            "jobType": 0,
            "status": 9,
            "buyerAgentId": "buyer-1",
            "providerAgentId": "asp-1",
            "serviceId": "svc-1",
            "paymentTokenAmount": "10.00",
            "paymentTokenSymbol": "USDT",
            "paymentTokenAddress": "0xtoken",
        });
        let event = json!({
            "buyerAgentId": "buyer-1",
            "providerAgentId": "asp-1",
            "providerAgentName": "Untrusted display name",
            "serviceId": "svc-1",
            "serviceName": "Untrusted service",
            "refundAmount": "10",
            "tokenSymbol": "USDT",
            "paymentTokenAddress": "0xtoken",
            "txHash": format!("0x{}", "ab".repeat(32)),
        });

        for (missing, expected_provider, expected_service) in [
            ("providerAgentId", "unavailable", "svc-1"),
            ("serviceId", "asp-1", "service unavailable"),
        ] {
            let mut detail = base.clone();
            detail.as_object_mut().unwrap().remove(missing);
            let detail = common::PreFetchedTaskContext::from_api_response(&detail);
            let evidence =
                verify_final_refund_event(Some(&event), Some(&detail), 9, "buyer-1").unwrap();
            assert_eq!(evidence.provider_agent_id, expected_provider);
            assert_eq!(evidence.service_name, expected_service);
        }

        let mut missing_symbol = base.clone();
        missing_symbol
            .as_object_mut()
            .unwrap()
            .remove("paymentTokenSymbol");
        let missing_symbol = common::PreFetchedTaskContext::from_api_response(&missing_symbol);
        let missing_symbol_evidence =
            verify_final_refund_event(Some(&event), Some(&missing_symbol), 9, "buyer-1").unwrap();
        assert_eq!(
            missing_symbol_evidence.token_symbol, "token symbol unavailable",
            "caller event labels must not fill an absent authoritative display field"
        );

        let detail = common::PreFetchedTaskContext::from_api_response(&base);
        let event_without_hash = json!({
            "buyerAgentId": "buyer-1",
            "providerAgentId": "asp-1",
            "serviceId": "svc-1",
            "refundAmount": "10",
            "tokenSymbol": "USDT",
            "paymentTokenAddress": "0xtoken",
        });
        assert!(
            verify_final_refund_event(Some(&event_without_hash), Some(&detail), 9, "buyer-1")
                .is_ok()
        );

        let evidence =
            verify_final_refund_event(Some(&event), Some(&detail), 9, "buyer-1").unwrap();
        assert_eq!(evidence.provider_name, "name unavailable");
        assert_eq!(evidence.service_name, "svc-1");
        assert!(evidence.tx_hash.is_none());
    }

    #[test]
    fn closed_subscription_keeps_read_only_refund_reconciliation_open() {
        let snapshot = snapshot(json!(1), json!(7), "10");
        let plan = snapshot.plan(None);
        assert_eq!(plan.reason, "task_closed_no_new_refund_action");
        assert!(!plan.recommend_stop);
        assert_eq!(snapshot.settlement_state(), "details_incomplete");

        let actions = plan_actions(&snapshot, &plan, None);
        assert_eq!(actions[0]["id"], "view_refund_status");
        assert_eq!(actions[1]["id"], "watch_task");
        assert!(snapshot.payload(None, &plan)["settlement"]["txHash"].is_null());
    }

    #[test]
    fn request_refund_reconciliation_exposes_optional_status_without_watch() {
        let actions = reconcile_actions("job-1", Some(RefundOperation::RequestRefund));
        assert_eq!(actions[0]["id"], "view_refund_status");
        assert_eq!(actions[0]["recommend"], false);
        assert_eq!(actions.as_array().unwrap().len(), 1);
        assert!(!actions
            .as_array()
            .unwrap()
            .iter()
            .any(|item| item["id"] == "watch_task"));

        let other_actions = reconcile_actions("job-1", Some(RefundOperation::DirectRefund));
        assert_eq!(other_actions[1]["id"], "watch_task");
    }

    #[test]
    fn pending_mutation_survives_snapshot_drift_until_lifecycle_advances() {
        let active_snapshot = snapshot(json!(1), json!(1), "10");
        let pending = PendingRefundMutation {
            schema_version: SCHEMA_VERSION,
            journal_revision: JOURNAL_REVISION,
            job_id: active_snapshot.job_id.clone(),
            user_agent_id: active_snapshot.buyer_agent_id.clone(),
            snapshot_id: active_snapshot.context_id(None),
            operation: "request-refund".to_string(),
            state: "unknown".to_string(),
            job_type: Some(active_snapshot.job_type),
            trial_type: active_snapshot.trial_type,
            period_index: active_snapshot.period_index,
            period_start_time: active_snapshot.period_start_time,
            period_end_time: active_snapshot.period_end_time,
            pkg_id: None,
            order_id: None,
            order_type: None,
            biz_uniq_key: None,
            tx_hash: None,
            account_id: None,
            address: None,
            chain_index: None,
            biz_type: None,
            original_amount: None,
            token_address: None,
            token_symbol: None,
            provider_agent_id: None,
            service_id: None,
            service_name: None,
            payment_mode: None,
            updated_at: 1,
        };
        assert!(!pending_mutation_resolved(&pending, &active_snapshot));

        let mut next_period = active_snapshot.clone();
        next_period.period_start_time = next_period.period_start_time.map(|value| value + 100_000);
        next_period.period_end_time = next_period.period_end_time.map(|value| value + 100_000);
        assert!(!pending_mutation_resolved(&pending, &next_period));

        let mut rejected = next_period;
        rejected.status = 3;
        assert!(pending_mutation_resolved(&pending, &rejected));

        let close_pending = PendingRefundMutation {
            operation: "direct-refund".to_string(),
            ..pending
        };
        let mut accepted = active_snapshot;
        accepted.status = 1;
        assert!(pending_mutation_resolved(&close_pending, &accepted));

        // Read-only migration support for a journal written by an older CLI.
        // The current RefundOperation enum exposes no matching write.
        let legacy_finalize_pending = PendingRefundMutation {
            operation: "finalize-expired-refund".to_string(),
            ..close_pending
        };
        let expired = snapshot(json!(1), json!(8), "10");
        assert!(pending_mutation_resolved(
            &legacy_finalize_pending,
            &expired
        ));
        let mut failed = expired;
        failed.status = 9;
        assert!(pending_mutation_resolved(&legacy_finalize_pending, &failed));
    }

    #[test]
    fn refund_display_is_english_display_ready_and_omits_transaction_hashes() {
        let mut detail = task(json!(0), json!(3), "1.25");
        detail.as_object_mut().unwrap().remove("serviceName");
        detail["rejectReason"] = json!("The result missed the requested scope");
        detail["expireTime"] = json!(1_700_100_000);
        let snapshot = RefundSnapshot::from_details("job-1", &detail, None, "buyer-1").unwrap();
        let display = snapshot.display_payload(None, &snapshot.original_amount);

        assert_eq!(display["serviceName"], "Audit task");
        assert_eq!(display["taskType"], "One-time");
        assert_eq!(display["refundAmount"], "1.25 USDT");
        assert_eq!(
            display["reasonForRefund"],
            "The result missed the requested scope"
        );
        assert!(display["resultDeadline"].as_str().is_some());
        assert!(display.get("txHash").is_none());
    }

    #[test]
    fn pending_refund_uses_top_level_reject_deadline_not_expiry_config_duration() {
        let mut detail = task(json!(0), json!(3), "1.25");
        detail["rejectDeadline"] = json!(1_788_861_865i64);
        detail["expireTime"] = json!(1_700_100_000i64);
        detail["expireConfig"] = json!({"rejectDeadline": 1200});

        let snapshot = RefundSnapshot::from_details("job-1", &detail, None, "buyer-1").unwrap();
        assert_eq!(snapshot.response_deadline, Some(1_788_861_865));
        assert_eq!(
            snapshot.display_payload(None, &snapshot.original_amount)["resultDeadline"],
            common::deadline::format_local_timestamp_with_offset(1_788_861_865)
                .map(Value::String)
                .unwrap_or(Value::Null)
        );
    }

    #[test]
    fn only_a_returned_business_verdict_is_a_definitive_rejection() {
        let business = anyhow::Error::new(crate::wallet_api::ApiCodeError {
            code: "50001".to_string(),
            msg: "rejected".to_string(),
            http_status: 200,
        })
        .context("write failed");
        assert!(is_definitive_api_rejection(&business));

        let server_error = anyhow::Error::new(crate::wallet_api::ApiCodeError {
            code: "500".to_string(),
            msg: "server error".to_string(),
            http_status: 500,
        });
        assert!(!is_definitive_api_rejection(&server_error));

        let prebroadcast = anyhow::anyhow!("pre-reject did not return typedData");
        assert!(!mutation_outcome_may_be_unknown(&prebroadcast));
        let unknown = anyhow::anyhow!("transport timeout").context("broadcast failed");
        assert!(mutation_outcome_may_be_unknown(&unknown));
        let malformed_receipt = strict_broadcast_receipt(json!({}))
            .unwrap_err()
            .context("broadcast receipt result is unknown");
        assert!(mutation_outcome_may_be_unknown(&malformed_receipt));
    }
}
