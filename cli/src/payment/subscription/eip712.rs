//! EIP-712 typed-data builders for the x402 `period` scheme.
//!
//! Two EIP-712 domains:
//! - **Permit2** domain (no `version`) — for `PermitSingle`.
//! - **A2APaySubscription / v1** domain — for `SubscriptionTerms`, `CancelAuth`,
//!   `PendingChangeCancelAuth`. `verifyingContract` is the subscription
//!   contract, fetched at runtime (never hardcoded).
//!
//! Field order and types here must match the backend typehash byte-for-byte,
//! since the TEE recomputes the digest from the typed-data we emit. The one
//! hash computed locally is [`permit_single_struct_hash`], written into
//! `terms.permitHash` before the terms are signed to bind the two signatures;
//! the SA byte-compares it (`permit_hash_mismatch`).

use std::str::FromStr;

use alloy_primitives::{keccak256, Address, B256, U256};
use anyhow::{Context, Result};
use serde_json::{json, Value};

/// The subscription EIP-712 domain (`A2APaySubscription`, version `1`).
/// Shared by `SubscriptionTerms`, `CancelAuth`, `PendingChangeCancelAuth`.
#[derive(Debug, Clone)]
pub struct SubDomain<'a> {
    pub chain_id: u64,
    /// Subscription contract address (PermitSingle.spender).
    pub verifying_contract: &'a str,
}

/// The signed `SubscriptionTerms` fields plus the domain. `plan.id` is a
/// business identifier carried in `extra.plan`, not in terms.
///
/// uint160/uint256 values are decimal strings (can exceed `u64`); smaller
/// uints are native width; `bytes32` are `0x`-prefixed 32-byte hex.
#[derive(Debug, Clone)]
pub struct SubscriptionTermsInput<'a> {
    pub payer: &'a str,
    pub merchant: &'a str,
    pub facilitator: &'a str,
    pub token: &'a str,
    /// uint160 atomic per-period amount.
    pub amount_per_period: &'a str,
    pub period_sec: u64,
    pub max_periods: u32,
    /// `0` → the contract substitutes `block.timestamp`.
    pub start_at: u64,
    pub initial_charge_periods: u32,
    /// uint160 atomic first-period amount.
    pub initial_charge_amount: &'a str,
    pub terms_deadline: u64,
    /// bytes32 — the PermitSingle struct hash (see [`permit_single_struct_hash`]).
    pub permit_hash: &'a str,
    /// bytes32 — buyer-generated random replay guard.
    pub salt: &'a str,
    pub plan_tier: u8,
    /// bytes32 — all-zero on create; the old subId on up/downgrade.
    pub change_from_sub_id: &'a str,
    /// 0 none / 1 immediate (upgrade) / 2 period_end (downgrade).
    pub change_effective_at: u8,
    /// 0 fixed_seconds (`period_sec > 0`) / 1 calendar_month (`period_sec` = 0).
    /// 17th signed field.
    pub period_mode: u8,
    pub domain: SubDomain<'a>,
}

/// `SubscriptionTerms` typestring — must match the backend byte-for-byte.
/// 17 fields; `periodMode` last.
pub const SUBSCRIPTION_TERMS_TYPESTRING: &str = "SubscriptionTerms(address payer,address merchant,address facilitator,address token,uint160 amountPerPeriod,uint64 periodSec,uint32 maxPeriods,uint64 startAt,uint32 initialChargePeriods,uint160 initialChargeAmount,uint64 termsDeadline,bytes32 permitHash,bytes32 salt,uint8 planTier,bytes32 changeFromSubId,uint8 changeEffectiveAt,uint8 periodMode)";

pub const CANCEL_AUTH_TYPESTRING: &str =
    "CancelAuth(uint8 action,bytes32 subId,uint8 initiator,bytes32 nonce,uint64 deadline)";

