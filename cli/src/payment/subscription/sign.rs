//! Signing orchestration for the x402 `period` scheme.
//!
//! Every subscription signature goes through the TEE `eip712` path
//! ([`crate::payment::permit2::sign::tee_sign_eip712`]) so the contract can `ecrecover`
//! `payer`.
//!
//! Subscribe / change are a double-sign: Permit2 `PermitSingle` (Permit2
//! domain) plus `SubscriptionTerms` (subscription domain), bound by
//! `terms.permitHash`. Cancel / cancel-pending-change are single signatures.
//! `allowance-status` is read immediately before signing so `nonce` /
//! `reservedAmount` reflect on-chain truth (no caching).

use alloy_primitives::U256;
use anyhow::{anyhow, bail, Context, Result};
use base64::{engine::general_purpose::STANDARD as B64, Engine};
use rand::RngCore;
use serde_json::Value;

use crate::payment::subscription::eip712::{
    access_proof_inner_hash, build_cancel_auth_typed_data,
    build_pending_change_cancel_auth_typed_data, build_permit_single_typed_data,
    build_subscription_terms_typed_data, hex0x, permit_single_struct_hash, terms_digest, SubDomain,
    SubscriptionTermsInput,
};
use crate::payment::subscription::facilitator::allowance_status;
use crate::payment::subscription::types::{
    CancelAuth, PendingChangeCancelAuth, PermitDetailsWire, PermitSingleWire, SubscriptionPayload,
    SubscriptionTermsWire,
};

const ZERO_BYTES32: &str = "0x0000000000000000000000000000000000000000000000000000000000000000";
/// Fallback signing/submission window when `maxTimeoutSeconds` is absent.
const DEFAULT_TIMEOUT_SECS: u64 = 3600;
/// Buffer added past the subscription end so a later on-chain `startAt` plus
/// last-charge timing can't expire the Permit2 allowance (`allowance_expired`).
const PERMIT_EXPIRATION_BUFFER_SECS: u64 = 86_400;
/// `periodMode == 1`: bill on natural calendar months (`periodSec == 0`),
/// mirroring the contract's `PeriodMode.CALENDAR_MONTH`.
const PERIOD_MODE_CALENDAR_MONTH: u8 = 1;

/// Add `months` calendar months to a Unix timestamp, clamping day-of-month the
/// same way the contract's `PeriodMathLib` / `DateTimeLib.addMonths` does (e.g.
/// Jan 31 + 1mo → Feb 28). Used to size the Permit2 allowance window for
/// calendar-month subscriptions. Falls back to a 31-day-per-month upper bound
/// (always ≥ the exact boundary) if the timestamp can't be represented.
fn add_calendar_months(unix_secs: u64, months: u32) -> u64 {
    use chrono::{DateTime, Months};
    DateTime::from_timestamp(unix_secs as i64, 0)
        .and_then(|d| d.checked_add_months(Months::new(months)))
        .map(|d| d.timestamp() as u64)
        .unwrap_or_else(|| unix_secs.saturating_add((months as u64).saturating_mul(31 * 86_400)))
}

/// Result of a subscribe / change double-sign: the `PAYMENT-SIGNATURE` payload
/// plus the locally-computed `subId` (the SA's returned subId is authoritative).
#[derive(Debug, Clone)]
pub struct SignedSubscription {
    pub payload: SubscriptionPayload,
    /// Locally-computed `termsDigest` (= subId).
    pub sub_id: String,
    pub chain_index: String,
    /// `extra.plan.id` business identifier — cache-only, not part of signed terms.
    pub plan_id: String,
}

/// Subscription term parameters read from the chosen 402 `accepts[]` entry.
struct TermsParams {
    token: String,
    merchant: String,
    facilitator: String,
    amount_per_period: String,
    period_sec: u64,
    /// 0 fixed_seconds / 1 calendar_month (facilitator §0.6).
    period_mode: u8,
    max_periods: u32,
    start_at: u64,
    initial_charge_periods: u32,
    initial_charge_amount: String,
    plan_tier: u8,
    plan_id: String,
    timeout_secs: u64,
    /// `extra.contracts.subscription` — A2APaySubscription contract
    /// (PermitSingle.spender + Terms domain verifyingContract).
    subscription_contract: String,
    /// `extra.contracts.permit2` — the Permit2 domain verifyingContract.
    permit2_contract: String,
}

