//! Confirm completion.
//!
//! User action: confirm completion — `onchainos agent complete`.
//!
//! A2A escrow only: `pre-complete(orderId, deadline)` → sign digest →
//! `complete(signatureData)` → sign uopHash → broadcast (release escrow).

use anyhow::{bail, Result};
use std::time::Duration;

use crate::audit;
use crate::commands::agent_commerce::task::common::network::task_api_client::TaskApiClient;
use crate::commands::agent_commerce::task::common::PaymentMode;
use crate::commands::agent_commerce::task::signing;

/// complete — review approved.
pub async fn handle_complete(client: &mut TaskApiClient, job_id: &str) -> Result<()> {
    let (account_id, address, agent_id) =
        signing::resolve_wallet_and_agent_for_task(client, job_id, None).await?;

    // Fetch task detail to obtain paymentMode.
    let resp = client
        .get_with_identity(&client.task_path(job_id), &agent_id)
        .await?;
    let task = &resp;
    let payment_mode = PaymentMode::from_int(task["paymentMode"].as_i64().unwrap_or(0) as i32);

    if payment_mode != PaymentMode::Escrow {
        bail!("legacy_a2mcp_flow_removed: task completion supports A2A escrow only");
    }

    crate::commands::agent_commerce::task::common::review_gate::check_and_consume(job_id)?;
    let result = signing::task_dual_sign_and_broadcast(
        client,
        job_id,
        "pre-complete",
        "complete",
        None,
        &account_id,
        &address,
        &agent_id,
        None,
    )
    .await?;

    audit::log(
        "cli",
        "user/complete_submitted",
        true,
        Duration::default(),
        Some(vec![
            format!("jobId={job_id}"),
            format!("agentId={agent_id}"),
            format!("paymentMode=escrow"),
            format!("txHash={}", result.tx_hash),
        ]),
        None,
    );

    Ok(())
}