pub const PENDING_CHANGE_CANCEL_AUTH_TYPESTRING: &str =
    "PendingChangeCancelAuth(bytes32 subId,bytes32 newSubId,bytes32 nonce,uint64 deadline)";

pub const PERMIT_DETAILS_TYPESTRING: &str =
    "PermitDetails(address token,uint160 amount,uint48 expiration,uint48 nonce)";

/// Note the concatenated `PermitDetails(...)` suffix — EIP-712 appends the
/// referenced struct's typestring.
pub const PERMIT_SINGLE_TYPESTRING: &str = "PermitSingle(PermitDetails details,address spender,uint256 sigDeadline)PermitDetails(address token,uint160 amount,uint48 expiration,uint48 nonce)";

const EIP712_DOMAIN_TYPESTRING: &str =
    "EIP712Domain(string name,string version,uint256 chainId,address verifyingContract)";

// ───────────────────────────── typed-data (for the TEE) ─────────────────────

/// `signTypedData_v4` shape for `SubscriptionTerms` (subscription domain).
/// Fields in typehash order; `planId` is excluded.
pub fn build_subscription_terms_typed_data(i: &SubscriptionTermsInput<'_>) -> Value {
    json!({
        "domain": {
            "name": "A2APaySubscription",
            "version": "1",
            "chainId": i.domain.chain_id,
            "verifyingContract": i.domain.verifying_contract,
        },
        "types": {
            "EIP712Domain": [
                { "name": "name",              "type": "string"  },
                { "name": "version",           "type": "string"  },
                { "name": "chainId",           "type": "uint256" },
                { "name": "verifyingContract", "type": "address" },
            ],
            "SubscriptionTerms": [
                { "name": "payer",                "type": "address" },
                { "name": "merchant",             "type": "address" },
                { "name": "facilitator",          "type": "address" },
                { "name": "token",                "type": "address" },
                { "name": "amountPerPeriod",      "type": "uint160" },
                { "name": "periodSec",            "type": "uint64"  },
                { "name": "maxPeriods",           "type": "uint32"  },
                { "name": "startAt",              "type": "uint64"  },
                { "name": "initialChargePeriods", "type": "uint32"  },
                { "name": "initialChargeAmount",  "type": "uint160" },
                { "name": "termsDeadline",        "type": "uint64"  },
                { "name": "permitHash",           "type": "bytes32" },
                { "name": "salt",                 "type": "bytes32" },
                { "name": "planTier",             "type": "uint8"   },
                { "name": "changeFromSubId",      "type": "bytes32" },
                { "name": "changeEffectiveAt",    "type": "uint8"   },
                { "name": "periodMode",           "type": "uint8"   },
            ],
        },
        "primaryType": "SubscriptionTerms",
        "message": {
            "payer":                i.payer,
            "merchant":             i.merchant,
            "facilitator":          i.facilitator,
            "token":                i.token,
            "amountPerPeriod":      i.amount_per_period,
            "periodSec":            i.period_sec,
            "maxPeriods":           i.max_periods,
            "startAt":              i.start_at,
            "initialChargePeriods": i.initial_charge_periods,
            "initialChargeAmount":  i.initial_charge_amount,
            "termsDeadline":        i.terms_deadline,
            "permitHash":           i.permit_hash,
            "salt":                 i.salt,
            "planTier":             i.plan_tier,
            "changeFromSubId":      i.change_from_sub_id,
            "changeEffectiveAt":    i.change_effective_at,
            "periodMode":           i.period_mode,
        },
    })
}

