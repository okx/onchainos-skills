//! Set to Public.
//!
//! User action: set to Public — `onchainos agent set-public`.
//!
//! New flow: setVisibility is now **off-chain** — no signing / broadcast needed.

use anyhow::Result;
use std::time::Duration;

use crate::audit;
use crate::commands::agent_commerce::task::common::network::task_api_client::TaskApiClient;
use crate::commands::agent_commerce::task::signing;

/// set-public — convert the task to public.
///
/// Backend `VisibilityEnum`: 0=PUBLIC / 1=PRIVATE.
/// Converting to public = `visibility=0`.
///
/// Off-chain operation — response no longer contains `uopData`.
pub async fn handle_set_public(client: &mut TaskApiClient, job_id: &str, explicit_agent_id: Option<&str>) -> Result<()> {
    let agent_id = match explicit_agent_id {
        Some(id) => id.to_string(),
        None => {
            let (_, _, id) = signing::resolve_wallet_and_agent_for_task(client, job_id, None).await?;
            id
        }
    };

    client.post_with_identity(
        &client.endpoint(job_id, "setVisibility"),
        &serde_json::json!({"visibility": 0}),
        &agent_id,
    ).await?;

    audit::log(
        "cli",
        "buyer/set_public_submitted",
        true,
        Duration::default(),
        Some(vec![
            format!("jobId={job_id}"),
            format!("agentId={agent_id}"),
        ]),
        None,
    );

    println!("✓ Task converted to public (off-chain); other providers can now see and apply.");
    Ok(())
}
