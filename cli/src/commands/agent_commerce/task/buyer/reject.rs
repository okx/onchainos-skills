//! Reject deliverable.
//!
//! User action: reject deliverable — `onchainos agent reject`.
//!
//! Flow: `pre-refuse(orderId, deadline)` → sign digest → `refuse(signatureData + reason)` → sign uopHash → broadcast.

use anyhow::Result;
use std::time::Duration;

use crate::audit;
use crate::commands::agent_commerce::task::common::network::task_api_client::TaskApiClient;
use crate::commands::agent_commerce::task::signing;

/// reject/refuse — reject review.
pub async fn handle_reject(
    client: &mut TaskApiClient,
    job_id: &str,
    reason: &str,
) -> Result<()> {
    let (account_id, address, agent_id) =
        signing::resolve_wallet_and_agent_for_task(client, job_id, None).await?;

    let result = signing::task_dual_sign_and_broadcast(
        client, job_id, "pre-refuse", "refuse",
        Some(&serde_json::json!({ "reason": reason })),
        &account_id, &address, &agent_id,
    ).await?;

    audit::log(
        "cli",
        "buyer/reject_submitted",
        true,
        Duration::default(),
        Some(vec![
            format!("jobId={job_id}"),
            format!("agentId={agent_id}"),
            format!("reasonLen={}", reason.chars().count()),
            format!("txHash={}", result.tx_hash),
        ]),
        None,
    );

    println!("✓ Review rejected (reason: {reason}); status → rejected.");
    println!("  The provider has 24 hours to file for arbitration.");
    println!("  txHash: {}", result.tx_hash);
    Ok(())
}
