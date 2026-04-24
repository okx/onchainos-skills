//! 发布任务（自定义签名流程）
//!
//! 买家动作：发布任务 — onchainos task create
//!
//! 身份校验：通过调用身份模块 CLI（`onchainos agent get`）检查当前用户
//! 是否拥有买家身份（role=1），再执行任务发布流程。

use anyhow::{bail, Result};
use tokio::process::Command;

use crate::commands::agentic_wallet::auth::ensure_tokens_refreshed;
use crate::commands::agentic_wallet::transfer::{build_broadcast_body, resolve_address};
use crate::commands::agent_commerce::task::common::network::task_api_client::TaskApiClient;
use crate::commands::agent_commerce::task::common::{
    AGENT_ROLE_BUYER, XLAYER_CHAIN_ID, XLAYER_CHAIN_INDEX, XLAYER_CHAIN_NAME,
};
use crate::wallet_api::UnsignedInfoResponse;

// ─── 校验函数 ────────────────────────────────────────────────────────────

/// 单次任务预算上限
const MAX_BUDGET: f64 = 10_000_000.0;

/// 解析 "72h" / "30m" / "3600" → 秒
fn parse_duration_secs(s: &str) -> Result<u64> {
    let s = s.trim();
    if let Some(h) = s.strip_suffix('h') {
        Ok(h.parse::<u64>()? * 3600)
    } else if let Some(m) = s.strip_suffix('m') {
        Ok(m.parse::<u64>()? * 60)
    } else {
        Ok(s.parse::<u64>()?)
    }
}

/// 校验货币符号
pub fn validate_currency(currency: &str) -> Result<()> {
    match currency.to_uppercase().as_str() {
        "USDT" | "USDG" => Ok(()),
        other => bail!("不支持的代币: {other}，仅支持 USDT 和 USDG"),
    }
}

/// 校验预算金额
fn validate_budget(budget: f64) -> Result<()> {
    if budget <= 0.0 {
        bail!("预算金额必须大于 0");
    }
    if budget > MAX_BUDGET {
        bail!("单次任务预算不得超过 {} USDT/USDG", MAX_BUDGET as u64);
    }
    Ok(())
}

// ─── 身份校验 ────────────────────────────────────────────────────────────

/// 调用身份模块 CLI（`onchainos agent get`）获取当前用户的 Agent 列表，
/// 返回第一个 role=AGENT_ROLE_BUYER（买家/requestor）的 (agentId, ownerAddress)。
async fn resolve_buyer_agent() -> Result<(String, String)> {
    let exe = std::env::current_exe()
        .map_err(|e| anyhow::anyhow!("无法获取当前可执行文件路径: {e}"))?;

    let output = Command::new(&exe)
        .args(["agent", "get"])
        .output()
        .await
        .map_err(|e| anyhow::anyhow!("调用 `onchainos agent get` 失败: {e}"))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        bail!("身份查询失败（`onchainos agent get` 退出码 {}）: {stderr}", output.status);
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let parsed: serde_json::Value = serde_json::from_str(&stdout)
        .map_err(|e| anyhow::anyhow!("解析 agent get 输出失败: {e}"))?;

    if !parsed["ok"].as_bool().unwrap_or(false) {
        let err_msg = parsed["error"].as_str().unwrap_or("未知错误");
        bail!("身份查询失败: {err_msg}");
    }

    // data.list[] 中查找 role=AGENT_ROLE_BUYER（买家/requestor）的 Agent
    let list = parsed["data"]["list"]
        .as_array()
        .ok_or_else(|| anyhow::anyhow!("未查到任何 Agent 身份，请先执行 onchainos agent create --role requestor 注册买家身份"))?;

    for agent in list {
        if agent["role"].as_i64() == Some(AGENT_ROLE_BUYER) {
            let agent_id = agent["agentId"]
                .as_str()
                .ok_or_else(|| anyhow::anyhow!("Agent 缺少 agentId 字段"))?
                .to_string();
            let owner_address = agent["ownerAddress"]
                .as_str()
                .unwrap_or("")
                .to_string();
            return Ok((agent_id, owner_address));
        }
    }

    bail!("当前账户没有买家（requestor）身份，请先执行 onchainos agent create --role requestor 注册");
}

// ─── 余额预检 ────────────────────────────────────────────────────────────

/// 调用 `onchainos wallet balance` 查询当前账户余额，
/// 若指定代币余额不足则发出警告（不阻断流程，合约层会做最终校验）。
async fn warn_if_insufficient_balance(budget: f64, currency: &str) {
    let exe = match std::env::current_exe() {
        Ok(e) => e,
        Err(_) => return,
    };

    let output = match Command::new(&exe)
        .args(["wallet", "balance"])
        .output()
        .await
    {
        Ok(o) if o.status.success() => o,
        _ => return,
    };

    let stdout = String::from_utf8_lossy(&output.stdout);
    let parsed: serde_json::Value = match serde_json::from_str(&stdout) {
        Ok(v) => v,
        Err(_) => return,
    };

    // 遍历 data.details[].tokenAssets[] 查找匹配代币的余额
    let currency_upper = currency.to_uppercase();
    if let Some(details) = parsed["data"]["details"].as_array() {
        for detail in details {
            if let Some(assets) = detail["tokenAssets"].as_array() {
                for asset in assets {
                    let symbol = asset["tokenSymbol"].as_str().unwrap_or("");
                    if symbol.to_uppercase() == currency_upper {
                        let balance_str = asset["balance"].as_str().unwrap_or("0");
                        let balance: f64 = balance_str.parse().unwrap_or(0.0);
                        if balance < budget {
                            eprintln!(
                                "⚠️  余额不足提醒：当前 {symbol} 余额为 {balance}，任务预算 {budget} {currency_upper}，请确保发布后账户有足够资金完成托管支付"
                            );
                        }
                        return;
                    }
                }
            }
        }
    }
}

