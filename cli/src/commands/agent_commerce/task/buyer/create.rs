//! 发布任务（自定义签名流程）
//!
//! 买家动作：发布任务 — onchainos agent create-task
//!
//! 身份校验：通过调用身份模块 CLI（`onchainos agent get`）检查当前用户
//! 是否拥有买家身份（role=1），再执行任务发布流程。

use anyhow::{bail, Result};
use tokio::process::Command;

use crate::commands::agentic_wallet::auth::ensure_tokens_refreshed;
use crate::commands::agent_commerce::task::common::{
    self, network::task_api_client::TaskApiClient,
    AGENT_ROLE_BUYER, XLAYER_CHAIN_ID,
};
use crate::commands::agent_commerce::task::signing;

// ─── 常量 ────────────────────────────────────────────────────────────────

const MAX_BUDGET: f64 = 10_000_000.0;
const MIN_DESCRIPTION_CHARS: usize = 20;
const MAX_DESCRIPTION_CHARS: usize = 2000;
const MAX_BUDGET_DECIMALS: usize = 5;
const MAX_SUMMARY_CHARS: usize = 200;
const ACCEPT_MIN: u64 = 10 * 60;
const ACCEPT_MAX: u64 = 180 * 86400;
const SUBMIT_MIN: u64 = 60;
const SUBMIT_MAX: u64 = 180 * 86400;

// ─── 参数结构体 ──────────────────────────────────────────────────────────

pub struct CreateTaskParams {
    pub description: String,
    pub description_summary: Option<String>,
    pub budget: f64,
    pub max_budget: f64,
    pub currency: String,
    pub deadline_open: String,
    pub deadline_submit: String,
    pub title: Option<String>,
    pub agent_id: Option<String>,
    pub provider: Option<String>,
}

struct ValidatedParams {
    currency: String,
    title: String,
    summary: String,
    open_secs: u64,
    submit_secs: u64,
}

impl CreateTaskParams {
    fn validate(&self) -> Result<ValidatedParams> {
        let desc_len = self.description.chars().count();
        if desc_len < MIN_DESCRIPTION_CHARS {
            bail!("描述太短，请补充需求细节（至少 {MIN_DESCRIPTION_CHARS} 字符，当前 {desc_len} 字符）");
        }
        if desc_len > MAX_DESCRIPTION_CHARS {
            bail!(
                "任务描述不能超过 {MAX_DESCRIPTION_CHARS} 字符（当前 {desc_len} 字符），\
                你可以让 AI 帮你提炼精简，或手动缩减描述内容后重试。"
            );
        }

        let currency = normalize_currency(&self.currency)?;
        validate_budget(self.budget)?;
        validate_budget_decimals(self.budget)?;

        if self.max_budget < self.budget {
            bail!("--max-budget ({}) 不能小于 --budget ({})", self.max_budget, self.budget);
        }
        validate_budget(self.max_budget)?;
        validate_budget_decimals(self.max_budget)?;

        let open_secs = parse_duration_secs(&self.deadline_open)
            .map_err(|e| anyhow::anyhow!("--deadline-open {e}"))?;
        if open_secs < ACCEPT_MIN {
            bail!("--deadline-open 不能少于 10m（10 分钟），当前值 {}，允许范围 10m ~ 180d", self.deadline_open);
        }
        if open_secs > ACCEPT_MAX {
            bail!("--deadline-open 不能超过 180d（6 个月），当前值 {}，允许范围 10m ~ 180d", self.deadline_open);
        }

        let submit_secs = parse_duration_secs(&self.deadline_submit)
            .map_err(|e| anyhow::anyhow!("--deadline-submit {e}"))?;
        if submit_secs < SUBMIT_MIN {
            bail!("--deadline-submit 不能少于 1m（1 分钟），当前值 {}，允许范围 1m ~ 180d", self.deadline_submit);
        }
        if submit_secs > SUBMIT_MAX {
            bail!("--deadline-submit 不能超过 180d（6 个月），当前值 {}，允许范围 1m ~ 180d", self.deadline_submit);
        }

        let title = match &self.title {
            Some(t) if t.chars().count() > 30 => t.chars().take(30).collect(),
            Some(t) => t.clone(),
            None => self.description.chars().take(30).collect(),
        };
        let summary = match &self.description_summary {
            Some(s) if s.chars().count() > MAX_SUMMARY_CHARS => s.chars().take(MAX_SUMMARY_CHARS).collect(),
            Some(s) => s.clone(),
            None => self.description.chars().take(MAX_SUMMARY_CHARS).collect(),
        };

        Ok(ValidatedParams { currency, title, summary, open_secs, submit_secs })
    }
}

// ─── 校验函数 ────────────────────────────────────────────────────────────

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

fn validate_budget(budget: f64) -> Result<()> {
    if budget <= 0.0 {
        bail!("预算金额必须大于 0");
    }
    if budget > MAX_BUDGET {
        bail!("单次任务预算不得超过 {} USDT/USDG", MAX_BUDGET as u64);
    }
    Ok(())
}

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

pub async fn handle_create(
    client: &mut TaskApiClient,
    params: CreateTaskParams,
) -> Result<()> {
    let validated = params.validate()?;

    ensure_tokens_refreshed().await
        .map_err(|e| anyhow::anyhow!("登录态已失效，请先执行 onchainos wallet login: {e}"))?;

    let (buyer_agent_id, _) = resolve_buyer_agent(params.agent_id.as_deref()).await?;
    eprintln!("[task-create] 买家身份校验通过 (agentId: {buyer_agent_id})");

    common::ensure_sufficient_balance(params.budget, &validated.currency).await?;

    let (account_id, address) = signing::resolve_wallet(None, None)?;

    let body = serde_json::json!({
        "title":              validated.title,
        "description":        params.description,
        "descriptionSummary": validated.summary,
        "paymentTokenSymbol": validated.currency.to_uppercase(),
        "paymentTokenAmount": params.budget.to_string(),
        "paymentMostTokenAmount": params.max_budget.to_string(),
        "chainId":            XLAYER_CHAIN_ID,
        "expireConfig": {
            "acceptDeadline":    validated.open_secs,
            "submittedDeadline": validated.submit_secs
        },
        "paymentMode":        0
    });

    let resp = client.post_with_identity("/priapi/v1/aieco/task/create", &body, &buyer_agent_id).await?;
    let job_id = resp["jobId"].as_str().unwrap_or("?").to_string();

    println!("✓ Calldata 已生成 (jobId: {job_id})");

    let tx_hash = signing::sign_uop_and_broadcast(
        client, &resp["uopData"], &account_id, &address,
        &job_id, 1, &buyer_agent_id,
    ).await?;

    if let Some(ref provider_id) = params.provider {
        super::negotiate::save_designated_provider(&job_id, provider_id)?;
    }

    println!("✓ 任务已上链");
    println!("  jobId:  {job_id}");
    println!("  txHash: {tx_hash}");
    println!("  状态:   open（等待 Provider 报名）");
    if let Some(ref provider_id) = params.provider {
        println!("  指定卖家: {provider_id}（跳过 recommend，直接路由）");
    }
    println!();
    if params.provider.is_some() {
        println!("下一步: 等待 job_created 通知，将自动查询指定卖家服务并路由");
    } else {
        println!("下一步: onchainos agent recommend {job_id}");
    }
    Ok(())
}