/// `signTypedData_v4` shape for Permit2 `PermitSingle`. Permit2 domain has
/// no `version`.
///
/// `permit2_contract` is the Permit2 domain `verifyingContract`, taken from
/// the Seller's `extra.contracts.permit2` (Seller-declared, not hardcoded).
#[allow(clippy::too_many_arguments)]
pub fn build_permit_single_typed_data(
    token: &str,
    amount: &str,
    expiration: u64,
    nonce: u64,
    spender: &str,
    sig_deadline: &str,
    permit2_contract: &str,
    chain_id: u64,
) -> Value {
    json!({
        "domain": {
            "name": "Permit2",
            "chainId": chain_id,
            "verifyingContract": permit2_contract,
        },
        "types": {
            "EIP712Domain": [
                { "name": "name",              "type": "string"  },
                { "name": "chainId",           "type": "uint256" },
                { "name": "verifyingContract", "type": "address" },
            ],
            "PermitSingle": [
                { "name": "details",     "type": "PermitDetails" },
                { "name": "spender",     "type": "address"       },
                { "name": "sigDeadline", "type": "uint256"       },
            ],
            "PermitDetails": [
                { "name": "token",      "type": "address" },
                { "name": "amount",     "type": "uint160" },
                { "name": "expiration", "type": "uint48"  },
                { "name": "nonce",      "type": "uint48"  },
            ],
        },
        "primaryType": "PermitSingle",
        "message": {
            "details": {
                "token":      token,
                "amount":     amount,
                "expiration": expiration,
                "nonce":      nonce,
            },
            "spender":     spender,
            "sigDeadline": sig_deadline,
        },
    })
}

/// `signTypedData_v4` shape for `CancelAuth` (subscription domain).
/// `action`: 0 = cancel_subscription. `initiator`: 0 = payer, 1 = merchant.
pub fn build_cancel_auth_typed_data(
    action: u8,
    sub_id: &str,
    initiator: u8,
    nonce: &str,
    deadline: u64,
    d: &SubDomain<'_>,
) -> Value {
    json!({
        "domain": {
            "name": "A2APaySubscription",
            "version": "1",
            "chainId": d.chain_id,
            "verifyingContract": d.verifying_contract,
        },
        "types": {
            "EIP712Domain": [
                { "name": "name",              "type": "string"  },
                { "name": "version",           "type": "string"  },
                { "name": "chainId",           "type": "uint256" },
                { "name": "verifyingContract", "type": "address" },
            ],
            "CancelAuth": [
                { "name": "action",    "type": "uint8"   },
                { "name": "subId",     "type": "bytes32" },
                { "name": "initiator", "type": "uint8"   },
                { "name": "nonce",     "type": "bytes32" },
                { "name": "deadline",  "type": "uint64"  },
            ],
        },
        "primaryType": "CancelAuth",
        "message": {
            "action":    action,
            "subId":     sub_id,
            "initiator": initiator,
            "nonce":     nonce,
            "deadline":  deadline,
        },
    })
}

/// `signTypedData_v4` shape for `PendingChangeCancelAuth` (subscription
/// domain, payer-only — cancels a not-yet-effective downgrade).
pub fn build_pending_change_cancel_auth_typed_data(
    sub_id: &str,
    new_sub_id: &str,
    nonce: &str,
    deadline: u64,
    d: &SubDomain<'_>,
) -> Value {
    json!({
        "domain": {
            "name": "A2APaySubscription",
            "version": "1",
            "chainId": d.chain_id,
            "verifyingContract": d.verifying_contract,
        },
        "types": {
            "EIP712Domain": [
                { "name": "name",              "type": "string"  },
                { "name": "version",           "type": "string"  },
                { "name": "chainId",           "type": "uint256" },
                { "name": "verifyingContract", "type": "address" },
            ],
            "PendingChangeCancelAuth": [
                { "name": "subId",    "type": "bytes32" },
                { "name": "newSubId", "type": "bytes32" },
                { "name": "nonce",    "type": "bytes32" },
                { "name": "deadline", "type": "uint64"  },
            ],
        },
        "primaryType": "PendingChangeCancelAuth",
        "message": {
            "subId":    sub_id,
            "newSubId": new_sub_id,
            "nonce":    nonce,
            "deadline": deadline,
        },
    })
}

// ──────────────────────────── local hash helpers ───────────────────────────

