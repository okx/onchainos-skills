//! Confirm completion.
//!
//! User action: confirm completion — `onchainos agent complete`.
//!
//! Split by payment mode:
//! - escrow: `pre-complete(orderId, deadline)` → sign digest → `complete(signatureData)` → sign uopHash → broadcast (release escrow).
//! - x402: `/direct/complete` single-signature → broadcast (funds were already paid during accept).

use anyhow::Result;
use std::time::Duration;

use crate::audit;
use crate::commands::agent_commerce::task::common::network::task_api_client::TaskApiClient;
use crate::commands::agent_commerce::task::common::PaymentMode;
use crate::commands::agent_commerce::task::signing;

/// complete — review approved.
pub async fn handle_complete(
    client: &mut TaskApiClient,
    job_id: &str,
) -> Result<()> {
    let (account_id, address, agent_id) =
        signing::resolve_wallet_and_agent_for_task(client, job_id, None).await?;

    // Fetch task detail to obtain paymentMode.
    let resp = client.get_with_identity(&client.task_path(job_id), &agent_id).await?;
    let task = &resp;
    let payment_mode = PaymentMode::from_int(task["paymentMode"].as_i64().unwrap_or(0) as i32);

    if payment_mode == PaymentMode::Escrow {
        // ── Review gate: in escrow mode, the approve_review pseudo event must have fired. ────
        crate::commands::agent_commerce::task::common::review_gate::check_and_consume(job_id)?;

        // ── Escrow: dual-sign pre-complete → complete. ──────────────────────
        let result = signing::task_dual_sign_and_broadcast(
            client, job_id, "pre-complete", "complete",
            None,
            &account_id, &address, &agent_id,
        ).await?;

        audit::log(
            "cli",
            "buyer/complete_submitted",
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
        println!("✓ Task review approved (escrow); status → complete; funds released.");
        println!("  txHash: {}", result.tx_hash);
    } else {
        // ── x402: /direct/complete single-signature (funds were already paid during accept). ────
        let resp = client.post_with_identity(
            &client.endpoint(job_id, "direct/complete"),
            &serde_json::json!({}),
            &agent_id,
        ).await?;

        let tx_hash = signing::sign_uop_and_broadcast(
            client, &resp["uopData"], &account_id, &address,
            job_id, signing::extract_biz_type(&resp), &agent_id,
        ).await?;

        audit::log(
            "cli",
            "buyer/complete_submitted",
            true,
            Duration::default(),
            Some(vec![
                format!("jobId={job_id}"),
                format!("agentId={agent_id}"),
                format!("paymentMode=x402"),
                format!("txHash={tx_hash}"),
            ]),
            None,
        );
        println!("✓ Task complete done (x402); status → complete.");
        println!("  txHash: {tx_hash}");
    }

    Ok(())
}