fn str_field<'a>(v: &'a Value, key: &str) -> Result<&'a str> {
    v.get(key)
        .and_then(Value::as_str)
        .ok_or_else(|| anyhow!("missing/invalid string field `{key}` in accepts entry"))
}

fn extract_terms_params(accepted: &Value) -> Result<TermsParams> {
    let extra = accepted
        .get("extra")
        .ok_or_else(|| anyhow!("accepts entry missing `extra` (subscription params)"))?;

    let initial_charge = extra.get("initialCharge");
    let (initial_charge_periods, initial_charge_amount) = match initial_charge {
        Some(ic) if !ic.is_null() => (
            ic.get("periodCount").and_then(Value::as_u64).unwrap_or(0) as u32,
            ic.get("totalAmount")
                .and_then(Value::as_str)
                .unwrap_or("0")
                .to_string(),
        ),
        _ => (0, "0".to_string()),
    };

    let plan = extra
        .get("plan")
        .ok_or_else(|| anyhow!("accepts entry `extra` missing `plan`"))?;
    let plan_tier =
        plan.get("tier")
            .and_then(Value::as_u64)
            .ok_or_else(|| anyhow!("`extra.plan.tier` missing or not an integer"))? as u8;
    // `plan.id` is a business string, not signed; relayed verbatim.
    let plan_id = plan
        .get("id")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string();

    let contracts = extra
        .get("contracts")
        .ok_or_else(|| anyhow!("accepts entry `extra` missing `contracts`"))?;
    let subscription_contract = contracts
        .get("subscription")
        .and_then(Value::as_str)
        .ok_or_else(|| anyhow!("`extra.contracts.subscription` missing or not a string"))?
        .to_string();
    let permit2_contract = contracts
        .get("permit2")
        .and_then(Value::as_str)
        .ok_or_else(|| anyhow!("`extra.contracts.permit2` missing or not a string"))?
        .to_string();

    let period_sec = extra
        .get("periodSec")
        .and_then(Value::as_u64)
        .ok_or_else(|| anyhow!("`extra.periodSec` missing or not an integer"))?;
    // periodMode: 0 fixed_seconds / 1 calendar_month (default 0). Enforce the
    // periodSec ↔ mode invariant.
    let period_mode = extra.get("periodMode").and_then(Value::as_u64).unwrap_or(0) as u8;
    match period_mode {
        0 if period_sec == 0 => bail!("fixed_seconds mode (periodMode=0) requires periodSec > 0"),
        1 if period_sec != 0 => {
            bail!("calendar_month mode (periodMode=1) requires periodSec == 0")
        }
        m if m > 1 => bail!("invalid periodMode {m} (expected 0 or 1)"),
        _ => {}
    }

    Ok(TermsParams {
        token: str_field(accepted, "asset")?.to_string(),
        merchant: str_field(accepted, "payTo")?.to_string(),
        facilitator: str_field(extra, "facilitator")?.to_string(),
        amount_per_period: str_field(extra, "amountPerPeriod")?.to_string(),
        period_sec,
        period_mode,
        max_periods: extra
            .get("maxPeriods")
            .and_then(Value::as_u64)
            .ok_or_else(|| anyhow!("`extra.maxPeriods` missing or not an integer"))?
            as u32,
        start_at: extra.get("startAt").and_then(Value::as_u64).unwrap_or(0),
        initial_charge_periods,
        initial_charge_amount,
        plan_tier,
        plan_id,
        timeout_secs: accepted
            .get("maxTimeoutSeconds")
            .and_then(Value::as_u64)
            .unwrap_or(DEFAULT_TIMEOUT_SECS),
        subscription_contract,
        permit2_contract,
    })
}

fn now_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

fn random_bytes32_hex() -> String {
    let mut bytes = [0u8; 32];
    rand::rngs::OsRng.fill_bytes(&mut bytes);
    format!("0x{}", hex::encode(bytes))
}

fn u256(s: &str) -> Result<U256> {
    U256::from_str_radix(
        s.trim().trim_start_matches("0x"),
        if s.trim().starts_with("0x") { 16 } else { 10 },
    )
    .with_context(|| format!("invalid uint256: {s}"))
}

