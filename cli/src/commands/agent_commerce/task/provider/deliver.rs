//! Provider submits deliverable.
//!
//! Provider action: deliver — onchainos agent deliver

use anyhow::{bail, Result};
use std::time::Duration;

use crate::audit;
use crate::commands::agent_commerce::task::common::network::task_api_client::TaskApiClient;
use crate::commands::agent_commerce::task::common::payment_mode::PaymentMode;
use crate::commands::agent_commerce::task::common::state_machine::Status;
use crate::commands::agent_commerce::task::common::DEBUG_LOG;
use crate::commands::agent_commerce::task::signing;

/// deliver — submit deliverable
///
/// 1. Precondition: job must be in accepted state (status=1) — i.e. the buyer has confirm-accept on-chain
/// 2. POST submit API (with identity headers) → fetch uopData
/// 3. Sign uopData + broadcast on-chain
pub async fn handle_deliver(
    client: &mut TaskApiClient,
    job_id: &str,
    file: &str,
    deliverable_text: &str,
    agent_id: &str,
) -> Result<()> {
    if agent_id.is_empty() {
        bail!("--agent-id is required (pass the provider's own agentId; beta backend rejects empty agenticId header)");
    }

    // Precondition: job must be accepted (buyer has confirm-accept, on-chain job_accepted notification received) before delivery.
    // Prevents the agent from racing to deliver right after apply without waiting for buyer confirmation — backend rejects this, but an early bail makes the error clearer.
    let task_resp = client.get_with_identity(&client.task_path(job_id), agent_id).await?;
    let status_int = task_resp["status"]
        .as_i64()
        .and_then(|n| i32::try_from(n).ok())
        .ok_or_else(|| anyhow::anyhow!("Task detail missing status field, cannot determine delivery eligibility"))?;
    let status = Status::from_int(status_int);
    if status != Status::Accepted {
        audit::log(
            "cli",
            "provider/deliver_blocked_wrong_status",
            false,
            Duration::default(),
            Some(vec![
                format!("jobId={job_id}"),
                format!("agentId={agent_id}"),
                format!("statusInt={status_int}"),
                format!("status={}", status.as_str()),
            ]),
            Some("status != accepted(1)"),
        );
        bail!(
            "Deliver rejected: current task status = {} ({}), must be accepted (1) before delivery.\n\
             If you just applied, wait for the buyer to confirm-accept on-chain and receive the `job_accepted` system notification before delivering.\n\
             Do NOT call xmtp_send to rush the buyer — confirm-accept is a user decision driven by the buyer's session.",
            status_int,
            status.as_str(),
        );
    }

    let pm_int = task_resp["paymentMode"]
        .as_i64()
        .and_then(|n| i32::try_from(n).ok())
        .unwrap_or(1);
    let pm = PaymentMode::from_int(pm_int);
    if pm != PaymentMode::Escrow {
        audit::log(
            "cli",
            "provider/deliver_blocked_wrong_payment_mode",
            false,
            Duration::default(),
            Some(vec![
                format!("jobId={job_id}"),
                format!("agentId={agent_id}"),
                format!("paymentMode={pm_int}"),
                format!("paymentModeStr={}", pm.as_str()),
            ]),
            Some("deliver is escrow-only"),
        );
        bail!(
            "Deliver rejected: paymentMode = {} ({}) — deliver/submit is only supported for escrow (1).\n\
             x402 tasks skip the submit step; the buyer obtains the deliverable by replaying the provider's endpoint and calls /direct/complete.",
            pm_int,
            pm.as_str(),
        );
    }

    let (account_id, address) = signing::resolve_wallet_by_agent_id(agent_id).await?;
    // Backend spec: submit endpoint accepts an `evidenceHash` field; for now pass an empty string placeholder (offchain
    // evidence is uploaded multipart via /evidence/upload — no hash is provided at submit stage).
    let body = serde_json::json!({
        "evidenceHash": "",
    });

    let resp = client.post_with_identity(
        &client.endpoint(job_id, "submit"), &body, agent_id,
    ).await?;

    let tx_hash = signing::sign_uop_and_broadcast(
        client, &resp["uopData"], &account_id, &address,
        job_id, signing::extract_biz_type(&resp), agent_id,
        None,
    ).await?;

    audit::log(
        "cli",
        "provider/deliver_submitted",
        true,
        Duration::default(),
        Some(vec![
            format!("jobId={job_id}"),
            format!("agentId={agent_id}"),
            format!("txHash={tx_hash}"),
        ]),
        None,
    );

    // Auto-save deliverable to persistent storage.
    // Runs after on-chain success so a failed save never blocks the deliver flow.
    let title = task_resp["title"].as_str().unwrap_or("(untitled)");
    let token_symbol = task_resp["tokenSymbol"].as_str();
    let token_amount = task_resp["tokenAmount"].as_str();
    let buyer_agent_id = task_resp["buyerAgentId"].as_str();
    let short_id = if job_id.len() > 12 {
        format!("{}…", &job_id[..8])
    } else {
        job_id.to_string()
    };

    if !file.is_empty() {
        // File deliverable: move the file to persistent storage.
        let src = std::path::Path::new(file);
        if src.exists() {
            let params = super::super::common::deliverables::SaveParams {
                job_id,
                role: "provider",
                file_path: file,
                deliverable_type: "file",
                title,
                short_id: &short_id,
                file_key: None,
                token_symbol,
                token_amount,
                counterparty_agent_id: buyer_agent_id,
                counterparty_name: None,
            };
            match super::super::common::deliverables::handle_save(&params) {
                Ok(r) => { if DEBUG_LOG { eprintln!("[deliver] deliverable auto-saved: {}", r.path); } }
                Err(e) => { if DEBUG_LOG { eprintln!("[deliver] deliverable auto-save failed (non-blocking): {e}"); } }
            }
        }
    } else if !deliverable_text.is_empty() {
        // Text deliverable: write deliverable_text content to a temp file and save.
        let tmp_dir = std::env::temp_dir();
        let tmp_path = tmp_dir.join(format!("deliverable_{}.txt", chrono::Local::now().format("%Y%m%d%H%M%S")));
        if let Ok(()) = std::fs::write(&tmp_path, deliverable_text) {
            let tmp_str = tmp_path.display().to_string();
            let params = super::super::common::deliverables::SaveParams {
                job_id,
                role: "provider",
                file_path: &tmp_str,
                deliverable_type: "text",
                title,
                short_id: &short_id,
                file_key: None,
                token_symbol,
                token_amount,
                counterparty_agent_id: buyer_agent_id,
                counterparty_name: None,
            };
            match super::super::common::deliverables::handle_save(&params) {
                Ok(r) => { if DEBUG_LOG { eprintln!("[deliver] text deliverable auto-saved: {}", r.path); } }
                Err(e) => { if DEBUG_LOG { eprintln!("[deliver] text deliverable auto-save failed (non-blocking): {e}"); } }
            }
        }
    }

    println!("✓ Deliverable submitted, waiting for on-chain confirmation (job_submitted)");
    println!("  txHash: {tx_hash}");
    println!();
    println!("⚠️  Next steps are driven by system notifications — do not proactively message the buyer:");
    println!("    - You will receive a `job_submitted` system notification after on-chain confirmation");
    Ok(())
}
