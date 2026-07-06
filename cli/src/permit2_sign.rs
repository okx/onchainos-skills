//! Permit2 EIP-712 signing for `exact + Permit2` and `upto` schemes.
//! All paths produce a 65-byte secp256k1 hex signature (`0x...`). TEE
//! variants (`sign_*_permit2`) sign via the enclave; local variants
//! (`sign_*_permit2_local`) sign with an on-disk EOA private key.

use alloy_sol_types::SolStruct;
use anyhow::{anyhow, Context, Result};
use base64::engine::general_purpose::STANDARD as B64;
use base64::Engine;
use serde_json::json;
use zeroize::Zeroize;

use crate::commands::agentic_wallet::auth::{ensure_tokens_refreshed, format_api_error};
use crate::keyring_store;
use crate::permit2_eip712::{
    build_exact_permit2_struct, build_exact_permit2_typed_data, build_upto_permit2_struct,
    build_upto_permit2_typed_data, permit2_domain, ExactPermit2Input, UptoPermit2Input,
};
use crate::permit2_types::{
    ExactPermit2Payload, Permit2Authorization, Permit2Permitted, Permit2Witness,
    UptoPermit2Authorization, UptoPermit2Payload, UptoPermit2Witness,
};
use crate::wallet_api::WalletApiClient;
use crate::wallet_store;

type Signature = String;

/// HPKE-decrypt the wallet session signing seed and Ed25519-sign `msg_hash`.
/// Returns `(signature_b64, session_cert)` used **only** to authenticate the
/// TEE `sign-msg` call — the final x402 payload still carries a pure secp256k1
/// signature, no sessionCert.
fn session_sign_msg_hash(msg_hash: &str) -> Result<(String, String)> {
    let session = wallet_store::load_session()?.ok_or_else(|| {
        anyhow!(crate::commands::agentic_wallet::common::ERR_NOT_LOGGED_IN)
    })?;
    let session_key = keyring_store::get("session_key")
        .map_err(|_| anyhow!(crate::commands::agentic_wallet::common::ERR_NOT_LOGGED_IN))?;

    let mut signing_seed =
        crate::crypto::hpke_decrypt_session_sk(&session.encrypted_session_sk, &session_key)?;
    let mut signing_seed_b64 = B64.encode(signing_seed.as_slice());
    signing_seed.zeroize();
    let signature_b64 = crate::crypto::ed25519_sign_hex(msg_hash, &signing_seed_b64)?;
    signing_seed_b64.zeroize();

    Ok((signature_b64, session.session_cert))
}

// ── TEE path (`onchainos payment pay`) ───────────────────────────────────

/// Sign exact + Permit2 via TEE.
pub async fn sign_exact_permit2(
    chain_index: &str,
    payer_addr: &str,
    input: &ExactPermit2Input<'_>,
) -> Result<ExactPermit2Payload> {
    let typed_data = build_exact_permit2_typed_data(input);
    let signature = tee_sign_eip712(chain_index, payer_addr, &typed_data).await?;

    Ok(ExactPermit2Payload {
        signature,
        permit2_authorization: Permit2Authorization {
            from: payer_addr.to_string(),
            permitted: Permit2Permitted {
                token: input.token.to_string(),
                amount: input.amount.to_string(),
            },
            spender: input.spender.to_string(),
            nonce: input.nonce.to_string(),
            deadline: input.deadline.to_string(),
            witness: Permit2Witness {
                to: input.witness_to.to_string(),
                valid_after: input.witness_valid_after.to_string(),
            },
        },
    })
}

/// Sign upto via TEE. Returns pure secp256k1 EIP-712 signature (no sessionCert).
pub async fn sign_upto_permit2(
    chain_index: &str,
    payer_addr: &str,
    input: &UptoPermit2Input<'_>,
) -> Result<UptoPermit2Payload> {
    let typed_data = build_upto_permit2_typed_data(input);
    let signature = tee_sign_eip712(chain_index, payer_addr, &typed_data).await?;

    Ok(UptoPermit2Payload {
        signature,
        permit2_authorization: UptoPermit2Authorization {
            from: payer_addr.to_string(),
            permitted: Permit2Permitted {
                token: input.token.to_string(),
                amount: input.amount.to_string(),
            },
            spender: input.spender.to_string(),
            nonce: input.nonce.to_string(),
            deadline: input.deadline.to_string(),
            witness: UptoPermit2Witness {
                to: input.witness_to.to_string(),
                facilitator: input.witness_facilitator.to_string(),
                valid_after: input.witness_valid_after.to_string(),
            },
        },
    })
}