/// Core double-sign shared by subscribe and change. For a create,
/// `change_from_sub_id` is `0x00..` and `change_effective_at` is `0`.
#[allow(clippy::too_many_arguments)]
async fn sign_double(
    chain_index: &str,
    chain_id: u64,
    payer: &str,
    accepted: &Value,
    change_from_sub_id: &str,
    change_effective_at: u8,
) -> Result<SignedSubscription> {
    let p = extract_terms_params(accepted)?;
    // Contract addresses come from the Seller's 402 `extra.contracts`, not
    // allowance-status (read only for nonce / reserved / permit2Allowance).
    let spender = p.subscription_contract.clone();

    // Read on-chain truth right before signing.
    let a = allowance_status(payer, &p.token, chain_index).await?;

    // newCommit = initialChargeAmount + (maxPeriods - initialChargePeriods) * amountPerPeriod
    let remaining_periods = (p.max_periods as u64)
        .checked_sub(p.initial_charge_periods as u64)
        .ok_or_else(|| anyhow!("initialChargePeriods exceeds maxPeriods"))?;
    let new_commit = u256(&p.initial_charge_amount)?
        + u256(&p.amount_per_period)? * U256::from(remaining_periods);
    // permit.details.amount >= reservedAmount + newCommit (cover full commitment)
    let reserved = if a.reserved_amount.trim().is_empty() {
        U256::ZERO
    } else {
        u256(&a.reserved_amount)?
    };
    let amount = reserved + new_commit;

    // Layer-1 (ERC20 -> Permit2) must be approved first; if short, the buyer
    // approves on-chain before retrying.
    if !a.permit2_allowance.is_empty() {
        if let Ok(layer1) = u256(&a.permit2_allowance) {
            if layer1 < amount {
                bail!(
                    "Layer-1 Permit2 allowance insufficient on token {} (chain {}). \
                     ERC20.allowance(buyer, Permit2) is {}, but this subscription needs {}. \
                     Approve once first: IERC20.approve({}, MAX) — e.g. via an on-chain \
                     contract call — then retry the subscription.",
                    p.token,
                    chain_index,
                    layer1,
                    amount,
                    p.permit2_contract
                );
            }
        }
    }

    let effective_start = if p.start_at == 0 {
        now_secs()
    } else {
        p.start_at
    };
    // Permit2 allowance must outlast the whole service window, or the contract
    // reverts `PermitExpirationTooSoon` at create (`_validatePermit` /
    // `_requiredPermitExpiration`). The required end is mode-dependent:
    //   fixed_seconds : startAt + maxPeriods × periodSec
    //   calendar_month: addMonths(anchor, startOffset + maxPeriods)  (periodSec == 0)
    // so calendar-month MUST NOT use the seconds formula — with `periodSec == 0`
    // it would collapse to ~now and every multi-month subscription would revert.
    // A period-end change (changeEffectiveAt == 2) activates at the next period
    // boundary, so shift the window forward one period. A buffer absorbs
    // sign→on-chain drift and last-charge timing.
    let new_sub_end = if p.period_mode == PERIOD_MODE_CALENDAR_MONTH {
        let defer_months = if change_effective_at == 2 { 1 } else { 0 };
        add_calendar_months(effective_start, p.max_periods.saturating_add(defer_months))
            .saturating_add(PERMIT_EXPIRATION_BUFFER_SECS)
    } else {
        let deferred_start = if change_effective_at == 2 {
            p.period_sec
        } else {
            0
        };
        effective_start
            .saturating_add(deferred_start)
            .saturating_add((p.max_periods as u64).saturating_mul(p.period_sec))
            .saturating_add(PERMIT_EXPIRATION_BUFFER_SECS)
    };
    let expiration = a.reserved_expiration.max(new_sub_end);
    let nonce = a.nonce;
    let now = now_secs();
    let sig_deadline = (now + p.timeout_secs).to_string();
    let terms_deadline = now + p.timeout_secs;
    let amount_str = amount.to_string();

    // 1) PermitSingle (Permit2 domain) — spender = subscription contract.
    let permit_td = build_permit_single_typed_data(
        &p.token,
        &amount_str,
        expiration,
        nonce,
        &spender,
        &sig_deadline,
        &p.permit2_contract,
        chain_id,
    );
    let permit_hash = hex0x(permit_single_struct_hash(
        &p.token,
        &amount_str,
        expiration,
        nonce,
        &spender,
        &sig_deadline,
    )?);
    let permit_sig =
        crate::payment::permit2::sign::tee_sign_eip712(chain_index, payer, &permit_td).await?;

    // 2) SubscriptionTerms (subscription domain) — permitHash binds the two sigs.
    let salt = random_bytes32_hex();
    let terms_in = SubscriptionTermsInput {
        payer,
        merchant: &p.merchant,
        facilitator: &p.facilitator,
        token: &p.token,
        amount_per_period: &p.amount_per_period,
        period_sec: p.period_sec,
        max_periods: p.max_periods,
        start_at: p.start_at,
        initial_charge_periods: p.initial_charge_periods,
        initial_charge_amount: &p.initial_charge_amount,
        terms_deadline,
        permit_hash: &permit_hash,
        salt: &salt,
        plan_tier: p.plan_tier,
        change_from_sub_id,
        change_effective_at,
        period_mode: p.period_mode,
        domain: SubDomain {
            chain_id,
            verifying_contract: &spender,
        },
    };
    let terms_td = build_subscription_terms_typed_data(&terms_in);
    let terms_sig =
        crate::payment::permit2::sign::tee_sign_eip712(chain_index, payer, &terms_td).await?;

    let sub_id = hex0x(terms_digest(&terms_in)?);

    // Clone the token for the permit details since `p.token` moves into
    // `terms.token` below.
    let permit_token = p.token.clone();
    let payload = SubscriptionPayload {
        terms: SubscriptionTermsWire {
            payer: payer.to_string(),
            merchant: p.merchant,
            facilitator: p.facilitator,
            token: p.token,
            amount_per_period: p.amount_per_period,
            period_sec: p.period_sec,
            max_periods: p.max_periods,
            start_at: p.start_at,
            initial_charge_periods: p.initial_charge_periods,
            initial_charge_amount: p.initial_charge_amount,
            terms_deadline,
            permit_hash,
            salt,
            // Business plan id string; not signed, carried verbatim on the wire.
            plan_id: p.plan_id.clone(),
            plan_tier: p.plan_tier,
            change_from_sub_id: change_from_sub_id.to_string(),
            change_effective_at,
            period_mode: p.period_mode,
        },
        terms_signature: terms_sig,
        permit: PermitSingleWire {
            details: PermitDetailsWire {
                token: permit_token,
                amount: amount_str,
                expiration,
                nonce,
            },
            spender,
            sig_deadline,
        },
        permit_signature: permit_sig,
    };

    Ok(SignedSubscription {
        payload,
        sub_id,
        chain_index: chain_index.to_string(),
        plan_id: p.plan_id,
    })
}

