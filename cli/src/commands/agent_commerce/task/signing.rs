//! Task system signing utilities.
//!
//! Provides reusable sign-and-broadcast helpers for task CLI commands.
//! All on-chain write operations go through one of these flows:
//!
//! - [`sign_uop_and_broadcast`] — sign uopData + broadcast (caller已拿到 uopData)
//! - [`task_dual_sign_and_broadcast`] — dual-sign for accept/complete/refuse
//!   (pre-endpoint → sign digest → main endpoint → sign uopHash → broadcast)

use anyhow::{bail, Result};
use base64::engine::{general_purpose::STANDARD as BASE64_STANDARD, Engine as _};
use serde_json::Value;

use crate::commands::agentic_wallet::transfer::{build_broadcast_body, resolve_address};
use crate::commands::agent_commerce::task::common::network::task_api_client::TaskApiClient;
use crate::commands::agent_commerce::task::common::{XLAYER_CHAIN_INDEX, XLAYER_CHAIN_NAME};
use crate::wallet_api::UnsignedInfoResponse;

/// Return value from sign-and-broadcast helpers.
pub struct BroadcastResult {
    /// The full API response from the task endpoint (before broadcast).
    pub api_response: Value,
    /// Transaction hash returned by the broadcast endpoint.
    pub tx_hash: String,
}

/// Business context for broadcast — 后端据此区分业务场景做额外校验/记账。
///
/// 枚举值对齐后端接口文档 bizType 定义（bizType=6 不存在，已跳过）。
#[repr(i32)]
#[derive(Debug, Clone, Copy)]
pub enum BizContext {
    JobCreate         = 1,
    DisputeCreate     = 2,
    VoteCommit        = 3,
    VoteReveal        = 4,
    ClaimRewards      = 5,
    // 6 is skipped in backend spec
    JobAccept         = 7,
    JobSubmit         = 8,
    JobComplete       = 9,
    JobRefuse         = 10,
    Stake             = 11,
    UnstakeRequest    = 12,
    UnstakeClaim      = 13,
    UnstakeCancel     = 14,
    JobApply          = 15,
    JobClose          = 16,
    JobSetVisibility  = 17,
    JobSetPaymentMode = 18,
    StakeIncrease     = 19,
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

/// Query task detail to resolve the buyer's wallet **and** agentId for signing.
///
/// Returns `(account_id, address, buyer_agent_id)`.
pub async fn resolve_wallet_and_agent_for_task(
    client: &mut TaskApiClient,
    job_id: &str,
) -> Result<(String, String, String)> {
    let url = format!("{}/priapi/v1/aieco/task/{job_id}", client.base_url());
    let resp = client.get(&url).await?;

    let task = &resp["task"];
    let buyer_address = task["buyerAgentAddress"]
        .as_str()
        .ok_or_else(|| anyhow::anyhow!("任务详情缺少 buyerAgentAddress 字段"))?;

    let buyer_agent_id = task["buyerAgentId"]
        .as_str()
        .unwrap_or("")
        .to_string();

    let (account_id, address) = resolve_wallet(None, Some(buyer_address))?;
    Ok((account_id, address, buyer_agent_id))
}

/// Query task detail to resolve the provider's wallet and agentId for signing.
///
/// Returns `(account_id, address, provider_agent_id)`.
pub async fn resolve_wallet_and_agent_for_provider(
    client: &mut TaskApiClient,
    job_id: &str,
) -> Result<(String, String, String)> {
    let url = format!("{}/priapi/v1/aieco/task/{job_id}", client.base_url());
    let resp = client.get(&url).await?;

    let task = &resp["task"];
    let provider_agent_id = task["providerAgentId"]
        .as_str()
        .unwrap_or("")
        .to_string();

    let (account_id, address) = resolve_wallet(None, None)?;
    Ok((account_id, address, provider_agent_id))
}

/// 签名 uopData + 广播上链（纯签名广播，不含 API 请求）
///
/// 接收后端返回的 `uopData`，签名后通过 `TaskApiClient` 广播到链上，返回 txHash。
/// API 请求由调用方通过 `TaskApiClient` 完成。
///
/// `biz_context` 标记业务场景（TaskAccept / DisputeCreate 等），随广播请求发送供后端区分。
pub async fn sign_uop_and_broadcast(
    client: &mut TaskApiClient,
    uop_data: &Value,
    account_id: &str,
    address: &str,
    job_id: &str,
    biz_context: BizContext,
) -> Result<String> {
    if uop_data.is_null() {
        bail!("后端未返回 uopData，无法签名上链");
    }

    let unsigned: UnsignedInfoResponse = serde_json::from_value(uop_data.clone())
        .map_err(|e| anyhow::anyhow!("解析 uopData 失败: {e}"))?;

    let mut broadcast_body = build_broadcast_body(
        &unsigned,
        account_id,
        address,
        XLAYER_CHAIN_INDEX,
        true,
        false,
        false,
    )
    .await?;
    broadcast_body["bizContext"] = serde_json::json!({
        "jobId": job_id,
        "bizType": biz_context as i32,
    });

    let bc_resp = client.post(&client.broadcast_url(), &broadcast_body).await
        .map_err(|e| anyhow::anyhow!("广播失败: {e}"))?;

    Ok(bc_resp[0]["txHash"]
        .as_str()
        .unwrap_or("pending")
        .to_string())
}

/// Dual-sign flow for accept/complete/refuse.
///
/// 1. POST `pre_endpoint_url` with `pre_body` + identity headers → get digest
/// 2. Sign digest with session key → signature
/// 3. POST `main_endpoint_url` with body built by `main_body_builder(signature)` + identity headers → uopData
/// 4. Sign uopHash + broadcast → tx_hash
#[allow(clippy::too_many_arguments)]
pub async fn task_dual_sign_and_broadcast(
    client: &mut TaskApiClient,
    pre_endpoint_url: &str,
    pre_body: &Value,
    main_endpoint_url: &str,
    main_body_builder: impl FnOnce(&str) -> Value,
    account_id: &str,
    address: &str,
    agent_id: &str,
    job_id: &str,
    biz_context: BizContext,
) -> Result<BroadcastResult> {
    // Step 1: POST pre-endpoint with identity headers → digest
    let pre_resp = client.post_with_identity(pre_endpoint_url, pre_body, agent_id, address).await
        .map_err(|e| anyhow::anyhow!("pre-sign 请求失败: {e}"))?;

    let digest = pre_resp["digest"]
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

    // Step 3: POST main endpoint with signature → uopData
    let main_body = main_body_builder(&signature);
    let main_resp = client.post_with_identity(main_endpoint_url, &main_body, agent_id, address).await
        .map_err(|e| anyhow::anyhow!("main 请求失败: {e}"))?;

    // Step 4: Sign uopHash + broadcast
    let tx_hash = sign_uop_and_broadcast(
        client, &main_resp["uopData"], account_id, address, job_id, biz_context,
    ).await?;

    Ok(BroadcastResult { api_response: main_resp, tx_hash })
}
