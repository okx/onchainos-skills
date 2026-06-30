//! Timeout auto-refund.
//!
//! User action: provider failed to submit a deliverable in time, or the provider's
//! arbitration timed out after the user rejected → claim the auto-refund —
//! `onchainos agent claim-auto-refund`.
//!
//! Response: `{ jobId, type: 23, uopData: { ... } }`

use anyhow::Result;
use std::time::Duration;

use crate::audit;
use crate::commands::agent_commerce::task::common::network::task_api_client::TaskApiClient;
use crate::commands::agent_commerce::task::signing;

/// claimAutoRefund — timeout auto-refund.
pub async fn handle_claim_auto_refund(client: &mut TaskApiClient, job_id: &str) -> Result<()> {
    let (account_id, address, agent_id) =
        signing::resolve_wallet_and_agent_for_task(client, job_id, None).await?;

    let resp = client.post_with_identity(
        &client.endpoint(job_id, "claimAutoRefund"),
        &serde_json::json!({}),
        &agent_id,
    ).await?;

    let tx_hash = signing::sign_uop_and_broadcast(
        client, &resp["uopData"], &account_id, &address,
        job_id, signing::extract_biz_type(&resp), &agent_id,
        None,
    ).await?;

    audit::log(
        "cli",
        "user/auto_refund_claimed",
        true,
        Duration::default(),
        Some(vec![
            format!("jobId={job_id}"),
            format!("agentId={agent_id}"),
            format!("txHash={tx_hash}"),
        ]),
        None,
    );

    println!("✓ Timeout auto-refund claimed; funds will return to the account.");
    println!("  jobId:  {job_id}");
    println!("  txHash: {tx_hash}");
    Ok(())
}
