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

/// Which payment header format to emit.
///
/// The three variants match the server-side schemes in section 5 of the
/// "onchainos CLI 自动付费方案" doc.
#[derive(Clone, Debug)]
pub enum PaymentMode {
    /// OKX Web3 open-api header `OK-ACCESS-PAYMENT-PAYLOAD`. Used by Market API.
    Ak,
    /// Standard x402 v1 header `X-PAYMENT`.
    V1,
    /// Standard x402 v2 header `PAYMENT-SIGNATURE`, scoped to a resource URI.
    V2 { resource: String },
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

fn resolve_entry(entry: &Value, scheme: Option<String>) -> Result<ResolvedEntry> {
    let network = entry["network"]
        .as_str()
        .ok_or_else(|| anyhow!("missing 'network' in accepts entry"))?
        .to_string();
    let amount = if let Some(s) = entry.get("amount").and_then(|v| v.as_str()) {
        s.to_string()
    } else if let Some(n) = entry.get("amount").and_then(|v| v.as_u64()) {
        n.to_string()
    } else if let Some(s) = entry.get("maxAmountRequired").and_then(|v| v.as_str()) {
        s.to_string()
    } else if let Some(n) = entry.get("maxAmountRequired").and_then(|v| v.as_u64()) {
        n.to_string()
    } else {
        bail!("missing 'amount' or 'maxAmountRequired' in accepts entry");
    };
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
/// `select_accept`) or a single object (used as-is). Returns the signed proof
/// and the selected entry so callers can build a header with the right scheme.
pub async fn sign_payment(accepts: &Value, from: Option<&str>) -> Result<(PaymentProof, Value)> {
    let (entry, scheme) = match accepts.as_array() {
        Some(arr) => select_accept(arr)?,
        None => (
            accepts.clone(),
            accepts["scheme"].as_str().map(|s| s.to_string()),
        ),
    };
    let params = resolve_entry(&entry, scheme)?;

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

    let client = WalletApiClient::new()?;

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
    mode: PaymentMode,
) -> Result<(&'static str, String)> {
    let payload_inner = json!({
        "signature": proof.signature,
        "authorization": proof.authorization,
    });

    // Embed sessionCert into entry.extra for aggr_deferred (server needs it).
    let mut requirements = entry.clone();
    if let Some(cert) = &proof.session_cert {
        if let Some(obj) = requirements.as_object_mut() {
            let extra = obj.entry("extra".to_string()).or_insert_with(|| json!({}));
            if let Some(extra_obj) = extra.as_object_mut() {
                extra_obj.insert("sessionCert".into(), json!(cert));
            }
        }
    }

    let (header_name, body) = match mode {
        PaymentMode::Ak => (
            "OK-ACCESS-PAYMENT-PAYLOAD",
            json!({
                "x402Version": 2,
                "paymentPayload": {
                    "x402Version": 2,
                    "payload": payload_inner,
                },
                "paymentRequirements": requirements,
            }),
        ),
        PaymentMode::V2 { resource } => (
            "PAYMENT-SIGNATURE",
            json!({
                "x402Version": 2,
                "resource": resource,
                "accepted": requirements,
                "payload": payload_inner,
            }),
        ),
        PaymentMode::V1 => {
            let scheme = requirements["scheme"].as_str().unwrap_or("exact");
            let network = requirements["network"].as_str().unwrap_or("");
            (
                "X-PAYMENT",
                json!({
                    "x402Version": 1,
                    "scheme": scheme,
                    "network": network,
                    "payload": payload_inner,
                }),
            )
        }
    };

    let encoded = B64.encode(serde_json::to_vec(&body).context("encode payment header body")?);
    Ok((header_name, encoded))
}

/// Extract numeric chain ID from a CAIP-2 `eip155:<chainId>` identifier.
fn parse_eip155_chain_id(network: &str) -> Result<u64> {
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
        let r = resolve_entry(&v, None).unwrap();
        assert_eq!(r.amount, "999");
    }

    #[test]
    fn resolve_entry_default_timeout() {
        let v = json!({"network":"eip155:1","amount":"1","payTo":"0xA","asset":"0xB"});
        let r = resolve_entry(&v, None).unwrap();
        assert_eq!(r.max_timeout_seconds, 300);
    }

    #[test]
    fn parse_eip155_chain_id_happy_path() {
        assert_eq!(parse_eip155_chain_id("eip155:8453").unwrap(), 8453);
    }

    #[test]
    fn parse_eip155_chain_id_missing_prefix_errors() {
        assert!(parse_eip155_chain_id("1").is_err());
    }

    #[test]
    fn build_payment_header_ak_returns_ok_access_payment_payload_header() {
        let proof = PaymentProof {
            signature: "sig".into(),
            authorization: json!({"from":"0xA"}),
            session_cert: None,
        };
        let entry = json!({"scheme":"exact","network":"eip155:1"});
        let (name, value) = build_payment_header(&proof, &entry, PaymentMode::Ak).unwrap();
        assert_eq!(name, "OK-ACCESS-PAYMENT-PAYLOAD");
        let decoded = B64.decode(&value).unwrap();
        let body: Value = serde_json::from_slice(&decoded).unwrap();
        assert_eq!(body["x402Version"], 2);
        assert_eq!(body["paymentPayload"]["payload"]["signature"], "sig");
        assert_eq!(body["paymentRequirements"]["scheme"], "exact");
    }

    #[test]
    fn build_payment_header_v1_returns_x_payment_header() {
        let proof = PaymentProof {
            signature: "sig".into(),
            authorization: json!({}),
            session_cert: None,
        };
        let entry = json!({"scheme":"exact","network":"eip155:8453"});
        let (name, value) = build_payment_header(&proof, &entry, PaymentMode::V1).unwrap();
        assert_eq!(name, "X-PAYMENT");
        let body: Value = serde_json::from_slice(&B64.decode(&value).unwrap()).unwrap();
        assert_eq!(body["x402Version"], 1);
        assert_eq!(body["scheme"], "exact");
        assert_eq!(body["network"], "eip155:8453");
    }

    #[test]
    fn build_payment_header_v2_includes_resource() {
        let proof = PaymentProof {
            signature: "sig".into(),
            authorization: json!({}),
            session_cert: None,
        };
        let entry = json!({"scheme":"exact","network":"eip155:1"});
        let (name, value) = build_payment_header(
            &proof,
            &entry,
            PaymentMode::V2 {
                resource: "https://api.example.com/foo".into(),
            },
        )
        .unwrap();
        assert_eq!(name, "PAYMENT-SIGNATURE");
        let body: Value = serde_json::from_slice(&B64.decode(&value).unwrap()).unwrap();
        assert_eq!(body["x402Version"], 2);
        assert_eq!(body["resource"], "https://api.example.com/foo");
    }

    #[test]
    fn build_payment_header_ak_embeds_session_cert_for_deferred() {
        let proof = PaymentProof {
            signature: "sig".into(),
            authorization: json!({}),
            session_cert: Some("cert-123".into()),
        };
        let entry = json!({"scheme":"aggr_deferred","network":"eip155:196"});
        let (_name, value) = build_payment_header(&proof, &entry, PaymentMode::Ak).unwrap();
        let body: Value = serde_json::from_slice(&B64.decode(&value).unwrap()).unwrap();
        assert_eq!(
            body["paymentRequirements"]["extra"]["sessionCert"],
            "cert-123"
        );
    }
}
