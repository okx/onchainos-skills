//! Read-only query commands (no on-chain signing) — user-only.
//!
//! payment

use anyhow::Result;

use crate::commands::agent_commerce::task::common::network::task_api_client::TaskApiClient;
use crate::commands::agent_commerce::task::common::{query as common_query, AGENT_ROLE_USER, XLAYER_CHAIN_ID};

/// Generate the payment invoice (sent by the provider to the user after `provider_applied`).
pub async fn handle_payment(client: &mut TaskApiClient, job_id: &str, agent_id: &str) -> Result<()> {
    let agent_id = common_query::resolve_agent_id(agent_id, AGENT_ROLE_USER).await;
    let resp = client.get_with_identity(&client.task_path(job_id), &agent_id).await?;

    let task = &resp;
    let amount = task["tokenAmount"].as_str().unwrap_or("?");
    let token_symbol = task["tokenSymbol"].as_str().unwrap_or("?");
    let provider_addr = task["providerAgentAddress"].as_str().unwrap_or("?");
    let payment_mode_int = task["paymentMode"].as_i64().unwrap_or(0);
    let payment_mode = crate::commands::agent_commerce::task::common::PaymentMode::from_int(payment_mode_int as i32);
    let payment_mode = payment_mode.as_str();

    println!("Payment invoice");
    println!("  jobId:        {job_id}");
    println!("  Amount:       {amount} {token_symbol}");
    println!("  Token:        {token_symbol} (XLayer)");
    println!("  Recipient:    {provider_addr}");
    println!("  Payment mode: {payment_mode}");
    println!("  Chain:        xlayer (chainId={})", XLAYER_CHAIN_ID);
    Ok(())
}

