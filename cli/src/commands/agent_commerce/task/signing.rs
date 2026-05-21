//! Task system signing utilities.
//!
//! Provides reusable sign-and-broadcast helpers for task CLI commands.
//! All on-chain write operations go through one of these flows:
//!
//! - [`sign_uop_and_broadcast`] — sign uopData + broadcast (caller already has uopData)
//! - [`task_dual_sign_and_broadcast`] — dual-sign for complete/refuse
//!   (pre-endpoint → sign typedData → main endpoint → sign uopHash → broadcast)

use anyhow::{bail, Result};
use base64::engine::{general_purpose::STANDARD as BASE64_STANDARD, Engine as _};
use serde_json::Value;
use std::time::Duration;

use crate::audit;
use crate::commands::agentic_wallet::transfer::{build_broadcast_body, resolve_address};
use crate::commands::agent_commerce::task::common::network::task_api_client::TaskApiClient;
use crate::commands::agent_commerce::task::common::{
    fetch_my_agent_by_id, fetch_my_agents, AGENT_ROLE_BUYER, AGENT_ROLE_PROVIDER,
    XLAYER_CHAIN_INDEX, XLAYER_CHAIN_NAME,
};
use crate::wallet_api::UnsignedInfoResponse;

/// Return value from sign-and-broadcast helpers.
pub struct BroadcastResult {
    /// The full API response from the task endpoint (before broadcast).
    pub api_response: Value,
    /// Transaction hash returned by the broadcast endpoint.
    pub tx_hash: String,
}

/// Extract bizType (numeric) from the previous step's API response and pass through to the broadcast endpoint as-is.
pub fn extract_biz_type(resp: &Value) -> i64 {
    resp["type"].as_i64().unwrap_or(0)
}

/// Resolve wallet account_id and address for XLayer.
///
/// - `account_id`: specified account ID. Pass `None` to use the current default wallet.
/// - `address`: specified address. Pass `None` to use the account's default XLayer address.
///
/// Returns (account_id, address).
pub fn resolve_wallet(
    account_id: Option<&str>,
    address: Option<&str>,
) -> Result<(String, String)> {
    let wallets = crate::wallet_store::load_wallets()?
        .ok_or_else(|| anyhow::anyhow!("未登录，请先执行 onchainos wallet auth"))?;

    let (resolved_acct, addr_info) = resolve_address(&wallets, address, XLAYER_CHAIN_NAME)?;

    let acct_id = account_id
        .map(|s| s.to_string())
        .unwrap_or(resolved_acct);

    Ok((acct_id, addr_info.address))
}

/// Query task detail to resolve the buyer's wallet **and** agentId for signing.
///
/// If `explicit_agent_id` is provided (from `--agent-id` CLI flag), it is used
/// directly for the GET header instead of auto-detecting via subprocess.
///
/// Returns `(account_id, address, buyer_agent_id)`.
pub async fn resolve_wallet_and_agent_for_task(
    client: &mut TaskApiClient,
    job_id: &str,
    explicit_agent_id: Option<&str>,
) -> Result<(String, String, String)> {
    let local_agent_id = if let Some(id) = explicit_agent_id {
        id.to_string()
    } else {
        resolve_agent_by_role(AGENT_ROLE_BUYER, "buyer（买家）", None)
            .await
            .map(|(id, _)| id)
            .unwrap_or_default()
    };

    let resp = client.get_with_identity(&client.task_path(job_id), &local_agent_id).await?;

    let buyer_address = resp["buyerAgentAddress"]
        .as_str()
        .ok_or_else(|| anyhow::anyhow!("任务详情缺少 buyerAgentAddress 字段"))?;

    let buyer_agent_id = resp["buyerAgentId"]
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
    // First fetch provider agentId from local identity list, used for GET request with agenticId header
    let local_agent_id = resolve_agent_by_role(AGENT_ROLE_PROVIDER, "provider（卖家）", None)
        .await
        .map(|(id, _)| id)
        .unwrap_or_default();

    let resp = client.get_with_identity(&client.task_path(job_id), &local_agent_id).await?;

    let provider_agent_id = resp["providerAgentId"]
        .as_str()
        .unwrap_or("")
        .to_string();

    let (account_id, address) = resolve_wallet(None, None)?;
    Ok((account_id, address, provider_agent_id))
}

