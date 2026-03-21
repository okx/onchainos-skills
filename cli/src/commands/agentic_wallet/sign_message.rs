use anyhow::{anyhow, bail, Context, Result};
use base64::engine::general_purpose::STANDARD as B64;
use base64::Engine;
use serde_json::json;
use tiny_keccak::{Hasher, Keccak};
use zeroize::Zeroize;

use crate::commands::agentic_wallet::auth::{ensure_tokens_refreshed, format_api_error};
use crate::{keyring_store, output, wallet_api::WalletApiClient, wallet_store};

/// Compute the EIP-191 personal_sign hash of a message.
///
/// hash = keccak256("\x19Ethereum Signed Message:\n" + len(msg_bytes) + msg_bytes)
fn eip191_hash(message: &[u8]) -> [u8; 32] {
    let prefix = format!("\x19Ethereum Signed Message:\n{}", message.len());
    let mut keccak = Keccak::v256();
    keccak.update(prefix.as_bytes());
    keccak.update(message);
    let mut hash = [0u8; 32];
    keccak.finalize(&mut hash);
    hash
}

/// Sign an arbitrary message (EIP-191 personal_sign) via the TEE backend.
///
/// Flow:
///   1. Resolve wallet address
///   2. Compute EIP-191 hash locally
///   3. Sign hash with Ed25519 session key (local authorization)
///   4. Send to backend `sign-msg` for ECDSA signature from TEE
pub async fn cmd_sign_message(
    message: &str,
    chain: &str,
    from: Option<&str>,
) -> Result<()> {
    if message.is_empty() {
        bail!("--message must not be empty");
    }

    let access_token = ensure_tokens_refreshed().await?;

    // Resolve chain
    let chain_entry =
        crate::commands::agentic_wallet::chain::get_chain_by_real_chain_index(chain)
            .await?
            .ok_or_else(|| anyhow!("unsupported chain: {chain}"))?;
    let chain_index = chain_entry["chainIndex"]
        .as_str()
        .map(|s| s.to_string())
        .or_else(|| chain_entry["chainIndex"].as_u64().map(|n| n.to_string()))
        .ok_or_else(|| anyhow!("missing chainIndex in chain entry"))?;
    let chain_name = chain_entry["chainName"]
        .as_str()
        .ok_or_else(|| anyhow!("missing chainName in chain entry"))?;

    // Resolve address
    let wallets = wallet_store::load_wallets()?
        .ok_or_else(|| anyhow!(super::common::ERR_NOT_LOGGED_IN))?;
    let (_acct_id, addr_info) =
        crate::commands::agentic_wallet::transfer::resolve_address(&wallets, from, chain_name)?;
    let signer_addr = &addr_info.address;

    // Load session credentials
    let session = wallet_store::load_session()?
        .ok_or_else(|| anyhow!(super::common::ERR_NOT_LOGGED_IN))?;
    let encrypted_session_sk = &session.encrypted_session_sk;
    let session_cert = &session.session_cert;
    let session_key = keyring_store::get("session_key")
        .map_err(|_| anyhow!(super::common::ERR_NOT_LOGGED_IN))?;

    // Encode message as hex (0x-prefixed)
    let msg_hex = format!("0x{}", hex::encode(message.as_bytes()));

    // Compute EIP-191 hash locally
    let msg_hash = eip191_hash(message.as_bytes());
    let msg_hash_hex = format!("0x{}", hex::encode(msg_hash));

    // Sign the hash with Ed25519 session key (authorization proof)
    let mut signing_seed =
        crate::crypto::hpke_decrypt_session_sk(encrypted_session_sk, &session_key)?;
    let session_signature = crate::crypto::ed25519_sign(&signing_seed, &msg_hash)?;
    signing_seed.zeroize();
    let session_signature_b64 = B64.encode(&session_signature);

    // Request ECDSA signature from backend TEE
    let client = WalletApiClient::new()?;

    let sign_body = json!({
        "chainIndex": chain_index,
        "signType": "personal_sign",
        "message": msg_hex,
        "from": signer_addr,
        "msgHash": msg_hash_hex,
        "sessionCert": session_cert,
        "sessionSignature": session_signature_b64,
    });

    let sign_resp = client
        .post_authed(
            "/priapi/v5/wallet/agentic/pre-transaction/sign-msg",
            &access_token,
            &sign_body,
        )
        .await
        .map_err(format_api_error)
        .context("sign-msg failed")?;

    // Response may be an array or object
    let signature = if let Some(arr) = sign_resp.as_array() {
        arr.first()
            .and_then(|v| v["signature"].as_str())
    } else {
        sign_resp["signature"].as_str()
    };

    let signature = signature.ok_or_else(|| {
        anyhow!(
            "Backend returned null signature. \
             The sign-msg endpoint may not support personal_sign yet. \
             Response: {sign_resp}"
        )
    })?;

    output::success(json!({
        "signature": signature,
        "address": signer_addr,
        "message": message,
        "msgHash": msg_hash_hex,
    }));
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn eip191_hash_known_vector() {
        // "hello" → well-known EIP-191 hash
        let hash = eip191_hash(b"hello");
        let hex_str = hex::encode(hash);
        assert_eq!(
            hex_str,
            "50b2c43fd39106bafbba0da34fc430e1f91e3c96ea2acee2bc34119f92b37750"
        );
    }

    #[test]
    fn eip191_hash_empty() {
        let hash = eip191_hash(b"");
        assert_eq!(hash.len(), 32);
    }
}
