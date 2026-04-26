//! 超时自动退款
//!
//! 买家动作：卖家未提交交付物超时 / 买家拒绝后卖家仲裁超时
//! → 领取自动退款 — onchainos agent claim-auto-refund

use anyhow::Result;

use crate::commands::agent_commerce::task::common::network::task_api_client::TaskApiClient;
use crate::commands::agent_commerce::task::signing;

/// claimAutoRefund — 超时自动退款
pub async fn handle_claim_auto_refund(client: &mut TaskApiClient, job_id: &str) -> Result<()> {
    let (account_id, address, agent_id) =
        signing::resolve_wallet_and_agent_for_task(client, job_id).await?;

    let resp = client.post_with_identity(
        &client.endpoint(job_id, "claimAutoRefund"),
        &serde_json::json!({}),
        &agent_id,
    ).await?;

    let tx_hash = signing::sign_uop_and_broadcast(
        client, &resp["uopData"], &account_id, &address,
        job_id, signing::BizContext::ClaimRewards,
    ).await?;

    println!("✓ 超时自动退款已领取，资金将退回账户");
    println!("  txHash: {tx_hash}");
    Ok(())
}
