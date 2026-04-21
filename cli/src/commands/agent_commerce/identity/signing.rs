//! Signing, broadcasting, and XLayer wallet address resolution for identity
//! mutations. Owns `Erc8004Payload` (broadcast-only strategy) and the
//! single `sign_and_broadcast_agent_transaction` entry point that every
//! write command funnels through.

use anyhow::{anyhow, bail, Context as _, Result};
use base64::Engine;
use serde_json::{json, Map, Value};

use crate::commands::agentic_wallet::auth::format_api_error;
use crate::commands::Context;
use crate::keyring_store;
use crate::wallet_store::{self, AddressInfo, WalletsJson};

use super::models::{AgentUnsignedTx, XLAYER_CHAIN_INDEX, XLAYER_CHAIN_NAME};
use super::utils::{
    reconstruct_post_url_for_log, redact_token_for_debug, wallet_client,
};

// ─── erc8004Msg payload ───────────────────────────────────────────────────

/// 广播阶段 erc8004Msg 子对象的内容。按产品规范：
/// - create：首次注册，4 个子字段全部携带（建立 agent 身份）
/// - update / feedback-submit：不允许修改 communicationAddress / role / keyUuid /
///   sessionSignature 任何一个，所以 erc8004Msg 是空对象 `{}`，避免后端误改 K1
///   或 agent 通信地址等持久身份。
pub(super) enum Erc8004Payload {
    Create {
        communication_address: String,
        role: String,
        key_uuid: String,
        session_signature: String,
    },
    Empty,
}

// ─── Wallet address resolution ────────────────────────────────────────────

pub(super) fn resolve_xlayer_signing_account(address: Option<&str>) -> Result<(String, String)> {
    let wallets = wallet_store::load_wallets()?
        .ok_or_else(|| anyhow!("no XLayer address found in current account"))?;
    if let Some(address) = address.filter(|value| !value.trim().is_empty()) {
        for (account_id, entry) in &wallets.accounts_map {
            for addr in &entry.address_list {
                if is_xlayer_address(addr) && addr.address.eq_ignore_ascii_case(address.trim()) {
                    return Ok((account_id.clone(), addr.address.clone()));
                }
            }
        }
        bail!("no XLayer address found in current account");
    }

    let (account_id, addr_info) = resolve_current_xlayer_address(&wallets)?;
    Ok((account_id, addr_info.address))
}

fn resolve_current_xlayer_address(wallets: &WalletsJson) -> Result<(String, AddressInfo)> {
    let account_id = wallets.selected_account_id.trim();
    if account_id.is_empty() {
        bail!("no XLayer address found in current account");
    }
    let entry = wallets
        .accounts_map
        .get(account_id)
        .ok_or_else(|| anyhow!("no XLayer address found in current account"))?;
    let addr = entry
        .address_list
        .iter()
        .find(|addr| is_xlayer_address(addr))
        .cloned()
        .ok_or_else(|| anyhow!("no XLayer address found in current account"))?;
    Ok((account_id.to_string(), addr))
}

fn is_xlayer_address(addr: &AddressInfo) -> bool {
    addr.chain_index == XLAYER_CHAIN_INDEX
        || addr.chain_name.eq_ignore_ascii_case(XLAYER_CHAIN_NAME)
}

// ─── Signing seed + session cert loading ──────────────────────────────────

pub(super) fn load_signing_seed() -> Result<[u8; 32]> {
    let session = wallet_store::load_session()?
        .ok_or_else(|| anyhow!("session expired, please login again: onchainos wallet login"))?;
    let session_key = keyring_store::get("session_key")
        .map_err(|_| anyhow!("session expired, please login again: onchainos wallet login"))?;
    crate::crypto::hpke_decrypt_session_sk(&session.encrypted_session_sk, &session_key)
}

pub(super) fn load_session_cert() -> Result<String> {
    let session = wallet_store::load_session()?
        .ok_or_else(|| anyhow!("session expired, please login again: onchainos wallet login"))?;
    Ok(session.session_cert)
}

/// 用 signing_seed 对 keyUuid 做 Ed25519 签名，作为 sessionSignature。
///
/// 按产品规范：不套 EIP-191 前缀，不做 Keccak-256 预哈希——直接把 keyUuid 的
/// UTF-8 字节喂给 Ed25519 签名算法。后端验签等价于：
///   VerifyKey(pubkey).verify(keyUuid.encode("utf-8"), base64_decode(sig))
///
/// 注意：`crypto::ed25519_sign_eip191` 是 agentic wallet（transfer.rs）签
/// EVM tx hash 用的协议路径，这里不复用，避免和 identity 的签名语义混淆。
pub(super) fn sign_key_uuid(key_uuid: &str, signing_seed: &[u8; 32]) -> Result<String> {
    let sig_bytes = crate::crypto::ed25519_sign(signing_seed, key_uuid.as_bytes())?;
    let signature = base64::engine::general_purpose::STANDARD.encode(&sig_bytes);
    if cfg!(feature = "debug-log") {
        eprintln!(
            "[DEBUG][agent-identity] sign_key_uuid: keyUuid={} keyUuid_utf8_bytes_hex={} signed_bytes_len={} signing_pubkey_hex={}",
            key_uuid,
            hex::encode(key_uuid.as_bytes()),
            key_uuid.as_bytes().len(),
            ed25519_pubkey_hex(signing_seed),
        );
    }
    Ok(signature)
}

fn ed25519_pubkey_hex(signing_seed: &[u8; 32]) -> String {
    use ed25519_dalek::SigningKey;
    let sk = SigningKey::from_bytes(signing_seed);
    hex::encode(sk.verifying_key().to_bytes())
}

