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
    parse_task_feedback_exists(stdout.trim())
}

fn parse_task_feedback_exists(stdout: &str) -> Result<bool> {
    let parsed: serde_json::Value = serde_json::from_str(stdout)
        .map_err(|error| anyhow::anyhow!("invalid task-feedback JSON: {error}"))?;
    let data = parsed
        .get("data")
        .and_then(serde_json::Value::as_array)
        .ok_or_else(|| anyhow::anyhow!("task-feedback response missing data array"))?;
    Ok(!data.is_empty())
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

#[cfg(test)]
mod tests {
    use super::parse_task_feedback_exists;

    #[test]
    fn task_feedback_parser_distinguishes_empty_and_existing_feedback() {
        assert!(!parse_task_feedback_exists(r#"{"data":[]}"#).unwrap());
        assert!(parse_task_feedback_exists(r#"{"data":[{"feedbackId":"1"}]}"#).unwrap());
    }

    #[test]
    fn task_feedback_parser_rejects_malformed_or_unexpected_success_output() {
        assert!(parse_task_feedback_exists("not-json").is_err());
        assert!(parse_task_feedback_exists(r#"{"ok":true}"#).is_err());
        assert!(parse_task_feedback_exists(r#"{"data":null}"#).is_err());
    }
}
