//! User terms changes (only in the Open state).
//!
//! - `set-token-and-budget` — change payment token and amount (on-chain, wait for `task_token_budget_change`).
//! - `set-provider`         — change provider (on-chain; do NOT wait for confirmation, continue immediately).
//! - `set-max-budget`       — change max budget (off-chain; succeeds when the API call returns).

use anyhow::Result;
use std::time::Duration;

use crate::audit;
use crate::commands::agent_commerce::task::common::network::task_api_client::TaskApiClient;
use crate::commands::agent_commerce::task::signing;

/// set-token-and-budget — change payment token and amount.
///
/// POST /priapi/v1/aieco/task/{jobId}/setTokenAndBudget
/// Request:  { "tokenSymbol": "<symbol>", "budget": "<human-readable amount>", "sessionCert": "..." }
/// Response: { code: "0", data: { jobId, type: 27, uopData } } → sign and broadcast.
pub async fn handle_set_token_and_budget(
    client: &mut TaskApiClient,
    job_id: &str,
    token_symbol: &str,
    budget: &str,
    explicit_agent_id: Option<&str>,
) -> Result<()> {
    let (account_id, address, agent_id) =
        signing::resolve_wallet_and_agent_for_task(client, job_id, explicit_agent_id).await?;

    let resp = client.post_with_identity(
        &client.endpoint(job_id, "setTokenAndBudget"),
        &serde_json::json!({
            "tokenSymbol": token_symbol,
            "budget": budget,
        }),
        &agent_id,
    ).await?;

    let tx_hash = signing::sign_uop_and_broadcast(
        client, &resp["uopData"], &account_id, &address,
        job_id, signing::extract_biz_type(&resp), &agent_id,
        None,
    ).await?;

    audit::log(
        "cli",
        "buyer/token_budget_change_submitted",
        true,
        Duration::default(),
        Some(vec![
            format!("jobId={job_id}"),
            format!("agentId={agent_id}"),
            format!("tokenSymbol={token_symbol}"),
            format!("budget={budget}"),
            format!("txHash={tx_hash}"),
        ]),
        None,
    );

    println!("✓ Payment terms change submitted; awaiting on-chain confirmation (task_token_budget_change).");
    println!("  token: {token_symbol}, budget: {budget}");
    println!("  txHash: {tx_hash}");
    Ok(())
}

/// set-provider — change the provider.
///
/// POST /priapi/v1/aieco/task/{jobId}/setProviderAndAgentId
/// Request:  { "providerAgentId": "<agentId>", "sessionCert": "..." }
/// Response: { code: "0", data: { jobId, type: 28, uopData } } → sign and broadcast.
pub async fn handle_set_provider(
    client: &mut TaskApiClient,
    job_id: &str,
    provider_agent_id: &str,
    explicit_agent_id: Option<&str>,
) -> Result<()> {
    let (account_id, address, agent_id) =
        signing::resolve_wallet_and_agent_for_task(client, job_id, explicit_agent_id).await?;

    let resp = client.post_with_identity(
        &client.endpoint(job_id, "setProviderAndAgentId"),
        &serde_json::json!({
            "providerAgentId": provider_agent_id,
        }),
        &agent_id,
    ).await?;

    let tx_hash = signing::sign_uop_and_broadcast(
        client, &resp["uopData"], &account_id, &address,
        job_id, signing::extract_biz_type(&resp), &agent_id,
        None,
    ).await?;

    super::negotiate::save_designated_provider(job_id, provider_agent_id)?;

    audit::log(
        "cli",
        "buyer/provider_change_submitted",
        true,
        Duration::default(),
        Some(vec![
            format!("jobId={job_id}"),
            format!("agentId={agent_id}"),
            format!("newProvider={provider_agent_id}"),
            format!("txHash={tx_hash}"),
        ]),
        None,
    );

    println!("✓ Provider change submitted (not waiting for on-chain confirmation; starting the new-provider flow immediately).");
    println!("  providerAgentId: {provider_agent_id}");
    println!("  txHash: {tx_hash}");
    println!();
    println!("Next: onchainos agent next-action --jobid {job_id} --event switch_provider --role buyer --agentId {agent_id} --provider {provider_agent_id}");
    Ok(())
}

/// set-max-budget — change the max budget (off-chain).
///
/// POST /priapi/v1/aieco/task/{jobId}/setBudget
/// Request:  { "paymentMostTokenAmount": "<human-readable amount>" }
/// Response: { code: 0, data: null }
pub async fn handle_set_max_budget(
    client: &mut TaskApiClient,
    job_id: &str,
    max_budget: &str,
    explicit_agent_id: Option<&str>,
) -> Result<()> {
    let agent_id = match explicit_agent_id {
        Some(id) => id.to_string(),
        None => {
            let (_, _, id) =
                signing::resolve_wallet_and_agent_for_task(client, job_id, None).await?;
            id
        }
    };

    client.post_with_identity(
        &client.endpoint(job_id, "setBudget"),
        &serde_json::json!({
            "paymentMostTokenAmount": max_budget,
        }),
        &agent_id,
    ).await?;

    audit::log(
        "cli",
        "buyer/max_budget_updated",
        true,
        Duration::default(),
        Some(vec![
            format!("jobId={job_id}"),
            format!("agentId={agent_id}"),
            format!("maxBudget={max_budget}"),
        ]),
        None,
    );

    println!("✓ Max budget updated to {max_budget}.");
    Ok(())
}
