//! Task system signing utilities.
//!
//! Provides reusable sign-and-broadcast helpers for task CLI commands.
//! All on-chain write operations go through one of these flows:
//!
//! - [`sign_uop_and_broadcast`] — sign uopData + broadcast (caller已拿到 uopData)
//! - [`task_dual_sign_and_broadcast`] — dual-sign for complete/refuse
//!   (pre-endpoint → sign typedData → main endpoint → sign uopHash → broadcast)

use anyhow::{bail, Result};
use base64::engine::{general_purpose::STANDARD as BASE64_STANDARD, Engine as _};
use serde_json::Value;
use tokio::process::Command;

use crate::commands::agentic_wallet::transfer::{build_broadcast_body, resolve_address};
use crate::commands::agent_commerce::task::common::network::task_api_client::TaskApiClient;
use crate::commands::agent_commerce::task::common::{
    AGENT_ROLE_BUYER, AGENT_ROLE_EVALUATOR, AGENT_ROLE_PROVIDER,
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

/// 从上一步 API 响应中提取 bizType（数字），原样透传给广播接口。
pub fn extract_biz_type(resp: &Value) -> i64 {
    resp["type"].as_i64().unwrap_or(0)
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
    // 先通过本地身份列表拿 buyer agentId，用于 GET 请求带 agenticId header
    let local_agent_id = resolve_agent_by_role(AGENT_ROLE_BUYER, "buyer（买家）", None)
        .await
        .map(|(id, _)| id)
        .unwrap_or_default();

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
    // 先通过本地身份列表拿 provider agentId，用于 GET 请求带 agenticId header
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

/// 通过子进程调用 `onchainos agent get` 拉取身份列表（调用方再按需筛选）。
async fn query_agent_list() -> Result<Vec<Value>> {
    let exe = std::env::current_exe()
        .map_err(|e| anyhow::anyhow!("无法获取当前可执行文件路径: {e}"))?;

    let output = Command::new(&exe)
        .args(["agent", "get"])
        .output()
        .await
        .map_err(|e| anyhow::anyhow!("调用 `onchainos agent get` 失败: {e}"))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        bail!(
            "身份查询失败（`onchainos agent get` 退出码 {}）: {stderr}",
            output.status
        );
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let parsed: Value = serde_json::from_str(&stdout)
        .map_err(|e| anyhow::anyhow!("解析 agent get 输出失败: {e}"))?;

    if !parsed["ok"].as_bool().unwrap_or(false) {
        let err_msg = parsed["error"].as_str().unwrap_or("未知错误");
        bail!("身份查询失败: {err_msg}");
    }

    // 后端 data 兼容两种形态：object `{list: [...]}` 或 array `[{list: [...]}]`。
    // 对齐 provider/find_jobs.rs 的兜底逻辑，避免环境差异导致身份解析炸掉。
    let data = &parsed["data"];
    let list = if data.is_array() {
        data.get(0).and_then(|v| v.get("list")).and_then(Value::as_array)
    } else {
        data["list"].as_array()
    }
    .ok_or_else(|| anyhow::anyhow!("未查到任何 Agent 身份"))?;

    Ok(list.clone())
}

/// 在 `onchainos agent get` 列表里按 `role` 筛选，可选限定 `ownerAddress`。
///
/// - `wallet_address`: 传 `Some(addr)` 则只匹配 `ownerAddress` 一致的身份（大小写不敏感）；
///   传 `None` 则取首个匹配 role 的（用于只需 agentId header 的只读场景）。
///
/// 返回 `(agent_id, owner_address)`。
async fn resolve_agent_by_role(
    role_code: i64,
    role_label: &str,
    wallet_address: Option<&str>,
) -> Result<(String, String)> {
    let list = query_agent_list().await
        .map_err(|e| anyhow::anyhow!("{e}（请先注册 {role_label} 身份）"))?;

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

/// 按 `agent_id` 在身份列表里精确定位，并校验 `role` 一致；返回该身份的 `ownerAddress`。
///
/// 入参 `agent_id` 来自系统消息 envelope 的顶层 `agentId` 字段——这是真后端识别身份的
/// 权威来源。CLI 用它反查 `ownerAddress`，再去 wallet store 找对应的本地账户来签名，
/// 保证多身份场景下"该 agentId 由其名下钱包签名"的对应关系不会错位。
async fn find_owner_address_by_agent_id(
    agent_id: &str,
    role_code: i64,
    role_label: &str,
) -> Result<String> {
    let id = agent_id.trim();
    if id.is_empty() {
        bail!("agent_id 不能为空");
    }
    let list = query_agent_list().await?;
    for agent in &list {
        let this_id = agent["agentId"].as_str().unwrap_or("");
        if this_id != id {
            continue;
        }
        let role = agent["role"].as_i64().unwrap_or(0);
        if role != role_code {
            bail!(
                "agentId={id} 不是 {role_label} 身份（role={role}），请确认 envelope.agentId 与角色匹配"
            );
        }
        let owner = agent["ownerAddress"].as_str().unwrap_or("").to_string();
        if owner.is_empty() {
            bail!("agentId={id} 缺少 ownerAddress 字段，无法定位钱包");
        }
        return Ok(owner);
    }
    bail!(
        "`onchainos agent get` 列表中没有 agentId={id} 的身份；请确认该 agentId 属于当前登录账户名下"
    )
}

/// Resolve wallet + evaluator agentId for signing.
///
/// 按 `agent_id` 在 `agent get` 列表中精确定位 → 拿 `ownerAddress` → 在 wallet
/// store 中找对应账户。`agent_id` 必传（来自系统消息 envelope 的顶层 `agentId`），
/// 是多身份场景下唯一的正确路径——禁用「默认钱包反查」兜底以防错位签名。
///
/// 唯一例外：`staking-config` 是 platform-level 只读 API，不签名、不动
/// 钱包，直接调 `resolve_agent_id_by_role(AGENT_ROLE_EVALUATOR)` 取 header 用。
///
/// Returns `(account_id, address, evaluator_agent_id)`.
pub async fn resolve_wallet_and_agent_for_evaluator(
    agent_id: &str,
) -> Result<(String, String, String)> {
    let id = agent_id.trim();
    if id.is_empty() {
        bail!("agent_id 不能为空（必须传 envelope 顶层 agentId）");
    }
    let owner = find_owner_address_by_agent_id(
        id,
        AGENT_ROLE_EVALUATOR,
        "evaluator（仲裁者）",
    )
    .await?;
    let (account_id, address) = resolve_wallet(None, Some(&owner)).map_err(|e| {
        anyhow::anyhow!(
            "agentId={id} 对应钱包 {owner} 不在本地（{e}），请先执行 `onchainos wallet auth`"
        )
    })?;
    Ok((account_id, address, id.to_string()))
}

/// 仅解析 agentId（不解析 wallet），用于只读查询命令 fallback。
/// 失败返回 Ok(String::new())，不阻断调用方。
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
    biz_type: i64,
    agent_id: &str,
) -> Result<String> {
    if uop_data.is_null() {
        bail!("后端未返回 uopData，无法签名上链");
    }

    let unsigned: UnsignedInfoResponse = serde_json::from_value(uop_data.clone())
        .map_err(|e| anyhow::anyhow!("解析 uopData 失败: {e}"))?;

    // 模拟执行失败兜底：后端返回 uopData 非空但 executeResult=false 表示链上 estimateGas
    // 已 revert（合约校验不过 / 余额不足 / approve 不够等），此时 hash/uopHash 都是空串，
    // 继续走 broadcast 只会被下游兜底拒掉、掩盖真实失败原因。这里直接抛 executeErrorMsg。
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
        bail!("交易模拟失败（链上 estimateGas revert）: {}", err_msg);
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

/// sign_uop_and_broadcast 的变体，仅 vote/commit 场景使用：
/// 把后端返回的 `commitSalt` 和 evaluator 选择的 `vote`(0/1) 附加到 bizContext，
/// 让链上广播能复原 `commitHash = keccak256(disputeId, vote, salt)` 的素材。
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
        bail!("交易模拟失败（链上 estimateGas revert）: {}", err_msg);
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

/// sign_uop_and_broadcast 的变体，支持在 bizContext 中附加 paymentVerify。
/// 仅 escrow accept 场景需要。
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
        bail!("交易模拟失败（链上 estimateGas revert）: {}", err_msg);
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

/// 用 session key 对 EIP-712 digest 进行签名，返回 hex 签名字符串。
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

/// 对 EIP-712 typedData 进行签名，返回最终的 ECDSA 签名 hex。
/// 委托给 `agentic_wallet::sign::eip712_sign_raw`（gen-msg-hash → ed25519 → sign-msg）。
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

    // Step 2: EIP-712 签名 typedData
    let signature = sign_typed_data(typed_data, address).await?;

    // Step 3: 构造 signatureData + merge extra fields → POST main endpoint
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
