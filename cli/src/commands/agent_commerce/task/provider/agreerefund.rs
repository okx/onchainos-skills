//! Provider agrees to refund.
//!
//! Provider action: agree to refund — onchainos agent agree-refund

use anyhow::{bail, Result};
use std::time::Duration;

use crate::audit;
use crate::commands::agent_commerce::task::common::network::task_api_client::TaskApiClient;
use crate::commands::agent_commerce::task::signing;

/// agree-refund — agree to refund
///
/// 1. POST agreeRefund API (with identity headers) → fetch uopData
/// 2. Sign uopData + broadcast on-chain
pub async fn handle_agree_refund(
    client: &mut TaskApiClient,
    job_id: &str,
    agent_id: &str,
) -> Result<()> {
    if agent_id.is_empty() {
        bail!("--agent-id is required (pass the provider's own agentId; beta backend rejects empty agenticId header)");
    }
    let (account_id, address) = signing::resolve_wallet_by_agent_id(agent_id).await?;
    let body = serde_json::json!({});

    let resp = client.post_with_identity(
        &client.endpoint(job_id, "agreeRefund"), &body, agent_id,
    ).await?;

    let tx_hash = signing::sign_uop_and_broadcast(
        client, &resp["uopData"], &account_id, &address,
        job_id, signing::extract_biz_type(&resp), agent_id,
        None,
    ).await?;

    audit::log(
        "cli",
        "provider/agree_refund_submitted",
        true,
        Duration::default(),
        Some(vec![
            format!("jobId={job_id}"),
            format!("agentId={agent_id}"),
            format!("txHash={tx_hash}"),
        ]),
        None,
    );

    println!("✓ Agreed to refund, waiting for on-chain confirmation (job_refunded)");
    println!("  txHash: {tx_hash}");
    println!();
    println!("⚠️  Next steps are driven by system notifications — do not proactively message the User Agent:");
    println!("    - You will receive a `job_refunded` system notification after on-chain confirmation");
    Ok(())
}
