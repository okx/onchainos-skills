//! Wire + cache types for the x402 `period` scheme.
//!
//! `*Wire` types are the JSON shapes the buyer SDK emits (in the
//! `PAYMENT-SIGNATURE` payload) or reads from the buyer-direct facilitator
//! endpoints. Field order / `camelCase` naming is canonical — a rename or
//! reorder breaks signature verification.

use serde::{Deserialize, Deserializer, Serialize};
use serde_json::Value;

// ───────────────────────────── signed payload ──────────────────────────────

/// The signed `SubscriptionTerms` fields, in EIP-712 order. Atomic amounts
/// (`uint160`) are decimal strings; `bytes32` and addresses are `0x`-hex.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SubscriptionTermsWire {
    pub payer: String,
    pub merchant: String,
    pub facilitator: String,
    pub token: String,
    pub amount_per_period: String,
    pub period_sec: u64,
    pub max_periods: u32,
    pub start_at: u64,
    pub initial_charge_periods: u32,
    pub initial_charge_amount: String,
    pub terms_deadline: u64,
    pub permit_hash: String,
    pub salt: String,
    /// Business plan id string (e.g. `"pro_monthly"`); not signed. Sent on the
    /// wire and echoed back by queries.
    pub plan_id: String,
    pub plan_tier: u8,
    pub change_from_sub_id: String,
    pub change_effective_at: u8,
    /// 0 fixed_seconds / 1 calendar_month. 17th field.
    pub period_mode: u8,
}

/// Permit2 `PermitDetails`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PermitDetailsWire {
    pub token: String,
    /// uint160 atomic, decimal string.
    pub amount: String,
    /// uint48 seconds.
    pub expiration: u64,
    /// uint48 Permit2 nonce, taken verbatim from `allowance-status`.
    pub nonce: u64,
}

/// Permit2 `PermitSingle` — `spender` is the subscription contract.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PermitSingleWire {
    pub details: PermitDetailsWire,
    pub spender: String,
    /// uint256 seconds, decimal string.
    pub sig_deadline: String,
}

/// `PAYMENT-SIGNATURE` payload body for subscribe / change: the double-signed
/// `{terms, termsSignature, permit, permitSignature}`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SubscriptionPayload {
    pub terms: SubscriptionTermsWire,
    /// 0x-prefixed 65-byte secp256k1 `r||s||v`; signer == payer.
    pub terms_signature: String,
    pub permit: PermitSingleWire,
    /// 0x-prefixed 65-byte secp256k1; signer == payer.
    pub permit_signature: String,
}

/// `CancelAuth` — cancel an active subscription. `initiator` 0 = payer.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CancelAuth {
    /// 0 = cancel_subscription.
    pub action: u8,
    pub sub_id: String,
    /// 0 = payer / 1 = merchant.
    pub initiator: u8,
    pub nonce: String,
    pub deadline: u64,
    /// 0x-prefixed 65-byte secp256k1.
    pub signature: String,
}

/// `PendingChangeCancelAuth` — cancel a not-yet-effective downgrade (payer only).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PendingChangeCancelAuth {
    pub sub_id: String,
    /// The pending downgrade's `newSubId` being canceled; signed, must equal
    /// the on-chain pending newSubId.
    pub new_sub_id: String,
    pub nonce: String,
    pub deadline: u64,
    pub signature: String,
}

// ─────────────────────────── local subId cache entry ───────────────────────

/// Convenience index only, never authoritative. Maps a resource host to the
/// buyer's active subId plus light metadata.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct SubscriptionCacheEntry {
    pub sub_id: String,
    pub resource_host: String,
    pub merchant: String,
    pub plan_id: String,
    pub plan_tier: u8,
    pub max_periods: u32,
    /// `active` / `canceled` / `changed`.
    pub state: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub changed_to_sub_id: Option<String>,
}

// ─────────────────── buyer-direct facilitator read responses ────────────────

