//! Reject a provider's apply (only valid while task is in `created` state).
//!
//! User action: `onchainos agent reject-apply`. Off-chain API call only — backend
//! clears the ASP binding on the task record and notifies the rejected ASP. No
//! signing / broadcast; status stays `created`.

use anyhow::Result;
use std::time::Duration;

use crate::audit;
use crate::commands::agent_commerce::task::common::network::task_api_client::TaskApiClient;
use crate::commands::agent_commerce::task::signing;

/// reject-apply — decline the ASP's apply (off-chain).
///
/// POST /priapi/v1/aieco/task/{jobId}/user/reject
/// Request:  {} (backend resolves the target ASP from task state)
/// Response: ignored — no uopData to sign, no tx to broadcast.
pub async fn handle_reject_apply(
    client: &mut TaskApiClient,
    job_id: &str,
    explicit_agent_id: Option<&str>,
) -> Result<()> {
    let (_, _, agent_id) =
        signing::resolve_wallet_and_agent_for_task(client, job_id, explicit_agent_id).await?;

    client.post_with_identity(
        &client.endpoint(job_id, "user/reject"),
        &serde_json::json!({}),
        &agent_id,
    ).await?;

    audit::log(
        "cli",
        "user/reject_apply_submitted",
        true,
        Duration::default(),
        Some(vec![
            format!("jobId={job_id}"),
            format!("agentId={agent_id}"),
        ]),
        None,
    );

    println!("✓ Reject-apply submitted; task remains in `created` state.");
    println!("  agentId: {agent_id}");
    Ok(())
}
