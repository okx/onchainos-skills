//! Task system signing utilities.
//!
//! Provides reusable sign-and-broadcast helpers for task CLI commands.
//! All on-chain write operations go through one of these flows:
//!
//! - [`task_sign_and_broadcast`] — standard single-sign (close, setVisibility, apply, claim, etc.)
//! - [`task_dual_sign_and_broadcast`] — dual-sign for accept/complete/refuse
//!   (pre-endpoint → sign digest → main endpoint → sign uopHash → broadcast)

use anyhow::{bail, Result};
use base64::engine::{general_purpose::STANDARD as BASE64_STANDARD, Engine as _};
use serde_json::Value;

use crate::commands::agentic_wallet::transfer::{build_broadcast_body, resolve_address};
use crate::commands::agent_commerce::task::common::{XLAYER_CHAIN_INDEX, XLAYER_CHAIN_NAME};
use crate::wallet_api::UnsignedInfoResponse;

/// Return value from sign-and-broadcast helpers.
pub struct BroadcastResult {
    /// The full API response from the task endpoint (before broadcast).
    pub api_response: Value,
    /// Transaction hash returned by the broadcast endpoint.
    pub tx_hash: String,
}

/// Resolve wallet account_id and address for XLayer.
///
/// - `account_id`: 指定账户 ID。传 `None` 使用当前默认钱包。
/// - `address`: 指定地址。传 `None` 使用该账户的默认 XLayer 地址。
///
/// Returns (account_id, address).
pub fn resolve_wallet(
    account_id: Option<&str>,
    address: Option<&str>,
) -> Result<(String, String)> {
    let wallets = crate::wallet_store::load_wallets()?
        .ok_or_else(|| anyhow::anyhow!("未登录，请先执行 onchainos wallet auth"))?;

    let acct_id = account_id
        .map(|s| s.to_string())
        .unwrap_or_else(|| wallets.selected_account_id.clone());

    let (_, addr_info) = resolve_address(&wallets, address, XLAYER_CHAIN_NAME)?;
    Ok((acct_id, addr_info.address))
}

/// Query task detail to resolve the buyer's wallet for signing.
///
/// Fetches `GET /priapi/v1/aieco/task/{jobId}`, extracts `buyerAgentAddress`,
/// then calls [`resolve_wallet`] with that address so the correct wallet is used
/// even when the current default wallet is different from the task creator.
pub async fn resolve_wallet_for_task(
    http: &reqwest::Client,
    api_base: &str,
    job_id: &str,
) -> Result<(String, String)> {
    let url = format!("{api_base}/priapi/v1/aieco/task/{job_id}");
    let resp: Value = http
        .get(&url)
        .send()
        .await
        .map_err(|e| anyhow::anyhow!("无法查询任务详情: {e}"))?
        .json()
        .await?;

    if resp["code"] != 0 {
        bail!(
            "查询任务失败: {}",
            resp["msg"].as_str().unwrap_or("unknown error")
        );
    }

    let task = &resp["data"]["task"];
    let buyer_address = task["buyerAgentAddress"]
        .as_str()
        .ok_or_else(|| anyhow::anyhow!("任务详情缺少 buyerAgentAddress 字段"))?;

    resolve_wallet(None, Some(buyer_address))
}

/// Standard single-sign flow for task write operations.
///
/// 1. POST `endpoint_url` with `request_body` → response containing `uopData`
/// 2. Sign uopHash via `build_broadcast_body`
/// 3. POST `broadcast_url` → tx_hash
///
/// Used by: close, setVisibility, apply, claim, direct/accept, etc.
pub async fn task_sign_and_broadcast(
    http: &reqwest::Client,
    endpoint_url: &str,
    request_body: &Value,
    broadcast_url: &str,
    account_id: &str,
    address: &str,
) -> Result<BroadcastResult> {
    // Step 1: Call task backend → get uopData
    let resp: Value = http
        .post(endpoint_url)
        .json(request_body)
        .send()
        .await
        .map_err(|e| anyhow::anyhow!("无法连接后端: {e}"))?
        .json()
        .await?;

    if resp["code"] != 0 {
        bail!(
            "后端返回错误: {}",
            resp["msg"].as_str().unwrap_or("unknown error")
        );
    }

    let uop_data = &resp["data"]["uopData"];
    if uop_data.is_null() {
        bail!("后端未返回 uopData，无法签名上链");
    }

    // Step 2: Sign uopHash
    let unsigned: UnsignedInfoResponse = serde_json::from_value(uop_data.clone())
        .map_err(|e| anyhow::anyhow!("解析 uopData 失败: {e}"))?;

    let broadcast_body = build_broadcast_body(
        &unsigned,
        account_id,
        address,
        XLAYER_CHAIN_INDEX,
        true,  // is_contract_call
        false, // mev_protection
        false, // force
    )
    .await?;

    // Step 3: Broadcast
    let bc_resp: Value = http
        .post(broadcast_url)
        .json(&broadcast_body)
        .send()
        .await
        .map_err(|e| anyhow::anyhow!("广播失败: {e}"))?
        .json()
        .await?;

    if bc_resp["code"] != 0 {
        bail!(
            "广播失败: {}",
            bc_resp["msg"].as_str().unwrap_or("unknown error")
        );
    }

    let tx_hash = bc_resp["data"][0]["txHash"]
        .as_str()
        .unwrap_or("pending")
        .to_string();

    Ok(BroadcastResult {
        api_response: resp,
        tx_hash,
    })
}

