//! Reject deliverable.
//!
//! User action: reject deliverable — `onchainos agent reject`.
//!
//! Flow: `pre-reject(orderId, deadline)` → sign digest → `reject(signatureData + reason)` → sign uopHash → broadcast.

use anyhow::{bail, Result};
use std::time::Duration;

use crate::audit;
use crate::commands::agent_commerce::task::common::network::task_api_client::TaskApiClient;
use crate::commands::agent_commerce::task::signing;

const MAX_REASON_CHARS: usize = 2000;

/// reject — reject review.
pub async fn handle_reject(
    client: &mut TaskApiClient,
    job_id: &str,
    reason: &str,
) -> Result<()> {
    if reason.chars().count() > MAX_REASON_CHARS {
        bail!("Reject reason exceeds {MAX_REASON_CHARS} characters. Please shorten it and try again.");
    }

    let (account_id, address, agent_id) =
        signing::resolve_wallet_and_agent_for_task(client, job_id, None).await?;

    let reason_json = serde_json::json!({ "reason": reason });
    let result = signing::task_dual_sign_and_broadcast(
        client, job_id, "pre-reject", "reject",
        None,
        &account_id, &address, &agent_id,
        Some(&reason_json),
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
    println!("  The provider may file for arbitration or agree to a refund.");
    println!("  txHash: {}", result.tx_hash);
    Ok(())
}
