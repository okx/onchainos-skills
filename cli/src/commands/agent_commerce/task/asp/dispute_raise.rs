//! Raise dispute (ASP) step 1 — onchainos agent dispute raise <jobId> --reason "..."
//!
//! Dispute is a two-stage on-chain flow; each stage has its own tx and its own chain event:
//!   Stage 1 (this command): POST /aieco/task/{jobId}/dispute/approve → ERC-20 token approve to the dispute contract
//!                     → wait for on-chain `dispute_approved` system notification
//!   Stage 2 (dispute confirm command): POST /aieco/task/{jobId}/dispute → actually raises the dispute
//!                     → wait for on-chain `job_disputed` system notification
//!
//! This command runs stage 1 only. After completion, wait for the `dispute_approved` notification
//! before calling `next-action` to fetch the stage 2 script — **do NOT call dispute confirm in the same turn**.
//! reason is a user-facing log only; not put on-chain.

use anyhow::{bail, Context, Result};
use std::time::Duration;

use crate::audit;
use crate::commands::agent_commerce::task::common::{
    self, network::task_api_client::TaskApiClient,
};
use crate::commands::agent_commerce::task::signing;

const MAX_REASON_CHARS: usize = 2000;

pub async fn handle_dispute_raise(
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

    // Dispute deposit precheck: wallet's matching token balance must be ≥ 5% of the job amount.
    // Insufficient balance bails immediately to avoid wasting gas on later approve / dispute on-chain txs.
    let task_resp = client
        .get_with_identity(&client.task_path(job_id), agent_id)
        .await
        .context("dispute raise: failed to fetch task details (deposit precheck)")?;
    let task_amount: f64 = task_resp["tokenAmount"]
        .as_str()
        .unwrap_or("0")
        .parse()
        .unwrap_or(0.0);
    let token_symbol = task_resp["tokenSymbol"].as_str().unwrap_or("?");
    if task_amount > 0.0 {
        let required = task_amount * 0.05;
        if let Err(e) = common::ensure_sufficient_balance_at(required, token_symbol, &address).await
        {
            // Preserve the dispute-bond framing, then enrich with the ASP signing
            // account's deposit address + stderr QR (FR-2 — explicit address, no
            // agentId resolution). enrich_blocking_at folds the full `{:#}` chain
            // (incl. this context) into the error message byte-for-byte.
            let e = e.context(format!(
                "Raising a dispute requires a deposit >= 5% of the task amount ({required} {token_symbol}; task amount {task_amount} {token_symbol})"
            ));
            let enriched = common::deposit_qr::enrich_blocking_at(e, &address);
            return print_dispute_funding_block_from_error(enriched);
        }
    }

    let body = serde_json::json!({});

    // POST /dispute/approve → uopData → sign + broadcast
    let approve_resp = client
        .post_with_identity(&client.endpoint(job_id, "dispute/approve"), &body, agent_id)
        .await
        .context("dispute raise (stage 1): dispute/approve API request failed")?;

    let approve_tx = signing::sign_uop_and_broadcast(
        client,
        &approve_resp["uopData"],
        &account_id,
        &address,
        job_id,
        signing::extract_biz_type(&approve_resp),
        agent_id,
        None,
    )
    .await
    .context("dispute raise (stage 1): approve on-chain broadcast failed")?;

    audit::log(
        "cli",
        "ASP/dispute_approve_submitted",
        true,
        Duration::default(),
        Some(vec![
            format!("jobId={job_id}"),
            format!("agentId={agent_id}"),
            format!("reasonLen={}", reason.chars().count()),
            format!("txHash={approve_tx}"),
        ]),
        None,
    );

    println!("✓ Dispute stage 1: approve on-chain (token approved to the dispute contract)");
    println!("  Reason logged: {reason}");
    println!("  txHash: {approve_tx}");
    println!();
    println!("⚠️  Stage 1 complete — **end this turn** and wait for the on-chain `dispute_approved` system notification:");
    println!("    - Do NOT call `dispute confirm` in the same turn");
    Ok(())
}

fn print_dispute_funding_block_from_error(err: anyhow::Error) -> Result<()> {
    match err.downcast_ref::<common::deposit_qr::InsufficientBalanceError>() {
        Some(ib) => {
            let mut warning = common::deposit_qr::balance_warning_base(ib);
            if let Some(address) = ib.deposit_address.as_deref() {
                warning["depositAddress"] = serde_json::Value::String(address.to_string());
                warning["depositChain"] = serde_json::Value::String(ib.deposit_chain.clone());
            }
            Err(crate::output::CliFundingBlocked {
                data: common::funding_notice::funding_blocked_envelope(
                    &warning,
                    "dispute-bond",
                    "Dispute bond",
                ),
            }
            .into())
        }
        None => Err(err),
    }
}
