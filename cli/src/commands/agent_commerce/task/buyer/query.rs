//! 只读查询命令（无链上签名）
//!
//! status, list, payment, pay

use anyhow::{bail, Result};

use crate::commands::agent_commerce::task::common::network::task_api_client::TaskApiClient;
use crate::commands::agent_commerce::task::common::XLAYER_CHAIN_ID;
use crate::commands::agent_commerce::task::signing;

/// --agent-id 未传时从本地身份列表解析 buyer agentId（不额外请求后端）
async fn resolve_agent_id(_client: &mut TaskApiClient, _job_id: Option<&str>, agent_id: &str) -> String {
    if !agent_id.is_empty() {
        return agent_id.to_string();
    }
    use crate::commands::agent_commerce::task::common::AGENT_ROLE_BUYER;
    signing::resolve_agent_id_by_role(AGENT_ROLE_BUYER)
        .await
        .unwrap_or_default()
}

/// 查询任务状态
pub async fn handle_status(client: &mut TaskApiClient, job_id: &str, agent_id: &str) -> Result<()> {
    let agent_id = resolve_agent_id(client, Some(job_id), agent_id).await;
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

/// 任务列表
pub async fn handle_list(
    client: &mut TaskApiClient,
    status: Option<&str>,
    page: u32,
    limit: u32,
    agent_id: &str,
) -> Result<()> {
    let agent_id = resolve_agent_id(client, None, agent_id).await;
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

/// 生成付款单（Provider 在 provider_applied 后发送给买家）
pub async fn handle_payment(client: &mut TaskApiClient, job_id: &str, agent_id: &str) -> Result<()> {
    let agent_id = resolve_agent_id(client, Some(job_id), agent_id).await;
    let resp = client.get_with_identity(&client.task_path(job_id), &agent_id).await?;

    let task = &resp;
    let amount = task["tokenAmount"].as_str().unwrap_or("?");
    let token_symbol = task["paymentTokenSymbol"].as_str().unwrap_or("USDT");
    let provider_addr = task["providerAgentAddress"].as_str().unwrap_or("?");
    let payment_type = task["paymentType"].as_i64().unwrap_or(0);
    let payment_mode = crate::commands::agent_commerce::task::common::payment_mode_to_str(payment_type as i32);

    println!("付款单（Invoice）");
    println!("  jobId:     {job_id}");
    println!("  金额:      {amount} {token_symbol}");
    println!("  支付代币:   {token_symbol}（XLayer）");
    println!("  收款地址:   {provider_addr}");
    println!("  支付方式:   {payment_mode}");
    println!("  链:        xlayer (chainId={})", XLAYER_CHAIN_ID);
    Ok(())
}

/// 非担保模式手动转账（展示转账命令）
pub async fn handle_pay(client: &mut TaskApiClient, job_id: &str, agent_id: &str) -> Result<()> {
    let agent_id = resolve_agent_id(client, Some(job_id), agent_id).await;
    let resp = client.get_with_identity(&client.task_path(job_id), &agent_id).await?;

    let task = &resp;
    let status = task["statusStr"].as_str().unwrap_or("");
    if status != "complete" {
        bail!("任务状态为 {status}，仅 complete 状态可执行 pay");
    }

    let provider_addr = task["providerAgentAddress"]
        .as_str()
        .ok_or_else(|| anyhow::anyhow!("任务详情缺少 providerAgentAddress"))?;
    let amount = task["tokenAmount"]
        .as_str()
        .ok_or_else(|| anyhow::anyhow!("任务详情缺少 tokenAmount"))?;
    let token_symbol = task["paymentTokenSymbol"]
        .as_str()
        .unwrap_or("USDT");
    let token_address = task["tokenAddress"]
        .as_str()
        .unwrap_or("");

    println!("非担保任务付款信息：");
    println!("  Provider: {provider_addr}");
    println!("  金额:     {amount} {token_symbol}");
    println!("  链:       xlayer (chainId={})", XLAYER_CHAIN_ID);
    println!();
    println!("请执行以下命令完成转账：");
    if token_address.is_empty() {
        println!("  onchainos wallet send --readable-amount {amount} --recipient {provider_addr} --chain xlayer");
    } else {
        println!("  onchainos wallet send --readable-amount {amount} --recipient {provider_addr} --chain xlayer --contract-token {token_address}");
    }
    Ok(())
}
