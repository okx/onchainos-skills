//! Signing helpers for identity mutations and the thin broadcast wrapper
//! that delegates to `agentic_wallet::broadcast::broadcast_unsigned`. The
//! only identity-specific behaviour at broadcast time is the optional
//! `erc8004Msg` overlay; everything else (msgForSign / extraData / 81362
//! handling) is shared with wallet transfer.

use anyhow::{anyhow, bail, Result};
use base64::Engine;
use serde_json::{json, Map, Value};

use crate::commands::agentic_wallet::broadcast::{broadcast_unsigned, BroadcastCtx};
use crate::keyring_store;
use crate::wallet_api::UnsignedInfoResponse;
use crate::wallet_store::{self, AddressInfo, WalletsJson};

use super::models::{XLAYER_CHAIN_INDEX, XLAYER_CHAIN_NAME};

// ─── Wallet address resolution ────────────────────────────────────────────

pub(super) fn resolve_xlayer_signing_account(
    address: Option<&str>,
) -> Result<(String, AddressInfo)> {
    let wallets = wallet_store::load_wallets()?
        .ok_or_else(|| anyhow!("no XLayer address found in current account"))?;
    if let Some(address) = address.filter(|value| !value.trim().is_empty()) {
        for (account_id, entry) in &wallets.accounts_map {
            for addr in &entry.address_list {
                if is_xlayer_address(addr) && addr.address.eq_ignore_ascii_case(address.trim()) {
                    return Ok((account_id.clone(), addr.clone()));
                }
            }
        }
        bail!("no XLayer address found in current account");
    }

    resolve_current_xlayer_address(&wallets)
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
    eprintln!(
        "[agent-identity] sign_key_uuid: keyUuid={} keyUuid_utf8_bytes_hex={} signed_bytes_len={} signing_pubkey_hex={}",
        key_uuid,
        hex::encode(key_uuid.as_bytes()),
        key_uuid.len(),
        ed25519_pubkey_hex(signing_seed),
    );
    Ok(signature)
}

fn ed25519_pubkey_hex(signing_seed: &[u8; 32]) -> String {
    use ed25519_dalek::SigningKey;
    let sk = SigningKey::from_bytes(signing_seed);
    hex::encode(sk.verifying_key().to_bytes())
}

// ─── erc8004Msg overlay ───────────────────────────────────────────────────

/// 按产品规范 build 出 `erc8004Msg` 对象。空字段不写；整体若为空则返回 None
/// （让上层不要把 `erc8004Msg` 写进 extraData）。当前只有 `agent create` 会
/// 产出非空的 overlay；`update` / `feedback-submit` 都传 None。
pub(super) fn build_erc8004_overlay(
    communication_address: &str,
    role: &str,
    key_uuid: &str,
) -> Option<Map<String, Value>> {
    let mut inner = Map::new();
    if !communication_address.is_empty() {
        inner.insert(
            "communicationAddress".into(),
            json!(communication_address),
        );
    }
    if !role.is_empty() {
        inner.insert("role".into(), json!(role));
    }
    if !key_uuid.is_empty() {
        inner.insert("keyUuid".into(), json!(key_uuid));
    }
    if inner.is_empty() {
        return None;
    }
    let mut overlay = Map::new();
    overlay.insert("erc8004Msg".into(), Value::Object(inner));
    Some(overlay)
}

// ─── Broadcast ────────────────────────────────────────────────────────────

pub(super) async fn sign_and_broadcast_agent_transaction(
    access_token: &str,
    unsigned: &UnsignedInfoResponse,
    extra_data_overlay: Option<Map<String, Value>>,
    address_override: Option<&str>,
) -> Result<String> {
    let (account_id, addr_info) = resolve_xlayer_signing_account(address_override)?;
    let session = wallet_store::load_session()?
        .ok_or_else(|| anyhow!("session expired, please login again: onchainos wallet login"))?;
    let session_key = keyring_store::get("session_key")
        .map_err(|_| anyhow!("session expired, please login again: onchainos wallet login"))?;
    let signing_seed =
        crate::crypto::hpke_decrypt_session_sk(&session.encrypted_session_sk, &session_key)?;
    let session_cert = session.session_cert;

    broadcast_unsigned(BroadcastCtx {
        access_token,
        account_id: &account_id,
        addr_info: &addr_info,
        session_cert: &session_cert,
        signing_seed: &signing_seed,
        unsigned,
        is_contract_call: true,
        mev_protection: false,
        force: false,
        extra_data_overlay,
        trace_headers: None,
    })
    .await
}