/// 32-byte EIP-712 word for an address (left-padded to 32 bytes).
fn word_addr(s: &str) -> Result<[u8; 32]> {
    let a = Address::from_str(s.trim()).with_context(|| format!("invalid address: {s}"))?;
    Ok(a.into_word().0)
}

/// 32-byte big-endian EIP-712 word for a decimal uint string.
fn word_uint_dec(s: &str) -> Result<[u8; 32]> {
    let v = U256::from_str(s.trim()).with_context(|| format!("invalid uint (decimal): {s}"))?;
    Ok(v.to_be_bytes::<32>())
}

/// 32-byte big-endian EIP-712 word for a `u64`.
fn word_u64(n: u64) -> [u8; 32] {
    U256::from(n).to_be_bytes::<32>()
}

/// 32-byte word for a `0x`-prefixed bytes32 hex string.
fn word_b32(s: &str) -> Result<[u8; 32]> {
    let b = B256::from_str(s.trim()).with_context(|| format!("invalid bytes32: {s}"))?;
    Ok(b.0)
}

/// `terms.permitHash` = the EIP-712 `hashStruct` of the `PermitSingle` the
/// buyer signs (NOT the full digest). The SA recomputes and byte-compares
/// this (`permit_hash_mismatch`).
///
/// ```text
/// detailsHash = keccak256( PERMIT_DETAILS_TYPEHASH ‖ token ‖ amount ‖ expiration ‖ nonce )
/// permitHash  = keccak256( PERMIT_SINGLE_TYPEHASH  ‖ detailsHash ‖ spender ‖ sigDeadline )
/// ```
pub fn permit_single_struct_hash(
    token: &str,
    amount: &str,
    expiration: u64,
    nonce: u64,
    spender: &str,
    sig_deadline: &str,
) -> Result<[u8; 32]> {
    let details_typehash = keccak256(PERMIT_DETAILS_TYPESTRING.as_bytes());
    let mut details = Vec::with_capacity(32 * 5);
    details.extend_from_slice(details_typehash.as_slice());
    details.extend_from_slice(&word_addr(token)?);
    details.extend_from_slice(&word_uint_dec(amount)?);
    details.extend_from_slice(&word_u64(expiration));
    details.extend_from_slice(&word_u64(nonce));
    let details_hash = keccak256(&details);

    let single_typehash = keccak256(PERMIT_SINGLE_TYPESTRING.as_bytes());
    let mut single = Vec::with_capacity(32 * 4);
    single.extend_from_slice(single_typehash.as_slice());
    single.extend_from_slice(details_hash.as_slice());
    single.extend_from_slice(&word_addr(spender)?);
    single.extend_from_slice(&word_uint_dec(sig_deadline)?);
    Ok(keccak256(&single).0)
}

/// EIP-712 `hashStruct(SubscriptionTerms)` over the 17 signed fields.
pub fn subscription_terms_struct_hash(i: &SubscriptionTermsInput<'_>) -> Result<[u8; 32]> {
    let typehash = keccak256(SUBSCRIPTION_TERMS_TYPESTRING.as_bytes());
    let mut buf = Vec::with_capacity(32 * 18);
    buf.extend_from_slice(typehash.as_slice());
    buf.extend_from_slice(&word_addr(i.payer)?);
    buf.extend_from_slice(&word_addr(i.merchant)?);
    buf.extend_from_slice(&word_addr(i.facilitator)?);
    buf.extend_from_slice(&word_addr(i.token)?);
    buf.extend_from_slice(&word_uint_dec(i.amount_per_period)?);
    buf.extend_from_slice(&word_u64(i.period_sec));
    buf.extend_from_slice(&word_u64(i.max_periods as u64));
    buf.extend_from_slice(&word_u64(i.start_at));
    buf.extend_from_slice(&word_u64(i.initial_charge_periods as u64));
    buf.extend_from_slice(&word_uint_dec(i.initial_charge_amount)?);
    buf.extend_from_slice(&word_u64(i.terms_deadline));
    buf.extend_from_slice(&word_b32(i.permit_hash)?);
    buf.extend_from_slice(&word_b32(i.salt)?);
    buf.extend_from_slice(&word_u64(i.plan_tier as u64));
    buf.extend_from_slice(&word_b32(i.change_from_sub_id)?);
    buf.extend_from_slice(&word_u64(i.change_effective_at as u64));
    buf.extend_from_slice(&word_u64(i.period_mode as u64));
    Ok(keccak256(&buf).0)
}

