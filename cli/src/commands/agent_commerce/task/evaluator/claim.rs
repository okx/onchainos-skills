use anyhow::Result;

use super::helpers::evaluator_agent_id;
use crate::commands::agent_commerce::task::common::network::task_api_client::TaskApiClient;
use crate::commands::agent_commerce::task::signing;

/// E3: claim reward after task/dispute resolved (evaluator side).
pub async fn handle_claim(client: &mut TaskApiClient, job_id: &str) -> Result<()> {
    let (account_id, address) = signing::resolve_wallet(None, None)?;
    let agent_id = evaluator_agent_id();

    let path = client.endpoint(job_id, "claim");
    let resp = client.post_with_identity(
        &path,
        &serde_json::json!({ "jobId": job_id }),
        &agent_id,
        &address,
    ).await?;

    let tx_hash = signing::sign_uop_and_broadcast(
        client, &resp["uopData"], &account_id, &address,
        job_id, signing::BizContext::ClaimRewards,
    ).await?;

    println!("reward claimed (jobId={job_id})");
    if let Some(amt) = resp["amount"].as_str() {
        println!("  amount:   {amt} {}", resp["currency"].as_str().unwrap_or("USDT"));
    }
    println!("  txHash:   {tx_hash}");
    Ok(())
}