/// Fetch the current active account's agent list — thin wrapper over
/// `fetch_my_agents()`. Kept as a function for clarity at call sites; the
/// shape-handling + ownerAddress filter lives in `common/mod.rs`.
async fn query_agent_list() -> Vec<Value> {
    fetch_my_agents().await
}

/// Filter the `onchainos agent get` list by `role`, optionally constrained by `ownerAddress`.
///
/// - `wallet_address`: pass `Some(addr)` to only match identities with matching `ownerAddress` (case-insensitive);
///   pass `None` to take the first matching role (for read-only scenarios that only need the agentId header).
///
/// Returns `(agent_id, owner_address)`.
async fn resolve_agent_by_role(
    role_code: i64,
    role_label: &str,
    wallet_address: Option<&str>,
) -> Result<(String, String)> {
    let list = query_agent_list().await;

    for agent in &list {
        if agent["role"].as_i64() != Some(role_code) {
            continue;
        }
        let owner = agent["ownerAddress"].as_str().unwrap_or("");
        if let Some(want) = wallet_address {
            if !owner.eq_ignore_ascii_case(want) {
                continue;
            }
        }
        let agent_id = agent["agentId"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("Agent 缺少 agentId 字段"))?
            .to_string();
        return Ok((agent_id, owner.to_string()));
    }

    if wallet_address.is_some() {
        bail!(
            "当前钱包没有 {role_label} 身份（ownerAddress 不匹配），请切换钱包或先注册"
        )
    } else {
        bail!("当前账户没有 {role_label} 身份，请先注册")
    }
}

/// Resolve wallet + evaluator agentId for signing.
///
/// Call `fetch_my_agent_by_id` by `agent_id` (does a full `agent get` pull, then client-side
/// filters by `agentId`) to get `agentWalletAddress` → find the corresponding account in wallet store.
/// `agent_id` is required (from system message envelope's top-level `agentId`); it is the only
/// correct path in multi-identity scenarios — the "default wallet reverse lookup" fallback is disabled to prevent mis-signing.
///
/// Sole exception: `staking-config` is a platform-level read-only API that doesn't sign and doesn't
/// touch the wallet; it calls `resolve_agent_id_by_role(AGENT_ROLE_EVALUATOR)` directly for the header.
///
/// Returns `(account_id, address, evaluator_agent_id)`.
pub async fn resolve_wallet_and_agent_for_evaluator(
    agent_id: &str,
) -> Result<(String, String, String)> {
    let id = agent_id.trim();
    if id.is_empty() {
        bail!("agent_id 不能为空（必须传 envelope 顶层 agentId）");
    }

    let owner = fetch_my_agent_by_id(id)
        .await
        .and_then(|a| {
            a.get("agentWalletAddress")
                .and_then(|v| v.as_str())
                .map(String::from)
        })
        .unwrap_or_default();
    if owner.is_empty() {
        audit::log(
            "cli",
            "evaluator/wallet_resolve_failed",
            false,
            Duration::default(),
            Some(vec![
                format!("agentId={id}"),
                "reason=missing_agent_wallet_address".into(),
            ]),
            Some("fetch_my_agent_by_id returned no agentWalletAddress"),
        );
        bail!(
            "无法获取 agentId={id} 的钱包地址；请确认该 agentId 在 `onchainos agent get` 中可查"
        );
    }

    let (account_id, address) = resolve_wallet(None, Some(&owner)).map_err(|e| {
        let msg = format!("{e}");
        audit::log(
            "cli",
            "evaluator/wallet_resolve_failed",
            false,
            Duration::default(),
            Some(vec![
                format!("agentId={id}"),
                format!("ownerAddress={owner}"),
                "reason=wallet_not_in_local_store".into(),
            ]),
            Some(&msg),
        );
        anyhow::anyhow!(
            "agentId={id} 对应钱包 {owner} 不在本地（{msg}）"
        )
    })?;
    Ok((account_id, address, id.to_string()))
}

/// Resolve agentId only (not wallet), used as fallback for read-only query commands.
/// On failure returns Ok(String::new()) without blocking the caller.
pub async fn resolve_agent_id_by_role(role_code: i64) -> Result<String> {
    let label = match role_code {
        1 => "buyer（买家）",
        2 => "provider（卖家）",
        3 => "evaluator（仲裁者）",
        _ => "unknown",
    };
    Ok(resolve_agent_by_role(role_code, label, None)
        .await
        .map(|(id, _)| id)
        .unwrap_or_default())
}