/// `domainSeparator` of the subscription (`A2APaySubscription` / v1) domain.
fn sub_domain_separator(d: &SubDomain<'_>) -> Result<[u8; 32]> {
    let typehash = keccak256(EIP712_DOMAIN_TYPESTRING.as_bytes());
    let name_hash = keccak256(b"A2APaySubscription");
    let version_hash = keccak256(b"1");
    let mut buf = Vec::with_capacity(32 * 5);
    buf.extend_from_slice(typehash.as_slice());
    buf.extend_from_slice(name_hash.as_slice());
    buf.extend_from_slice(version_hash.as_slice());
    buf.extend_from_slice(&word_u64(d.chain_id));
    buf.extend_from_slice(&word_addr(d.verifying_contract)?);
    Ok(keccak256(&buf).0)
}

/// `subId` = `termsDigest` = `keccak256(0x1901 ‖ domainSeparator ‖ termsStructHash)`.
/// The SA also returns it in `PAYMENT-RESPONSE`, which remains authoritative.
pub fn terms_digest(i: &SubscriptionTermsInput<'_>) -> Result<[u8; 32]> {
    let domain_sep = sub_domain_separator(&i.domain)?;
    let struct_hash = subscription_terms_struct_hash(i)?;
    let mut buf = Vec::with_capacity(2 + 32 + 32);
    buf.extend_from_slice(&[0x19, 0x01]);
    buf.extend_from_slice(&domain_sep);
    buf.extend_from_slice(&struct_hash);
    Ok(keccak256(&buf).0)
}

/// Byte width the AccessProof `timestamp` is packed as inside
/// `abi.encodePacked`. Defaults to `uint256` (32 bytes); must match the
/// Seller's `buildAccessMessage` encoding (set to `8` / `4` for `uint64` /
/// `uint32`).
const ACCESS_PROOF_TIMESTAMP_BYTES: usize = 32;

/// AccessProof `inner = keccak256(abi.encodePacked(subId, payer, timestamp))`;
/// the buyer EIP-191 personal-signs this 32-byte `inner`.
///
/// `abi.encodePacked` uses raw, unpadded bytes: `subId` 32B, `payer` 20B (not
/// left-padded), `timestamp` big-endian over [`ACCESS_PROOF_TIMESTAMP_BYTES`].
pub fn access_proof_inner_hash(sub_id: &str, payer: &str, timestamp: u64) -> Result<[u8; 32]> {
    let sub = B256::from_str(sub_id.trim())
        .with_context(|| format!("invalid subId bytes32: {sub_id}"))?;
    let addr = Address::from_str(payer.trim())
        .with_context(|| format!("invalid payer address: {payer}"))?;
    let ts = U256::from(timestamp).to_be_bytes::<32>();

    let mut buf = Vec::with_capacity(32 + 20 + ACCESS_PROOF_TIMESTAMP_BYTES);
    buf.extend_from_slice(sub.as_slice()); // 32B
    buf.extend_from_slice(addr.as_slice()); // 20B (raw — packed, not padded)
    buf.extend_from_slice(&ts[32 - ACCESS_PROOF_TIMESTAMP_BYTES..]); // big-endian low bytes
    Ok(keccak256(&buf).0)
}

