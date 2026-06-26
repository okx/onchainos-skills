//! ASP rejects a designated assignment.
//!
//! ASP action: decline a User Agent-designated task before negotiation begins —
//! `onchainos agent asp-reject <jobId> --agent-id <id> [--reason <text>]`
//!
//! Backend endpoint: `POST /priapi/v1/aieco/task/{jobId}/asp/reject`.
//! Off-chain operation (no signing / broadcast); the backend flips the designation
//! so the User Agent can either re-route to another ASP or fall back to public.

use anyhow::{bail, Result};
use std::time::Duration;

use crate::audit;
use crate::commands::agent_commerce::task::common::network::task_api_client::TaskApiClient;

/// `asp-reject` — decline a designated task assignment.
pub async fn handle_asp_reject(
    client: &mut TaskApiClient,
    job_id: &str,
    agent_id: &str,
    reason: &str,
) -> Result<()> {
    if agent_id.is_empty() {
        bail!("--agent-id is required (pass the ASP's own agentId; beta backend rejects empty agenticId header)");
    }

    let body = if reason.is_empty() {
        serde_json::json!({})
    } else {
        serde_json::json!({ "reason": reason })
    };

    let path = client.endpoint(job_id, "asp/reject");
    let resp = client.post_with_identity(&path, &body, agent_id).await?;

    audit::log(
        "cli",
        "ASP/asp_reject_submitted",
        true,
        Duration::default(),
        Some(vec![
            format!("jobId={job_id}"),
            format!("agentId={agent_id}"),
            format!("reason={reason}"),
        ]),
        None,
    );

    println!("✓ Designation declined for jobId={job_id}");
    if let Some(code) = resp.get("code").and_then(|v| v.as_i64()) {
        println!("  backend code: {code}");
    }
    if let Some(msg) = resp.get("msg").and_then(|v| v.as_str()) {
        println!("  backend msg:  {msg}");
    }
    println!();
    println!("⚠️  This is an off-chain decline. Next steps:");
    println!("    - Do NOT call `apply`. Do NOT proceed to the JobCreated playbook.");
    println!("    - The User Agent is now free to designate a different ASP or fall back to public.");
    println!("    - No further system events are expected for this jobId on your side.");
    Ok(())
}
