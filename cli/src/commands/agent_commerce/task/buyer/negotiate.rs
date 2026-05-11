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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub payment_most_token_amount: Option<String>,
}

// ─── 协商护栏常量 ────────────────────────────────────────────────────────

pub const MAX_COUNTER_ROUNDS: u32 = 3;
pub const NEGOTIATE_TIMEOUT_SECS: i64 = 300;
/// Grace period for `counter` event: seller already replied, but agent processing may lag
pub const COUNTER_GRACE_SECS: i64 = 30;

/// Per-seller negotiation tracking (timer + counter safeguards)
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NegotiateTracking {
    pub counter_count: u32,
    #[serde(default, skip_serializing_if = "Option::is_none", alias = "lastProposeTs")]
    pub last_sent_ts: Option<i64>,
    pub status: String,
}

impl Default for NegotiateTracking {
    fn default() -> Self {
        Self {
            counter_count: 0,
            last_sent_ts: None,
            status: "active".to_string(),
        }
    }
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
    /// Per-seller negotiation tracking: timer + COUNTER limit
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub negotiate_tracking: HashMap<String, NegotiateTracking>,
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
        negotiate_tracking: HashMap::new(),
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
        super::create::resolve_buyer_agent(None)
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
                negotiate_tracking: HashMap::new(),
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

/// 清理状态文件（accept 成功后调用）
pub fn cleanup(job_id: &str) -> Result<()> {
    let dir = state_dir(job_id)?;
    if dir.exists() {
        std::fs::remove_dir_all(&dir)?;
    }
    Ok(())
}

// ─── negotiate-tick 护栏 ─────────────────────────────────────────────────

fn is_terminated(status: &str) -> bool {
    matches!(status, "timeout" | "counter_exceeded" | "rejected" | "completed")
}

pub fn handle_negotiate_tick(
    job_id: &str,
    _agent_id: &str,
    seller_agent_id: &str,
    event: &str,
) -> Result<()> {
    let mut state = match load(job_id) {
        Ok(s) => s,
        Err(_) => {
            if matches!(event, "sent" | "propose" | "counter" | "timeout_check") {
                println!("{}", serde_json::json!({
                    "ok": false, "error": "No negotiate state. Run `onchainos agent recommend` first."
                }));
            } else {
                println!("{}", serde_json::to_string_pretty(&serde_json::json!({
                    "ok": true, "action": event
                }))?);
            }
            return Ok(());
        }
    };

    let path = state_path(job_id)?;
    let t = state.negotiate_tracking
        .entry(seller_agent_id.to_string())
        .or_default();


    match event {
        "sent" | "propose" => {
            if is_terminated(&t.status) {
                println!("{}", serde_json::to_string_pretty(&serde_json::json!({
                    "ok": true,
                    "action": "already_terminated",
                    "status": t.status,
                    "reason": format!("该卖家协商已终止（status={}），不再重置计时器", t.status)
                }))?);
                return Ok(());
            }
            t.last_sent_ts = Some(chrono::Utc::now().timestamp());
            t.status = "active".to_string();

            let output = serde_json::json!({
                "ok": true,
                "action": "continue",
                "sellerAgentId": seller_agent_id,
                "counterCount": t.counter_count,
                "counterLimit": MAX_COUNTER_ROUNDS,
                "timeoutSecs": NEGOTIATE_TIMEOUT_SECS
            });
            flush_state(&state, &path)?;
            println!("{}", serde_json::to_string_pretty(&output)?);
        }
        "counter" => {
            if is_terminated(&t.status) {
                println!("{}", serde_json::to_string_pretty(&serde_json::json!({
                    "ok": true,
                    "action": "already_terminated",
                    "status": t.status,
                    "reason": format!("该卖家协商已终止（status={}），忽略 COUNTER", t.status)
                }))?);
                return Ok(());
            }
            if let Some(ts) = t.last_sent_ts {
                let effective_timeout = NEGOTIATE_TIMEOUT_SECS + COUNTER_GRACE_SECS;
                if chrono::Utc::now().timestamp() - ts >= effective_timeout {
                    t.status = "timeout".to_string();
        
                    let output = serde_json::json!({
                        "ok": true, "action": "timeout",
                        "reason": format!("协商超时（{}秒未回复）", NEGOTIATE_TIMEOUT_SECS)
                    });
                    flush_state(&state, &path)?;
                    println!("{}", serde_json::to_string_pretty(&output)?);
                    return Ok(());
                }
            }
            t.counter_count += 1;

            if t.counter_count >= MAX_COUNTER_ROUNDS {
                t.status = "counter_exceeded".to_string();
                let output = serde_json::json!({
                    "ok": true, "action": "counter_exceeded",
                    "counterCount": t.counter_count,
                    "counterLimit": MAX_COUNTER_ROUNDS,
                    "reason": format!("卖家已发送 {} 次 COUNTER（上限 {}），自动终止协商", t.counter_count, MAX_COUNTER_ROUNDS)
                });
                flush_state(&state, &path)?;
                println!("{}", serde_json::to_string_pretty(&output)?);
                return Ok(());
            }
            let remaining = MAX_COUNTER_ROUNDS - t.counter_count;
            let count = t.counter_count;
            let output = serde_json::json!({
                "ok": true, "action": "continue",
                "counterCount": count,
                "counterLimit": MAX_COUNTER_ROUNDS,
                "remaining": remaining
            });
            flush_state(&state, &path)?;
            println!("{}", serde_json::to_string_pretty(&output)?);
        }
        "timeout_check" => {
            if is_terminated(&t.status) {
                println!("{}", serde_json::to_string_pretty(&serde_json::json!({
                    "ok": true,
                    "action": "already_terminated",
                    "status": t.status,
                    "reason": format!("该卖家协商已终止（status={}）", t.status)
                }))?);
                return Ok(());
            }
            let elapsed = t.last_sent_ts
                .map(|ts| chrono::Utc::now().timestamp() - ts)
                .unwrap_or(0);
            if elapsed >= NEGOTIATE_TIMEOUT_SECS {
                t.status = "timeout".to_string();
    
                let output = serde_json::json!({
                    "ok": true, "action": "timeout",
                    "elapsedSecs": elapsed,
                    "timeoutSecs": NEGOTIATE_TIMEOUT_SECS,
                    "reason": format!("协商超时（已过 {}秒，上限 {}秒）", elapsed, NEGOTIATE_TIMEOUT_SECS)
                });
                flush_state(&state, &path)?;
                println!("{}", serde_json::to_string_pretty(&output)?);
            } else {
                println!("{}", serde_json::to_string_pretty(&serde_json::json!({
                    "ok": true, "action": "continue",
                    "elapsedSecs": elapsed,
                    "timeoutSecs": NEGOTIATE_TIMEOUT_SECS,
                    "remainingSecs": NEGOTIATE_TIMEOUT_SECS - elapsed
                }))?);
            }
        }
        "reject" | "confirm" => {
            let status = if event == "reject" { "rejected" } else { "completed" };
            t.status = status.to_string();

            flush_state(&state, &path)?;
            println!("{}", serde_json::to_string_pretty(&serde_json::json!({
                "ok": true, "action": status
            }))?);
        }
        other => {
            println!("{}", serde_json::json!({
                "ok": false,
                "error": format!("Unknown event: {other}. Valid: sent, propose, counter, timeout_check, reject, confirm")
            }));
        }
    }
    Ok(())
}

fn flush_state(state: &NegotiateState, path: &std::path::Path) -> Result<()> {
    let json = serde_json::to_string_pretty(state)?;
    std::fs::write(path, json)?;
    Ok(())
}
