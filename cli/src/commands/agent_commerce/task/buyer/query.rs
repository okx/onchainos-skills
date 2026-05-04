//! 只读查询命令（无链上签名）— buyer 专用
//!
//! payment, pay

use anyhow::{bail, Result};

use crate::commands::agent_commerce::task::common::network::task_api_client::TaskApiClient;
use crate::commands::agent_commerce::task::common::{query as common_query, AGENT_ROLE_BUYER, XLAYER_CHAIN_ID};

/// 生成付款单（Provider 在 provider_applied 后发送给买家）
pub async fn handle_payment(client: &mut TaskApiClient, job_id: &str, agent_id: &str) -> Result<()> {
    let agent_id = common_query::resolve_agent_id(agent_id, AGENT_ROLE_BUYER).await;
    let resp = client.get_with_identity(&client.task_path(job_id), &agent_id).await?;

    let task = &resp;
    let amount = task["tokenAmount"].as_str().unwrap_or("?");
    let token_symbol = task["paymentTokenSymbol"].as_str().unwrap_or("USDT");
    let provider_addr = task["providerAgentAddress"].as_str().unwrap_or("?");
    let payment_mode_int = task["paymentMode"].as_i64().unwrap_or(0);
    let payment_mode = crate::commands::agent_commerce::task::common::payment_mode_to_str(payment_mode_int as i32);

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
    let agent_id = common_query::resolve_agent_id(agent_id, AGENT_ROLE_BUYER).await;
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
