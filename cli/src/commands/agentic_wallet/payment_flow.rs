//! Shared x402 payment signing.
//!
//! Used by:
//! - `onchainos wallet x402-pay` (manual signing, prints JSON proof).
//! - `ApiClient` auto-payment (transparently attaches a signed header to paid
//!   requests and retries 402 responses).
//!
//! Signing always goes through the TEE session (session cert + Ed25519 session
//! key). Local EIP-3009 signing with a raw private key lives in
//! `payment.rs::cmd_eip3009_sign` and is intentionally kept separate.

use alloy_primitives::U256;
use anyhow::{anyhow, bail, Context, Result};
use base64::{engine::general_purpose::STANDARD as B64, Engine};
use serde_json::{json, Value};
use zeroize::Zeroize;

use crate::commands::agentic_wallet::auth::{ensure_tokens_refreshed, format_api_error};
use crate::commands::agentic_wallet::common::ERR_NOT_LOGGED_IN;
use crate::{keyring_store, wallet_api::WalletApiClient, wallet_store};

/// Which pricing tier to sign for. The server's config response groups paths
/// into `BASIC` / `PREMIUM` tiers, and each accepts entry's `amount` is an
/// object keyed by these tier names. The caller decides which tier to sign
/// against based on the path being requested.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, serde::Serialize)]
#[serde(rename_all = "lowercase")]
pub enum PaymentTier {
    Basic,
    Premium,
}

impl PaymentTier {
    /// JSON key used in the `amount` object (e.g. `"basic"` / `"premium"`).
    pub fn as_key(self) -> &'static str {
        match self {
            Self::Basic => "basic",
            Self::Premium => "premium",
        }
    }

    /// Parse from the server tier string (case-insensitive). Returns `None`
    /// for anything not in `{BASIC, PREMIUM}`.
    pub fn from_server_str(s: &str) -> Option<Self> {
        if s.eq_ignore_ascii_case("basic") {
            Some(Self::Basic)
        } else if s.eq_ignore_ascii_case("premium") {
            Some(Self::Premium)
        } else {
            None
        }
    }
}

/// Result of signing an x402 payment authorization.
pub struct PaymentProof {
    /// Base64 EIP-3009 signature (for `exact` scheme) or base64 Ed25519 session
    /// signature (for `aggr_deferred` scheme).
    pub signature: String,
    /// EIP-3009 authorization fields echoed back to the payer.
    pub authorization: Value,
    /// Session cert — populated only for `aggr_deferred` (the server needs it to
    /// verify the Ed25519 signature).
    pub session_cert: Option<String>,
}

/// Select the best accepts entry.
///
/// Priority: `exact` > `aggr_deferred` > first entry. Mirrors the selection
/// used by the manual `x402-pay` command so auto-payment and manual runs pick
/// the same scheme.
pub fn select_accept(accepts: &[Value]) -> Result<(Value, Option<String>)> {
    if accepts.is_empty() {
        bail!("accepts array is empty");
    }
    if let Some(e) = accepts
        .iter()
        .find(|a| a["scheme"].as_str() == Some("exact"))
    {
        return Ok((e.clone(), Some("exact".into())));
    }
    if let Some(e) = accepts
        .iter()
        .find(|a| a["scheme"].as_str() == Some("aggr_deferred"))
    {
        return Ok((e.clone(), Some("aggr_deferred".into())));
    }
    Ok((
        accepts[0].clone(),
        accepts[0]["scheme"].as_str().map(|s| s.into()),
    ))
}

struct ResolvedEntry {
    network: String,
    amount: String,
    pay_to: String,
    asset: String,
    max_timeout_seconds: u64,
    scheme: Option<String>,
}

