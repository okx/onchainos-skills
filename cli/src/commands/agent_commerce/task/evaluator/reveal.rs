use anyhow::{bail, Result};

use super::helpers::{evaluator_agent_id, parse_job_id};
use crate::commands::agent_commerce::task::common::network::task_api_client::TaskApiClient;
use crate::commands::agent_commerce::task::signing;
use crate::commands::Context;

/// E2b: reveal a previously-committed vote. Backend reads the stored {vote, salt}
/// and returns revealVote(vote, salt) calldata (uopData). No local cache.
pub async fn run_reveal(dispute_id: String, _ctx: &Context) -> Result<()> {
    let job_id = parse_job_id(&dispute_id)?;
    let (account_id, address) = signing::resolve_wallet(None, None)?;
    let agent_id = evaluator_agent_id();
    let client = TaskApiClient::new();

    // Optional pre-check; mock returns canReveal=true once committed.
    let pre_url = format!("{}?voter={}", client.endpoint(&job_id, "vote/canReveal"), address);
    let pre = client.get_with_identity(&pre_url, &agent_id, &address).await?;
    if pre["data"]["canReveal"] == false {
        let why = pre["data"]["reason"].as_str().unwrap_or("not ready");
        bail!("cannot reveal yet: {why}");
    }

    let resp = client.post_with_identity(
        &client.endpoint(&job_id, "vote/reveal"),
        &serde_json::json!({}),
        &agent_id,
        &address,
    ).await?;

    let tx_hash = signing::sign_uop_and_broadcast(
        &client, &resp["data"]["uopData"], &account_id, &address,
        signing::BizContext::VoteReveal,
    ).await?;

    let d = &resp["data"];
    println!("vote revealed (disputeId={dispute_id})");
    if let Some(v) = d["revealedVote"].as_u64() {
        let label = if v == 1 { "Provider wins" } else { "Client wins" };
        println!("  revealedVote: {v} ({label})");
    }
    println!("  txHash:       {tx_hash}");
    if d["settled"] == true {
        if let Some(w) = d["winner"].as_str() {
            println!("  settled:      yes ({w} wins)");
        }
        if let Some(v) = d["verdict"].as_str() {
            println!("  verdict:      {v}");
        }
    } else {
        println!("  settled:      no (waiting for other voters)");
    }
    Ok(())
}