// ── Local-key path (`onchainos payment pay-local`) ───────────────────────

/// `crypto::secp256k1_sign` produces modern v (0/1); on-chain Permit2
/// `ecrecover` expects legacy v (27/28). Sign + convert + hex-encode.
fn sign_eip712_legacy_v(pk_bytes: &[u8], digest: &[u8]) -> Result<String> {
    let mut sig_bytes = crate::crypto::secp256k1_sign(pk_bytes, digest)?;
    sig_bytes[64] += 27;
    Ok(format!("0x{}", hex::encode(&sig_bytes)))
}

/// Sign exact + Permit2 locally — same `crypto::secp256k1_sign` path as EIP-3009.
pub fn sign_exact_permit2_local(
    pk_bytes: &[u8],
    payer_addr: &str,
    input: &ExactPermit2Input<'_>,
) -> Result<ExactPermit2Payload> {
    let permit = build_exact_permit2_struct(input)?;
    let domain = permit2_domain(input.chain_id);
    let digest = permit.eip712_signing_hash(&domain);

    let signature = sign_eip712_legacy_v(pk_bytes, digest.as_ref())?;

    Ok(ExactPermit2Payload {
        signature,
        permit2_authorization: Permit2Authorization {
            from: payer_addr.to_string(),
            permitted: Permit2Permitted {
                token: input.token.to_string(),
                amount: input.amount.to_string(),
            },
            spender: input.spender.to_string(),
            nonce: input.nonce.to_string(),
            deadline: input.deadline.to_string(),
            witness: Permit2Witness {
                to: input.witness_to.to_string(),
                valid_after: input.witness_valid_after.to_string(),
            },
        },
    })
}

/// Sign upto locally — same `crypto::secp256k1_sign` path as EIP-3009.
pub fn sign_upto_permit2_local(
    pk_bytes: &[u8],
    payer_addr: &str,
    input: &UptoPermit2Input<'_>,
) -> Result<UptoPermit2Payload> {
    let permit = build_upto_permit2_struct(input)?;
    let domain = permit2_domain(input.chain_id);
    let digest = permit.eip712_signing_hash(&domain);

    let signature = sign_eip712_legacy_v(pk_bytes, digest.as_ref())?;

    Ok(UptoPermit2Payload {
        signature,
        permit2_authorization: UptoPermit2Authorization {
            from: payer_addr.to_string(),
            permitted: Permit2Permitted {
                token: input.token.to_string(),
                amount: input.amount.to_string(),
            },
            spender: input.spender.to_string(),
            nonce: input.nonce.to_string(),
            deadline: input.deadline.to_string(),
            witness: UptoPermit2Witness {
                to: input.witness_to.to_string(),
                facilitator: input.witness_facilitator.to_string(),
                valid_after: input.witness_valid_after.to_string(),
            },
        },
    })
}

// ── TEE shared helpers ───────────────────────────────────────────────────

/// POST `gen-msg-hash` with EIP-712 typed-data; returns `0x`-prefixed msgHash.
///
/// `pub(crate)` so the `period` scheme (`subscription_sign`) can
/// reuse the exact same digest path the facilitator uses.
pub(crate) async fn tee_gen_msg_hash(
    chain_index: &str,
    typed_data: &serde_json::Value,
) -> Result<String> {
    let access_token = ensure_tokens_refreshed().await?;
    let mut client = WalletApiClient::new()?;

    let body = json!({
        "chainIndex": chain_index,
        "payload": [{
            "msgType": "eip712",
            "message": typed_data,
        }]
    });
    let resp = client
        .post_authed(
            "/priapi/v5/wallet/agentic/pre-transaction/gen-msg-hash",
            &access_token,
            &body,
        )
        .await
        .map_err(format_api_error)
        .context("permit2 gen-msg-hash failed")?;
    resp[0]["msgHash"]
        .as_str()
        .ok_or_else(|| anyhow!("missing msgHash in gen-msg-hash response"))
        .map(str::to_string)
}