/// `GET /buyers/{buyer}/allowance-status` — inputs for assembling a
/// `PermitSingle` and deciding whether a layer-1 `token.approve(permit2)` is
/// needed.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AllowanceStatus {
    /// Layer-2 Permit2 allowance amount granted to the subscription contract.
    #[serde(default, deserialize_with = "flex_string")]
    pub approved_amount: String,
    /// Layer-2 allowance expiration.
    #[serde(default, deserialize_with = "flex_u64")]
    pub expiration: u64,
    /// Current Permit2 nonce — signed verbatim into the next PermitSingle (not
    /// `+1`; it auto-increments on use).
    #[serde(default, deserialize_with = "flex_u64")]
    pub nonce: u64,
    /// Allowance already reserved by existing active subscriptions.
    #[serde(default, deserialize_with = "flex_string")]
    pub reserved_amount: String,
    /// Lower bound for `permit.expiration`.
    #[serde(default, deserialize_with = "flex_u64")]
    pub reserved_expiration: u64,
    #[serde(default, deserialize_with = "flex_string")]
    pub token_balance: String,
    #[serde(default, deserialize_with = "flex_string")]
    pub available_amount: String,
    /// Layer-1 `ERC20.allowance(buyer, Permit2)`. If insufficient, the buyer
    /// must first `token.approve(permit2Contract, MAX)`.
    #[serde(default, deserialize_with = "flex_string")]
    pub permit2_allowance: String,
    /// `PermitSingle.spender` — the subscription contract address.
    #[serde(default)]
    pub subscription_contract: String,
    /// Layer-1 approve target (Permit2).
    #[serde(default)]
    pub permit2_contract: String,
}

/// One item of `GET /buyers/{buyer}/subscriptions`. Excludes merchant-side
/// identifiers by design.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BuyerSubscriptionItem {
    #[serde(default, deserialize_with = "flex_u64")]
    pub chain_index: u64,
    pub sub_id: String,
    /// `SubscriptionState`: 0 pending / 1 active / 2 completed / 3 canceled /
    /// 4 changed / 99 failed.
    #[serde(default)]
    pub state: u8,
    #[serde(default)]
    pub payer: String,
    #[serde(default)]
    pub token: String,
    #[serde(default, deserialize_with = "flex_string")]
    pub amount_per_period: String,
    #[serde(default, deserialize_with = "flex_u64")]
    pub period_sec: u64,
    /// 0 fixed_seconds / 1 calendar_month.
    #[serde(default)]
    pub period_mode: u8,
    /// Calendar-month billing anchor (Unix secs); 0 = pending / fixed mode.
    #[serde(default, deserialize_with = "flex_u64")]
    pub billing_anchor_at: u64,
    #[serde(default)]
    pub max_periods: u32,
    #[serde(default, deserialize_with = "flex_u64")]
    pub start_at: u64,
    #[serde(default)]
    pub initial_charge_periods: u32,
    #[serde(default, deserialize_with = "flex_string")]
    pub initial_charge_amount: String,
    #[serde(default)]
    pub last_charged_period: u32,
    #[serde(default, deserialize_with = "flex_string")]
    pub total_pulled: String,
    #[serde(default)]
    pub plan_id: String,
    #[serde(default)]
    pub plan_tier: u8,
    /// Successor subId once this subscription is changed (up/downgrade).
    #[serde(default)]
    pub changed_to_sub_id: Option<String>,
    #[serde(default)]
    pub is_active: bool,
    #[serde(default)]
    pub service_ended: bool,
    #[serde(default)]
    pub current_period: u32,
    #[serde(default)]
    pub next_chargeable_at: Option<u64>,
}

/// Envelope of `GET /buyers/{buyer}/subscriptions`.
#[derive(Debug, Clone, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct BuyerSubscriptionListResp {
    #[serde(default)]
    pub subscriptions: Vec<BuyerSubscriptionItem>,
}

// ─────────────────────────── flexible deserializers ────────────────────────
// The facilitator may encode numeric fields as JSON numbers or strings; accept
// both.

/// Deserialize a JSON number-or-string into a `String`.
fn flex_string<'de, D>(d: D) -> Result<String, D::Error>
where
    D: Deserializer<'de>,
{
    match Value::deserialize(d)? {
        Value::String(s) => Ok(s),
        Value::Number(n) => Ok(n.to_string()),
        Value::Null => Ok(String::new()),
        other => Err(serde::de::Error::custom(format!(
            "expected string or number, got {other}"
        ))),
    }
}

