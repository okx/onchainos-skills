//! 提交证据（双方）— onchainos agent dispute evidence <jobId> --summary "..."

use anyhow::Result;

use crate::commands::agent_commerce::task::common::network::task_api_client::TaskApiClient;
use crate::commands::agent_commerce::task::signing;

pub async fn handle_dispute_evidence(
    client: &mut TaskApiClient,
    job_id: &str,
    summary: &str,
) -> Result<()> {
    let (account_id, address, agent_id) =
        signing::resolve_wallet_and_agent_for_provider(client, job_id).await?;
    let body = serde_json::json!({ "text": summary });

    let resp = client.post_with_identity(
        &client.endpoint(job_id, "evidence"), &body, &agent_id,
    ).await?;

    let tx_hash = signing::sign_uop_and_broadcast(
        client, &resp["uopData"], &account_id, &address,
        job_id, signing::BizContext::DisputeCreate,
    ).await?;

    println!("✓ 证据已提交");
    println!("  jobId:  {job_id}");
    println!("  摘要:   {summary}");
    println!("  txHash: {tx_hash}");
    println!();
    println!("⚠️  证据提交后无需主动通知对方：");
    println!("    - 禁止立即调 `xmtp_send` 告诉对方 \"证据已提交\"");
    println!("    - 仲裁者会自动拉取链上证据进行裁决，无需通过 XMTP 提醒");
    Ok(())
}
