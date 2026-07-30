//! Reject deliverable (unified for regular and subscription tasks).
//!
//! User action: reject deliverable — `onchainos agent reject`.
//!
//! Internally queries task detail to determine jobType:
//! - jobType == 1 (subscription) → delegates to `subscription_ops::handle_subscribe_reject`
//! - otherwise (regular)         → pre-reject → reject dual-sign flow

use anyhow::{bail, Result};
use std::time::Duration;

use crate::audit;
use crate::commands::agent_commerce::task::common::network::task_api_client::TaskApiClient;
use crate::commands::agent_commerce::task::signing;

const MAX_REASON_CHARS: usize = 2000;
const JOB_TYPE_SUBSCRIBE: i64 = 1;

/// reject — unified reject (auto-detects subscription vs regular).
pub async fn handle_reject(
    client: &mut TaskApiClient,
    job_id: &str,
    reason: &str,
) -> Result<()> {
    if reason.is_empty() {
        bail!("--reason is required for reject");
    }
    if reason.chars().count() > MAX_REASON_CHARS {
        bail!("Reject reason exceeds {MAX_REASON_CHARS} characters. Please shorten it and try again.");
    }

    let (local_agent_id, _) = super::create::resolve_user_agent().await?;
    let task_resp = client.get_with_identity(&client.task_path(job_id), &local_agent_id).await?;

    let job_type = task_resp["jobType"].as_i64()
        .or_else(|| task_resp["jobType"].as_str().and_then(|s| s.parse().ok()))
        .unwrap_or(0);

    if job_type == JOB_TYPE_SUBSCRIBE {
        return super::subscription_ops::handle_subscribe_reject_inner(
            client, job_id, reason, &local_agent_id,
        ).await;
    }

    // Regular task: pre-reject → reject dual-sign flow
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
        "user/reject_submitted",
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
