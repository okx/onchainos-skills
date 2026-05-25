//! Review gate.
//!
//! Prevents the agent from skipping the user's review decision and calling `complete` directly
//! in escrow mode.
//!
//! Write points (automatic at the code level, not driven by prompts):
//! - `next-action --jobStatus job_submitted --role buyer` → writes `pending`
//! - `next-action --jobStatus approve_review --role buyer` → `pending` → `approved`
//!
//! Check points:
//! - `complete.rs` escrow path: `approved` lets the call through and clears the gate; everything else is rejected.

use anyhow::{bail, Result};
use std::path::PathBuf;

fn gate_path(job_id: &str) -> Result<PathBuf> {
    let home = dirs::home_dir().ok_or_else(|| anyhow::anyhow!("unable to determine HOME directory"))?;
    let dir = home.join(".onchainos").join("task").join(job_id);
    std::fs::create_dir_all(&dir)?;
    Ok(dir.join("review-gate"))
}

pub fn mark_pending(job_id: &str) -> Result<()> {
    let path = gate_path(job_id)?;
    std::fs::write(&path, "pending")?;
    eprintln!("[review-gate] mark_pending: {}", path.display());
    Ok(())
}

pub fn mark_approved(job_id: &str) -> Result<()> {
    let path = gate_path(job_id)?;
    match std::fs::read_to_string(&path) {
        Ok(content) if content.trim() == "pending" => {
            std::fs::write(&path, "approved")?;
            eprintln!("[review-gate] mark_approved: {}", path.display());
            Ok(())
        }
        Ok(content) => {
            bail!(
                "review-gate state error: expected 'pending', got '{}'. \
                 Please run next-action --jobStatus job_submitted first.",
                content.trim()
            );
        }
        Err(_) => {
            bail!(
                "review-gate file does not exist (job_submitted flow was not executed). \
                 Please call next-action --jobStatus job_submitted --role buyer first."
            );
        }
    }
}

pub fn check_and_consume(job_id: &str) -> Result<()> {
    let path = gate_path(job_id)?;
    match std::fs::read_to_string(&path) {
        Ok(content) if content.trim() == "approved" => {
            let _ = std::fs::remove_file(&path);
            eprintln!("[review-gate] check_and_consume: approved, gate cleared");
            Ok(())
        }
        Ok(content) if content.trim() == "pending" => {
            bail!(
                "User has not made a review decision yet (review-gate = pending). \
                 Please enqueue a review decision via `onchainos agent pending-decisions-v2 request` and wait for the user's reply. \
                 After receiving `[USER_DECISION_RELAY] decision: <verbatim>`, call next-action --jobStatus approve_review to get the review playbook."
            );
        }
        Ok(content) => {
            bail!("review-gate state error: '{}'", content.trim());
        }
        Err(_) => {
            bail!(
                "review-gate file does not exist. In escrow mode you must run \
                 next-action --jobStatus job_submitted review flow first. \
                 Direct calls to complete are not allowed."
            );
        }
    }
}
