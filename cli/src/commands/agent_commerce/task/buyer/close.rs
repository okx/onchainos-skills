//! 关单
//!
//! 买家动作：关单 — onchainos agent close
//! 附带：领取仲裁奖金 (onchainos agent arbitration-claim)

use anyhow::Result;

use crate::commands::agent_commerce::task::common::network::task_api_client::TaskApiClient;
use crate::commands::agent_commerce::task::signing;

/// close — 关闭任务
pub async fn handle_close(client: &mut TaskApiClient, job_id: &str, explicit_agent_id: Option<&str>) -> Result<()> {
    let (account_id, address, agent_id) =
        signing::resolve_wallet_and_agent_for_task(client, job_id, explicit_agent_id).await?;

    let resp = client.post_with_identity(
        &client.endpoint(job_id, "close"),
        &serde_json::json!({}),
        &agent_id,
    ).await?;

    let tx_hash = signing::sign_uop_and_broadcast(
        client, &resp["uopData"], &account_id, &address,
        job_id, signing::extract_biz_type(&resp), &agent_id,
    ).await?;

    println!("✓ 任务已关闭，状态 → close");
    println!("  txHash: {tx_hash}");
    Ok(())
}
