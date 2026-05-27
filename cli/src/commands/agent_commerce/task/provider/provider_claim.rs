//! Provider claims after the submit→complete timeout (claimAutoComplete).
//!
//! Provider action: timeout claim — onchainos agent claim-auto-complete
//!
//! Trigger: buyer fails to review within the completedWindow (neither completes nor rejects)
//! → backend keeper pushes a system notification to the provider
//! → provider calls this endpoint (permissionless on-chain claim)
//! → AP.complete → status becomes complete, funds optimistically settle to the provider.

use anyhow::{bail, Result};
use std::time::Duration;

use crate::audit;
use crate::commands::agent_commerce::task::common::network::task_api_client::TaskApiClient;
use crate::commands::agent_commerce::task::signing;

/// claim-auto-complete — optimistic settlement on submit→complete timeout
///
/// 1. POST claimAutoComplete API (with identity headers) → fetch uopData (spec: empty Request)
/// 2. Sign uopData + broadcast on-chain
pub async fn handle_claim_auto_complete(
    client: &mut TaskApiClient,
    job_id: &str,
    agent_id: &str,
) -> Result<()> {
    if agent_id.is_empty() {
        bail!("--agent-id is required (pass the provider's own agentId; beta backend rejects empty agenticId header)");
    }
    let (account_id, address) = signing::resolve_wallet(None, None)?;
    let body = serde_json::json!({});

    let resp = client.post_with_identity(
        &client.endpoint(job_id, "claimAutoComplete"), &body, agent_id,
    ).await?;

    let tx_hash = signing::sign_uop_and_broadcast(
        client, &resp["uopData"], &account_id, &address,
        job_id, signing::extract_biz_type(&resp), agent_id,
        None,
    ).await?;

    audit::log(
        "cli",
        "provider/claim_auto_complete_submitted",
        true,
        Duration::default(),
        Some(vec![
            format!("jobId={job_id}"),
            format!("agentId={agent_id}"),
            format!("txHash={tx_hash}"),
        ]),
        None,
    );

    println!("✓ Timeout claim submitted (claimAutoComplete), waiting for on-chain confirmation (job_completed)");
    println!("  txHash: {tx_hash}");
    println!();
    println!("⚠️  Next steps are driven by system notifications:");
    println!("    - You will receive a `job_completed` system notification after on-chain confirmation (funds released to you)");
    println!("    - Once notified, run `onchainos agent next-action --jobid {job_id} --event job_completed --jobStatus job_completed --role provider`");
    Ok(())
}
