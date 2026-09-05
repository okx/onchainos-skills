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
    crate::commands::agent_commerce::task::arbitration_trace::record(
        "dispute-confirm-input",
        job_id,
        &serde_json::json!({
            "command": "agent dispute confirm",
            "agentId": agent_id,
            "reason": reason,
            "reasonChars": reason.chars().count(),
        }),
        Some(&serde_json::json!({"received": true})),
        None,
    );
    if agent_id.is_empty() {
        bail!("--agent-id is required (pass the ASP's own agentId; beta backend rejects empty agenticId header)");
    }
    if reason.chars().count() > MAX_REASON_CHARS {
        bail!("Dispute reason exceeds {MAX_REASON_CHARS} characters. Please shorten it and try again.");
    }
    let wallet_result = signing::resolve_wallet_by_agent_id(agent_id).await;
    match &wallet_result {
        Ok((account_id, address)) => {
            crate::commands::agent_commerce::task::arbitration_trace::record(
                "dispute-confirm-wallet-resolution",
                job_id,
                &serde_json::json!({"agentId": agent_id}),
                Some(&serde_json::json!({"accountId": account_id, "address": address})),
                None,
            );
        }
        Err(error) => crate::commands::agent_commerce::task::arbitration_trace::record(
            "dispute-confirm-wallet-resolution",
            job_id,
            &serde_json::json!({"agentId": agent_id}),
            None,
            Some(&format!("{error:#}")),
        ),
    }
    let (account_id, address) = wallet_result?;
    let body = serde_json::json!({});

    let dispute_path = client.endpoint(job_id, "dispute");
    let dispute_result = client
        .post_with_identity(&dispute_path, &body, agent_id)
        .await;
    match &dispute_result {
        Ok(response) => crate::commands::agent_commerce::task::arbitration_trace::record(
            "dispute-confirm-api",
            job_id,
            &serde_json::json!({"path": dispute_path, "agentId": agent_id, "body": body}),
            Some(response),
            None,
        ),
        Err(error) => crate::commands::agent_commerce::task::arbitration_trace::record(
            "dispute-confirm-api",
            job_id,
            &serde_json::json!({"path": dispute_path, "agentId": agent_id, "body": body}),
            None,
            Some(&format!("{error:#}")),
        ),
    }
    let dispute_resp =
        dispute_result.context("dispute confirm (stage 2): dispute API request failed")?;

    let reason_json = serde_json::json!({ "reason": reason });
    let dispute_tx = signing::sign_uop_and_broadcast_traced(
        client,
        &dispute_resp["uopData"],
        &account_id,
        &address,
        job_id,
        signing::extract_biz_type(&dispute_resp),
        agent_id,
        Some(&reason_json),
        "dispute-confirm",
    )
    .await
        .context("dispute confirm (stage 2): dispute on-chain broadcast failed")?;

    crate::commands::agent_commerce::task::arbitration_trace::record(
        "dispute-confirm-complete",
        job_id,
        &serde_json::json!({"agentId": agent_id, "reason": reason}),
        Some(&serde_json::json!({"txHash": dispute_tx, "waitFor": "job_disputed"})),
        None,
    );

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
    println!("✓ Arbitration transaction submitted; wait for `job_disputed` to start the evidence workflow");
    Ok(())
}
