//! 仲裁者 commit 投票（commit-reveal 第一阶段）— onchainos agent evaluator commit

use anyhow::{bail, Result};

use super::helpers::parse_job_id;
use crate::commands::agent_commerce::task::common::network::task_api_client::TaskApiClient;
use crate::commands::agent_commerce::task::signing;

/// E2a: commit a vote. Backend generates salt, stores it in task_dispute_voter keyed by voter,
/// computes commitHash = keccak256(disputeId, vote, salt), and returns commitVote() calldata.
/// Vote semantics per backend: 1 = Approve (Provider wins), 2 = Reject (Client wins).
///
/// Request body is strictly `{ vote }` per real API spec (§11175). The evaluator's rationale
/// is NOT part of this API — it lives in agent thinking / session memory only (per evaluator.md
/// §3.7: judgments are never pushed to the user; users perceive the result only via later
/// `reward_claimed` / `slashed` events). Not persisted to backend, not surfaced via xmtp.
///
/// No local persistence: reveal is driven by the `reveal_started` system event whose
/// envelope carries `disputeId`, and backend reads vote+salt from `task_dispute_voter`
/// at reveal time — neither side nor any other commit metadata needs client-side storage.
pub async fn handle_commit(
    client: &mut TaskApiClient,
    dispute_id: &str,
    side: u8,
    agent_id_hint: Option<&str>,
) -> Result<()> {
    if side != 1 && side != 2 {
        bail!("--side must be 1 (provider wins) or 2 (client wins)");
    }
    let job_id = parse_job_id(dispute_id)?;
    let (account_id, address, agent_id) =
        signing::resolve_wallet_and_agent_for_evaluator(agent_id_hint).await?;

    let body = serde_json::json!({ "vote": side });
    let path = client.endpoint(&job_id, "vote/commit");
    let resp = client.post_with_identity(
        &path,
        &body,
        &agent_id,
    ).await?;

    let tx_hash = signing::sign_uop_and_broadcast(
        client, &resp["uopData"], &account_id, &address,
        &job_id, signing::BizContext::VoteCommit, &agent_id,
    ).await?;

    let side_label = if side == 1 { "Provider wins (Approve)" } else { "Client wins (Reject)" };
    let commit_hash = resp["commitHash"].as_str().unwrap_or("");

    println!("vote committed (disputeId={dispute_id})");
    println!("  side:       {side} ({side_label})");
    println!("  voter:      {address}");
    if !commit_hash.is_empty() {
        println!("  commitHash: {commit_hash}");
    }
    println!("  txHash:     {tx_hash}");
    println!(
        "next: on reveal_started run `onchainos agent evaluator reveal <disputeId>` \
         (no --side; backend reads vote+salt from task_dispute_voter)"
    );
    Ok(())
}