/// gen-msg-hash → local Ed25519 session-sign → sign-msg (TEE secp256k1).
/// `skipWarning: true` because this is an automated x402 payment, not an
/// interactive transfer.
///
/// `pub(crate)` so the `period` scheme reuses the same
/// `signType: "eip712"` secp256k1 path (the subscription contract recovers
/// `payer` via `ecrecover`, so an Ed25519 session signature won't do).
pub(crate) async fn tee_sign_eip712(
    chain_index: &str,
    payer_addr: &str,
    typed_data: &serde_json::Value,
) -> Result<Signature> {
    let msg_hash = tee_gen_msg_hash(chain_index, typed_data).await?;

    let access_token = ensure_tokens_refreshed().await?;
    let (session_signature, session_cert) = session_sign_msg_hash(&msg_hash)?;

    let mut client = WalletApiClient::new()?;

    let sign_body = json!({
        "chainIndex":  chain_index,
        "from":        payer_addr,
        "sessionCert": session_cert,
        "payload": [{
            "signType":         "eip712",
            "message":          typed_data,
            "sessionSignature": session_signature,
        }],
        "skipWarning": true,
    });
    let sign_resp = client
        .post_authed(
            "/priapi/v5/wallet/agentic/pre-transaction/sign-msg",
            &access_token,
            &sign_body,
        )
        .await
        .map_err(format_api_error)
        .context("permit2 sign-msg failed")?;
    sign_resp[0]["signature"]
        .as_str()
        .ok_or_else(|| anyhow!("missing signature in sign-msg response"))
        .map(str::to_string)
}

/// TEE `personalSign` over a pre-hashed 32-byte value (`value_hex` = `0x` +
/// 64 hex). The session key signs the EIP-191 message
/// `keccak256("\x19Ethereum Signed Message:\n32" ‖ value)` (matching the
/// subscription AccessProof spec, 02 §4.2), and the TEE returns the 65-byte
/// secp256k1 signature (signer == `from`). `pub(crate)` for `subscription_sign`.
pub(crate) async fn tee_sign_personal(
    chain_index: &str,
    from: &str,
    value_hex: &str,
) -> Result<Signature> {
    let access_token = ensure_tokens_refreshed().await?;
    let session = wallet_store::load_session()?
        .ok_or_else(|| anyhow!(crate::commands::agentic_wallet::common::ERR_NOT_LOGGED_IN))?;
    let session_key = keyring_store::get("session_key")
        .map_err(|_| anyhow!(crate::commands::agentic_wallet::common::ERR_NOT_LOGGED_IN))?;

    let mut signing_seed =
        crate::crypto::hpke_decrypt_session_sk(&session.encrypted_session_sk, &session_key)?;
    // EVM EIP-191 personal sign over the 32-byte `value` (encoding = "hex" →
    // the helper hex-decodes to bytes, prefixes "\x19...\n32", keccaks, signs).
    let session_signature =
        crate::crypto::ed25519_sign_eip191(value_hex, signing_seed.as_slice(), "hex")?;
    signing_seed.zeroize();

    let mut client = WalletApiClient::new()?;
    let body = json!({
        "chainIndex":  chain_index,
        "from":        from,
        "sessionCert": session.session_cert,
        "payload": [{
            "signType":         "personalSign",
            "message":          { "value": value_hex },
            "sessionSignature": session_signature,
        }],
        "skipWarning": true,
    });
    let resp = client
        .post_authed(
            "/priapi/v5/wallet/agentic/pre-transaction/sign-msg",
            &access_token,
            &body,
        )
        .await
        .map_err(format_api_error)
        .context("AccessProof personalSign failed")?;
    resp[0]["signature"]
        .as_str()
        .ok_or_else(|| anyhow!("missing signature in personalSign response"))
        .map(str::to_string)
}
