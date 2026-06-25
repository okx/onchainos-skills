//! Prefilled ASP rating cache — per-job.
//!
//! Pre-decided ASP rating (score + comment) persisted at deliverable_received
//! time so the `job_completed` event playbook can dispatch `feedback-submit`
//! in-process without an LLM decision round-trip.
//!
//! File: `~/.onchainos/task/<jobId>/prefilled-rating.json`
//! Shape: `{ "score": "4.50", "comment": "..." }`
//! Cleanup: piggy-backs on `buyer::negotiate::cleanup()` (deletes all regular
//! files under the per-job state dir), which is invoked by the `session-cleanup`
//! CLI on terminal-state events.

use anyhow::Result;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Rating {
    pub score: String,
    pub comment: String,
}

fn state_dir(job_id: &str) -> Result<std::path::PathBuf> {
    let home = dirs::home_dir()
        .ok_or_else(|| anyhow::anyhow!("could not resolve HOME directory"))?;
    Ok(home.join(".onchainos").join("task").join(job_id))
}

fn cache_path(job_id: &str) -> Result<std::path::PathBuf> {
    Ok(state_dir(job_id)?.join("prefilled-rating.json"))
}

/// Persist the pre-decided rating. Overwrites any existing entry.
pub fn save(job_id: &str, score: &str, comment: &str) -> Result<()> {
    let dir = state_dir(job_id)?;
    std::fs::create_dir_all(&dir)?;
    let rating = Rating { score: score.to_string(), comment: comment.to_string() };
    let json = serde_json::to_string_pretty(&rating)?;
    std::fs::write(cache_path(job_id)?, json)?;
    Ok(())
}

/// Read the pre-decided rating, if any.
pub fn get(job_id: &str) -> Result<Option<Rating>> {
    let path = cache_path(job_id)?;
    if !path.exists() {
        return Ok(None);
    }
    let raw = std::fs::read_to_string(&path)?;
    let rating: Rating = serde_json::from_str(&raw)?;
    if rating.score.is_empty() {
        return Ok(None);
    }
    Ok(Some(rating))
}
