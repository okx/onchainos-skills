//! Prefilled notify cache — per-job, event-keyed user-notify content.
//!
//! Pre-translated `user-notify` content persisted at task-create time (via the
//! backup-session prefetch) so on-chain event playbooks can dispatch
//! `user-notify` immediately without an LLM translation round-trip.
//!
//! File: `~/.onchainos/task/<jobId>/cache/prefilled-notify.json`
//!
//! The cache lives in a `cache/` subdirectory (not the per-job state dir root)
//! so unrelated callers of `buyer::negotiate::cleanup()` — which deletes only
//! regular files in the root, not subdirectories — cannot accidentally wipe
//! it before its consumer events (e.g. `job_completed`) read it. Terminal-state
//! cleanup is the only path that purges this cache, via `clear()` invoked by
//! `session_cleanup`.
//!
//! Shape: `{ "<event_key>": "<translated content>", ... }`
//!
//! Keep this module storage-only — no event-specific knowledge. Callers pick
//! their own `event_key` (e.g. `job_created_designated`).

use anyhow::Result;

fn cache_dir(job_id: &str) -> Result<std::path::PathBuf> {
    let home = dirs::home_dir()
        .ok_or_else(|| anyhow::anyhow!("could not resolve HOME directory"))?;
    Ok(home.join(".onchainos").join("task").join(job_id).join("cache"))
}

fn cache_path(job_id: &str) -> Result<std::path::PathBuf> {
    Ok(cache_dir(job_id)?.join("prefilled-notify.json"))
}

/// Persist a pre-translated notification under `event_key`.
/// Existing keys are preserved; the same key is overwritten.
pub fn save(job_id: &str, event_key: &str, content: &str) -> Result<()> {
    let dir = cache_dir(job_id)?;
    std::fs::create_dir_all(&dir)?;
    let path = cache_path(job_id)?;
    let mut map: serde_json::Map<String, serde_json::Value> = if path.exists() {
        let raw = std::fs::read_to_string(&path)?;
        serde_json::from_str(&raw).unwrap_or_default()
    } else {
        serde_json::Map::new()
    };
    map.insert(event_key.to_string(), serde_json::Value::String(content.to_string()));
    let json = serde_json::to_string_pretty(&serde_json::Value::Object(map))?;
    std::fs::write(&path, json)?;
    Ok(())
}

/// Read the pre-translated notification for `event_key`, if any.
pub fn get(job_id: &str, event_key: &str) -> Result<Option<String>> {
    let path = cache_path(job_id)?;
    if !path.exists() {
        return Ok(None);
    }
    let raw = std::fs::read_to_string(&path)?;
    let v: serde_json::Value = serde_json::from_str(&raw)?;
    Ok(v.get(event_key)
        .and_then(|x| x.as_str())
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string()))
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