/// Subscribe (create): `changeFromSubId = 0x00..`, `changeEffectiveAt = 0`.
pub async fn sign_subscribe(
    chain_index: &str,
    chain_id: u64,
    payer: &str,
    accepted: &Value,
) -> Result<SignedSubscription> {
    sign_double(chain_index, chain_id, payer, accepted, ZERO_BYTES32, 0).await
}

/// Change (up/downgrade). `change_from_sub_id` falls back to
/// `extra.changeFrom.fromSubId` when `old_sub_id` is empty; `changeEffectiveAt`
/// is derived from `extra.changeFrom` (immediate → 1, period_end → 2).
pub async fn sign_change(
    chain_index: &str,
    chain_id: u64,
    payer: &str,
    old_sub_id: &str,
    accepted: &Value,
) -> Result<SignedSubscription> {
    let change_from = accepted.get("extra").and_then(|e| e.get("changeFrom"));
    let from_sub_id = if !old_sub_id.is_empty() {
        old_sub_id.to_string()
    } else {
        change_from
            .and_then(|c| c.get("fromSubId"))
            .and_then(Value::as_str)
            .ok_or_else(|| anyhow!("change requires --sub-id or extra.changeFrom.fromSubId"))?
            .to_string()
    };
    let effective_at = change_effective_at_from(change_from)?;
    sign_double(
        chain_index,
        chain_id,
        payer,
        accepted,
        &from_sub_id,
        effective_at,
    )
    .await
}

/// Map `extra.changeFrom` to `changeEffectiveAt` (1 immediate / 2 period_end).
/// Prefers explicit `effectiveAt`, falls back to `direction`.
fn change_effective_at_from(change_from: Option<&Value>) -> Result<u8> {
    let cf = change_from.ok_or_else(|| anyhow!("change offer missing extra.changeFrom"))?;
    if let Some(ea) = cf.get("effectiveAt").and_then(Value::as_str) {
        return match ea {
            "immediate" => Ok(1),
            "period_end" => Ok(2),
            other => bail!("unknown changeFrom.effectiveAt: {other}"),
        };
    }
    match cf.get("direction").and_then(Value::as_str) {
        Some("upgrade") => Ok(1),
        Some("downgrade") => Ok(2),
        Some(other) => bail!("unknown changeFrom.direction: {other}"),
        None => bail!("changeFrom missing both effectiveAt and direction"),
    }
}

