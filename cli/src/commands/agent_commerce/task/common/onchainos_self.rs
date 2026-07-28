//! Synchronous wrappers around the running `onchainos` binary itself.
//!
//! Used by in-process playbook fast paths that need to invoke an `onchainos
//! agent ...` subcommand whose handler depends on the global CLI `Context`
//! (e.g. `feedback-submit` needing `wallet_client(ctx)`). Spawning the
//! current exe is the simplest way to reuse that handler without threading
//! `Context` through every `flow_lifecycle` function.
//!
//! Spawn cost is ~100-200ms (process init + token refresh) — only use this
//! on cold-path event handlers, never in hot loops.

use anyhow::Result;
use std::process::Command;

/// Check whether `agent_id` has already rated `task_id`.
///
/// Spawns `onchainos agent task-feedback` and parses the JSON output.
/// Returns `true` if `data` contains at least one entry (already rated).
pub fn task_feedback_exists(agent_id: &str, task_id: &str) -> Result<bool> {
    let exe = std::env::current_exe()
        .map_err(|e| anyhow::anyhow!("could not resolve current exe: {e}"))?;
    let out = Command::new(exe)
        .args([
            "agent", "task-feedback",
            "--agent-id", agent_id,
            "--task-id", task_id,
        ])
        .output()
        .map_err(|e| anyhow::anyhow!("spawn failed: {e}"))?;
    if !out.status.success() {
        let stderr = String::from_utf8_lossy(&out.stderr);
        anyhow::bail!("onchainos agent task-feedback exit {status}: {stderr}", status = out.status);
    }
    let stdout = String::from_utf8_lossy(&out.stdout);
    let parsed: serde_json::Value = serde_json::from_str(stdout.trim())
        .unwrap_or(serde_json::Value::Null);
    let data = &parsed["data"];
    Ok(data.is_array() && !data.as_array().unwrap().is_empty())
}

/// Spawn `onchainos agent feedback-submit ...` as a child process.
///
/// Used by `job_completed` escrow fast path after the user sub session
/// has pre-decided the score / comment via the `cache-rating` prefetch.
pub fn feedback_submit(
    provider_agent_id: &str,
    user_agent_id: &str,
    score: &str,
    job_id: &str,
    comment: &str,
) -> Result<()> {
    let exe = std::env::current_exe()
        .map_err(|e| anyhow::anyhow!("could not resolve current exe: {e}"))?;
    let out = Command::new(exe)
        .args([
            "agent", "feedback-submit",
            "--agent-id", provider_agent_id,
            "--creator-id", user_agent_id,
            "--score", score,
            "--task-id", job_id,
            "--description", comment,
        ])
        .output()
        .map_err(|e| anyhow::anyhow!("spawn failed: {e}"))?;
    if !out.status.success() {
        let stderr = String::from_utf8_lossy(&out.stderr);
        anyhow::bail!("onchainos agent feedback-submit exit {status}: {stderr}", status = out.status);
    }
    Ok(())
}
