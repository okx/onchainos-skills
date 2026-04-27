use anyhow::{bail, Context, Result};
use base64::{engine::general_purpose::STANDARD as B64, Engine};
use serde_json::{json, Value};
use zeroize::Zeroize;

use crate::commands::agentic_wallet::auth::{ensure_tokens_refreshed, format_api_error};
use crate::commands::agentic_wallet::common::handle_confirming_error;
use crate::{keyring_store, output, wallet_api::WalletApiClient, wallet_store};

/// onchainos wallet sign-message
pub(super) async fn cmd_sign_message(
    sign_type: &str,
    message: &str,
    chain: &str,
    from: &str,
    force: bool,
) -> Result<()> {
    if message.is_empty() {
        bail!("--message must not be empty");
    }
    if chain.is_empty() {
        bail!("--chain must not be empty");
    }
    if from.is_empty() {
        bail!("--from must not be empty");
    }

    if cfg!(feature = "debug-log") {
        eprintln!(
            "[DEBUG][cmd_sign_message] enter: sign_type={}, message_len={}, chain={}, from={}",
            sign_type,
            message.len(),
            chain,
            from
        );
    }

    match sign_type {
        "personal" => personal_sign(message, chain, from, force).await,
        "eip712" => eip712_sign(message, chain, from, force).await,
        _ => bail!("unsupported --type: {sign_type}, expected 'personal' or 'eip712'"),
    }
}

// ── shared: resolve chain + address ──────────────────────────────────

/// Resolve realChainIndex → (chainIndex string, chainName), then resolve from address.
async fn resolve_chain_and_address(chain: &str, from: &str) -> Result<(String, String)> {
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

    if cfg!(feature = "debug-log") {
        eprintln!(
            "[DEBUG][resolve_chain_and_address] resolved realChainIndex={} -> chainIndex={}, chainName={}",
            chain, chain_index, chain_name
        );
    }

    let wallets = wallet_store::load_wallets()?
        .ok_or_else(|| anyhow::anyhow!(super::common::ERR_NOT_LOGGED_IN))?;
    let (_acct_id, addr_info) = super::transfer::resolve_address(&wallets, Some(from), chain_name)?;

    if cfg!(feature = "debug-log") {
        eprintln!(
            "[DEBUG][resolve_chain_and_address] resolve_address => from_address={}",
            addr_info.address
        );
    }

    Ok((chain_index, addr_info.address))
}

// ── personalSign ─────────────────────────────────────────────────────