/// Sign uopData + broadcast on-chain (pure sign-broadcast, no API request).
///
/// Receives `uopData` from the backend, signs it, then broadcasts on-chain via `TaskApiClient` and returns txHash.
/// The API request is performed by the caller via `TaskApiClient`.
///
/// `biz_context` marks the business scenario (TaskAccept / DisputeCreate etc.) and is sent with the broadcast request so the backend can distinguish them.
pub async fn sign_uop_and_broadcast(
    client: &mut TaskApiClient,
    uop_data: &Value,
    account_id: &str,
    address: &str,
    job_id: &str,
    biz_type: i64,
    agent_id: &str,
) -> Result<String> {
    if uop_data.is_null() {
        bail!("后端未返回 uopData，无法签名上链");
    }

    let unsigned: UnsignedInfoResponse = serde_json::from_value(uop_data.clone())
        .map_err(|e| anyhow::anyhow!("解析 uopData 失败: {e}"))?;

    // Simulation-failure guard: backend returns non-empty uopData but executeResult=false means
    // on-chain estimateGas already reverted (contract check failed / insufficient balance / insufficient approve, etc.);
    // at this point hash/uopHash are empty strings, and continuing to broadcast would only be
    // rejected by downstream guards and mask the real failure reason. Throw executeErrorMsg directly here.
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
        bail!("交易模拟失败（链上 estimateGas revert，与 gas / native 余额无关）: {}", err_msg);
    }

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
        "bizType": biz_type,
    });

    let bc_resp = client.post_with_identity(client.broadcast_path(), &broadcast_body, agent_id).await
        .map_err(|e| anyhow::anyhow!("广播失败: {e}"))?;

    Ok(bc_resp[0]["txHash"]
        .as_str()
        .unwrap_or("pending")
        .to_string())
}

/// Variant of sign_uop_and_broadcast used only for vote/commit scenarios:
/// attaches the backend-returned `commitSalt` and the evaluator's chosen `vote` (0/1) to bizContext,
/// so the on-chain broadcast can reconstruct the material for `commitHash = keccak256(disputeId, vote, salt)`.
#[allow(clippy::too_many_arguments)]
pub async fn sign_uop_and_broadcast_with_commit_meta(
    client: &mut TaskApiClient,
    uop_data: &Value,
    account_id: &str,
    address: &str,
    job_id: &str,
    biz_type: i64,
    agent_id: &str,
    commit_salt: &str,
    vote: u8,
) -> Result<String> {
    if uop_data.is_null() {
        bail!("后端未返回 uopData，无法签名上链");
    }

    let unsigned: UnsignedInfoResponse = serde_json::from_value(uop_data.clone())
        .map_err(|e| anyhow::anyhow!("解析 uopData 失败: {e}"))?;

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
        bail!("交易模拟失败（链上 estimateGas revert，与 gas / native 余额无关）: {}", err_msg);
    }

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
        "bizType": biz_type,
        "commitSalt": commit_salt,
        "vote": vote,
    });

    let bc_resp = client.post_with_identity(client.broadcast_path(), &broadcast_body, agent_id).await
        .map_err(|e| anyhow::anyhow!("广播失败: {e}"))?;

    Ok(bc_resp[0]["txHash"]
        .as_str()
        .unwrap_or("pending")
        .to_string())
}

/// Variant of sign_uop_and_broadcast that supports attaching paymentVerify in bizContext.
/// Required only for the escrow accept scenario.
#[allow(clippy::too_many_arguments)]
pub async fn sign_uop_and_broadcast_with_payment(
    client: &mut TaskApiClient,
    uop_data: &Value,
    account_id: &str,
    address: &str,
    job_id: &str,
    biz_type: i64,
    agent_id: &str,
    payment_verify: Value,
) -> Result<String> {
    if uop_data.is_null() {
        bail!("后端未返回 uopData，无法签名上链");
    }

    let unsigned: UnsignedInfoResponse = serde_json::from_value(uop_data.clone())
        .map_err(|e| anyhow::anyhow!("解析 uopData 失败: {e}"))?;

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
        bail!("交易模拟失败（链上 estimateGas revert，与 gas / native 余额无关）: {}", err_msg);
    }

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
        "bizType": biz_type,
        "paymentVerify": payment_verify,
    });

    let bc_resp = client.post_with_identity(client.broadcast_path(), &broadcast_body, agent_id).await
        .map_err(|e| anyhow::anyhow!("广播失败: {e}"))?;

    Ok(bc_resp[0]["txHash"]
        .as_str()
        .unwrap_or("pending")
        .to_string())
}

