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

/// Spawn `onchainos agent feedback-submit ...` as a child process.
///
/// Used by `job_completed` escrow fast path after the buyer sub session
/// has pre-decided the score / comment via the `cache-rating` prefetch.
pub fn feedback_submit(
    provider_agent_id: &str,
    buyer_agent_id: &str,
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
            "--creator-id", buyer_agent_id,
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
