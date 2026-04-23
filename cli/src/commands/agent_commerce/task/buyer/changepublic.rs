//! 设为 Public
//!
//! 买家动作：设为 Public — onchainos task set-public

use anyhow::Result;

use crate::commands::agent_commerce::task::common::network::task_api_client::TaskApiClient;
use crate::commands::agent_commerce::task::signing;

/// set-public — 转为公开任务
pub async fn handle_set_public(client: &TaskApiClient, job_id: &str) -> Result<()> {
    let (account_id, address, agent_id) =
        signing::resolve_wallet_and_agent_for_task(client, job_id).await?;

    let resp = client.post_with_identity(
        &client.endpoint(job_id, "setVisibility"),
        &serde_json::json!({"visibility": 1}),
        &agent_id,
        &address,
    ).await?;

    let tx_hash = signing::sign_uop_and_broadcast(
        client, &resp["data"]["uopData"], &account_id, &address,
        job_id, signing::BizContext::TaskCreate,
    ).await?;

    println!("✓ 任务已转为公开，其他卖家可以看到并报名");
    println!("  txHash: {tx_hash}");
    Ok(())
}