/// Same as [`task_sign_and_broadcast`] but adds `X-Agent-Id` and `X-Wallet-Address` headers.
#[allow(clippy::too_many_arguments)]
pub async fn task_sign_and_broadcast_with_headers(
    http: &reqwest::Client,
    endpoint_url: &str,
    request_body: &Value,
    broadcast_url: &str,
    account_id: &str,
    address: &str,
    agent_id: &str,
) -> Result<BroadcastResult> {
    let resp: Value = http
        .post(endpoint_url)
        .header("X-Agent-Id", agent_id)
        .header("X-Wallet-Address", address)
        .json(request_body)
        .send()
        .await
        .map_err(|e| anyhow::anyhow!("无法连接后端: {e}"))?
        .json()
        .await?;

    if resp["code"] != 0 {
        bail!(
            "后端返回错误: {}",
            resp["msg"].as_str().unwrap_or("unknown error")
        );
    }

    let uop_data = &resp["data"]["uopData"];
    if uop_data.is_null() {
        bail!("后端未返回 uopData，无法签名上链");
    }

    let unsigned: UnsignedInfoResponse = serde_json::from_value(uop_data.clone())
        .map_err(|e| anyhow::anyhow!("解析 uopData 失败: {e}"))?;

    let broadcast_body = build_broadcast_body(
        &unsigned,
        account_id,
        address,
        XLAYER_CHAIN_INDEX,
        true,
        false,
        false,
    )
    .await?;

    let bc_resp: Value = http
        .post(broadcast_url)
        .json(&broadcast_body)
        .send()
        .await
        .map_err(|e| anyhow::anyhow!("广播失败: {e}"))?
        .json()
        .await?;

    if bc_resp["code"] != 0 {
        bail!(
            "广播失败: {}",
            bc_resp["msg"].as_str().unwrap_or("unknown error")
        );
    }

    let tx_hash = bc_resp["data"][0]["txHash"]
        .as_str()
        .unwrap_or("pending")
        .to_string();

    Ok(BroadcastResult {
        api_response: resp,
        tx_hash,
    })
}

/// Dual-sign flow for accept/complete/refuse.
///
/// 1. POST `pre_endpoint_url` with `pre_body` → get digest
/// 2. Sign digest with session key → signature
/// 3. POST `main_endpoint_url` with body built by `main_body_builder(signature)` → uopData
/// 4. Sign uopHash → broadcast
///
/// 【待确认】pre-endpoint 返回的 digest 即为 main endpoint 的 signature 入参。
/// 当后端确认字段名后更新 `digest_field` 参数默认值。
#[allow(clippy::too_many_arguments)]
pub async fn task_dual_sign_and_broadcast(
    http: &reqwest::Client,
    pre_endpoint_url: &str,
    pre_body: &Value,
    main_endpoint_url: &str,
    main_body_builder: impl FnOnce(&str) -> Value,
    broadcast_url: &str,
    account_id: &str,
    address: &str,
) -> Result<BroadcastResult> {
    // Step 1: Call pre-endpoint → get digest
    let pre_resp: Value = http
        .post(pre_endpoint_url)
        .json(pre_body)
        .send()
        .await
        .map_err(|e| anyhow::anyhow!("无法连接后端 (pre-sign): {e}"))?
        .json()
        .await?;

    if pre_resp["code"] != 0 {
        bail!(
            "pre-sign 请求失败: {}",
            pre_resp["msg"].as_str().unwrap_or("unknown error")
        );
    }

    // 【待确认】digest 字段名，当前假设为 "digest"
    let digest = pre_resp["data"]["digest"]
        .as_str()
        .ok_or_else(|| anyhow::anyhow!("pre-sign 未返回 digest 字段"))?;

    // Step 2: Sign digest with session key
    let session = crate::wallet_store::load_session()?
        .ok_or_else(|| anyhow::anyhow!("未登录，请先执行 onchainos wallet auth"))?;
    let session_key = crate::keyring_store::get("session_key")
        .map_err(|_| anyhow::anyhow!("未登录，请先执行 onchainos wallet auth"))?;

    let signing_seed =
        crate::crypto::hpke_decrypt_session_sk(&session.encrypted_session_sk, &session_key)?;
    let signing_seed_b64 = BASE64_STANDARD.encode(signing_seed);
    let signature = crate::crypto::ed25519_sign_hex(digest, &signing_seed_b64)?;

    // Step 3: Call main endpoint with signature
    let main_body = main_body_builder(&signature);

    // Reuse single-sign flow for the rest (main endpoint → sign uopHash → broadcast)
    task_sign_and_broadcast(
        http,
        main_endpoint_url,
        &main_body,
        broadcast_url,
        account_id,
        address,
    )
    .await
}
