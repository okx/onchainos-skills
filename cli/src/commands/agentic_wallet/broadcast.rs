//! Shared post-unsignedInfo broadcast pipeline.
//!
//! Both wallet transfer / contract-call and agent-identity mutations funnel
//! through this helper. Given an already-fetched `UnsignedInfoResponse` and
//! the caller's signing state, it performs:
//!
//!   1. `executeResult` simulation check
//!   2. `msgForSign` assembly (signature / authSignatureFor7702 / unsignedTxHash+
//!      sessionSignature / unsignedTx / jitoUnsignedTx+jitoSessionSignature /
//!      sessionCert — each gated on non-empty)
//!   3. `extraData` assembly (`checkBalance`, `uopHash`, `encoding`, `signType`,
//!      `msgForSign`, optional `txType=2` for non-contract-call, optional
//!      `isMEV` / `skipWarning`, and a caller-provided overlay that can merge
//!      extra top-level keys such as `erc8004Msg`)
//!   4. `broadcast-transaction` POST + 81362 `CliConfirming` mapping
//!
//! Callers that need pre-broadcast logging should log before calling;
//! `cfg!(feature = "debug-log")` still gates the detailed extraData dump.

use anyhow::{bail, Context as _, Result};
use base64::Engine;
use serde_json::{json, Map, Value};

use crate::wallet_api::{UnsignedInfoResponse, WalletApiClient};
use crate::wallet_store::AddressInfo;

use super::common::handle_confirming_error;

pub(crate) struct BroadcastCtx<'a> {
    pub access_token: &'a str,
    pub account_id: &'a str,
    pub addr_info: &'a AddressInfo,
    pub session_cert: &'a str,
    pub signing_seed: &'a [u8; 32],
    pub unsigned: &'a UnsignedInfoResponse,
    pub is_contract_call: bool,
    pub mev_protection: bool,
    pub force: bool,
    pub extra_data_overlay: Option<Map<String, Value>>,
    pub trace_headers: Option<&'a [(&'a str, &'a str)]>,
}

pub(crate) async fn broadcast_unsigned(ctx: BroadcastCtx<'_>) -> Result<String> {
    let BroadcastCtx {
        access_token,
        account_id,
        addr_info,
        session_cert,
        signing_seed,
        unsigned,
        is_contract_call,
        mev_protection,
        force,
        extra_data_overlay,
        trace_headers,
    } = ctx;

    let exec_ok = match &unsigned.execute_result {
        Value::Bool(b) => *b,
        Value::Null => true,
        _ => true,
    };
    if !exec_ok {
        let err_msg = if unsigned.execute_error_msg.is_empty() {
            "transaction simulation failed".to_string()
        } else {
            unsigned.execute_error_msg.clone()
        };
        bail!("transaction simulation failed: {}", err_msg);
    }

    let signing_seed_b64 = base64::engine::general_purpose::STANDARD.encode(signing_seed);

    let mut msg_for_sign_map = Map::new();
    if !unsigned.hash.is_empty() {
        let sig = crate::crypto::ed25519_sign_eip191(&unsigned.hash, signing_seed, "hex")?;
        msg_for_sign_map.insert("signature".into(), json!(sig));
    }
    if !unsigned.auth_hash_for7702.is_empty() {
        let sig = crate::crypto::ed25519_sign_hex(&unsigned.auth_hash_for7702, &signing_seed_b64)?;
        msg_for_sign_map.insert("authSignatureFor7702".into(), json!(sig));
    }
    if !unsigned.unsigned_tx_hash.is_empty() {
        let sig = crate::crypto::ed25519_sign_encoded(
            &unsigned.unsigned_tx_hash,
            &signing_seed_b64,
            &unsigned.encoding,
        )?;
        msg_for_sign_map.insert("unsignedTxHash".into(), json!(&unsigned.unsigned_tx_hash));
        msg_for_sign_map.insert("sessionSignature".into(), json!(sig));
    }
    if !unsigned.unsigned_tx.is_empty() {
        msg_for_sign_map.insert("unsignedTx".into(), json!(&unsigned.unsigned_tx));
    }
    if !unsigned.jito_unsigned_tx.is_empty() {
        let jito_sig = crate::crypto::ed25519_sign_encoded(
            &unsigned.jito_unsigned_tx,
            &signing_seed_b64,
            &unsigned.encoding,
        )?;
        msg_for_sign_map.insert("jitoUnsignedTx".into(), json!(&unsigned.jito_unsigned_tx));
        msg_for_sign_map.insert("jitoSessionSignature".into(), json!(jito_sig));
    }
    if !session_cert.is_empty() {
        msg_for_sign_map.insert("sessionCert".into(), json!(session_cert));
    }
    let msg_for_sign = Value::Object(msg_for_sign_map);

    let mut extra_data_obj = if unsigned.extra_data.is_object() {
        unsigned.extra_data.clone()
    } else {
        json!({})
    };
    extra_data_obj["checkBalance"] = json!(true);
    extra_data_obj["uopHash"] = json!(unsigned.uop_hash);
    extra_data_obj["encoding"] = json!(unsigned.encoding);
    extra_data_obj["signType"] = json!(unsigned.sign_type);
    extra_data_obj["msgForSign"] = msg_for_sign;
    if !is_contract_call {
        extra_data_obj["txType"] = json!(2);
    }
    if mev_protection {
        extra_data_obj["isMEV"] = json!(true);
    }
    if force {
        extra_data_obj["skipWarning"] = json!(true);
    }
    if let Some(overlay) = extra_data_overlay {
        if let Value::Object(ref mut map) = extra_data_obj {
            for (k, v) in overlay {
                map.insert(k, v);
            }
        }
    }

    if cfg!(feature = "debug-log") {
        eprintln!(
            "[DEBUG][broadcast_unsigned] extraData={}",
            serde_json::to_string_pretty(&extra_data_obj).unwrap_or_default()
        );
    }
    let extra_data_str =
        serde_json::to_string(&extra_data_obj).context("failed to serialize extraData")?;

    let mut client = WalletApiClient::new()?;
    let broadcast_resp = client
        .broadcast_transaction(
            access_token,
            account_id,
            &addr_info.address,
            &addr_info.chain_index,
            &extra_data_str,
            trace_headers,
        )
        .await
        .map_err(|e| handle_confirming_error(e, force))?;

    if cfg!(feature = "debug-log") {
        eprintln!(
            "[DEBUG][broadcast_unsigned] === END SUCCESS: txHash={}",
            broadcast_resp.tx_hash
        );
    }

    Ok(broadcast_resp.tx_hash)
}
