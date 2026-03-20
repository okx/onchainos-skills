use anyhow::{bail, Context, Result};
use base64::{engine::general_purpose::STANDARD as B64, Engine};
use serde_json::{json, Value};
use zeroize::Zeroize;

use crate::commands::agentic_wallet::auth::{ensure_tokens_refreshed, format_api_error};
use crate::{keyring_store, output, wallet_api::WalletApiClient, wallet_store};

/// onchainos wallet sign-message
pub(super) async fn cmd_sign_message(
    sign_type: &str,
    message: &str,
    chain: &str,
    from: Option<&str>,
) -> Result<()> {
    if message.is_empty() {
        bail!("--message must not be empty");
    }
    if chain.is_empty() {
        bail!("--chain must not be empty");
    }

    match sign_type {
        "personal" => personal_sign(message, chain, from).await,
        "eip712" => eip712_sign(message, chain, from).await,
        _ => bail!("unsupported --type: {sign_type}, expected 'personal' or 'eip712'"),
    }
}

// ── shared: resolve chain + address ──────────────────────────────────

/// Resolve realChainIndex → (chainIndex string, chainName), then resolve from address.
async fn resolve_chain_and_address(
    chain: &str,
    from: Option<&str>,
) -> Result<(String, String)> {
    let chain_entry = super::chain::get_chain_by_real_chain_index(chain)
        .await?
        .ok_or_else(|| anyhow::anyhow!("unsupported chain: {chain}"))?;
    let chain_index = chain_entry["chainIndex"]
        .as_str()
        .map(|s| s.to_string())
        .or_else(|| chain_entry["chainIndex"].as_u64().map(|n| n.to_string()))
        .ok_or_else(|| anyhow::anyhow!("missing chainIndex in chain entry"))?;
    let chain_name = chain_entry["chainName"]
        .as_str()
        .ok_or_else(|| anyhow::anyhow!("missing chainName in chain entry"))?;

    let wallets = wallet_store::load_wallets()?
        .ok_or_else(|| anyhow::anyhow!(super::common::ERR_NOT_LOGGED_IN))?;
    let (_acct_id, addr_info) =
        super::transfer::resolve_address(&wallets, from, chain_name)?;

    Ok((chain_index, addr_info.address))
}

// ── personalSign ─────────────────────────────────────────────────────

async fn personal_sign(message: &str, chain: &str, from: Option<&str>) -> Result<()> {
    let access_token = ensure_tokens_refreshed().await?;
    let (chain_index, from_address) = resolve_chain_and_address(chain, from).await?;

    let session = wallet_store::load_session()?
        .ok_or_else(|| anyhow::anyhow!(super::common::ERR_NOT_LOGGED_IN))?;
    let session_cert = &session.session_cert;
    let encrypted_session_sk = &session.encrypted_session_sk;
    let session_key = keyring_store::get("session_key")
        .map_err(|_| anyhow::anyhow!(super::common::ERR_NOT_LOGGED_IN))?;

    // Decrypt signing seed via HPKE
    let mut signing_seed =
        crate::crypto::hpke_decrypt_session_sk(encrypted_session_sk, &session_key)?;

    // EIP-191 sign: hex-encode message bytes → ed25519_sign_eip191
    let hex_msg = hex::encode(message.as_bytes());
    let session_signature =
        crate::crypto::ed25519_sign_eip191(&hex_msg, &signing_seed)?;
    signing_seed.zeroize();

    // Encode message value: base58 for Solana (chain 501), hex for EVM
    let encoded_value = encode_message_value(message.as_bytes(), chain);

    // Call sign-msg API
    let client = WalletApiClient::new()?;
    let body = json!({
        "chainIndex": chain_index,
        "from": from_address,
        "sessionCert": session_cert,
        "payload": [{
            "signType": "personalSign",
            "message": { "value": encoded_value },
            "sessionSignature": session_signature,
        }]
    });

    let data = client
        .post_authed(
            "/priapi/v5/wallet/agentic/pre-transaction/sign-msg",
            &access_token,
            &body,
        )
        .await
        .map_err(format_api_error)?;

    output_sign_result(&data)
}

// ── eip712 ───────────────────────────────────────────────────────────

