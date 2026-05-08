//! 发布任务（自定义签名流程）
//!
//! 买家动作：发布任务 — onchainos agent task create
//!
//! 身份校验：通过调用身份模块 CLI（`onchainos agent get`）检查当前用户
//! 是否拥有买家身份（role=1），再执行任务发布流程。

use anyhow::{bail, Result};
use tokio::process::Command;

use crate::commands::agentic_wallet::auth::ensure_tokens_refreshed;
use crate::commands::agentic_wallet::transfer::{build_broadcast_body, resolve_address};
use crate::commands::agent_commerce::task::common::{
    self, network::task_api_client::TaskApiClient,
    AGENT_ROLE_BUYER, XLAYER_CHAIN_ID, XLAYER_CHAIN_INDEX, XLAYER_CHAIN_NAME,
};
use crate::wallet_api::UnsignedInfoResponse;

// ─── 校验函数 ────────────────────────────────────────────────────────────

/// 单次任务预算上限
const MAX_BUDGET: f64 = 10_000_000.0;
/// 任务描述字符上限
const MIN_DESCRIPTION_CHARS: usize = 20;
const MAX_DESCRIPTION_CHARS: usize = 2000;

/// 解析截止时间 → 秒。必须带单位后缀：`d`（天）、`h`（小时）、`m`（分钟）、`s`（秒）。
fn parse_duration_secs(s: &str) -> Result<u64> {
    let s = s.trim();
    if let Some(d) = s.strip_suffix('d') {
        Ok(d.parse::<u64>()? * 86400)
    } else if let Some(h) = s.strip_suffix('h') {
        Ok(h.parse::<u64>()? * 3600)
    } else if let Some(m) = s.strip_suffix('m') {
        Ok(m.parse::<u64>()? * 60)
    } else if let Some(sec) = s.strip_suffix('s') {
        Ok(sec.parse::<u64>()?)
    } else {
        bail!("请指定时间单位，例如 3d（天）、72h（小时）、30m（分钟）、3600s（秒）")
    }
}

/// 归一化并校验货币符号。
/// 接受 USDT / USD₮0（链上实际 symbol）/ USDG，返回后端标准符号。
pub fn normalize_currency(currency: &str) -> Result<String> {
    let normalized: String = currency.chars()
        .map(|c| if c == '₮' { 'T' } else { c })
        .collect::<String>()
        .to_uppercase();
    match normalized.as_str() {
        "USDT" | "USDT0" => Ok("USDT".to_string()),
        "USDG" => Ok("USDG".to_string()),
        _ => bail!("不支持的代币: {currency}，仅支持 USDT（USD₮0）和 USDG"),
    }
}

/// 预算小数位数上限
const MAX_BUDGET_DECIMALS: usize = 5;

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

/// 校验预算小数位数（≤5 位）。
/// f64 转字符串后去尾零再数小数位数。
fn validate_budget_decimals(budget: f64) -> Result<()> {
    let s = format!("{budget}");
    if let Some(dot_pos) = s.find('.') {
        let frac = s[dot_pos + 1..].trim_end_matches('0');
        if frac.len() > MAX_BUDGET_DECIMALS {
            bail!(
                "预算精度限 {MAX_BUDGET_DECIMALS} 位小数，当前 {} 位",
                frac.len()
            );
        }
    }
    Ok(())
}

// ─── 身份校验 ────────────────────────────────────────────────────────────

/// 调用身份模块 CLI（`onchainos agent get`）获取当前用户的 Agent 列表，
/// 返回 role=AGENT_ROLE_BUYER（买家/requestor）的 (agentId, ownerAddress)。
///
/// - `specified_id = Some("424")` → 校验该 agent 存在且为 buyer，直接使用
/// - `specified_id = None` → 自动选择：仅一个 buyer 时直接用，多个 buyer 时报错提示指定
pub(crate) async fn resolve_buyer_agent(specified_id: Option<&str>) -> Result<(String, String)> {
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

    let list = parsed["data"]["list"]
        .as_array()
        .ok_or_else(|| anyhow::anyhow!("未查到任何 Agent 身份，请先执行 onchainos agent create --role requestor 注册买家身份"))?;

    let buyers: Vec<_> = list.iter()
        .filter(|a| a["role"].as_i64() == Some(AGENT_ROLE_BUYER))
        .collect();

    if buyers.is_empty() {
        bail!("当前账户没有买家（requestor）身份，请先执行 onchainos agent create --role requestor 注册");
    }

    if let Some(id) = specified_id {
        let agent = buyers.iter()
            .find(|a| a["agentId"].as_str() == Some(id))
            .ok_or_else(|| anyhow::anyhow!(
                "指定的 agent-id {id} 不是买家身份或不存在，当前买家 agent: {}",
                buyers.iter()
                    .filter_map(|a| a["agentId"].as_str())
                    .collect::<Vec<_>>()
                    .join(", ")
            ))?;
        let owner_address = agent["ownerAddress"].as_str().unwrap_or("").to_string();
        return Ok((id.to_string(), owner_address));
    }

    if buyers.len() == 1 {
        let agent = buyers[0];
        let agent_id = agent["agentId"].as_str()
            .ok_or_else(|| anyhow::anyhow!("Agent 缺少 agentId 字段"))?
            .to_string();
        let owner_address = agent["ownerAddress"].as_str().unwrap_or("").to_string();
        return Ok((agent_id, owner_address));
    }

    let ids: Vec<&str> = buyers.iter()
        .filter_map(|a| a["agentId"].as_str())
        .collect();
    bail!(
        "当前钱包下有多个买家身份: {}，请通过 --agent-id 指定使用哪个",
        ids.join(", ")
    );
}

