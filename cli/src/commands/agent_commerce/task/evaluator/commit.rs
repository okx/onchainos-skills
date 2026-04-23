use anyhow::{bail, Result};

use super::helpers::{evaluator_agent_id, parse_job_id};
use crate::commands::agent_commerce::task::common::network::task_api_client::TaskApiClient;
use crate::commands::agent_commerce::task::signing;
use crate::commands::Context;

/// E2a: commit a vote. Backend stores {vote, salt}; returns commitVote() calldata (uopData).
/// Vote semantics per backend: 1 = Approve (Provider wins), 2 = Reject (Client wins).
pub async fn run_commit(dispute_id: String, side: u8, reason: String, _ctx: &Context) -> Result<()> {
    if side != 1 && side != 2 {
        bail!("--side must be 1 (provider wins) or 2 (client wins)");
    }
    if reason.trim().is_empty() {
        bail!("--reason is required and must be specific");
    }
    let job_id = parse_job_id(&dispute_id)?;
    let (account_id, address) = signing::resolve_wallet(None, None)?;
    let agent_id = evaluator_agent_id();
    let client = TaskApiClient::new();

    let body = serde_json::json!({ "vote": side, "reason": reason });
    let resp = client.post_with_identity(
        &client.endpoint(&job_id, "vote/commit"),
        &body,
        &agent_id,
        &address,
    ).await?;

    let tx_hash = signing::sign_uop_and_broadcast(
        &client, &resp["data"]["uopData"], &account_id, &address,
        signing::BizContext::VoteCommit,
    ).await?;

    let d = &resp["data"];
    let side_label = if side == 1 { "Provider wins" } else { "Client wins" };
    println!("vote committed (disputeId={dispute_id})");
    println!("  side:       {side} ({side_label})");
    println!("  voter:      {address}");
    if let Some(h) = d["commitHash"].as_str() {
        println!("  commitHash: {h}");
    }
    println!("  txHash:     {tx_hash}");
    println!("next: wait for reveal window, then run `onchainos agent evaluator reveal {dispute_id}`");
    Ok(())
}
