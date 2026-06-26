//! Fetch recommended tasks (provider-initiated discovery of Public tasks).
//!
//! Maps to backend `POST /priapi/v1/aieco/task/job/match`.
//! Identity is taken from the X-Agent-Id / X-Wallet-Address headers; no request body.
//! Backend matches a list of relevant Public tasks against the provider's skill description.

use anyhow::{bail, Result};

use crate::commands::agent_commerce::task::common::network::task_api_client::TaskApiClient;
use crate::commands::agent_commerce::task::signing;

pub async fn handle_recommend_task(client: &mut TaskApiClient, agent_id: &str) -> Result<()> {
    if agent_id.is_empty() {
        bail!("--agent-id is required (pass the provider's own agentId; beta backend rejects empty agenticId header)");
    }
    let _ = signing::resolve_wallet(None, None)?;

    let resp = client
        .post_with_identity("/priapi/v1/aieco/task/job/match", &serde_json::json!({}), agent_id)
        .await?;

    let tasks = resp["tasks"].as_array().cloned().unwrap_or_default();

    if tasks.is_empty() {
        println!("[Agent {agent_id}] No matching tasks");
        return Ok(());
    }

    println!("[Agent {agent_id}] Matched {} Public task(s):\n", tasks.len());
    for (i, t) in tasks.iter().enumerate() {
        let token_amount = t["tokenAmount"].as_str().unwrap_or("?");
        let token_addr = t["tokenAddress"].as_str().unwrap_or("");
        let min_credit = t["minCreditScore"].as_f64().unwrap_or(0.0);
        println!("  {}. jobId: {}", i + 1, t["jobId"].as_str().unwrap_or("?"));
        println!("     Title:      {}", t["title"].as_str().unwrap_or("?"));
        println!("     Description: {}", t["description"].as_str().unwrap_or("?"));
        println!("     Budget:     {token_amount} (token: {token_addr})");
        println!("     Min credit: {min_credit}");
        println!("     Created:    {}", t["createTime"].as_str().unwrap_or("?"));
        println!();
    }
    Ok(())
}
