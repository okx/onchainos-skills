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
pub fn save(job_id: &str, providers: Vec<ProviderInfo>) -> Result<()> {
    let dir = state_dir(job_id)?;
    std::fs::create_dir_all(&dir)?;

    let state = NegotiateState {
        job_id: job_id.to_string(),
        providers,
        current_index: 0,
        created_at: chrono::Utc::now().to_rfc3339(),
        agreed: HashMap::new(),
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
pub fn save_agreed(job_id: &str, provider_agent_id: &str, token_symbol: &str, token_amount: &str) -> Result<()> {
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
            }
        }
    };
    state.agreed.insert(provider_agent_id.to_string(), AgreedTerms {
        token_symbol: token_symbol.to_string(),
        token_amount: token_amount.to_string(),
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

/// 清理状态文件（accept 成功后调用）
pub fn cleanup(job_id: &str) -> Result<()> {
    let dir = state_dir(job_id)?;
    if dir.exists() {
        std::fs::remove_dir_all(&dir)?;
    }
    Ok(())
}