// ─── 创建任务 ────────────────────────────────────────────────────────────

#[allow(clippy::too_many_arguments)]
pub async fn handle_create(
    client: &mut TaskApiClient,
    description: String,
    description_summary: Option<String>,
    budget: f64,
    max_budget: Option<f64>,
    currency: String,
    deadline_open: String,
    deadline_submit: String,
    title: Option<String>,
) -> Result<()> {
    validate_currency(&currency)?;
    validate_budget(budget)?;

    let max_budget_val = max_budget.unwrap_or(budget);
    if max_budget_val < budget {
        bail!("--max-budget ({max_budget_val}) 不能小于 --budget ({budget})");
    }
    validate_budget(max_budget_val)?;

    let open_secs = parse_duration_secs(&deadline_open)
        .map_err(|_| anyhow::anyhow!("--deadline-open 格式错误，例如 72h 或 3600"))?;
    let submit_secs = parse_duration_secs(&deadline_submit)
        .map_err(|_| anyhow::anyhow!("--deadline-submit 格式错误，例如 48h 或 3600"))?;

    let title_str = title.unwrap_or_else(|| description.chars().take(30).collect());
    let summary = description_summary
        .unwrap_or_else(|| description.chars().take(200).collect());

    // ── Pre-check: 登录态有效性 ──────────────────────
    // 先在主进程校验一次 token，若已过期立即报错，
    // 避免后续子进程各自触发 relogin 导致多次 OTP 发送。
    ensure_tokens_refreshed().await
        .map_err(|e| anyhow::anyhow!("登录态已失效，请先执行 onchainos wallet login: {e}"))?;

    // ── Step 0: 校验买家身份 ──────────────────────────
    let (buyer_agent_id, _buyer_owner_address) = resolve_buyer_agent().await?;
    eprintln!("[task-create] 买家身份校验通过 (agentId: {buyer_agent_id})");

    // ── Step 0.5: 余额预检（警告，不阻断）──────────────
    warn_if_insufficient_balance(budget, &currency).await;

    // ── Step 0.6: 解析钱包地址 ───────────────────────────
    let wallets = crate::wallet_store::load_wallets()?
        .ok_or_else(|| anyhow::anyhow!("未登录，请先执行 onchainos wallet auth"))?;

    let selected_account_id = &wallets.selected_account_id;
    let (_, selected_addr) = resolve_address(&wallets, None, XLAYER_CHAIN_NAME)?;

    let account_id = selected_account_id.clone();
    let addr_info = selected_addr;

    // ── Step 1: 生成 calldata ────────────────────────
    let body = serde_json::json!({
        "title":              title_str,
        "description":        description,
        "description_summary": summary,
        "paymentTokenSymbol": currency.to_uppercase(),
        "paymentTokenAmount": budget.to_string(),
        "maxPaymentTokenAmount": max_budget_val.to_string(),
        "chainId":            XLAYER_CHAIN_ID,
        "expireConfig": {
            "acceptDeadline":    open_secs,
            "submittedDeadline": submit_secs
        },
        "paymentMode":        0,
        "visibility":         0
    });

    let resp = client.post("/priapi/v1/aieco/task/create", &body).await?;

    let job_id = resp["jobId"].as_str().unwrap_or("?").to_string();
    let uop_data = &resp["uopData"];

    println!("✓ Calldata 已生成 (jobId: {job_id})");

    // ── Step 2: 签名 uopHash ─────────────
    let unsigned: UnsignedInfoResponse = serde_json::from_value(uop_data.clone())
        .map_err(|e| anyhow::anyhow!("解析 uopData 失败: {e}"))?;

    let mut broadcast_body = build_broadcast_body(
        &unsigned,
        &account_id,
        &addr_info.address,
        XLAYER_CHAIN_INDEX,
        true,
        false,
        false,
    )
    .await?;
    broadcast_body["bizContext"] = serde_json::json!({
        "jobId": job_id,
        "bizType": 1,
    });

    println!("✓ 签名完成");

    // ── Step 3: 广播上链 ──────────
    let bc_resp = client.post(client.broadcast_path(), &broadcast_body).await?;
    let tx_hash = bc_resp[0]["txHash"].as_str().unwrap_or("pending");
    println!("✓ 任务已上链");
    println!("  jobId:  {job_id}");
    println!("  txHash: {tx_hash}");
    println!("  状态:   open（等待 Provider 报名）");
    println!();
    println!("下一步: onchainos agent recommend {job_id}");
    Ok(())
}
