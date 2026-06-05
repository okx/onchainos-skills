//! Set to Public.
//!
//! User action: set to Public — `onchainos agent set-public`.

use anyhow::Result;
use std::time::Duration;

use crate::audit;
use crate::commands::agent_commerce::task::common::network::task_api_client::TaskApiClient;
use crate::commands::agent_commerce::task::signing;

/// set-public — convert the task to public.
///
/// Backend `VisibilityEnum`: 0=PUBLIC / 1=PRIVATE.
/// Converting to public = `visibility=0`.
pub async fn handle_set_public(client: &mut TaskApiClient, job_id: &str, explicit_agent_id: Option<&str>) -> Result<()> {
    let (account_id, address, agent_id) =
        signing::resolve_wallet_and_agent_for_task(client, job_id, explicit_agent_id).await?;

    let resp = client.post_with_identity(
        &client.endpoint(job_id, "setVisibility"),
        &serde_json::json!({"visibility": 0}),
        &agent_id,
    ).await?;

    let tx_hash = signing::sign_uop_and_broadcast(
        client, &resp["uopData"], &account_id, &address,
        job_id, signing::extract_biz_type(&resp), &agent_id,
        None,
    ).await?;

    audit::log(
        "cli",
        "buyer/set_public_submitted",
        true,
        Duration::default(),
        Some(vec![
            format!("jobId={job_id}"),
            format!("agentId={agent_id}"),
            format!("txHash={tx_hash}"),
        ]),
        None,
    );

    println!("✓ Task converted to public; other providers can now see and apply.");
    println!("  txHash: {tx_hash}");
    Ok(())
}
