//! 仲裁者领取奖励（account 级 pull，一次到账）— onchainos agent evaluator claim

use anyhow::Result;

use crate::commands::agent_commerce::task::common::network::task_api_client::TaskApiClient;
use crate::commands::agent_commerce::task::signing;

/// Account-level pull claim: one call drains all pending rewards across every settled dispute.
///
/// API: `POST /priapi/v1/aieco/task/claim` with empty body. Returns `claimRewards()`
/// calldata — no per-token / per-job arguments. Not scoped to a single jobId.
pub async fn handle_claim(client: &mut TaskApiClient) -> Result<()> {
    let (account_id, address, agent_id) =
        signing::resolve_wallet_and_agent_for_evaluator().await?;

    let path = "/priapi/v1/aieco/task/claim";
    let resp = client.post_with_identity(
        path,
        &serde_json::json!({}),
        &agent_id,
    ).await?;

    let tx_hash = signing::sign_uop_and_broadcast(
        client, &resp["uopData"], &account_id, &address,
        "", signing::BizContext::ClaimRewards, &agent_id,
    ).await?;

    println!("reward claim submitted (account={address})");
    println!("  txHash:   {tx_hash}");
    println!("note: 一次性领取所有已结算争议的奖励，到账金额会在链上确认后通知。");
    Ok(())
}