/// Extract a minimal-unit amount string from an accepts entry.
///
/// Supports three server formats:
/// - `amount: {"basic": "100", "premium": "500"}` — new tiered schema; requires
///   `tier` to pick a side.
/// - `amount: "100"` / `amount: 100` — legacy scalar.
/// - `maxAmountRequired: "100"` / `maxAmountRequired: 100` — legacy alternate.
fn resolve_amount(entry: &Value, tier: Option<PaymentTier>) -> Result<String> {
    if let Some(obj) = entry.get("amount").and_then(|v| v.as_object()) {
        let tier = tier.ok_or_else(|| {
            anyhow!(
                "accepts.amount is a tiered object ({{basic, premium}}) but no tier was specified"
            )
        })?;
        let key = tier.as_key();
        let val = obj
            .get(key)
            .ok_or_else(|| anyhow!("accepts.amount is missing '{}' key", key))?;
        return match val {
            Value::String(s) => Ok(s.clone()),
            Value::Number(n) => Ok(n.to_string()),
            _ => bail!("accepts.amount.{} must be a string or number", key),
        };
    }
    if let Some(s) = entry.get("amount").and_then(|v| v.as_str()) {
        return Ok(s.to_string());
    }
    if let Some(n) = entry.get("amount").and_then(|v| v.as_u64()) {
        return Ok(n.to_string());
    }
    if let Some(s) = entry.get("maxAmountRequired").and_then(|v| v.as_str()) {
        return Ok(s.to_string());
    }
    if let Some(n) = entry.get("maxAmountRequired").and_then(|v| v.as_u64()) {
        return Ok(n.to_string());
    }
    bail!("missing 'amount' or 'maxAmountRequired' in accepts entry");
}

fn resolve_entry(
    entry: &Value,
    scheme: Option<String>,
    tier: Option<PaymentTier>,
) -> Result<ResolvedEntry> {
    let network = entry["network"]
        .as_str()
        .ok_or_else(|| anyhow!("missing 'network' in accepts entry"))?
        .to_string();
    let amount = resolve_amount(entry, tier)?;
    let pay_to = entry["payTo"]
        .as_str()
        .ok_or_else(|| anyhow!("missing 'payTo' in accepts entry"))?
        .to_string();
    let asset = entry["asset"]
        .as_str()
        .ok_or_else(|| anyhow!("missing 'asset' in accepts entry"))?
        .to_string();
    let max_timeout_seconds = entry["maxTimeoutSeconds"].as_u64().unwrap_or(300);
    Ok(ResolvedEntry {
        network,
        amount,
        pay_to,
        asset,
        max_timeout_seconds,
        scheme,
    })
}

