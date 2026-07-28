//! Timeout auto-refund (unified for regular and subscription tasks).
//!
//! User action: provider failed to submit a deliverable in time, or the provider's
//! arbitration timed out after the user rejected → claim the auto-refund —
//! `onchainos agent claim-auto-refund`.
//!
//! Internally queries task detail to determine jobType:
//! - jobType == 1 (subscription) → POST /subscribe/{jobId}/claimAutoRefund
//! - otherwise (regular)         → POST /task/job/{jobId}/claimAutoRefund

use anyhow::Result;
use std::time::Duration;

use crate::audit;
use crate::commands::agent_commerce::task::common::network::task_api_client::TaskApiClient;
use crate::commands::agent_commerce::task::signing;
use crate::commands::agent_commerce::task::user::create_subscribe::SUBSCRIBE_API_PREFIX;

const JOB_TYPE_SUBSCRIBE: i64 = 1;

/// claimAutoRefund — timeout auto-refund (auto-detects subscription vs regular).
pub async fn handle_claim_auto_refund(client: &mut TaskApiClient, job_id: &str) -> Result<()> {
    let (local_agent_id, _) = super::create::resolve_user_agent().await?;

    let task_resp = client.get_with_identity(&client.task_path(job_id), &local_agent_id).await?;

    let user_address = task_resp["buyerAgentAddress"]
        .as_str()
        .ok_or_else(|| anyhow::anyhow!("task detail missing buyerAgentAddress field"))?;
    let agent_id = task_resp["buyerAgentId"]
        .as_str()
        .unwrap_or("")
        .to_string();
    let (account_id, address) = signing::resolve_wallet(None, Some(user_address))?;

    let job_type = task_resp["jobType"].as_i64()
        .or_else(|| task_resp["jobType"].as_str().and_then(|s| s.parse().ok()))
        .unwrap_or(0);
    let is_subscription = job_type == JOB_TYPE_SUBSCRIBE;

    let endpoint = if is_subscription {
        format!("{SUBSCRIBE_API_PREFIX}/{job_id}/claimAutoRefund")
    } else {
        client.endpoint(job_id, "claimAutoRefund")
    };

    let resp = client.post_with_identity(
        &endpoint,
        &serde_json::json!({}),
        &agent_id,
    ).await?;

    let tx_hash = signing::sign_uop_and_broadcast(
        client, &resp["uopData"], &account_id, &address,
        job_id, signing::extract_biz_type(&resp), &agent_id,
        None,
    ).await?;

    let audit_action = if is_subscription {
        "user/subscribe_claim_refund"
    } else {
        "user/auto_refund_claimed"
    };
    audit::log(
        "cli", audit_action, true, Duration::default(),
        Some(vec![
            format!("jobId={job_id}"),
            format!("agentId={agent_id}"),
            format!("txHash={tx_hash}"),
        ]),
        None,
    );

    let label = if is_subscription { "subscription" } else { "task" };
    println!("✓ Timeout auto-refund claimed for {label}; funds will return to the account.");
    println!("  jobId:  {job_id}");
    println!("  txHash: {tx_hash}");
    Ok(())
}