// ─── 创建任务 ────────────────────────────────────────────────────────────

#[allow(clippy::too_many_arguments)]
pub async fn handle_create(
    client: &mut TaskApiClient,
    description: String,
    description_summary: Option<String>,
    budget: f64,
    max_budget: f64,
    currency: String,
    deadline_open: String,
    deadline_submit: String,
    title: Option<String>,
    payment_mode: Option<String>,
    agent_id: Option<String>,
) -> Result<()> {
    let desc_len = description.chars().count();
    if desc_len < MIN_DESCRIPTION_CHARS {
        bail!("描述太短，请补充需求细节（至少 {MIN_DESCRIPTION_CHARS} 字符，当前 {desc_len} 字符）");
    }
    if desc_len > MAX_DESCRIPTION_CHARS {
        bail!(
            "任务描述不能超过 {MAX_DESCRIPTION_CHARS} 字符（当前 {desc_len} 字符），\
            你可以让 AI 帮你提炼精简，或手动缩减描述内容后重试。"
        );
    }

    let currency = normalize_currency(&currency)?;
    validate_budget(budget)?;
    validate_budget_decimals(budget)?;

    if max_budget < budget {
        bail!("--max-budget ({max_budget}) 不能小于 --budget ({budget})");
    }
    validate_budget(max_budget)?;
    validate_budget_decimals(max_budget)?;

    let open_secs = parse_duration_secs(&deadline_open)
        .map_err(|e| anyhow::anyhow!("--deadline-open {e}"))?;
    const ACCEPT_MIN: u64 = 10 * 60;       // 10 分钟
    const ACCEPT_MAX: u64 = 180 * 86400;   // 6 个月
    if open_secs < ACCEPT_MIN {
        bail!("--deadline-open 不能少于 10m（10 分钟），当前值 {deadline_open}，允许范围 10m ~ 180d");
    }
    if open_secs > ACCEPT_MAX {
        bail!("--deadline-open 不能超过 180d（6 个月），当前值 {deadline_open}，允许范围 10m ~ 180d");
    }

    let submit_secs = parse_duration_secs(&deadline_submit)
        .map_err(|e| anyhow::anyhow!("--deadline-submit {e}"))?;
    const SUBMIT_MIN: u64 = 60;            // 1 分钟
    const SUBMIT_MAX: u64 = 180 * 86400;   // 6 个月
    if submit_secs < SUBMIT_MIN {
        bail!("--deadline-submit 不能少于 1m（1 分钟），当前值 {deadline_submit}，允许范围 1m ~ 180d");
    }
    if submit_secs > SUBMIT_MAX {
        bail!("--deadline-submit 不能超过 180d（6 个月），当前值 {deadline_submit}，允许范围 1m ~ 180d");
    }

    let title_str = match title {
        Some(t) if t.chars().count() > 30 => t.chars().take(30).collect(),
        Some(t) => t,
        None => description.chars().take(30).collect(),
    };
    const MAX_SUMMARY_CHARS: usize = 200;
    let summary = match description_summary {
        Some(s) if s.chars().count() > MAX_SUMMARY_CHARS => s.chars().take(MAX_SUMMARY_CHARS).collect(),
        Some(s) => s,
        None => description.chars().take(MAX_SUMMARY_CHARS).collect(),
    };

    // ── Pre-check: 登录态有效性 ──────────────────────
    // 先在主进程校验一次 token，若已过期立即报错，
    // 避免后续子进程各自触发 relogin 导致多次 OTP 发送。
    ensure_tokens_refreshed().await
        .map_err(|e| anyhow::anyhow!("登录态已失效，请先执行 onchainos wallet login: {e}"))?;

    // ── Step 0: 校验买家身份 ──────────────────────────
    let (buyer_agent_id, _buyer_owner_address) = resolve_buyer_agent(agent_id.as_deref()).await?;
    eprintln!("[task-create] 买家身份校验通过 (agentId: {buyer_agent_id})");

    // ── Step 0.5: 余额预检（余额不足则阻断）──────────────
    common::ensure_sufficient_balance(budget, &currency).await?;

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
        "descriptionSummary": summary,
        "paymentTokenSymbol": currency.to_uppercase(),
        "paymentTokenAmount": budget.to_string(),
        "paymentMostTokenAmount": max_budget.to_string(),
        "chainId":            XLAYER_CHAIN_ID,
        "expireConfig": {
            "acceptDeadline":    open_secs,
            "submittedDeadline": submit_secs
        },
        "paymentMode":        payment_mode.as_deref()
                                .map(|m| crate::commands::agent_commerce::task::common::PaymentMode::from_str(m).as_int())
                                .unwrap_or(0)
    });

    let resp = client.post_with_identity("/priapi/v1/aieco/task/create", &body, &buyer_agent_id).await?;

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
    let bc_resp = client.post_with_identity(client.broadcast_path(), &broadcast_body, &buyer_agent_id).await?;
    let tx_hash = bc_resp[0]["txHash"].as_str().unwrap_or("pending");
    println!("✓ 任务已上链");
    println!("  jobId:  {job_id}");
    println!("  txHash: {tx_hash}");
    println!("  状态:   open（等待 Provider 报名）");
    println!();
    println!("下一步: onchainos agent recommend {job_id}");
    Ok(())
}