/// Sign an x402 payment authorization.
///
/// `accepts` accepts either an array (we select the best entry with
/// `select_accept`) or a single object (used as-is). `tier` picks which amount
/// to sign when the server returns the new `amount: {basic, premium}` object
/// schema; for legacy scalar `amount` / `maxAmountRequired` it is ignored.
/// Returns the signed proof and the selected entry so callers can build a
/// header with the right scheme.
pub async fn sign_payment(
    accepts: &Value,
    from: Option<&str>,
    tier: Option<PaymentTier>,
) -> Result<(PaymentProof, Value)> {
    let (mut entry, scheme) = match accepts.as_array() {
        Some(arr) => select_accept(arr)?,
        None => (
            accepts.clone(),
            accepts["scheme"].as_str().map(|s| s.to_string()),
        ),
    };
    let params = resolve_entry(&entry, scheme, tier)?;
    // `/config` returns `amount` as a tiered object `{"basic": "100", "premium": "500"}`,
    // but x402 V2 requires the `accepted.amount` embedded in the header to be a
    // scalar string. Collapse to the tier we're signing for.
    if entry.get("amount").map(Value::is_object).unwrap_or(false) {
        entry["amount"] = json!(params.amount);
    }

    let access_token = ensure_tokens_refreshed().await?;
    let real_chain_id = parse_eip155_chain_id(&params.network)?;

    let chain_entry = crate::commands::agentic_wallet::chain::get_chain_by_real_chain_index(
        &real_chain_id.to_string(),
    )
    .await?
    .ok_or_else(|| anyhow!("chain not found for realChainIndex {}", real_chain_id))?;
    let chain_index = chain_entry["chainIndex"]
        .as_str()
        .map(|s| s.to_string())
        .or_else(|| chain_entry["chainIndex"].as_u64().map(|n| n.to_string()))
        .ok_or_else(|| anyhow!("missing chainIndex in chain entry"))?;
    let chain_name = chain_entry["chainName"]
        .as_str()
        .ok_or_else(|| anyhow!("missing chainName in chain entry"))?;

    let wallets =
        wallet_store::load_wallets()?.ok_or_else(|| anyhow::anyhow!(ERR_NOT_LOGGED_IN))?;
    let (_acct_id, addr_info) =
        crate::commands::agentic_wallet::transfer::resolve_address(&wallets, from, chain_name)?;
    let payer_addr = &addr_info.address;

    let is_deferred = params
        .scheme
        .as_deref()
        .map(|s| s.eq_ignore_ascii_case("aggr_deferred"))
        .unwrap_or(false);
    let valid_before = if is_deferred {
        // aggr_deferred: authorization never expires — server settles later.
        U256::MAX.to_string()
    } else {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)?
            .as_secs();
        now.checked_add(params.max_timeout_seconds)
            .ok_or_else(|| anyhow!("timeout overflow"))?
            .to_string()
    };
    let nonce = {
        use rand::RngCore;
        let mut n = [0u8; 32];
        rand::rngs::OsRng.fill_bytes(&mut n);
        format!("0x{}", hex::encode(n))
    };

    let base_fields = json!({
        "chainIndex": chain_index,
        "from": payer_addr,
        "to": &params.pay_to,
        "value": &params.amount,
        "validAfter": "0",
        "validBefore": valid_before,
        "nonce": nonce,
        "verifyingContract": &params.asset,
    });

    let session =
        wallet_store::load_session()?.ok_or_else(|| anyhow::anyhow!(ERR_NOT_LOGGED_IN))?;
    let encrypted_session_sk = &session.encrypted_session_sk;
    let session_cert = &session.session_cert;
    let session_key =
        keyring_store::get("session_key").map_err(|_| anyhow::anyhow!(ERR_NOT_LOGGED_IN))?;

    let mut client = WalletApiClient::new()?;

    let unsigned_hash_resp = client
        .post_authed(
            "/priapi/v5/wallet/agentic/pre-transaction/gen-msg-hash",
            &access_token,
            &base_fields,
        )
        .await
        .map_err(format_api_error)
        .context("x402 gen-msg-hash failed")?;
    let msg_hash = unsigned_hash_resp[0]["msgHash"]
        .as_str()
        .ok_or_else(|| anyhow!("missing msgHash in gen-msg-hash response"))?;
    let domain_hash = unsigned_hash_resp[0]["domainHash"]
        .as_str()
        .ok_or_else(|| anyhow!("missing domainHash in gen-msg-hash response"))?;

    let mut signing_seed =
        crate::crypto::hpke_decrypt_session_sk(encrypted_session_sk, &session_key)?;
    let msg_hash_bytes =
        hex::decode(msg_hash.trim_start_matches("0x")).context("invalid msgHash hex")?;
    let session_signature = crate::crypto::ed25519_sign(&signing_seed, &msg_hash_bytes)?;
    signing_seed.zeroize();
    let session_signature_b64 = B64.encode(&session_signature);

    let authorization = json!({
        "from": payer_addr,
        "to": &params.pay_to,
        "value": &params.amount,
        "validAfter": "0",
        "validBefore": valid_before,
        "nonce": nonce,
    });

    if is_deferred {
        Ok((
            PaymentProof {
                signature: session_signature_b64,
                authorization,
                session_cert: Some(session_cert.clone()),
            },
            entry,
        ))
    } else {
        let mut signed_hash_body = base_fields.clone();
        signed_hash_body["domainHash"] = json!(domain_hash);
        signed_hash_body["sessionCert"] = json!(session_cert);
        signed_hash_body["sessionSignature"] = json!(session_signature_b64);

        let signed_hash_resp = client
            .post_authed(
                "/priapi/v5/wallet/agentic/pre-transaction/sign-msg",
                &access_token,
                &signed_hash_body,
            )
            .await
            .map_err(format_api_error)
            .context("x402 sign-msg failed")?;
        let eip3009_signature = signed_hash_resp[0]["signature"]
            .as_str()
            .ok_or_else(|| anyhow!("missing signature in sign-msg response"))?;

        Ok((
            PaymentProof {
                signature: eip3009_signature.to_string(),
                authorization,
                session_cert: None,
            },
            entry,
        ))
    }
}