async fn personal_sign(message: &str, chain: &str, from: &str, force: bool) -> Result<()> {
    let chain: &str = &crate::chains::resolve_chain(chain);
    if cfg!(feature = "debug-log") {
        eprintln!(
            "[DEBUG][personal_sign] enter: chain={}, from={}",
            chain, from
        );
    }

    let access_token = ensure_tokens_refreshed().await?;
    if cfg!(feature = "debug-log") {
        eprintln!("[DEBUG][personal_sign] Step 1: access_token refreshed OK");
    }

    let (chain_index, from_address) = resolve_chain_and_address(chain, from).await?;
    if cfg!(feature = "debug-log") {
        eprintln!(
            "[DEBUG][personal_sign] Step 2: chain_index={}, from_address={}",
            chain_index, from_address
        );
    }

    let session = wallet_store::load_session()?
        .ok_or_else(|| anyhow::anyhow!(super::common::ERR_NOT_LOGGED_IN))?;
    let session_cert = &session.session_cert;
    let encrypted_session_sk = &session.encrypted_session_sk;
    let session_key = keyring_store::get("session_key")
        .map_err(|_| anyhow::anyhow!(super::common::ERR_NOT_LOGGED_IN))?;
    if cfg!(feature = "debug-log") {
        eprintln!(
            "[DEBUG][personal_sign] Step 3: session loaded, session_cert length={}, encrypted_session_sk length={}, session_key length={}",
            session_cert.len(), encrypted_session_sk.len(), session_key.len()
        );
    }

    // Decrypt signing seed via HPKE
    let mut signing_seed =
        crate::crypto::hpke_decrypt_session_sk(encrypted_session_sk, &session_key)?;
    if cfg!(feature = "debug-log") {
        eprintln!(
            "[DEBUG][personal_sign] Step 4: HPKE decrypt OK, signing_seed length={}",
            signing_seed.len()
        );
    }

    let session_signature = if chain == "501" {
        let hex_msg = hex::encode(message.as_bytes());
        // Solana: sign the hex message directly via ed25519_sign_hex
        let mut seed_b64 = B64.encode(signing_seed);
        signing_seed.zeroize();
        let sig = crate::crypto::ed25519_sign_hex(&hex_msg, &seed_b64)?;
        seed_b64.zeroize();
        sig
    } else {
        // EVM: EIP-191 personal sign (prefix + keccak256 + ed25519)
        let (msg_to_sign, encoding) = if super::common::is_hex_string(message, None) {
            // Pad odd-length hex so that hex::decode won't fail
            // e.g. "0x123" → "0x0123"
            let hex_part = &message[2..];
            if !hex_part.len().is_multiple_of(2) {
                (format!("0x0{hex_part}"), "hex")
            } else {
                (message.to_string(), "hex")
            }
        } else {
            (message.to_string(), "utf8")
        };
        let sig = crate::crypto::ed25519_sign_eip191(&msg_to_sign, &signing_seed, encoding)?;
        signing_seed.zeroize();
        sig
    };
    if cfg!(feature = "debug-log") {
        eprintln!(
            "[DEBUG][personal_sign] Step 5: signed OK (chain={}), session_signature length={}",
            chain,
            session_signature.len()
        );
    }

    // Encode message value: base58 for Solana (chain 501), raw for EVM
    let encoded_value = encode_message_value(message, chain);
    if cfg!(feature = "debug-log") {
        eprintln!(
            "[DEBUG][personal_sign] Step 6: encoded_value={}",
            encoded_value
        );
    }

    // Call sign-msg API
    let mut client = WalletApiClient::new()?;
    let mut body = json!({
        "chainIndex": chain_index,
        "from": from_address,
        "sessionCert": session_cert,
        "payload": [{
            "signType": "personalSign",
            "message": { "value": encoded_value },
            "sessionSignature": session_signature,
        }]
    });
    if force {
        body["skipWarning"] = json!(true);
    }
    if cfg!(feature = "debug-log") {
        eprintln!(
            "[DEBUG][personal_sign] Step 7: calling sign-msg API, body={}",
            serde_json::to_string(&body).unwrap_or_default()
        );
    }

    let data = client
        .post_authed(
            "/priapi/v5/wallet/agentic/pre-transaction/sign-msg",
            &access_token,
            &body,
        )
        .await
        .map_err(|e| handle_confirming_error(e, force))?;
    if cfg!(feature = "debug-log") {
        eprintln!(
            "[DEBUG][personal_sign] Step 8: sign-msg response={}",
            serde_json::to_string(&data).unwrap_or_default()
        );
    }

    output_sign_result(&data, chain, &from_address)
}

// ── eip712 ───────────────────────────────────────────────────────────

