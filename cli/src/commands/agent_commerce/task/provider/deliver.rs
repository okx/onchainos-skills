//! Provider submits deliverable.
//!
//! Provider action: deliver — onchainos agent deliver

use anyhow::{bail, Result};
use std::time::Duration;

use crate::audit;
use crate::commands::agent_commerce::task::common::network::task_api_client::TaskApiClient;
use crate::commands::agent_commerce::task::common::state_machine::Status;
use crate::commands::agent_commerce::task::signing;

/// deliver — submit deliverable
///
/// 1. Precondition: job must be in accepted state (status=1) — i.e. the buyer has confirm-accept on-chain
/// 2. POST submit API (with identity headers) → fetch uopData
/// 3. Sign uopData + broadcast on-chain
pub async fn handle_deliver(
    client: &mut TaskApiClient,
    job_id: &str,
    _file: &str,
    _message: &str,
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

    let (account_id, address) = signing::resolve_wallet(None, None)?;
    // Backend spec: submit endpoint accepts an `evidenceHash` field; for now pass an empty string placeholder (offchain
    // evidence is uploaded multipart via /evidence/upload — no hash is provided at submit stage). file/message are
    // kept as CLI input placeholders only; not put on-chain.
    let body = serde_json::json!({
        "evidenceHash": "",
    });

    let resp = client.post_with_identity(
        &client.endpoint(job_id, "submit"), &body, agent_id,
    ).await?;

    let tx_hash = signing::sign_uop_and_broadcast(
        client, &resp["uopData"], &account_id, &address,
        job_id, signing::extract_biz_type(&resp), agent_id,
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

    println!("✓ Deliverable submitted, waiting for on-chain confirmation (job_submitted)");
    println!("  txHash: {tx_hash}");
    println!();
    println!("⚠️  Next steps are driven by system notifications — do not proactively message the buyer:");
    println!("    - Do NOT call `xmtp_send` to tell the buyer \"deliverable is on-chain, please review\" or similar");
    println!("    - You will receive a `job_submitted` system notification after on-chain confirmation");
    println!("    - Once notified, run `onchainos agent next-action --jobid {job_id} --jobStatus job_submitted --role provider`");
    Ok(())
}