/// Build the base64-encoded payment header for the chosen mode.
/// Returns `(header_name, header_value)`.
///
/// `entry` is the selected accepts entry (from `sign_payment`'s second return
/// value). For `aggr_deferred`, the caller's proof carries a `session_cert`
/// which is embedded into `entry.extra` so the server can verify the
/// Ed25519 signature.
pub fn build_payment_header(
    proof: &PaymentProof,
    entry: &Value,
    resource: &str,
) -> Result<(&'static str, String)> {
    let payload_inner = json!({
        "signature": proof.signature,
        "authorization": proof.authorization,
    });

    // Embed sessionCert into entry.extra for aggr_deferred (server needs it).
    let mut accepted = entry.clone();
    if let Some(cert) = &proof.session_cert {
        if let Some(obj) = accepted.as_object_mut() {
            let extra = obj.entry("extra".to_string()).or_insert_with(|| json!({}));
            if let Some(extra_obj) = extra.as_object_mut() {
                extra_obj.insert("sessionCert".into(), json!(cert));
            }
        }
    }

    let body = json!({
        "x402Version": 2,
        "resource": {
            "url": resource,
            "mimeType": "application/json",
        },
        "accepted": accepted,
        "payload": payload_inner,
    });

    let encoded = B64.encode(serde_json::to_vec(&body).context("encode payment header body")?);
    Ok(("PAYMENT-SIGNATURE", encoded))
}

