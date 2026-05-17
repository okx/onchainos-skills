//! 买家条款变更（仅 Open 状态）
//!
//! - `set-token-and-budget` — 修改支付代币及支付金额（上链，等 task_token_budget_change 通知）
//! - `set-provider`         — 修改卖家（上链，不等确认直接继续）
//! - `set-max-budget`       — 修改最高预算（不上链，接口成功即完成）

use anyhow::Result;

use crate::commands::agent_commerce::task::common::network::task_api_client::TaskApiClient;
use crate::commands::agent_commerce::task::signing;

/// set-token-and-budget — 修改支付代币及支付金额
///
/// POST /priapi/v1/aieco/task/{jobId}/setTokenAndBudget
/// Request:  { "tokenSymbol": "<symbol>", "budget": "<human金额>", "sessionCert": "..." }
/// Response: { code: "0", data: { jobId, type: 27, uopData } } → 签名广播
pub async fn handle_set_token_and_budget(
    client: &mut TaskApiClient,
    job_id: &str,
    token_symbol: &str,
    budget: &str,
    explicit_agent_id: Option<&str>,
) -> Result<()> {
    let (account_id, address, agent_id) =
        signing::resolve_wallet_and_agent_for_task(client, job_id, explicit_agent_id).await?;

    let resp = client.post_with_identity(
        &client.endpoint(job_id, "setTokenAndBudget"),
        &serde_json::json!({
            "tokenSymbol": token_symbol,
            "budget": budget,
        }),
        &agent_id,
    ).await?;

    let tx_hash = signing::sign_uop_and_broadcast(
        client, &resp["uopData"], &account_id, &address,
        job_id, signing::extract_biz_type(&resp), &agent_id,
    ).await?;

    println!("✓ 支付条款变更已提交，等待上链确认（task_token_budget_change）");
    println!("  token: {token_symbol}, budget: {budget}");
    println!("  txHash: {tx_hash}");
    Ok(())
}

/// set-provider — 修改卖家
///
/// POST /priapi/v1/aieco/task/{jobId}/setProviderAndAgentId
/// Request:  { "providerAgentId": "<agentId>", "sessionCert": "..." }
/// Response: { code: "0", data: { jobId, type: 28, uopData } } → 签名广播
pub async fn handle_set_provider(
    client: &mut TaskApiClient,
    job_id: &str,
    provider_agent_id: &str,
    explicit_agent_id: Option<&str>,
) -> Result<()> {
    let (account_id, address, agent_id) =
        signing::resolve_wallet_and_agent_for_task(client, job_id, explicit_agent_id).await?;

    let resp = client.post_with_identity(
        &client.endpoint(job_id, "setProviderAndAgentId"),
        &serde_json::json!({
            "providerAgentId": provider_agent_id,
        }),
        &agent_id,
    ).await?;

    let tx_hash = signing::sign_uop_and_broadcast(
        client, &resp["uopData"], &account_id, &address,
        job_id, signing::extract_biz_type(&resp), &agent_id,
    ).await?;

    println!("✓ 卖家变更已提交（不等上链确认，立即启动新卖家流程）");
    println!("  providerAgentId: {provider_agent_id}");
    println!("  txHash: {tx_hash}");
    println!();
    println!("下一步: onchainos agent next-action --jobid {job_id} --jobStatus switch_provider --role buyer --agentId {agent_id} --provider {provider_agent_id}");
    Ok(())
}

/// set-max-budget — 修改最高预算（不上链）
///
/// POST /priapi/v1/aieco/task/{jobId}/setBudget
/// Request:  { "paymentMostTokenAmount": "<human金额>" }
/// Response: { code: 0, data: null }
pub async fn handle_set_max_budget(
    client: &mut TaskApiClient,
    job_id: &str,
    max_budget: &str,
    explicit_agent_id: Option<&str>,
) -> Result<()> {
    let agent_id = match explicit_agent_id {
        Some(id) => id.to_string(),
        None => {
            let (_, _, id) =
                signing::resolve_wallet_and_agent_for_task(client, job_id, None).await?;
            id
        }
    };

    client.post_with_identity(
        &client.endpoint(job_id, "setBudget"),
        &serde_json::json!({
            "paymentMostTokenAmount": max_budget,
        }),
        &agent_id,
    ).await?;

    println!("✓ 最高预算已更新为 {max_budget}");
    Ok(())
}
