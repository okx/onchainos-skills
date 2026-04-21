//! 获取推荐卖家
//!
//! 买家动作：获取推荐卖家 — onchainos task recommend
//!
//! - 默认：调用 /match API 获取推荐列表并缓存到本地（index=0）
//! - --next：从本地状态推进到下一个 provider 并返回
//! - --current：返回当前 index 的 provider（不推进）

use anyhow::{bail, Result};

use super::negotiate;
use crate::commands::agent_commerce::task::common::network::task_api_client::TaskApiClient;

/// 查询推荐卖家（默认模式：调用 API + 缓存）
pub async fn handle_recommend(client: &TaskApiClient, job_id: &str) -> Result<()> {
    let url = client.endpoint(job_id, "match");
    let resp: serde_json::Value = client.http()
        .post(&url)
        .send().await
        .map_err(|e| anyhow::anyhow!("无法连接后端: {e}"))?
        .json().await?;

    if resp["code"] != 0 {
        bail!("{}", resp["msg"].as_str().unwrap_or("error"));
    }
    let recs = resp["data"]["recommendations"].as_array()
        .cloned().unwrap_or_default();

    // 构造 ProviderInfo 列表并缓存
    let providers: Vec<negotiate::ProviderInfo> = recs.iter().map(|r| {
        negotiate::ProviderInfo {
            provider_address: r["providerAddress"].as_str().unwrap_or("").to_string(),
            provider_agent_id: r["providerAgentId"].as_str().unwrap_or("").to_string(),
            match_score: r["matchScore"].as_f64().unwrap_or(0.0),
            credit_score: r["creditScore"].as_i64().unwrap_or(0),
            capability_summary: r["capabilitySummary"].as_str().unwrap_or("").to_string(),
        }
    }).collect();

    negotiate::save(job_id, providers.clone())?;

    // 输出列表
    println!("推荐卖家列表（共 {} 个，已缓存，当前 index=0）：", providers.len());
    for (i, p) in providers.iter().enumerate() {
        print_provider(i, p);
    }
    Ok(())
}

/// --current：返回当前 provider
pub fn handle_recommend_current(job_id: &str) -> Result<()> {
    let state = negotiate::load(job_id)?;
    match state.providers.get(state.current_index) {
        Some(p) => {
            println!("当前协商卖家（index={}，共 {} 个）：", state.current_index, state.providers.len());
            print_provider(state.current_index, p);
        }
        None => {
            println!("推荐列表已全部遍历（{}/{}），无更多卖家", state.current_index, state.providers.len());
        }
    }
    Ok(())
}

/// --next：推进到下一个 provider
pub fn handle_recommend_next(job_id: &str) -> Result<()> {
    match negotiate::next(job_id)? {
        Some(p) => {
            let state = negotiate::load(job_id)?;
            println!("切换到下一个卖家（index={}，共 {} 个）：", state.current_index, state.providers.len());
            print_provider(state.current_index, &p);
        }
        None => {
            let state = negotiate::load(job_id)?;
            println!("推荐列表已全部遍历（{}/{}），无更多卖家", state.current_index, state.providers.len());
            println!("建议：onchainos agent set-public {job_id} 或 onchainos agent close {job_id}");
        }
    }
    Ok(())
}

fn print_provider(index: usize, p: &negotiate::ProviderInfo) {
    println!("  {}. AgentID: {}  匹配分: {}  信用分: {}",
        index + 1, p.provider_agent_id, p.match_score, p.credit_score,
    );
    println!("     能力: {}", p.capability_summary);
    println!("     地址: {}", p.provider_address);
}
