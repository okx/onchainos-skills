//! 协商状态管理
//!
//! 本地持久化推荐列表 + 当前协商索引，供 Agent 遍历卖家列表时使用。
//!
//! 状态文件：~/.onchainos/task/{jobId}/negotiate-state.json
//! 清理时机：买家执行 confirm-accept 成功后

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

/// 协商状态
#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NegotiateState {
    pub job_id: String,
    pub providers: Vec<ProviderInfo>,
    pub current_index: usize,
    pub created_at: String,
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
    state.current_index += 1;

    // 保存新 index
    let json = serde_json::to_string_pretty(&state)?;
    std::fs::write(state_path(job_id)?, json)?;

    Ok(state.providers.get(state.current_index).cloned())
}

/// 清理状态文件（accept 成功后调用）
pub fn cleanup(job_id: &str) -> Result<()> {
    let dir = state_dir(job_id)?;
    if dir.exists() {
        std::fs::remove_dir_all(&dir)?;
    }
    Ok(())
}