/// `0x`-prefixed lowercase hex of a 32-byte hash.
pub fn hex0x(bytes: [u8; 32]) -> String {
    format!("0x{}", hex::encode(bytes))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::chains::PERMIT2_ADDRESS;

    const SUB_CONTRACT: &str = "0x4020000000000000000000000000000000000003";
    const TOKEN: &str = "0x779ded0c9e1022225f8e0630b35a9b54be713736";
    const ZERO32: &str = "0x0000000000000000000000000000000000000000000000000000000000000000";

    fn sample_terms() -> SubscriptionTermsInput<'static> {
        SubscriptionTermsInput {
            payer: "0x000000000000000000000000000000000000beef",
            merchant: "0x000000000000000000000000000000000000cafe",
            facilitator: "0x000000000000000000000000000000000000dead",
            token: TOKEN,
            amount_per_period: "5000000",
            period_sec: 2_592_000,
            max_periods: 12,
            start_at: 0,
            initial_charge_periods: 1,
            initial_charge_amount: "5000000",
            terms_deadline: 1_750_000_000,
            permit_hash: ZERO32,
            salt: "0x1111111111111111111111111111111111111111111111111111111111111111",
            plan_tier: 2,
            change_from_sub_id: ZERO32,
            change_effective_at: 0,
            period_mode: 1,
            domain: SubDomain {
                chain_id: 196,
                verifying_contract: SUB_CONTRACT,
            },
        }
    }

    #[test]
    fn terms_typed_data_domain_and_primary_type() {
        let td = build_subscription_terms_typed_data(&sample_terms());
        assert_eq!(td["domain"]["name"], "A2APaySubscription");
        assert_eq!(td["domain"]["version"], "1");
        assert_eq!(td["domain"]["chainId"], 196);
        assert_eq!(td["domain"]["verifyingContract"], SUB_CONTRACT);
        assert_eq!(td["primaryType"], "SubscriptionTerms");
    }

    #[test]
    fn terms_typed_data_has_17_signed_fields_no_plan_id() {
        let td = build_subscription_terms_typed_data(&sample_terms());
        let fields = td["types"]["SubscriptionTerms"].as_array().unwrap();
        assert_eq!(fields.len(), 17, "exactly 17 signed fields");
        // planId must not leak into the message.
        assert!(td["message"].get("planId").is_none());
        // Field order is load-bearing: first and last anchor the sequence.
        assert_eq!(fields[0]["name"], "payer");
        assert_eq!(fields[4]["name"], "amountPerPeriod");
        assert_eq!(fields[4]["type"], "uint160");
        assert_eq!(fields[15]["name"], "changeEffectiveAt");
        assert_eq!(fields[16]["name"], "periodMode");
        assert_eq!(fields[16]["type"], "uint8");
        assert_eq!(td["message"]["periodMode"], 1);
    }

    #[test]
    fn permit_single_typed_data_no_version_and_uint48_fields() {
        let td = build_permit_single_typed_data(
            TOKEN,
            "60000000",
            1_782_000_000,
            7,
            SUB_CONTRACT,
            "1750000000",
            PERMIT2_ADDRESS,
            196,
        );
        assert_eq!(td["domain"]["name"], "Permit2");
        assert!(td["domain"].get("version").is_none());
        assert_eq!(td["domain"]["verifyingContract"], PERMIT2_ADDRESS);
        assert_eq!(td["primaryType"], "PermitSingle");
        let details = td["types"]["PermitDetails"].as_array().unwrap();
        assert_eq!(details[1]["type"], "uint160"); // amount
        assert_eq!(details[2]["type"], "uint48"); // expiration
        assert_eq!(details[3]["type"], "uint48"); // nonce
        assert_eq!(td["message"]["details"]["nonce"], 7);
        assert_eq!(td["message"]["spender"], SUB_CONTRACT);
    }

    #[test]
    fn cancel_auth_typed_data_shape() {
        let d = SubDomain {
            chain_id: 196,
            verifying_contract: SUB_CONTRACT,
        };
        let td = build_cancel_auth_typed_data(0, ZERO32, 0, ZERO32, 1_750_000_000, &d);
        assert_eq!(td["primaryType"], "CancelAuth");
        let fields = td["types"]["CancelAuth"].as_array().unwrap();
        assert_eq!(fields.len(), 5);
        assert_eq!(fields[0]["name"], "action");
        assert_eq!(fields[1]["name"], "subId");
        assert_eq!(td["message"]["action"], 0);
    }

    #[test]
    fn permit_single_struct_hash_is_deterministic() {
        let a = permit_single_struct_hash(
            TOKEN,
            "60000000",
            1_782_000_000,
            7,
            SUB_CONTRACT,
            "1750000000",
        )
        .unwrap();
        let b = permit_single_struct_hash(
            TOKEN,
            "60000000",
            1_782_000_000,
            7,
            SUB_CONTRACT,
            "1750000000",
        )
        .unwrap();
        assert_eq!(a, b);
        // A different nonce must change the hash.
        let c = permit_single_struct_hash(
            TOKEN,
            "60000000",
            1_782_000_000,
            8,
            SUB_CONTRACT,
            "1750000000",
        )
        .unwrap();
        assert_ne!(a, c);
    }

    #[test]
    fn typestrings_match_backend_02_section_12_3() {
        // Byte-for-byte lock on the typestrings the backend pins. Any drift
        // here changes the typehash and breaks signature verification.
        assert_eq!(
            PERMIT_DETAILS_TYPESTRING,
            "PermitDetails(address token,uint160 amount,uint48 expiration,uint48 nonce)"
        );
        assert_eq!(
            PERMIT_SINGLE_TYPESTRING,
            "PermitSingle(PermitDetails details,address spender,uint256 sigDeadline)\
             PermitDetails(address token,uint160 amount,uint48 expiration,uint48 nonce)"
        );
        assert_eq!(
            CANCEL_AUTH_TYPESTRING,
            "CancelAuth(uint8 action,bytes32 subId,uint8 initiator,bytes32 nonce,uint64 deadline)"
        );
        // SubscriptionTerms: 17 comma-separated fields inside the parens
        // (matches the contract typehash, incl. the trailing `uint8 periodMode`).
        let inner = SUBSCRIPTION_TERMS_TYPESTRING
            .strip_prefix("SubscriptionTerms(")
            .and_then(|s| s.strip_suffix(")"))
            .unwrap();
        assert_eq!(inner.split(',').count(), 17);
    }

    #[test]
    fn terms_digest_changes_with_salt() {
        let t1 = sample_terms();
        let mut t2 = sample_terms();
        t2.salt = "0x2222222222222222222222222222222222222222222222222222222222222222";
        assert_ne!(terms_digest(&t1).unwrap(), terms_digest(&t2).unwrap());
    }

    #[test]
    fn hex0x_formats_lowercase_0x() {
        let bytes = [0u8; 32];
        assert_eq!(
            hex0x(bytes),
            "0x0000000000000000000000000000000000000000000000000000000000000000"
        );
    }

    #[test]
    fn access_proof_inner_hash_deterministic_and_sensitive() {
        let sub = "0x1111111111111111111111111111111111111111111111111111111111111111";
        let payer = "0x000000000000000000000000000000000000beef";
        let a = access_proof_inner_hash(sub, payer, 1_747_200_000).unwrap();
        assert_eq!(
            a,
            access_proof_inner_hash(sub, payer, 1_747_200_000).unwrap()
        );
        // Sensitive to timestamp and payer.
        assert_ne!(
            a,
            access_proof_inner_hash(sub, payer, 1_747_200_001).unwrap()
        );
        let payer2 = "0x000000000000000000000000000000000000cafe";
        assert_ne!(
            a,
            access_proof_inner_hash(sub, payer2, 1_747_200_000).unwrap()
        );
        // Invalid inputs error out rather than panic.
        assert!(access_proof_inner_hash("not-hex", payer, 1).is_err());
    }
}