/// Sign an EIP-712 digest with the session key and return a hex signature string.
pub fn sign_digest_with_session_key(digest: &str) -> Result<String> {
    let session = crate::wallet_store::load_session()?
        .ok_or_else(|| anyhow::anyhow!("未登录，请先执行 onchainos wallet auth"))?;
    let session_key = crate::keyring_store::get("session_key")
        .map_err(|_| anyhow::anyhow!("未登录，请先执行 onchainos wallet auth"))?;

    let signing_seed =
        crate::crypto::hpke_decrypt_session_sk(&session.encrypted_session_sk, &session_key)?;
    let signing_seed_b64 = BASE64_STANDARD.encode(signing_seed);
    crate::crypto::ed25519_sign_hex(digest, &signing_seed_b64)
}

/// Sign EIP-712 typedData and return the final ECDSA signature hex.
/// Delegates to `agentic_wallet::sign::eip712_sign_raw` (gen-msg-hash → ed25519 → sign-msg).
pub async fn sign_typed_data(typed_data: &Value, from_address: &str) -> Result<String> {
    eprintln!("[debug] sign_typed_data 入参: from={from_address}, typedData primaryType={}", typed_data["primaryType"]);
    let sig = crate::commands::agentic_wallet::sign::eip712_sign_raw(
        typed_data,
        XLAYER_CHAIN_INDEX,
        from_address,
    ).await?;
    eprintln!("[debug] sign_typed_data 返回签名: {sig}");
    Ok(sig)
}

/// Dual-sign flow for complete/refuse.
///
/// 1. Compute `deadline = now + 1800`
/// 2. POST `pre-{action}` with `{ deadline }` + identity headers → typedData + nonce
/// 3. Sign typedData via wallet API (gen-msg-hash → ed25519 → sign-msg) → ECDSA signature
/// 4. POST `{action}` with `{ signatureData: { signature, deadline, nonce }, ...extra }` → uopData
/// 5. Sign uopHash + broadcast → tx_hash
#[allow(clippy::too_many_arguments)]
pub async fn task_dual_sign_and_broadcast(
    client: &mut TaskApiClient,
    job_id: &str,
    pre_action: &str,
    main_action: &str,
    extra_main_fields: Option<&Value>,
    account_id: &str,
    address: &str,
    agent_id: &str,
) -> Result<BroadcastResult> {
    let deadline = chrono::Utc::now().timestamp() + 1800;

    // Step 1: POST pre-endpoint → typedData + nonce
    let pre_url = client.endpoint(job_id, pre_action);
    let pre_body = serde_json::json!({ "deadline": deadline });
    let pre_resp = client.post_with_identity(&pre_url, &pre_body, agent_id).await
        .map_err(|e| anyhow::anyhow!("{pre_action} 请求失败: {e}"))?;

    let typed_data = &pre_resp["typedData"];
    if typed_data.is_null() {
        bail!("{pre_action} 未返回 typedData");
    }
    let nonce = pre_resp["nonce"].as_str().unwrap_or("");

    // Step 2: EIP-712 sign typedData
    let signature = sign_typed_data(typed_data, address).await?;

    // Step 3: build signatureData + merge extra fields → POST main endpoint
    let mut main_body = serde_json::json!({
        "signatureData": {
            "signature": signature,
            "deadline": deadline,
            "nonce": nonce,
        }
    });
    if let Some(extra) = extra_main_fields {
        if let (Some(main_obj), Some(extra_obj)) =
            (main_body.as_object_mut(), extra.as_object())
        {
            for (k, v) in extra_obj {
                main_obj.insert(k.clone(), v.clone());
            }
        }
    }

    let main_url = client.endpoint(job_id, main_action);
    let main_resp = client.post_with_identity(&main_url, &main_body, agent_id).await
        .map_err(|e| anyhow::anyhow!("{main_action} 请求失败: {e}"))?;

    // Step 4: Sign uopHash + broadcast
    let biz_type = extract_biz_type(&main_resp);
    let tx_hash = sign_uop_and_broadcast(
        client, &main_resp["uopData"], account_id, address, job_id, biz_type, agent_id,
    ).await?;

    Ok(BroadcastResult { api_response: main_resp, tx_hash })
}
