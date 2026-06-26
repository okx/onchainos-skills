//! Provider submits deliverable.
//!
//! Provider action: deliver — onchainos agent deliver
//!
//! Full pipeline (all handled internally):
//!   1. Precondition checks (status, payment mode)
//!   2. file_upload (if file or long text) → xmtp_send to User Agent
//!   3. On-chain submit (POST /submit → sign → broadcast)
//!   4. Local persistent save

use anyhow::{bail, Result};
use std::time::Duration;

use crate::audit;
use crate::commands::agent_commerce::task::common::network::task_api_client::TaskApiClient;
use crate::commands::agent_commerce::task::common::okx_a2a;
use crate::commands::agent_commerce::task::common::payment_mode::PaymentMode;
use crate::commands::agent_commerce::task::common::state_machine::Status;
use crate::commands::agent_commerce::task::common::DEBUG_LOG;
use crate::commands::agent_commerce::task::signing;

const LONG_TEXT_THRESHOLD: usize = 200;

/// Deliverable preparation result — carries the info needed by later stages
/// (xmtp message was already sent; this tracks what to save locally).
enum Prepared {
    /// Native file or long-text-converted-to-md: local path + upload metadata.
    File {
        local_path: String,
        file_key: String,
    },
    /// Short inline text (≤ threshold).
    Text {
        tmp_path: String,
    },
}

