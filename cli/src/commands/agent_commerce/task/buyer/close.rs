//! 关单
//!
//! 买家动作：关单 — onchainos task close
//! 附带：领取仲裁奖金 (task claim)

use anyhow::Result;

use crate::commands::agent_commerce::task::common::network::task_api_client::TaskApiClient;
use crate::commands::agent_commerce::task::signing;

/// close — 关闭任务
pub async fn handle_close(client: &TaskApiClient, job_id: &str) -> Result<()> {
    let (account_id, address, agent_id) =
        signing::resolve_wallet_and_agent_for_task(client, job_id).await?;

    let resp = client.post_with_identity(
        &client.endpoint(job_id, "close"),
        &serde_json::json!({}),
        &agent_id,
        &address,
    ).await?;

    let tx_hash = signing::sign_uop_and_broadcast(
        client, &resp["data"]["uopData"], &account_id, &address,
        job_id, signing::BizContext::JobClose,
    ).await?;

    println!("✓ 任务已关闭，状态 → close");
    println!("  txHash: {tx_hash}");
    Ok(())
}

/// claim — 仲裁奖金领取
pub async fn handle_claim(client: &TaskApiClient, job_id: &str) -> Result<()> {
    let (account_id, address, agent_id) =
        signing::resolve_wallet_and_agent_for_task(client, job_id).await?;

    let resp = client.post_with_identity(
        &client.endpoint(job_id, "claim"),
        &serde_json::json!({ "jobId": job_id }),
        &agent_id,
        &address,
    ).await?;

    let tx_hash = signing::sign_uop_and_broadcast(
        client, &resp["data"]["uopData"], &account_id, &address,
        job_id, signing::BizContext::ClaimRewards,
    ).await?;

    println!("✓ 仲裁奖金已领取");
    println!("  txHash: {tx_hash}");
    Ok(())
}