async fn eip712_sign(message: &str, chain: &str, from: &str, force: bool) -> Result<()> {
    let chain: &str = &crate::chains::resolve_chain(chain);
    if chain == "501" {
        bail!("eip712 signing is not supported on Solana (chain 501)");
    }

    if cfg!(feature = "debug-log") {
        eprintln!(
            "[DEBUG][eip712_sign] enter: chain={}, from={}, message_len={}",
            chain,
            from,
            message.len()
        );
    }

    let parsed_message: Value =
        serde_json::from_str(message).context("--message must be valid JSON for eip712")?;
    if cfg!(feature = "debug-log") {
        eprintln!("[DEBUG][eip712_sign] Step 1: message parsed as JSON OK");
    }

    let access_token = ensure_tokens_refreshed().await?;
    if cfg!(feature = "debug-log") {
        eprintln!("[DEBUG][eip712_sign] Step 2: access_token refreshed OK");
    }

    let (chain_index, from_address) = resolve_chain_and_address(chain, from).await?;
    if cfg!(feature = "debug-log") {
        eprintln!(
            "[DEBUG][eip712_sign] Step 3: chain_index={}, from_address={}",
            chain_index, from_address
        );
    }

    let mut client = WalletApiClient::new()?;

    // Step 1: gen-msg-hash
    let gen_hash_body = json!({
        "chainIndex": chain_index,
        "payload": [{
            "msgType": "eip712",
            "message": parsed_message,
        }]
    });
    if cfg!(feature = "debug-log") {
        eprintln!(
            "[DEBUG][eip712_sign] Step 4: calling gen-msg-hash API, body={}",
            serde_json::to_string(&gen_hash_body).unwrap_or_default()
        );
    }

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
    if cfg!(feature = "debug-log") {
        eprintln!(
            "[DEBUG][eip712_sign] Step 5: gen-msg-hash response, msgHash={}",
            msg_hash
        );
    }

    // Step 2: local sign with session key
    let session = wallet_store::load_session()?
        .ok_or_else(|| anyhow::anyhow!(super::common::ERR_NOT_LOGGED_IN))?;
    let session_cert = &session.session_cert;
    let encrypted_session_sk = &session.encrypted_session_sk;
    let session_key = keyring_store::get("session_key")
        .map_err(|_| anyhow::anyhow!(super::common::ERR_NOT_LOGGED_IN))?;
    if cfg!(feature = "debug-log") {
        eprintln!(
            "[DEBUG][eip712_sign] Step 6: session loaded, session_cert length={}, encrypted_session_sk length={}, session_key length={}",
            session_cert.len(), encrypted_session_sk.len(), session_key.len()
        );
    }

    let mut signing_seed =
        crate::crypto::hpke_decrypt_session_sk(encrypted_session_sk, &session_key)?;
    if cfg!(feature = "debug-log") {
        eprintln!(
            "[DEBUG][eip712_sign] Step 7: HPKE decrypt OK, signing_seed length={}",
            signing_seed.len()
        );
    }
    let mut signing_seed_b64 = B64.encode(signing_seed.as_slice());
    signing_seed.zeroize();

    // ed25519_sign_hex: msg_hash is already hex from gen-msg-hash API
    let session_signature = crate::crypto::ed25519_sign_hex(msg_hash, &signing_seed_b64)?;
    signing_seed_b64.zeroize();
    if cfg!(feature = "debug-log") {
        eprintln!(
            "[DEBUG][eip712_sign] Step 8: ed25519_sign_hex OK, session_signature length={}",
            session_signature.len()
        );
    }

    // Step 3: sign-msg API
    let mut sign_body = json!({
        "chainIndex": chain_index,
        "from": from_address,
        "sessionCert": session_cert,
        "payload": [{
            "signType": "eip712",
            "message": parsed_message,
            "sessionSignature": session_signature,
        }]
    });
    if force {
        sign_body["skipWarning"] = json!(true);
    }
    if cfg!(feature = "debug-log") {
        eprintln!(
            "[DEBUG][eip712_sign] Step 9: calling sign-msg API, body={}",
            serde_json::to_string(&sign_body).unwrap_or_default()
        );
    }

    let data = client
        .post_authed(
            "/priapi/v5/wallet/agentic/pre-transaction/sign-msg",
            &access_token,
            &sign_body,
        )
        .await
        .map_err(|e| handle_confirming_error(e, force))?;
    if cfg!(feature = "debug-log") {
        eprintln!(
            "[DEBUG][eip712_sign] Step 10: sign-msg response={}",
            serde_json::to_string(&data).unwrap_or_default()
        );
    }

    output_sign_result(&data, chain, &from_address)
}