/// deliver — submit deliverable
///
/// 1. Precondition: job must be in accepted state (status=1)
/// 2. Prepare deliverable + xmtp_send to User Agent
/// 3. POST submit API (with identity headers) → sign uopData → broadcast on-chain
/// 4. Auto-save deliverable locally
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

    // ── 1. Precondition checks ──────────────────────────────────────────

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
             If you just applied, wait for the User Agent to confirm-accept on-chain and receive the `job_accepted` system notification before delivering.\n\
             Do NOT call `okx-a2a xmtp-send` to rush the User Agent — confirm-accept is a user decision driven by the User Agent's session.",
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
             x402 tasks skip the submit step; the User Agent obtains the deliverable by replaying the provider's endpoint and calls /direct/complete.",
            pm_int,
            pm.as_str(),
        );
    }

    // Extract fields needed by later stages (available from task_resp already fetched above).
    let user_agent_id = task_resp["buyerAgentId"]
        .as_str()
        .unwrap_or("");
    let title = task_resp["title"].as_str().unwrap_or("(untitled)");
    let token_symbol = task_resp["tokenSymbol"].as_str();
    let token_amount = task_resp["tokenAmount"].as_str();
    let short_id = if job_id.len() > 12 {
        format!("{}…", &job_id[..8])
    } else {
        job_id.to_string()
    };

    // ── 2. Prepare deliverable + xmtp_send ──────────────────────────────

    let base_tags = vec![format!("jobId={job_id}"), format!("agentId={agent_id}")];

    let prepared = if !file.is_empty() {
        // ▸ Native file (image / PDF / document)
        let src = std::path::Path::new(file);
        if !src.exists() {
            bail!("file not found: {file}");
        }
        audit::log("cli", "provider/deliver_file_upload", true, Duration::default(),
            Some([base_tags.clone(), vec![format!("path={file}")]].concat()), None);
        let upload = okx_a2a::file_upload(file, agent_id, job_id, None, None)?;
        audit::log("cli", "provider/deliver_file_uploaded", true, Duration::default(),
            Some([base_tags.clone(), vec![format!("fileKey={}", upload.file_key)]].concat()), None);

        let msg = super::content::build_file_deliver_message(job_id, &upload);
        if !user_agent_id.is_empty() {
            match okx_a2a::xmtp_send(job_id, user_agent_id, &msg) {
                Ok(()) => {
                    audit::log("cli", "provider/deliver_xmtp_sent", true, Duration::default(),
                        Some([base_tags.clone(), vec!["type=file".into()]].concat()), None);
                }
                Err(e) => {
                    audit::log("cli", "provider/deliver_xmtp_failed", false, Duration::default(),
                        Some([base_tags.clone(), vec!["type=file".into()]].concat()), Some(&e.to_string()));
                }
            }
        }
        Prepared::File {
            local_path: file.to_string(),
            file_key: upload.file_key,
        }
    } else if !deliverable_text.is_empty() {
        let text_len = deliverable_text.chars().count();
        let is_long = text_len > LONG_TEXT_THRESHOLD;
        audit::log("cli", "provider/deliver_text_prepare", true, Duration::default(),
            Some([base_tags.clone(), vec![format!("charCount={text_len}"), format!("isLong={is_long}")]].concat()), None);

        if is_long {
            // ▸ Long text → write .md → file_upload → file-format xmtp
            //   Fallback: if tmp write or file_upload fails, degrade to inline text.
            let file_result = (|| -> Result<Prepared> {
                let tmp_dir = std::env::temp_dir();
                let tmp_path = tmp_dir.join(format!("deliverable_{}.md", job_id));
                std::fs::write(&tmp_path, deliverable_text)?;
                let tmp_str = tmp_path.display().to_string();
                let upload = okx_a2a::file_upload(&tmp_str, agent_id, job_id, None, None)?;
                audit::log("cli", "provider/deliver_long_text_uploaded", true, Duration::default(),
                    Some([base_tags.clone(), vec![format!("fileKey={}", upload.file_key), format!("path={tmp_str}")]].concat()), None);

                let msg = super::content::build_file_deliver_message(job_id, &upload);
                if !user_agent_id.is_empty() {
                    match okx_a2a::xmtp_send(job_id, user_agent_id, &msg) {
                        Ok(()) => {
                            audit::log("cli", "provider/deliver_xmtp_sent", true, Duration::default(),
                                Some([base_tags.clone(), vec!["type=file_from_long_text".into()]].concat()), None);
                        }
                        Err(e) => {
                            audit::log("cli", "provider/deliver_xmtp_failed", false, Duration::default(),
                                Some([base_tags.clone(), vec!["type=file_from_long_text".into()]].concat()), Some(&e.to_string()));
                        }
                    }
                }
                Ok(Prepared::File {
                    local_path: tmp_str,
                    file_key: upload.file_key,
                })
            })();
            match file_result {
                Ok(p) => p,
                Err(e) => {
                    audit::log("cli", "provider/deliver_long_text_fallback", false, Duration::default(),
                        Some([base_tags.clone(), vec![format!("charCount={text_len}")]].concat()), Some(&e.to_string()));

                    let msg = super::content::build_text_deliver_message(job_id, deliverable_text);
                    if !user_agent_id.is_empty() {
                        match okx_a2a::xmtp_send(job_id, user_agent_id, &msg) {
                            Ok(()) => {
                                audit::log("cli", "provider/deliver_xmtp_sent", true, Duration::default(),
                                    Some([base_tags.clone(), vec!["type=text_fallback".into()]].concat()), None);
                            }
                            Err(e) => {
                                audit::log("cli", "provider/deliver_xmtp_failed", false, Duration::default(),
                                    Some([base_tags.clone(), vec!["type=text_fallback".into()]].concat()), Some(&e.to_string()));
                            }
                        }
                    }
                    let tmp_dir = std::env::temp_dir();
                    let tmp_path = tmp_dir.join(format!(
                        "deliverable_{}.txt",
                        chrono::Local::now().format("%Y%m%d%H%M%S")
                    ));
                    let _ = std::fs::write(&tmp_path, deliverable_text);
                    Prepared::Text {
                        tmp_path: tmp_path.display().to_string(),
                    }
                }
            }
        } else {
            // ▸ Short text → inline text-format xmtp
            let msg = super::content::build_text_deliver_message(job_id, deliverable_text);
            if !user_agent_id.is_empty() {
                match okx_a2a::xmtp_send(job_id, user_agent_id, &msg) {
                    Ok(()) => {
                        audit::log("cli", "provider/deliver_xmtp_sent", true, Duration::default(),
                            Some([base_tags.clone(), vec!["type=text".into()]].concat()), None);
                    }
                    Err(e) => {
                        audit::log("cli", "provider/deliver_xmtp_failed", false, Duration::default(),
                            Some([base_tags.clone(), vec!["type=text".into()]].concat()), Some(&e.to_string()));
                    }
                }
            }
            // Write text to temp file for local persistent save.
            let tmp_dir = std::env::temp_dir();
            let tmp_path = tmp_dir.join(format!(
                "deliverable_{}.txt",
                chrono::Local::now().format("%Y%m%d%H%M%S")
            ));
            let _ = std::fs::write(&tmp_path, deliverable_text);
            Prepared::Text {
                tmp_path: tmp_path.display().to_string(),
            }
        }
    } else {
        bail!("Either --file or --deliverable-text must be provided");
    };

    // ── 3. On-chain submit ──────────────────────────────────────────────

    let (account_id, address) = signing::resolve_wallet_by_agent_id(agent_id).await?;
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

    // ── 4. Local persistent save ────────────────────────────────────────

    match &prepared {
        Prepared::File { local_path, file_key } => {
            if std::path::Path::new(local_path).exists() {
                let params = super::super::common::deliverables::SaveParams {
                    job_id,
                    role: "asp",
                    file_path: local_path,
                    deliverable_type: "file",
                    title,
                    short_id: &short_id,
                    file_key: Some(file_key),
                    token_symbol,
                    token_amount,
                    counterparty_agent_id: Some(user_agent_id).filter(|s| !s.is_empty()),
                    counterparty_name: None,
                };
                match super::super::common::deliverables::handle_save(&params) {
                    Ok(r) => { if DEBUG_LOG { eprintln!("[deliver] deliverable auto-saved: {}", r.path); } }
                    Err(e) => { if DEBUG_LOG { eprintln!("[deliver] deliverable auto-save failed (non-blocking): {e}"); } }
                }
            }
        }
        Prepared::Text { tmp_path } => {
            if std::path::Path::new(tmp_path).exists() {
                let params = super::super::common::deliverables::SaveParams {
                    job_id,
                    role: "asp",
                    file_path: tmp_path,
                    deliverable_type: "text",
                    title,
                    short_id: &short_id,
                    file_key: None,
                    token_symbol,
                    token_amount,
                    counterparty_agent_id: Some(user_agent_id).filter(|s| !s.is_empty()),
                    counterparty_name: None,
                };
                match super::super::common::deliverables::handle_save(&params) {
                    Ok(r) => { if DEBUG_LOG { eprintln!("[deliver] text deliverable auto-saved: {}", r.path); } }
                    Err(e) => { if DEBUG_LOG { eprintln!("[deliver] text deliverable auto-save failed (non-blocking): {e}"); } }
                }
            }
        }
    }

    println!("✓ Deliverable submitted, waiting for on-chain confirmation (job_submitted)");
    println!("  txHash: {tx_hash}");
    println!();
    println!("⚠️  Next steps are driven by system notifications — do not proactively message the User Agent:");
    println!("    - You will receive a `job_submitted` system notification after on-chain confirmation");
    Ok(())
}
