//! 协商状态管理
//!
//! 本地持久化推荐列表 + 当前协商索引，供 Agent 遍历卖家列表时使用。
//!
//! 状态文件：~/.onchainos/task/{jobId}/negotiate-state.json
//! 清理时机：买家执行 confirm-accept 成功后

use std::collections::HashMap;
use anyhow::{bail, Result};
use serde::{Deserialize, Serialize};

/// 推荐卖家信息（从 /match 接口返回的子集）
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderInfo {
    pub provider_address: String,
    pub provider_agent_id: String,
    #[serde(default)]
    pub provider_name: String,
    pub match_score: f64,
    pub credit_score: i64,
    pub capability_summary: String,
    pub completed_task_count: i64,
    /// true = x402 支付方式，false = escrow/direct
    #[serde(default)]
    pub support_a2mcp: bool,
    #[serde(default)]
    pub services: Vec<ServiceInfo>,
}

/// Provider 提供的服务信息（从 /match 接口 services[] 返回）
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ServiceInfo {
    pub service_id: String,
    pub service_name: String,
    #[serde(default)]
    pub service_description: String,
    /// 服务类型，如 "A2A"
    pub service_type: String,
    /// 服务端点 URL
    pub endpoint: String,
    #[serde(default)]
    pub sort_order: i64,
    /// 费用金额
    #[serde(default)]
    pub fee_amount: f64,
    /// 费用 token symbol（如 "USDT"）
    #[serde(default)]
    pub fee_token_symbol: String,
    /// 费用 token 合约地址
    #[serde(default)]
    pub fee_token: String,
}

/// 某个卖家的协商确定条款
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgreedTerms {
    pub token_symbol: String,
    pub token_amount: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub payment_most_token_amount: Option<String>,
}

/// 协商状态
#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NegotiateState {
    pub job_id: String,
    pub providers: Vec<ProviderInfo>,
    pub current_index: usize,
    pub created_at: String,
    /// 按 provider_agent_id 存储各卖家的协商结果（支持同时与多个卖家协商）
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub agreed: HashMap<String, AgreedTerms>,
    /// 当前页码（0-based）
    #[serde(default)]
    pub page: usize,
    /// 协商失败的 provider agentId 列表（跨页保留，accept 成功时 cleanup 清除）
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub failed_providers: Vec<String>,
}

// ─── 路径 ────────────────────────────────────────────────────────────

fn state_dir(job_id: &str) -> Result<std::path::PathBuf> {
    let home = dirs::home_dir()
        .ok_or_else(|| anyhow::anyhow!("无法获取 HOME 目录"))?;
    Ok(home.join(".onchainos").join("task").join(job_id))
}

fn state_path(job_id: &str) -> Result<std::path::PathBuf> {
    Ok(state_dir(job_id)?.join("negotiate-state.json"))
}

// ─── 公共函数 ────────────────────────────────────────────────────────

/// 保存推荐列表，index 重置为 0
///
/// `page` 为当前页码（0-based）。会从已有状态合并 `failed_providers`。
pub fn save(job_id: &str, providers: Vec<ProviderInfo>, page: usize) -> Result<()> {
    let dir = state_dir(job_id)?;
    std::fs::create_dir_all(&dir)?;

    let existing_failed = load(job_id)
        .map(|s| s.failed_providers)
        .unwrap_or_default();

    let state = NegotiateState {
        job_id: job_id.to_string(),
        providers,
        current_index: 0,
        created_at: chrono::Utc::now().to_rfc3339(),
        agreed: HashMap::new(),
        page,
        failed_providers: existing_failed,
    };

    let json = serde_json::to_string_pretty(&state)?;
    std::fs::write(state_path(job_id)?, json)?;
    Ok(())
}

/// 读取当前状态
pub fn load(job_id: &str) -> Result<NegotiateState> {
    let path = state_path(job_id)?;
    if !path.exists() {
        bail!("未找到协商状态，请先执行 onchainos agent recommend {job_id}");
    }
    let raw = std::fs::read_to_string(&path)?;
    let state: NegotiateState = serde_json::from_str(&raw)?;
    Ok(state)
}

/// 获取当前 index 的 provider（不推进）
pub fn current(job_id: &str) -> Result<Option<ProviderInfo>> {
    let state = load(job_id)?;
    Ok(state.providers.get(state.current_index).cloned())
}

/// 推进到下一个 provider 并返回；如果列表已遍历完返回 None
pub fn next(job_id: &str) -> Result<Option<ProviderInfo>> {
    let mut state = load(job_id)?;

    // 清除旧 provider 的协商结果（协商失败才会切换）
    if let Some(old) = state.providers.get(state.current_index) {
        state.agreed.remove(&old.provider_agent_id);
    }

    state.current_index += 1;

    let json = serde_json::to_string_pretty(&state)?;
    std::fs::write(state_path(job_id)?, json)?;

    Ok(state.providers.get(state.current_index).cloned())
}

