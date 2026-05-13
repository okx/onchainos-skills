//! 只读查询命令（无链上签名）— buyer 专用
//!
//! payment

use anyhow::Result;

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
    let payment_mode = crate::commands::agent_commerce::task::common::PaymentMode::from_int(payment_mode_int as i32);
    let payment_mode = payment_mode.as_str();

    println!("付款单（Invoice）");
    println!("  jobId:     {job_id}");
    println!("  金额:      {amount} {token_symbol}");
    println!("  支付代币:   {token_symbol}（XLayer）");
    println!("  收款地址:   {provider_addr}");
    println!("  支付方式:   {payment_mode}");
    println!("  链:        xlayer (chainId={})", XLAYER_CHAIN_ID);
    Ok(())
}

