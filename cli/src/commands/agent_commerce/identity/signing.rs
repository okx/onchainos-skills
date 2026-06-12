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

/// In-memory bundle of every material a single agent identity broadcast needs:
/// XLayer signing account, session cert, and decrypted signing seed. Built
/// once at the top of each `*_impl`, threaded down by reference into broadcast.
/// Must never be serialized, logged, or persisted — `signing_seed` is the
/// raw secret. Drops with the calling stack frame.
pub(super) struct AgentSigningSession {
    pub account_id: String,
    pub addr_info: AddressInfo,
    pub session_cert: String,
    pub signing_seed: [u8; 32],
}

pub(super) fn load_agent_signing_session(
    address: Option<&str>,
) -> Result<AgentSigningSession> {
    let (account_id, addr_info) = resolve_xlayer_signing_account(address)?;
    let session = wallet_store::load_session()?
        .ok_or_else(|| anyhow!("session expired, please login again: onchainos wallet login"))?;
    let session_key = keyring_store::get("session_key")
        .map_err(|_| anyhow!("session expired, please login again: onchainos wallet login"))?;
    let signing_seed =
        crate::crypto::hpke_decrypt_session_sk(&session.encrypted_session_sk, &session_key)?;
    Ok(AgentSigningSession {
        account_id,
        addr_info,
        session_cert: session.session_cert,
        signing_seed,
    })
}

/// Sign keyUuid with signing_seed using Ed25519, producing the sessionSignature.
///
/// Per spec: no EIP-191 prefix, no Keccak-256 pre-hash — the raw UTF-8 bytes of
/// keyUuid are fed directly into Ed25519. Backend verification is equivalent to:
///   VerifyKey(pubkey).verify(keyUuid.encode("utf-8"), base64_decode(sig))
///
/// Note: `crypto::ed25519_sign_eip191` is the protocol path used by agentic
/// wallet (transfer.rs) for EVM tx-hash signing; it is intentionally not reused
/// here to avoid conflating identity signing semantics.
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

/// Build the `erc8004Msg` overlay per spec. Empty fields are omitted; if the
/// result is entirely empty, returns None so the caller omits `erc8004Msg` from
/// extraData altogether.
///
/// Fields used by each command (empty = not written):
/// - `agent create`          → communicationAddress / role / keyUuid
/// - `agent update`          → none (always pass `&[]`)
/// - `agent feedback-submit` → taskId / feedBackAgentId
pub(super) fn build_erc8004_overlay(fields: &[(&str, &str)]) -> Option<Map<String, Value>> {
    let mut inner = Map::new();
    for (key, value) in fields {
        if !value.is_empty() {
            inner.insert((*key).into(), json!(value));
        }
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
    session: &AgentSigningSession,
) -> Result<String> {
    broadcast_unsigned(BroadcastCtx {
        access_token,
        account_id: &session.account_id,
        addr_info: &session.addr_info,
        session_cert: &session.session_cert,
        signing_seed: &session.signing_seed,
        unsigned,
        is_contract_call: true,
        mev_protection: false,
        force: false,
        extra_data_overlay,
        trace_headers: None,
    })
    .await
}