async fn eip712_sign(message: &str, chain: &str, from: Option<&str>) -> Result<()> {
    let parsed_message: Value =
        serde_json::from_str(message).context("--message must be valid JSON for eip712")?;

    let access_token = ensure_tokens_refreshed().await?;
    let (chain_index, from_address) = resolve_chain_and_address(chain, from).await?;
    let client = WalletApiClient::new()?;

    // Step 1: gen-msg-hash
    let gen_hash_body = json!({
        "chainIndex": chain_index,
        "payload": [{
            "msgType": "eip712",
            "message": parsed_message,
        }]
    });

    let hash_resp = client
        .post_authed(
            "/priapi/v5/wallet/agentic/pre-transaction/gen-msg-hash",
            &access_token,
            &gen_hash_body,
        )
        .await
        .map_err(format_api_error)
        .context("gen-msg-hash failed")?;

    let msg_hash = hash_resp[0]["msgHash"]
        .as_str()
        .ok_or_else(|| anyhow::anyhow!("missing msgHash in gen-msg-hash response"))?;

    // Step 2: local sign with session key
    let session = wallet_store::load_session()?
        .ok_or_else(|| anyhow::anyhow!(super::common::ERR_NOT_LOGGED_IN))?;
    let session_cert = &session.session_cert;
    let encrypted_session_sk = &session.encrypted_session_sk;
    let session_key = keyring_store::get("session_key")
        .map_err(|_| anyhow::anyhow!(super::common::ERR_NOT_LOGGED_IN))?;

    let mut signing_seed =
        crate::crypto::hpke_decrypt_session_sk(encrypted_session_sk, &session_key)?;
    let msg_hash_bytes =
        hex::decode(msg_hash.trim_start_matches("0x")).context("invalid msgHash hex")?;
    let signature_bytes = crate::crypto::ed25519_sign(&signing_seed, &msg_hash_bytes)?;
    signing_seed.zeroize();
    let session_signature = B64.encode(&signature_bytes);

    // Step 3: sign-msg API
    let sign_body = json!({
        "chainIndex": chain_index,
        "from": from_address,
        "sessionCert": session_cert,
        "payload": [{
            "signType": "eip712",
            "message": parsed_message,
            "sessionSignature": session_signature,
        }]
    });

    let data = client
        .post_authed(
            "/priapi/v5/wallet/agentic/pre-transaction/sign-msg",
            &access_token,
            &sign_body,
        )
        .await
        .map_err(format_api_error)?;

    output_sign_result(&data)
}

// ── helpers ──────────────────────────────────────────────────────────

/// Encode message bytes: base58 for Solana (chain "501"), hex for EVM chains.
fn encode_message_value(msg: &[u8], chain: &str) -> String {
    if chain == "501" {
        bs58::encode(msg).into_string()
    } else {
        format!("0x{}", hex::encode(msg))
    }
}

fn output_sign_result(data: &Value) -> Result<()> {
    let item = data
        .as_array()
        .and_then(|arr| arr.first())
        .ok_or_else(|| anyhow::anyhow!("sign-msg: empty response data"))?;

    let signature = item["signature"]
        .as_str()
        .ok_or_else(|| anyhow::anyhow!("missing signature in sign-msg response"))?;

    let mut result = json!({ "signature": signature });

    // Include r, s, v if present and non-empty
    for field in &["r", "s", "v"] {
        if let Some(val) = item[*field].as_str() {
            if !val.is_empty() {
                result[*field] = json!(val);
            }
        }
    }

    output::success(result);
    Ok(())
}

// ── Tests ────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encode_message_value_hex_for_evm() {
        let msg = b"Hello World";
        let encoded = encode_message_value(msg, "1");
        assert_eq!(encoded, format!("0x{}", hex::encode(msg)));
    }

    #[test]
    fn encode_message_value_base58_for_solana() {
        let msg = b"Hello World";
        let encoded = encode_message_value(msg, "501");
        assert_eq!(encoded, bs58::encode(msg).into_string());
    }

    #[test]
    fn encode_message_value_hex_for_bsc() {
        let encoded = encode_message_value(b"test", "56");
        assert!(encoded.starts_with("0x"));
    }

    #[test]
    fn output_sign_result_extracts_signature() {
        let data = json!([{
            "signature": "0xabc123",
            "r": "",
            "s": "",
            "v": ""
        }]);
        assert!(output_sign_result(&data).is_ok());
    }

    #[test]
    fn output_sign_result_includes_rsv_when_present() {
        let data = json!([{
            "signature": "0xabc",
            "r": "0x01",
            "s": "0x02",
            "v": "27"
        }]);
        assert!(output_sign_result(&data).is_ok());
    }

    #[test]
    fn output_sign_result_errors_on_empty_array() {
        let data = json!([]);
        assert!(output_sign_result(&data).is_err());
    }

    #[test]
    fn output_sign_result_errors_on_missing_signature() {
        let data = json!([{ "r": "0x01" }]);
        assert!(output_sign_result(&data).is_err());
    }
}
