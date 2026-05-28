//! Permit2 EIP-712 signing — both schemes funnel through the TEE's
//! `gen-msg-hash` so the digest is computed by the same code path the
//! facilitator backend uses.
//!
//! - [`sign_exact_permit2`]: TEE returns 65-byte secp256k1 hex.
//! - [`sign_upto_permit2_session`]: buyer Ed25519-signs the digest with the
//!   session key; facilitator handles secp256k1 conversion on-chain.

use anyhow::{anyhow, Context, Result};
use base64::engine::general_purpose::STANDARD as B64;
use base64::Engine;
use serde_json::json;
use zeroize::Zeroize;

use crate::commands::agentic_wallet::auth::{ensure_tokens_refreshed, format_api_error};
use crate::keyring_store;
use crate::permit2_eip712::{
    build_exact_permit2_typed_data, build_upto_permit2_typed_data, ExactPermit2Input,
    UptoPermit2Input,
};
use crate::permit2_types::{
    ExactPermit2Payload, Permit2Authorization, Permit2Permitted, Permit2Witness,
    UptoPermit2Authorization, UptoPermit2Payload, UptoPermit2Witness,
};
use crate::wallet_api::WalletApiClient;
use crate::wallet_store;

type Signature = String;

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

/// Returns the payload (base64 Ed25519 signature) plus the `sessionCert`
/// that must be embedded into `accepted.extra` before replay.
pub async fn sign_upto_permit2_session(
    chain_index: &str,
    payer_addr: &str,
    input: &UptoPermit2Input<'_>,
) -> Result<(UptoPermit2Payload, String)> {
    let typed_data = build_upto_permit2_typed_data(input);
    let msg_hash = tee_gen_msg_hash(chain_index, &typed_data).await?;

    let session = wallet_store::load_session()?.ok_or_else(|| {
        anyhow!(crate::commands::agentic_wallet::common::ERR_NOT_LOGGED_IN)
    })?;
    let session_key = keyring_store::get("session_key")
        .map_err(|_| anyhow!(crate::commands::agentic_wallet::common::ERR_NOT_LOGGED_IN))?;

    let mut signing_seed =
        crate::crypto::hpke_decrypt_session_sk(&session.encrypted_session_sk, &session_key)?;
    let mut signing_seed_b64 = B64.encode(signing_seed.as_slice());
    signing_seed.zeroize();
    let signature_b64 = crate::crypto::ed25519_sign_hex(&msg_hash, &signing_seed_b64)?;
    signing_seed_b64.zeroize();

    let payload = UptoPermit2Payload {
        signature: signature_b64,
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
    };
    Ok((payload, session.session_cert))
}

/// POST `gen-msg-hash` with EIP-712 typed-data; returns `0x`-prefixed msgHash.
async fn tee_gen_msg_hash(
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
async fn tee_sign_eip712(
    chain_index: &str,
    payer_addr: &str,
    typed_data: &serde_json::Value,
) -> Result<Signature> {
    let msg_hash = tee_gen_msg_hash(chain_index, typed_data).await?;

    let access_token = ensure_tokens_refreshed().await?;
    let session = wallet_store::load_session()?.ok_or_else(|| {
        anyhow!(crate::commands::agentic_wallet::common::ERR_NOT_LOGGED_IN)
    })?;
    let session_key = keyring_store::get("session_key")
        .map_err(|_| anyhow!(crate::commands::agentic_wallet::common::ERR_NOT_LOGGED_IN))?;

    let mut signing_seed =
        crate::crypto::hpke_decrypt_session_sk(&session.encrypted_session_sk, &session_key)?;
    let mut signing_seed_b64 = B64.encode(signing_seed.as_slice());
    signing_seed.zeroize();
    let session_signature = crate::crypto::ed25519_sign_hex(&msg_hash, &signing_seed_b64)?;
    signing_seed_b64.zeroize();

    let mut client = WalletApiClient::new()?;

    let sign_body = json!({
        "chainIndex":  chain_index,
        "from":        payer_addr,
        "sessionCert": &session.session_cert,
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn signature_inputs_compile() {
        let exact_input = ExactPermit2Input {
            token: "0x0",
            amount: "0",
            spender: "0x0",
            nonce: "0",
            deadline: "0",
            witness_to: "0x0",
            witness_valid_after: "0",
            chain_id: 1,
        };
        let _ = std::mem::size_of_val(&exact_input);

        let upto_input = UptoPermit2Input {
            token: "0x0",
            amount: "0",
            spender: "0x0",
            nonce: "0",
            deadline: "0",
            witness_to: "0x0",
            witness_facilitator: "0x0",
            witness_valid_after: "0",
            chain_id: 1,
        };
        let _ = std::mem::size_of_val(&upto_input);
    }
}