/// Extract numeric chain ID from a CAIP-2 `eip155:<chainId>` identifier.
pub(crate) fn parse_eip155_chain_id(network: &str) -> Result<u64> {
    let id_str = network.strip_prefix("eip155:").ok_or_else(|| {
        anyhow!(
            "unsupported network format: expected 'eip155:<chainId>', got '{}'",
            network
        )
    })?;
    id_str.parse::<u64>().map_err(|_| {
        anyhow!(
            "invalid chain ID '{}': must be a valid unsigned integer",
            id_str
        )
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn select_accept_prefers_exact() {
        let accepts: Vec<Value> = serde_json::from_str(
            r#"[
            {"scheme":"aggr_deferred","network":"eip155:196"},
            {"scheme":"exact","network":"eip155:196"}
        ]"#,
        )
        .unwrap();
        let (_entry, scheme) = select_accept(&accepts).unwrap();
        assert_eq!(scheme.as_deref(), Some("exact"));
    }

    #[test]
    fn select_accept_falls_back_to_aggr_deferred() {
        let accepts: Vec<Value> =
            serde_json::from_str(r#"[{"scheme":"aggr_deferred","network":"eip155:1"}]"#).unwrap();
        let (_entry, scheme) = select_accept(&accepts).unwrap();
        assert_eq!(scheme.as_deref(), Some("aggr_deferred"));
    }

    #[test]
    fn select_accept_empty_array_errors() {
        assert!(select_accept(&[]).is_err());
    }

    #[test]
    fn resolve_entry_extracts_amount_from_max_amount_required() {
        let v = json!({"network":"eip155:1","maxAmountRequired":"999","payTo":"0xA","asset":"0xB"});
        let r = resolve_entry(&v, None, None).unwrap();
        assert_eq!(r.amount, "999");
    }

    #[test]
    fn resolve_entry_default_timeout() {
        let v = json!({"network":"eip155:1","amount":"1","payTo":"0xA","asset":"0xB"});
        let r = resolve_entry(&v, None, None).unwrap();
        assert_eq!(r.max_timeout_seconds, 300);
    }

    #[test]
    fn resolve_amount_object_selects_by_tier() {
        let v = json!({"amount": {"basic": "100", "premium": "500"}});
        assert_eq!(resolve_amount(&v, Some(PaymentTier::Basic)).unwrap(), "100");
        assert_eq!(
            resolve_amount(&v, Some(PaymentTier::Premium)).unwrap(),
            "500"
        );
    }

    #[test]
    fn resolve_amount_object_requires_tier() {
        let v = json!({"amount": {"basic": "100", "premium": "500"}});
        assert!(resolve_amount(&v, None).is_err());
    }

    #[test]
    fn resolve_amount_object_missing_key_errors() {
        let v = json!({"amount": {"basic": "100"}});
        assert!(resolve_amount(&v, Some(PaymentTier::Premium)).is_err());
    }

    #[test]
    fn resolve_amount_scalar_ignores_tier() {
        let v = json!({"amount": "42"});
        assert_eq!(resolve_amount(&v, Some(PaymentTier::Basic)).unwrap(), "42");
        assert_eq!(resolve_amount(&v, None).unwrap(), "42");
    }

    #[test]
    fn resolve_amount_number_form() {
        let v = json!({"amount": 42});
        assert_eq!(resolve_amount(&v, None).unwrap(), "42");
    }

    #[test]
    fn resolve_entry_object_amount_with_tier() {
        let v = json!({
            "network": "eip155:196",
            "amount": {"basic": "100", "premium": "500"},
            "payTo": "0xA",
            "asset": "0xB"
        });
        let r = resolve_entry(&v, None, Some(PaymentTier::Premium)).unwrap();
        assert_eq!(r.amount, "500");
    }

    #[test]
    fn payment_tier_roundtrip() {
        assert_eq!(
            PaymentTier::from_server_str("BASIC"),
            Some(PaymentTier::Basic)
        );
        assert_eq!(
            PaymentTier::from_server_str("basic"),
            Some(PaymentTier::Basic)
        );
        assert_eq!(
            PaymentTier::from_server_str("Premium"),
            Some(PaymentTier::Premium)
        );
        assert_eq!(PaymentTier::from_server_str("other"), None);
        assert_eq!(PaymentTier::Basic.as_key(), "basic");
        assert_eq!(PaymentTier::Premium.as_key(), "premium");
    }

    #[test]
    fn build_payment_header_includes_resource() {
        let proof = PaymentProof {
            signature: "sig".into(),
            authorization: json!({}),
            session_cert: None,
        };
        let entry = json!({"scheme":"exact","network":"eip155:1"});
        let (name, value) =
            build_payment_header(&proof, &entry, "https://api.example.com/foo").unwrap();
        assert_eq!(name, "PAYMENT-SIGNATURE");
        let body: Value = serde_json::from_slice(&B64.decode(&value).unwrap()).unwrap();
        assert_eq!(body["x402Version"], 2);
        assert_eq!(body["resource"]["url"], "https://api.example.com/foo");
        assert_eq!(body["resource"]["mimeType"], "application/json");
    }

    #[test]
    fn build_payment_header_embeds_session_cert_for_deferred() {
        let proof = PaymentProof {
            signature: "sig".into(),
            authorization: json!({}),
            session_cert: Some("cert-123".into()),
        };
        let entry = json!({"scheme":"aggr_deferred","network":"eip155:196"});
        let (_name, value) =
            build_payment_header(&proof, &entry, "https://api.example.com/foo").unwrap();
        let body: Value = serde_json::from_slice(&B64.decode(&value).unwrap()).unwrap();
        assert_eq!(body["accepted"]["extra"]["sessionCert"], "cert-123");
    }
}