/// 保存协商确定的支付参数（协商完成时由 Agent 调用）
///
/// 会查询任务详情获取 `paymentMostTokenAmount`（最高预算），
/// 若协商金额超过最高预算则拒绝保存。
pub async fn save_agreed(
    client: &mut crate::commands::agent_commerce::task::common::network::task_api_client::TaskApiClient,
    job_id: &str,
    provider_agent_id: &str,
    token_symbol: &str,
    token_amount: &str,
    agent_id: Option<&str>,
) -> Result<()> {
    // 查询任务详情获取最高预算
    let agent_id = if let Some(id) = agent_id.filter(|s| !s.is_empty()) {
        id.to_string()
    } else {
        super::create::resolve_buyer_agent()
            .await
            .map(|(id, _)| id)
            .unwrap_or_default()
    };
    let task_path = format!("/priapi/v1/aieco/task/{job_id}");
    let task_detail = client.get_with_identity(&task_path, &agent_id).await;

    let max_amount_saved = if let Ok(detail) = &task_detail {
        let max_amount_str = detail["paymentMostTokenAmount"].as_str().unwrap_or("");
        if !max_amount_str.is_empty() {
            let agreed: f64 = token_amount.parse().unwrap_or(0.0);
            let max_budget: f64 = max_amount_str.parse().unwrap_or(0.0);
            if max_budget > 0.0 && agreed > max_budget {
                bail!(
                    "协商金额 {token_amount} {token_symbol} 超过任务最高预算 {max_amount_str} {token_symbol}，不能接受此报价"
                );
            }
            Some(max_amount_str.to_string())
        } else {
            None
        }
    } else {
        None
    };

    let mut state = match load(job_id) {
        Ok(s) => s,
        Err(_) => {
            let dir = state_dir(job_id)?;
            std::fs::create_dir_all(&dir)?;
            NegotiateState {
                job_id: job_id.to_string(),
                providers: vec![],
                current_index: 0,
                created_at: chrono::Utc::now().to_rfc3339(),
                agreed: HashMap::new(),
                page: 0,
                failed_providers: vec![],
            }
        }
    };
    state.agreed.insert(provider_agent_id.to_string(), AgreedTerms {
        token_symbol: token_symbol.to_string(),
        token_amount: token_amount.to_string(),
        payment_most_token_amount: max_amount_saved,
    });
    let json = serde_json::to_string_pretty(&state)?;
    std::fs::write(state_path(job_id)?, json)?;
    println!("✓ 协商结果已保存: provider={provider_agent_id}, {token_symbol} {token_amount} (job={job_id})");
    Ok(())
}

/// 读取协商确定的支付参数，返回 (token_symbol, token_amount)
///
/// provider_agent_id 为 Some 时精确匹配；为 None 时回退到 current_index 对应的 provider
pub fn load_agreed(job_id: &str, provider_agent_id: Option<&str>) -> Result<Option<(String, String)>> {
    let state = match load(job_id) {
        Ok(s) => s,
        Err(_) => return Ok(None),
    };
    let key = match provider_agent_id {
        Some(id) => id.to_string(),
        None => match state.providers.get(state.current_index) {
            Some(p) => p.provider_agent_id.clone(),
            None => return Ok(None),
        },
    };
    Ok(state.agreed.get(&key).map(|t| (t.token_symbol.clone(), t.token_amount.clone())))
}

/// 保存指定卖家（create-task --provider 指定，job_created 跳过 recommend）
pub fn save_designated_provider(job_id: &str, provider_agent_id: &str) -> Result<()> {
    let dir = state_dir(job_id)?;
    std::fs::create_dir_all(&dir)?;
    let path = dir.join("designated-provider.json");
    let json = serde_json::json!({ "agentId": provider_agent_id });
    std::fs::write(path, serde_json::to_string_pretty(&json)?)?;
    Ok(())
}

/// 检查指定卖家文件是否存在（不消费）
pub fn has_designated_provider(job_id: &str) -> bool {
    state_dir(job_id)
        .map(|d| d.join("designated-provider.json").exists())
        .unwrap_or(false)
}

/// 读取并删除指定卖家文件（consume-on-read：job_created 只触发一次，读完即清）
pub fn take_designated_provider(job_id: &str) -> Result<Option<String>> {
    let path = state_dir(job_id)?.join("designated-provider.json");
    if !path.exists() {
        return Ok(None);
    }
    let raw = std::fs::read_to_string(&path)?;
    let _ = std::fs::remove_file(&path);
    let v: serde_json::Value = serde_json::from_str(&raw)?;
    Ok(v["agentId"].as_str().map(|s| s.to_string()))
}

/// 标记某个 provider 协商失败（后续 recommend 展示时过滤掉）
pub fn mark_failed(job_id: &str, provider_agent_id: &str) -> Result<()> {
    let mut state = match load(job_id) {
        Ok(s) => s,
        Err(_) => {
            let dir = state_dir(job_id)?;
            std::fs::create_dir_all(&dir)?;
            NegotiateState {
                job_id: job_id.to_string(),
                providers: vec![],
                current_index: 0,
                created_at: chrono::Utc::now().to_rfc3339(),
                agreed: HashMap::new(),
                page: 0,
                failed_providers: vec![],
            }
        }
    };
    let pid = provider_agent_id.to_string();
    if !state.failed_providers.contains(&pid) {
        state.failed_providers.push(pid);
    }
    let json = serde_json::to_string_pretty(&state)?;
    std::fs::write(state_path(job_id)?, json)?;
    println!("✓ 已标记 provider {provider_agent_id} 为协商失败 (job={job_id})");
    Ok(())
}

/// 读取失败 provider 列表
pub fn load_failed(job_id: &str) -> Vec<String> {
    load(job_id)
        .map(|s| s.failed_providers)
        .unwrap_or_default()
}

/// 清理状态文件（accept 成功后调用，同时清除 designated-provider.json）
pub fn cleanup(job_id: &str) -> Result<()> {
    let dir = state_dir(job_id)?;
    if dir.exists() {
        std::fs::remove_dir_all(&dir)?;
    }
    Ok(())
}