// ─── Broadcast ────────────────────────────────────────────────────────────

pub(super) async fn sign_and_broadcast_agent_transaction(
    ctx: &Context,
    access_token: &str,
    unsigned: &AgentUnsignedTx,
    erc8004: &Erc8004Payload,
    address_override: Option<&str>,
) -> Result<String> {
    let client = wallet_client(ctx)?;
    let (account_id, from_addr) = resolve_xlayer_signing_account(address_override)?;
    let session = wallet_store::load_session()?
        .ok_or_else(|| anyhow!("session expired, please login again: onchainos wallet login"))?;
    let session_key = keyring_store::get("session_key")
        .map_err(|_| anyhow!("session expired, please login again: onchainos wallet login"))?;
    let signing_seed =
        crate::crypto::hpke_decrypt_session_sk(&session.encrypted_session_sk, &session_key)?;
    let signing_seed_b64 = base64::engine::general_purpose::STANDARD.encode(signing_seed);
    let session_cert = session.session_cert;
    if session_cert.is_empty() {
        bail!("session cert missing, please login again: onchainos wallet login");
    }
    if unsigned.hash.is_empty()
        && unsigned.auth_hash_for7702.is_empty()
        && unsigned.unsigned_tx_hash.is_empty()
    {
        bail!("pre-transaction response missing signable hashes");
    }

    // msgForSign follows transfer.rs's conditional-insert rules: each hash field
    // is only signed+populated when present, sessionCert is always included.
    let mut msg_for_sign = Map::new();
    if !unsigned.hash.is_empty() {
        msg_for_sign.insert(
            "signature".to_string(),
            json!(crate::crypto::ed25519_sign_eip191(
                &unsigned.hash,
                &signing_seed,
                "hex"
            )?),
        );
    }
    if !unsigned.auth_hash_for7702.is_empty() {
        msg_for_sign.insert(
            "authSignatureFor7702".to_string(),
            json!(crate::crypto::ed25519_sign_hex(
                &unsigned.auth_hash_for7702,
                &signing_seed_b64
            )?),
        );
    }
    if !unsigned.unsigned_tx_hash.is_empty() {
        msg_for_sign.insert(
            "unsignedTxHash".to_string(),
            json!(unsigned.unsigned_tx_hash),
        );
        msg_for_sign.insert(
            "sessionSignature".to_string(),
            json!(crate::crypto::ed25519_sign_encoded(
                &unsigned.unsigned_tx_hash,
                &signing_seed_b64,
                &unsigned.encoding,
            )?),
        );
    }
    if !unsigned.unsigned_tx.is_empty() {
        msg_for_sign.insert("unsignedTx".to_string(), json!(unsigned.unsigned_tx));
    }
    msg_for_sign.insert("sessionCert".to_string(), json!(session_cert));

    let mut extra_data = if unsigned.extra_data.is_object() {
        unsigned.extra_data.clone()
    } else {
        json!({})
    };
    extra_data["txType"] = json!(3);
    extra_data["syncWaitOnChain"] = json!(true);
    extra_data["checkBalance"] = json!(true);
    extra_data["uopHash"] = json!(unsigned.uop_hash);
    if !unsigned.encoding.is_empty() {
        extra_data["encoding"] = json!(unsigned.encoding);
    }
    if !unsigned.sign_type.is_empty() {
        extra_data["signType"] = json!(unsigned.sign_type);
    }
    extra_data["msgForSign"] = Value::Object(msg_for_sign);
    // erc8004Msg：按 Erc8004Payload 决定塞什么。
    // - Create：4 个子字段全部携带（首次注册 agent 身份）
    // - Empty：空 `{}`，update / feedback-submit 都用这个，避免覆盖 agent 持久身份
    extra_data["erc8004Msg"] = match erc8004 {
        Erc8004Payload::Create {
            communication_address,
            role,
            key_uuid,
            session_signature,
        } => json!({
            "communicationAddress": communication_address,
            "role": role,
            "keyUuid": key_uuid,
            "sessionSignature": session_signature,
        }),
        Erc8004Payload::Empty => json!({}),
    };

    let extra_data_str =
        serde_json::to_string(&extra_data).context("failed to serialize broadcast extraData")?;

    if cfg!(feature = "debug-log") {
        eprintln!(
            "[DEBUG][agent-identity] broadcast request prepared: \
             url={} access_token_len={} access_token_prefix={} \
             accountId={} address={} chainIndex={} extraData={}",
            reconstruct_post_url_for_log(
                ctx,
                "/priapi/v5/wallet/agentic/pre-transaction/broadcast-transaction",
            ),
            access_token.len(),
            redact_token_for_debug(access_token),
            account_id,
            from_addr,
            XLAYER_CHAIN_INDEX,
            extra_data_str,
        );
    }

    let resp_result = client
        .broadcast_transaction(
            access_token,
            &account_id,
            &from_addr,
            XLAYER_CHAIN_INDEX,
            &extra_data_str,
            None,
        )
        .await;

    if cfg!(feature = "debug-log") {
        match &resp_result {
            Ok(r) => eprintln!(
                "[DEBUG][agent-identity] broadcast response ok: txHash={} pkgId={} orderId={} orderType={}",
                r.tx_hash, r.pkg_id, r.order_id, r.order_type
            ),
            Err(e) => eprintln!("[DEBUG][agent-identity] broadcast response err: {:#}", e),
        }
    }

    let resp = resp_result.map_err(format_api_error)?;
    if resp.tx_hash.is_empty() {
        bail!("broadcast response missing txHash");
    }
    Ok(resp.tx_hash)
}