/// Sign a `CancelAuth` to cancel an active subscription (`initiator = 0` payer).
/// `verifying_contract` is the subscription contract.
pub async fn sign_cancel(
    chain_index: &str,
    chain_id: u64,
    payer: &str,
    sub_id: &str,
    verifying_contract: &str,
) -> Result<CancelAuth> {
    let nonce = random_bytes32_hex();
    let deadline = now_secs() + DEFAULT_TIMEOUT_SECS;
    let d = SubDomain {
        chain_id,
        verifying_contract,
    };
    let td = build_cancel_auth_typed_data(0, sub_id, 0, &nonce, deadline, &d);
    let signature = crate::payment::permit2::sign::tee_sign_eip712(chain_index, payer, &td).await?;
    Ok(CancelAuth {
        action: 0,
        sub_id: sub_id.to_string(),
        initiator: 0,
        nonce,
        deadline,
        signature,
    })
}

/// Sign a `PendingChangeCancelAuth` to cancel a not-yet-effective downgrade
/// (payer only).
pub async fn sign_cancel_pending_change(
    chain_index: &str,
    chain_id: u64,
    payer: &str,
    sub_id: &str,
    new_sub_id: &str,
    verifying_contract: &str,
) -> Result<PendingChangeCancelAuth> {
    let nonce = random_bytes32_hex();
    let deadline = now_secs() + DEFAULT_TIMEOUT_SECS;
    let d = SubDomain {
        chain_id,
        verifying_contract,
    };
    let td = build_pending_change_cancel_auth_typed_data(sub_id, new_sub_id, &nonce, deadline, &d);
    let signature = crate::payment::permit2::sign::tee_sign_eip712(chain_index, payer, &td).await?;
    Ok(PendingChangeCancelAuth {
        sub_id: sub_id.to_string(),
        new_sub_id: new_sub_id.to_string(),
        nonce,
        deadline,
        signature,
    })
}