/// Deserialize a JSON number-or-string into a `u64`.
fn flex_u64<'de, D>(d: D) -> Result<u64, D::Error>
where
    D: Deserializer<'de>,
{
    match Value::deserialize(d)? {
        Value::Number(n) => n
            .as_u64()
            .ok_or_else(|| serde::de::Error::custom("number out of u64 range")),
        Value::String(s) => {
            let t = s.trim();
            if t.is_empty() {
                return Ok(0);
            }
            t.parse::<u64>()
                .map_err(|e| serde::de::Error::custom(format!("invalid u64 string {s:?}: {e}")))
        }
        Value::Null => Ok(0),
        other => Err(serde::de::Error::custom(format!(
            "expected u64 number or string, got {other}"
        ))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn subscription_payload_round_trips_camel_case() {
        let payload = SubscriptionPayload {
            terms: SubscriptionTermsWire {
                payer: "0xpayer".into(),
                merchant: "0xmerchant".into(),
                facilitator: "0xfac".into(),
                token: "0xtoken".into(),
                amount_per_period: "5000000".into(),
                period_sec: 2_592_000,
                max_periods: 12,
                start_at: 0,
                initial_charge_periods: 1,
                initial_charge_amount: "5000000".into(),
                terms_deadline: 1_750_000_000,
                permit_hash: "0xph".into(),
                salt: "0xsalt".into(),
                plan_id: "0xplan".into(),
                plan_tier: 2,
                change_from_sub_id: "0x00".into(),
                change_effective_at: 0,
                period_mode: 0,
            },
            terms_signature: "0xtsig".into(),
            permit: PermitSingleWire {
                details: PermitDetailsWire {
                    token: "0xtoken".into(),
                    amount: "60000000".into(),
                    expiration: 1_782_000_000,
                    nonce: 7,
                },
                spender: "0xsub".into(),
                sig_deadline: "1750000000".into(),
            },
            permit_signature: "0xpsig".into(),
        };
        let v = serde_json::to_value(&payload).unwrap();
        // camelCase on the wire.
        assert_eq!(v["terms"]["amountPerPeriod"], "5000000");
        assert_eq!(v["terms"]["changeFromSubId"], "0x00");
        assert!(v["terms"].get("amount_per_period").is_none());
        assert_eq!(v["permit"]["sigDeadline"], "1750000000");
        assert_eq!(v["permit"]["details"]["nonce"], 7);
        assert_eq!(v["termsSignature"], "0xtsig");
        assert_eq!(v["permitSignature"], "0xpsig");

        let back: SubscriptionPayload = serde_json::from_value(v).unwrap();
        assert_eq!(back.terms.plan_tier, 2);
        assert_eq!(back.permit.details.nonce, 7);
    }

    #[test]
    fn allowance_status_accepts_numbers_or_strings() {
        // nonce as number, amounts as strings.
        let a: AllowanceStatus = serde_json::from_value(json!({
            "approvedAmount": "100",
            "expiration": 1690000000u64,
            "nonce": 7,
            "reservedAmount": "5000000",
            "reservedExpiration": "1690000000",
            "tokenBalance": "9999",
            "availableAmount": "94999000",
            "permit2Allowance": "0",
            "subscriptionContract": "0xsub",
            "permit2Contract": "0xpermit2"
        }))
        .unwrap();
        assert_eq!(a.nonce, 7);
        assert_eq!(a.reserved_amount, "5000000");
        assert_eq!(a.reserved_expiration, 1_690_000_000);
        assert_eq!(a.subscription_contract, "0xsub");

        // nonce as string, expiration as number — both must parse.
        let b: AllowanceStatus = serde_json::from_value(json!({
            "nonce": "12",
            "reservedAmount": 0,
            "permit2Allowance": 500,
            "subscriptionContract": "0xsub2",
            "permit2Contract": "0xp2"
        }))
        .unwrap();
        assert_eq!(b.nonce, 12);
        assert_eq!(b.reserved_amount, "0");
        assert_eq!(b.permit2_allowance, "500");
    }

    #[test]
    fn buyer_subscription_list_parses_and_follows_changed_to() {
        let resp: BuyerSubscriptionListResp = serde_json::from_value(json!({
            "subscriptions": [
                {
                    "chainIndex": 196,
                    "subId": "0xsub",
                    "state": 4,
                    "payer": "0xbuyer",
                    "token": "0xtoken",
                    "amountPerPeriod": "5000000",
                    "periodSec": 2592000,
                    "maxPeriods": 12,
                    "planTier": 2,
                    "changedToSubId": "0xnewsub",
                    "isActive": false,
                    "currentPeriod": 3,
                    "nextChargeableAt": null
                }
            ]
        }))
        .unwrap();
        assert_eq!(resp.subscriptions.len(), 1);
        let item = &resp.subscriptions[0];
        assert_eq!(item.state, 4);
        assert_eq!(item.changed_to_sub_id.as_deref(), Some("0xnewsub"));
        assert!(!item.is_active);
        assert_eq!(item.next_chargeable_at, None);
    }
}
