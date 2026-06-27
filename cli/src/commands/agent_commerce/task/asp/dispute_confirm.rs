//! Raise dispute (ASP) step 2 — onchainos agent dispute confirm <jobId>
//!
//! Step 2 of the two-stage on-chain dispute flow. Preconditions:
//!   1. `dispute raise` has been run (stage 1 approve on-chain)
//!   2. On-chain `dispute_approved` system notification has been received
//!
//! This command calls POST /aieco/task/{jobId}/dispute → uopData → sign + broadcast.
//! After completion, wait for the on-chain `job_disputed` notification, then call next-action to enter the evidence preparation window.

use anyhow::{bail, Context, Result};
use std::time::Duration;

use crate::audit;
use crate::commands::agent_commerce::task::common::network::task_api_client::TaskApiClient;
use crate::commands::agent_commerce::task::signing;

const MAX_REASON_CHARS: usize = 2000;

pub async fn handle_dispute_confirm(
    client: &mut TaskApiClient,
    job_id: &str,
    reason: &str,
    agent_id: &str,
) -> Result<()> {
    if agent_id.is_empty() {
        bail!("--agent-id is required (pass the ASP's own agentId; beta backend rejects empty agenticId header)");
    }
    if reason.chars().count() > MAX_REASON_CHARS {
        bail!("Dispute reason exceeds {MAX_REASON_CHARS} characters. Please shorten it and try again.");
    }
    let (account_id, address) = signing::resolve_wallet_by_agent_id(agent_id).await?;
    let body = serde_json::json!({});

    let dispute_resp = client.post_with_identity(
        &client.endpoint(job_id, "dispute"), &body, agent_id,
    ).await
        .context("dispute confirm (stage 2): dispute API request failed")?;

    let reason_json = serde_json::json!({ "reason": reason });
    let dispute_tx = signing::sign_uop_and_broadcast(
        client, &dispute_resp["uopData"], &account_id, &address,
        job_id, signing::extract_biz_type(&dispute_resp), agent_id,
        Some(&reason_json),
    ).await
        .context("dispute confirm (stage 2): dispute on-chain broadcast failed")?;

    audit::log(
        "cli",
        "ASP/dispute_confirm_submitted",
        true,
        Duration::default(),
        Some(vec![
            format!("jobId={job_id}"),
            format!("agentId={agent_id}"),
            format!("txHash={dispute_tx}"),
        ]),
        None,
    );

    println!("✓ Dispute stage 2: dispute on-chain");
    println!("  txHash: {dispute_tx}");
    println!();
    println!("⚠️  Stage 2 complete — **end this turn** and wait for the on-chain `job_disputed` system notification:");
    println!("    - Once you receive the `job_disputed` notification, proceed with the evidence upload script");
    Ok(())
}
