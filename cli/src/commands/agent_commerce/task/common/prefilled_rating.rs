//! Prefilled ASP rating cache — per-job.
//!
//! Pre-decided ASP rating (score + comment) persisted at deliverable_received
//! time so the `job_completed` event playbook can dispatch `feedback-submit`
//! in-process without an LLM decision round-trip.
//!
//! File: `~/.onchainos/task/<jobId>/cache/prefilled-rating.json`
//!
//! The cache lives in a `cache/` subdirectory (not the per-job state dir root)
//! so unrelated callers of `buyer::negotiate::cleanup()` — which deletes only
//! regular files in the root, not subdirectories — cannot accidentally wipe
//! it before `job_completed` reads it. Terminal-state cleanup is the only
//! path that purges this cache, via `clear()` invoked by `session_cleanup`.
//!
//! Shape: `{ "score": "4.50", "comment": "..." }`

use anyhow::Result;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Rating {
    pub score: String,
    pub comment: String,
}

fn cache_dir(job_id: &str) -> Result<std::path::PathBuf> {
    let home = dirs::home_dir()
        .ok_or_else(|| anyhow::anyhow!("could not resolve HOME directory"))?;
    Ok(home.join(".onchainos").join("task").join(job_id).join("cache"))
}

fn cache_path(job_id: &str) -> Result<std::path::PathBuf> {
    Ok(cache_dir(job_id)?.join("prefilled-rating.json"))
}

/// Persist the pre-decided rating. Overwrites any existing entry.
pub fn save(job_id: &str, score: &str, comment: &str) -> Result<()> {
    let dir = cache_dir(job_id)?;
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

/// Remove the cache file. Safe to call when the file does not exist.
/// Invoked by `session_cleanup` on terminal-state events.
pub fn clear(job_id: &str) -> Result<()> {
    let path = cache_path(job_id)?;
    if path.exists() {
        std::fs::remove_file(&path)?;
    }
    Ok(())
}
