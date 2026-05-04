use anyhow::{bail, Result};

use super::helpers::parse_job_id;
use crate::commands::agent_commerce::task::common::network::task_api_client::TaskApiClient;
use crate::commands::agent_commerce::task::signing;

pub async fn handle_commit(
    client: &mut TaskApiClient,
    dispute_id: &str,
    vote: u8,
    agent_id: &str,
) -> Result<()> {
    if vote != 0 && vote != 1 {
        bail!("--vote must be 0 (Approve, Client wins) or 1 (Reject, Provider wins)");
    }
    let job_id = parse_job_id(dispute_id)?;
    let (account_id, address, agent_id) =
        signing::resolve_wallet_and_agent_for_evaluator(agent_id).await?;

    let body = serde_json::json!({ "vote": vote });
    let path = client.endpoint(&job_id, "vote/commit");
    let resp = client.post_with_identity(
        &path,
        &body,
        &agent_id,
    ).await?;

    // 后端 commit 响应里返回 `salt` 与 `commitHash`，broadcast bizContext
    let salt = resp["salt"].as_str()
        .unwrap_or("");
    if salt.is_empty() {
        bail!("后端未返回 salt，无法广播 vote/commit");
    }
    let commit_hash = resp["commitHash"].as_str().unwrap_or("");

    let tx_hash = signing::sign_uop_and_broadcast_with_commit_meta(
        client, &resp["uopData"], &account_id, &address,
        &job_id, signing::extract_biz_type(&resp), &agent_id,
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
    Ok(())
}