// ── EIP-7702 signing ────────────────────────────────────────────────

/// Sign a 7702 authorization hash for Gas Station (first-time EIP-7702 upgrade per chain).
///
/// Self-contained flow (mirrors eip712_sign pattern):
/// 1. Load session + decrypt signing key via HPKE
/// 2. ed25519_sign_hex(auth_hash) — the hash is already computed by the backend
/// 3. Zeroize key material
///
/// Returns base64-encoded signature.
pub fn sign_eip7702_auth(auth_hash: &str) -> Result<String> {
    let session = wallet_store::load_session()?
        .ok_or_else(|| anyhow::anyhow!(super::common::ERR_NOT_LOGGED_IN))?;
    let session_key = keyring_store::get("session_key")
        .map_err(|_| anyhow::anyhow!(super::common::ERR_NOT_LOGGED_IN))?;

    let mut signing_seed =
        crate::crypto::hpke_decrypt_session_sk(&session.encrypted_session_sk, &session_key)?;
    let mut signing_seed_b64 = B64.encode(signing_seed.as_slice());
    signing_seed.zeroize();

    let signature = crate::crypto::ed25519_sign_hex(auth_hash, &signing_seed_b64)?;
    signing_seed_b64.zeroize();

    Ok(signature)
}

// ── helpers ──────────────────────────────────────────────────────────

/// Encode message: base58 for Solana (chain "501"), raw passthrough for EVM chains.
fn encode_message_value(message: &str, chain: &str) -> String {
    if chain == "501" {
        bs58::encode(message.as_bytes()).into_string()
    } else {
        message.to_string()
    }
}

fn output_sign_result(data: &Value, chain: &str, from_address: &str) -> Result<()> {
    let item = data
        .as_array()
        .and_then(|arr| arr.first())
        .ok_or_else(|| anyhow::anyhow!("sign-msg: empty response data"))?;

    let signature = item["signature"]
        .as_str()
        .ok_or_else(|| anyhow::anyhow!("missing signature in sign-msg response"))?;

    if chain == "501" {
        // Solana: convert hex signature to base58, include publicKey
        let sig_bytes = hex::decode(signature.trim_start_matches("0x"))
            .context("invalid hex signature from API")?;
        let sig_b58 = bs58::encode(&sig_bytes).into_string();
        output::success(json!({
            "signature": sig_b58,
            "publicKey": from_address,
        }));
    } else {
        output::success(json!({ "signature": signature }));
    }

    Ok(())
}

// ── Tests ────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encode_message_value_raw_for_evm() {
        let encoded = encode_message_value("Hello World", "1");
        assert_eq!(encoded, "Hello World");
    }

    #[test]
    fn encode_message_value_base58_for_solana() {
        let msg = "Hello World";
        let encoded = encode_message_value(msg, "501");
        assert_eq!(encoded, bs58::encode(msg.as_bytes()).into_string());
    }

    #[test]
    fn encode_message_value_raw_for_bsc() {
        let encoded = encode_message_value("test", "56");
        assert_eq!(encoded, "test");
    }

    #[test]
    fn output_sign_result_extracts_signature_evm() {
        let data = json!([{ "signature": "0xabc123" }]);
        assert!(output_sign_result(&data, "1", "0xAddr").is_ok());
    }

    #[test]
    fn output_sign_result_solana_converts_to_base58() {
        // hex signature → base58
        let hex_sig = format!("0x{}", hex::encode(b"test_signature"));
        let data = json!([{ "signature": hex_sig }]);
        assert!(output_sign_result(&data, "501", "SolAddr123").is_ok());
    }

    #[test]
    fn output_sign_result_errors_on_empty_array() {
        let data = json!([]);
        assert!(output_sign_result(&data, "1", "0xAddr").is_err());
    }

    #[test]
    fn output_sign_result_errors_on_missing_signature() {
        let data = json!([{}]);
        assert!(output_sign_result(&data, "1", "0xAddr").is_err());
    }
}
