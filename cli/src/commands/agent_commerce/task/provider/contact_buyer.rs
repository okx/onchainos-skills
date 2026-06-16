//! ASP cold-start: create the group and send a "self-intro + interest" opener
//! to the User Agent in one shot.
//!
//! Provider action: contact the buyer at the start of negotiation —
//! `onchainos agent contact-buyer <jobId> --agent-id <id>`
//!
//! Internally:
//!   1. GET /task/{jobId}  → buyerAgentId + title
//!   2. okx-a2a session create   (creates the group + records the sessionKey)
//!   3. okx-a2a session send     (the first message to the buyer)
//!
//! Replaces the old two-step playbook (xmtp_start_conversation + xmtp_send)
//! that the LLM had to chain manually — fewer turns, no sessionKey passing
//! between MCP calls, idempotent on the same jobId (`session create` is
//! idempotent in okx-a2a's SessionStore).
//!
//! Opener content is the canonical template (self-intro + interest + ask the
//! three negotiation topics). Not user-customizable on purpose — keeps the
//! cold-start consistent and prevents the LLM from injecting price / work
//! content / fabricated `[intent:*]` literals.

use anyhow::{bail, Result};
use std::time::Duration;

use crate::audit;
use crate::commands::agent_commerce::task::common::network::task_api_client::TaskApiClient;
use crate::commands::agent_commerce::task::common::okx_a2a;

/// Canonical cold-start opener. Plain natural language — XMTP plugin auto-
/// wraps into the a2a-agent-chat envelope at send time.
fn build_opener(task_title: &str, agent_id: &str) -> String {
    let title = if task_title.is_empty() { "your job" } else { task_title };
    format!(
        "Hi, I'm your service provider (agentId={agent_id}). I noticed your job \"{title}\" — \
         I can do it. Looking forward to hearing your specific budget / acceptance criteria / \
         preferred payment mode (escrow), so we can finalize the terms together."
    )
}

/// `contact-buyer` — proactive ASP cold-start: create the group, send the opener.
pub async fn handle_contact_buyer(
    client: &mut TaskApiClient,
    job_id: &str,
    agent_id: &str,
) -> Result<()> {
    if agent_id.is_empty() {
        bail!("--agent-id is required (pass the provider's own agentId; beta backend rejects empty agenticId header)");
    }
    if job_id.is_empty() {
        bail!("<jobId> is required");
    }

    // Step 1: fetch task to resolve buyerAgentId + title.
    let task = client
        .get_with_identity(&client.task_path(job_id), agent_id)
        .await
        .map_err(|e| anyhow::anyhow!("failed to fetch task {job_id}: {e}"))?;
    let buyer_agent_id = task["buyerAgentId"].as_str().unwrap_or("");
    let task_title = task["title"].as_str().unwrap_or("");
    if buyer_agent_id.is_empty() {
        bail!("task {job_id} has no buyerAgentId — cannot contact the buyer");
    }

    let opener = build_opener(task_title, agent_id);

    // Step 2: create the session (idempotent in okx-a2a's SessionStore).
    let session_key = okx_a2a::session_create(job_id, agent_id, buyer_agent_id)
        .map_err(|e| anyhow::anyhow!("session create failed: {e}"))?;

    // Step 3: send the opener to the buyer.
    okx_a2a::session_send_by_job(job_id, Some(buyer_agent_id), &opener)
        .map_err(|e| anyhow::anyhow!("session send failed: {e}"))?;

    audit::log(
        "cli",
        "provider/contact_buyer_submitted",
        true,
        Duration::default(),
        Some(vec![
            format!("jobId={job_id}"),
            format!("agentId={agent_id}"),
            format!("buyerAgentId={buyer_agent_id}"),
            format!("openerLen={}", opener.chars().count()),
        ]),
        None,
    );

    println!("✓ Cold-start opener sent to buyer.");
    println!("  jobId:         {job_id}");
    println!("  buyerAgentId:  {buyer_agent_id}");
    println!("  taskTitle:     {task_title}");
    println!("  sessionKey:    {session_key}");
    println!();
    println!("⚠️  End this turn now. Wait for the User Agent's reply.");
    println!("    On the next inbound a2a-agent-chat, call:");
    println!("    onchainos agent next-action --role provider --agentId {agent_id} \\");
    println!("      --message '{{\"event\":\"job_created\",\"jobId\":\"{job_id}\"}}'");
    Ok(())
}
