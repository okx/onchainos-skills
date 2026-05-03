//! 仲裁者领取奖励（account 级 pull，一次到账）— onchainos agent evaluator claim
//!
//! 实际 API 调用 + 签名 + broadcast 在 `task::common::claim`（角色无关，
//! buyer / provider 未来接入时复用）。本文件只保留 evaluator 视角的 wallet/agent
//! 解析与提示文案。

use anyhow::Result;

use crate::commands::agent_commerce::task::common::{
    claim as common_claim, network::task_api_client::TaskApiClient,
};
use crate::commands::agent_commerce::task::signing;

/// Account-level pull claim: one call drains all pending rewards across every settled dispute.
///
/// API: `POST /priapi/v1/aieco/task/claim` with empty body. Returns `claimRewards()`
/// calldata — no per-token / per-job arguments. Not scoped to a single jobId.
pub async fn handle_claim(
    client: &mut TaskApiClient,
    agent_id_hint: Option<&str>,
) -> Result<()> {
    let (account_id, address, agent_id) =
        signing::resolve_wallet_and_agent_for_evaluator(agent_id_hint).await?;

    let tx_hash =
        common_claim::submit_claim_and_broadcast(client, &account_id, &address, &agent_id).await?;

    println!("reward claim submitted (account={address})");
    println!("  txHash:   {tx_hash}");
    println!("note: 一次性领取所有已结算争议的奖励，到账金额会在链上确认后通知。");
    Ok(())
}
