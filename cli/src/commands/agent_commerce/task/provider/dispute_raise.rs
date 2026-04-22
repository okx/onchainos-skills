//! 发起仲裁（卖家）— onchainos agent dispute raise <jobId> --reason "..."

use anyhow::Result;

use crate::commands::agent_commerce::task::common::network::task_api_client::TaskApiClient;
use crate::commands::agent_commerce::task::signing;

pub async fn handle_dispute_raise(
    client: &TaskApiClient,
    job_id: &str,
    reason: &str,
) -> Result<()> {
    let (account_id, address, agent_id) =
        signing::resolve_wallet_and_agent_for_provider(client.http(), client.base_url(), job_id).await?;
    let body = serde_json::json!({ "reason": reason });

    let resp = client.post_with_identity(
        &client.endpoint(job_id, "dispute"), &body, &agent_id, &address,
    ).await?;

    let tx_hash = signing::sign_uop_and_broadcast(
        client, &resp["data"]["uopData"], &account_id, &address,
        signing::BizContext::DisputeCreate,
    ).await?;

    println!("✓ 已发起仲裁，等待链上确认（TASK_DISPUTED）");
    println!("  原因: {reason}");
    println!("  txHash: {tx_hash}");
    Ok(())
}