/// Build the `APP-Access` header for accessing a Seller's protected routes.
/// Value is `base64(JSON SubscriptionProof)`
/// `{kind:"subscription-id", subId, payer, timestamp, signature}`, where the
/// signature is an EIP-191 personal_sign (signer == payer) over
/// `keccak256(abi.encodePacked(subId, payer, timestamp))`.
///
/// Returns `(header_name, header_value)`. Rebuilt per request with a fresh
/// timestamp (no nonce; replay bounded by the Seller's timestamp window).
pub async fn build_access_proof(
    chain_index: &str,
    payer: &str,
    sub_id: &str,
) -> Result<(&'static str, String)> {
    let timestamp = now_secs();
    let inner = access_proof_inner_hash(sub_id, payer, timestamp)?;
    let value_hex = hex0x(inner);
    let signature =
        crate::payment::permit2::sign::tee_sign_personal(chain_index, payer, &value_hex).await?;
    let proof = serde_json::json!({
        "kind": "subscription-id",
        "subId": sub_id,
        "payer": payer,
        "timestamp": timestamp,
        "signature": signature,
    });
    let encoded = B64.encode(serde_json::to_vec(&proof).context("encode AccessProof")?);
    Ok(("APP-Access", encoded))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn sample_accepted() -> Value {
        json!({
            "scheme": "period",
            "network": "eip155:196",
            "amount": "5000000",
            "asset": "0x779ded0c9e1022225f8e0630b35a9b54be713736",
            "payTo": "0x000000000000000000000000000000000000cafe",
            "maxTimeoutSeconds": 600,
            "extra": {
                "contracts": { "subscription": "0x4020000000000000000000000000000000000003", "permit2": "0x000000000022D473030F116dDEE9F6B43aC78BA3" },
                "facilitator": "0x000000000000000000000000000000000000dead",
                "amountPerPeriod": "5000000",
                "periodSec": 2592000,
                "maxPeriods": 12,
                "startAt": 0,
                "initialCharge": { "periodCount": 1, "totalAmount": "5000000", "coversFirstPeriods": true },
                "plan": { "id": "pro_monthly", "tier": 2 }
            }
        })
    }

    #[test]
    fn extract_terms_params_reads_all_fields() {
        let p = extract_terms_params(&sample_accepted()).unwrap();
        assert_eq!(p.token, "0x779ded0c9e1022225f8e0630b35a9b54be713736");
        assert_eq!(p.merchant, "0x000000000000000000000000000000000000cafe");
        assert_eq!(p.facilitator, "0x000000000000000000000000000000000000dead");
        assert_eq!(p.amount_per_period, "5000000");
        assert_eq!(p.period_sec, 2_592_000);
        assert_eq!(p.max_periods, 12);
        assert_eq!(p.initial_charge_periods, 1);
        assert_eq!(p.initial_charge_amount, "5000000");
        assert_eq!(p.plan_tier, 2);
        assert_eq!(p.plan_id, "pro_monthly");
        assert_eq!(p.timeout_secs, 600);
        assert_eq!(
            p.subscription_contract,
            "0x4020000000000000000000000000000000000003"
        );
        assert_eq!(
            p.permit2_contract,
            "0x000000000022D473030F116dDEE9F6B43aC78BA3"
        );
    }

    #[test]
    fn extract_terms_params_defaults_initial_charge_when_absent() {
        let mut a = sample_accepted();
        a["extra"].as_object_mut().unwrap().remove("initialCharge");
        let p = extract_terms_params(&a).unwrap();
        assert_eq!(p.initial_charge_periods, 0);
        assert_eq!(p.initial_charge_amount, "0");
    }

    #[test]
    fn add_calendar_months_does_not_collapse() {
        // Regression for the calendar_month permit window: with periodSec == 0 the
        // old `maxPeriods * periodSec` formula collapsed to ~now and the contract
        // reverted PermitExpirationTooSoon. The window must span real months.
        let base = 1_700_000_000u64; // 2023-11-14T22:13:20Z
        let span = add_calendar_months(base, 12) - base;
        assert!(span >= 360 * 86_400, "12 months collapsed to {span}s");
        assert!(span <= 366 * 86_400, "12 months overshot to {span}s");
    }

    #[test]
    fn add_calendar_months_clamps_month_end() {
        // Jan 31 + 1 month clamps to Feb 28 (2023, non-leap), matching the
        // contract's DateTimeLib.addMonths day-of-month clamping.
        let jan31 = 1_675_123_200u64; // 2023-01-31T00:00:00Z
        let feb28 = 1_677_542_400u64; // 2023-02-28T00:00:00Z
        assert_eq!(add_calendar_months(jan31, 1), feb28);
    }

    #[test]
    fn new_commit_formula() {
        // newCommit = 5000000 + (12 - 1) * 5000000 = 60000000
        let p = extract_terms_params(&sample_accepted()).unwrap();
        let remaining = (p.max_periods as u64) - (p.initial_charge_periods as u64);
        let new_commit = u256(&p.initial_charge_amount).unwrap()
            + u256(&p.amount_per_period).unwrap() * U256::from(remaining);
        assert_eq!(new_commit, U256::from(60_000_000u64));
    }

    #[test]
    fn change_effective_at_prefers_effective_at_then_direction() {
        assert_eq!(
            change_effective_at_from(Some(&json!({"effectiveAt": "immediate"}))).unwrap(),
            1
        );
        assert_eq!(
            change_effective_at_from(Some(&json!({"effectiveAt": "period_end"}))).unwrap(),
            2
        );
        assert_eq!(
            change_effective_at_from(Some(&json!({"direction": "upgrade"}))).unwrap(),
            1
        );
        assert_eq!(
            change_effective_at_from(Some(&json!({"direction": "downgrade"}))).unwrap(),
            2
        );
        assert!(change_effective_at_from(Some(&json!({}))).is_err());
        assert!(change_effective_at_from(None).is_err());
    }

    #[test]
    fn u256_parses_decimal_and_hex() {
        assert_eq!(u256("100").unwrap(), U256::from(100u64));
        assert_eq!(u256("0x10").unwrap(), U256::from(16u64));
        assert!(u256("nope").is_err());
    }

    #[test]
    fn random_bytes32_hex_shape() {
        let s = random_bytes32_hex();
        assert!(s.starts_with("0x"));
        assert_eq!(s.len(), 66); // 0x + 64 hex
        assert_ne!(s, random_bytes32_hex());
    }
}
