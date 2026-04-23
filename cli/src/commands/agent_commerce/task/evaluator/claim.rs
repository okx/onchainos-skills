use anyhow::Result;

use super::helpers::evaluator_agent_id;
use crate::commands::agent_commerce::task::common::network::task_api_client::TaskApiClient;
use crate::commands::agent_commerce::task::signing;
use crate::commands::Context;

/// E3: claim reward after task/dispute resolved (evaluator side).
pub async fn run_claim(job_id: String, _ctx: &Context) -> Result<()> {
    let (account_id, address) = signing::resolve_wallet(None, None)?;
    let agent_id = evaluator_agent_id();
    let client = TaskApiClient::new();

    let resp = client.post_with_identity(
        &client.endpoint(&job_id, "claim"),
        &serde_json::json!({ "jobId": job_id }),
        &agent_id,
        &address,
    ).await?;

    let tx_hash = signing::sign_uop_and_broadcast(
        &client, &resp["data"]["uopData"], &account_id, &address,
        signing::BizContext::ClaimRewards,
    ).await?;

    let d = &resp["data"];
    println!("reward claimed (jobId={job_id})");
    if let Some(amt) = d["amount"].as_str() {
        println!("  amount:   {amt} {}", d["currency"].as_str().unwrap_or("USDT"));
    }
    println!("  txHash:   {tx_hash}");
    Ok(())
}
