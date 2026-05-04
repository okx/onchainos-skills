//! 仲裁者 commit 投票（commit-reveal 第一阶段）— onchainos agent evaluator commit

use anyhow::{bail, Result};

use super::helpers::parse_job_id;
use crate::commands::agent_commerce::task::common::network::task_api_client::TaskApiClient;
use crate::commands::agent_commerce::task::signing;

/// E2a: commit a vote. Backend generates salt, stores it in task_dispute_voter keyed by voter,
/// computes commitHash = keccak256(disputeId, vote, salt), and returns commitVote() calldata.
/// Vote semantics per backend: 0 = Approve (Client wins), 1 = Reject (Provider wins).
///
/// Request body is strictly `{ vote }` per real API spec. The evaluator's rationale
/// is NOT part of this API — it lives in agent thinking / session memory only (per evaluator.md
/// references/evaluator-decision-rubric.md 7: judgments are never pushed to the user; users perceive the result only via later
/// `reward_claimed` / `slashed` events). Not persisted to backend, not surfaced via xmtp.
///
/// No local persistence: reveal is driven by the `reveal_started` system event whose
/// envelope carries `disputeId`, and backend reads vote+salt from `task_dispute_voter`
/// at reveal time — neither vote nor any other commit metadata needs client-side storage.
pub async fn handle_commit(
    client: &mut TaskApiClient,
    dispute_id: &str,
    vote: u8,
    agent_id_hint: Option<&str>,
) -> Result<()> {
    if vote != 0 && vote != 1 {
        bail!("--vote must be 0 (Approve, Client wins) or 1 (Reject, Provider wins)");
    }
    let job_id = parse_job_id(dispute_id)?;
    let (account_id, address, agent_id) =
        signing::resolve_wallet_and_agent_for_evaluator(agent_id_hint).await?;

    let body = serde_json::json!({ "vote": vote });
    let path = client.endpoint(&job_id, "vote/commit");
    let resp = client.post_with_identity(
        &path,
        &body,
        &agent_id,
    ).await?;

    // 后端 commit 响应里返回 `salt` / `commitSalt` 与 `commitHash`，broadcast bizContext
    // 必须把 commitSalt + vote 一起带上，链上重算 keccak256(disputeId, vote, salt) 才匹配。
    let salt = resp["salt"].as_str()
        .unwrap_or("");
    if salt.is_empty() {
        bail!("后端未返回 salt，无法广播 vote/commit");
    }
    let commit_hash = resp["commitHash"].as_str().unwrap_or("");

    let tx_hash = signing::sign_uop_and_broadcast_with_commit_meta(
        client, &resp["uopData"], &account_id, &address,
        &job_id, signing::BizContext::VoteCommit, &agent_id,
        salt, vote,
    ).await?;

    let vote_label = if vote == 0 { "Approve (Client wins)" } else { "Reject (Provider wins)" };

    println!("vote committed (disputeId={dispute_id})");
    println!("  vote:       {vote} ({vote_label})");
    println!("  voter:      {address}");
    if !commit_hash.is_empty() {
        println!("  commitHash: {commit_hash}");
    }
    println!("  txHash:     {tx_hash}");
    println!(
        "next: on reveal_started run `onchainos agent evaluator reveal <disputeId>` \
         (no --vote; backend reads vote+salt from task_dispute_voter)"
    );
    Ok(())
}
