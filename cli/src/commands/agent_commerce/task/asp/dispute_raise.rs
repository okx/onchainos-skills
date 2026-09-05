//! Raise dispute (ASP) step 1 — onchainos agent dispute raise <jobId> --reason "..."
//!
//! Dispute is a two-stage on-chain flow; each stage has its own tx and its own chain event:
//!   Stage 1 (this command): POST /aieco/task/{jobId}/dispute/approve → ERC-20 token approve to the dispute contract
//!                     → wait for on-chain `dispute_approved` system notification
//!   Stage 2 (dispute confirm command): POST /aieco/task/{jobId}/dispute → actually raises the dispute
//!                     → wait for on-chain `job_disputed` system notification
//!
//! This command runs stage 1 only. The task sub-session handles `dispute confirm`
//! after the `dispute_approved` notification arrives.
//! reason is included in the stage-1 broadcast bizContext for the later dispute creation.

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
    crate::commands::agent_commerce::task::arbitration_trace::record(
        "dispute-raise-input",
        job_id,
        &serde_json::json!({
            "command": "agent dispute raise",
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
    if reason.trim().is_empty() {
        bail!("Dispute reason is required. Pass the user's arbitration reason with --reason.");
    }
    if reason.chars().count() > MAX_REASON_CHARS {
        bail!("Dispute reason exceeds {MAX_REASON_CHARS} characters. Please shorten it and try again.");
    }
    let wallet_result = signing::resolve_wallet_by_agent_id(agent_id).await;
    match &wallet_result {
        Ok((account_id, address)) => {
            crate::commands::agent_commerce::task::arbitration_trace::record(
                "dispute-raise-wallet-resolution",
                job_id,
                &serde_json::json!({"agentId": agent_id}),
                Some(&serde_json::json!({"accountId": account_id, "address": address})),
                None,
            );
        }
        Err(error) => crate::commands::agent_commerce::task::arbitration_trace::record(
            "dispute-raise-wallet-resolution",
            job_id,
            &serde_json::json!({"agentId": agent_id}),
            None,
            Some(&format!("{error:#}")),
        ),
    }
    let (account_id, address) = wallet_result?;

    // Dispute deposit precheck: wallet's matching token balance must be ≥ 5% of the job amount.
    // Insufficient balance bails immediately to avoid wasting gas on later approve / dispute on-chain txs.
    let task_path = client.task_path(job_id);
    let task_result = client.get_with_identity(&task_path, agent_id).await;
    match &task_result {
        Ok(response) => crate::commands::agent_commerce::task::arbitration_trace::record(
            "dispute-raise-task-detail",
            job_id,
            &serde_json::json!({"path": task_path, "agentId": agent_id}),
            Some(&serde_json::json!({
                "status": response.get("status"),
                "tokenAmount": response.get("tokenAmount"),
                "tokenSymbol": response.get("tokenSymbol"),
                "providerAgentId": response.get("providerAgentId"),
            })),
            None,
        ),
        Err(error) => crate::commands::agent_commerce::task::arbitration_trace::record(
            "dispute-raise-task-detail",
            job_id,
            &serde_json::json!({"path": task_path, "agentId": agent_id}),
            None,
            Some(&format!("{error:#}")),
        ),
    }
    let task_resp =
        task_result.context("dispute raise: failed to fetch task details (deposit precheck)")?;
    let task_amount: f64 = task_resp["tokenAmount"]
        .as_str()
        .unwrap_or("0")
        .parse()
        .unwrap_or(0.0);
    let token_symbol = task_resp["tokenSymbol"].as_str().unwrap_or("?");
    if task_amount > 0.0 {
        let required = task_amount * 0.05;
        let balance_result =
            common::ensure_sufficient_balance_at(required, token_symbol, &address).await;
        match &balance_result {
            Ok(()) => crate::commands::agent_commerce::task::arbitration_trace::record(
                "dispute-raise-deposit-check",
                job_id,
                &serde_json::json!({
                    "address": address,
                    "taskAmount": task_amount,
                    "requiredAmount": required,
                    "tokenSymbol": token_symbol,
                }),
                Some(&serde_json::json!({"sufficient": true})),
                None,
            ),
            Err(error) => crate::commands::agent_commerce::task::arbitration_trace::record(
                "dispute-raise-deposit-check",
                job_id,
                &serde_json::json!({
                    "address": address,
                    "taskAmount": task_amount,
                    "requiredAmount": required,
                    "tokenSymbol": token_symbol,
                }),
                Some(&serde_json::json!({"sufficient": false})),
                Some(&format!("{error:#}")),
            ),
        }
        if let Err(e) = balance_result {
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
    let approve_path = client.endpoint(job_id, "dispute/approve");
    let approve_result = client
        .post_with_identity(&approve_path, &body, agent_id)
        .await;
    match &approve_result {
        Ok(response) => crate::commands::agent_commerce::task::arbitration_trace::record(
            "dispute-raise-approve-api",
            job_id,
            &serde_json::json!({"path": approve_path, "agentId": agent_id, "body": body}),
            Some(response),
            None,
        ),
        Err(error) => crate::commands::agent_commerce::task::arbitration_trace::record(
            "dispute-raise-approve-api",
            job_id,
            &serde_json::json!({"path": approve_path, "agentId": agent_id, "body": body}),
            None,
            Some(&format!("{error:#}")),
        ),
    }
    let approve_resp =
        approve_result.context("dispute raise (stage 1): dispute/approve API request failed")?;

    let reason_json = serde_json::json!({ "reason": reason });
    let approve_tx = signing::sign_uop_and_broadcast_traced(
        client,
        &approve_resp["uopData"],
        &account_id,
        &address,
        job_id,
        signing::extract_biz_type(&approve_resp),
        agent_id,
        Some(&reason_json),
        "dispute-raise",
    )
    .await
    .context("dispute raise (stage 1): approve on-chain broadcast failed")?;

    crate::commands::agent_commerce::task::arbitration_trace::record(
        "dispute-raise-complete",
        job_id,
        &serde_json::json!({"agentId": agent_id, "reason": reason}),
        Some(&serde_json::json!({"txHash": approve_tx, "waitFor": "dispute_approved"})),
        None,
    );

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
    println!("✓ Stage 1 broadcast submitted; the `dispute_approved` signal will continue with `dispute confirm`");
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
