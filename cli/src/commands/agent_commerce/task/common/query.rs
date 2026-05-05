//! 通用只读查询命令（buyer / provider 共用）
//!
//! status — 查询单个任务状态
//! list   — 查询「我的」任务列表

use anyhow::Result;

use super::network::task_api_client::TaskApiClient;
use crate::commands::agent_commerce::task::signing;

/// --agent-id 未传时，按角色从本地身份列表解析 agentId
pub async fn resolve_agent_id(agent_id: &str, role: i64) -> String {
    if !agent_id.is_empty() {
        return agent_id.to_string();
    }
    signing::resolve_agent_id_by_role(role)
        .await
        .unwrap_or_default()
}

/// 查询任务状态
pub async fn handle_status(client: &mut TaskApiClient, job_id: &str, agent_id: &str, role: i64) -> Result<()> {
    let agent_id = resolve_agent_id(agent_id, role).await;
    let resp = client.get_with_identity(&client.task_path(job_id), &agent_id).await?;

    let t = &resp;
    let token_sym = t["paymentTokenSymbol"].as_str().unwrap_or("USDT");
    println!("任务状态: {}", t["statusStr"].as_str().unwrap_or("?"));
    println!("  jobId:    {job_id}");
    println!("  标题:     {}", t["title"].as_str().unwrap_or("?"));
    println!("  预算:     {} {}", t["tokenAmount"].as_str().unwrap_or("?"), token_sym);
    println!("  买家:     {}", t["buyerAgentId"].as_str().unwrap_or("?"));
    if let Some(pid) = t["providerAgentId"].as_str() {
        println!("  卖家:     {pid}");
    }
    println!("  更新时间: {}", t["updateTime"].as_str().unwrap_or("?"));
    Ok(())
}

/// 查询「我的」任务列表
pub async fn handle_list(
    client: &mut TaskApiClient,
    status: Option<&str>,
    page: u32,
    limit: u32,
    agent_id: &str,
    role: i64,
) -> Result<()> {
    let agent_id = resolve_agent_id(agent_id, role).await;
    let mut path = format!("/priapi/v1/aieco/task/my?page={page}&page_size={limit}");
    if let Some(s) = status { path.push_str(&format!("&status={s}")); }

    let resp = client.get_with_identity(&path, &agent_id).await?;
    let tasks = resp["list"].as_array().cloned().unwrap_or_default();
    let total = resp["total"].as_u64().unwrap_or(0);
    println!("任务列表（共 {total} 个，第 {page} 页）：");
    for t in &tasks {
        let sym = t["paymentTokenSymbol"].as_str().unwrap_or("USDT");
        println!("  [{}] {} — {} {}",
            t["statusStr"].as_str().unwrap_or("?"),
            t["jobId"].as_str().unwrap_or("?"),
            t["tokenAmount"].as_str().unwrap_or("?"),
            sym,
        );
        println!("       {}", t["title"].as_str().unwrap_or("?"));
    }
    Ok(())
}
