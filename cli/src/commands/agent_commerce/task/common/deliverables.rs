//! Persistent deliverable storage for both user and provider roles.
//!
//! Layout: `~/.onchainos/deliverables/<role>/<jobId>/`
//!   - Files are moved (not copied) from the platform download directory.
//!   - A `manifest.json` per job tracks metadata + task context.
//!   - The entire directory can later be packaged as evidence.

use anyhow::{bail, Result};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::path::{Path, PathBuf};

const MAX_FILE_SIZE: u64 = 100 * 1024 * 1024; // 100 MB

fn deliverables_root() -> Result<PathBuf> {
    let home = dirs::home_dir()
        .ok_or_else(|| anyhow::anyhow!("could not resolve HOME directory"))?;
    Ok(home.join(".onchainos").join("deliverables"))
}

fn sanitize_title(title: &str, job_id: &str) -> String {
    let result: String = title
        .trim()
        .chars()
        .filter(|c| c.is_alphanumeric())
        .take(20)
        .collect();
    if result.is_empty() {
        let end = job_id.len().min(10);
        format!("job_{}", &job_id[..end])
    } else {
        result
    }
}

/// Resolve the deliverables directory for a job. Supports both old-style
/// (`<jobId>/`) and new-style (`<jobId>_<title>/`) layouts via prefix scan.
pub(crate) fn deliverables_dir(role: &str, job_id: &str) -> Result<PathBuf> {
    let role_dir = deliverables_root()?.join(role);
    let exact = role_dir.join(job_id);
    if exact.exists() {
        return Ok(exact);
    }
    if role_dir.exists() {
        let prefix = format!("{}_", job_id);
        if let Ok(entries) = std::fs::read_dir(&role_dir) {
            for entry in entries.flatten() {
                if entry.file_type().map(|t| t.is_dir()).unwrap_or(false) {
                    let name = entry.file_name().to_string_lossy().to_string();
                    if name.starts_with(&prefix) {
                        return Ok(entry.path());
                    }
                }
            }
        }
    }
    Ok(exact)
}

fn manifest_path(role: &str, job_id: &str) -> Result<PathBuf> {
    Ok(deliverables_dir(role, job_id)?.join("manifest.json"))
}

// ── Manifest schema ──────────────────────────────────────────────────

#[derive(Serialize, Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct Manifest {
    pub job_id: String,
    pub role: String,
    pub task: TaskContext,
    pub entries: Vec<DeliverableEntry>,
}

#[derive(Serialize, Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct TaskContext {
    pub short_id: String,
    pub title: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub token_symbol: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub token_amount: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub counterparty_agent_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub counterparty_name: Option<String>,
}

#[derive(Serialize, Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct DeliverableEntry {
    pub filename: String,
    pub original_name: String,
    pub deliverable_type: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub file_key: Option<String>,
    pub saved_at: String,
    pub size_bytes: u64,
}

pub(crate) fn read_manifest(role: &str, job_id: &str) -> Result<Option<Manifest>> {
    let path = manifest_path(role, job_id)?;
    if !path.exists() {
        return Ok(None);
    }
    let data = std::fs::read_to_string(&path)?;
    let m: Manifest = serde_json::from_str(&data)?;
    Ok(Some(m))
}

fn write_manifest(m: &Manifest) -> Result<()> {
    let path = manifest_path(&m.role, &m.job_id)?;
    let json = serde_json::to_string_pretty(m)?;
    std::fs::write(&path, json)?;
    Ok(())
}

// ── Save ─────────────────────────────────────────────────────────────

pub struct SaveParams<'a> {
    pub job_id: &'a str,
    pub role: &'a str,
    pub file_path: &'a str,
    pub deliverable_type: &'a str,
    pub title: &'a str,
    pub short_id: &'a str,
    pub file_key: Option<&'a str>,
    pub token_symbol: Option<&'a str>,
    pub token_amount: Option<&'a str>,
    pub counterparty_agent_id: Option<&'a str>,
    pub counterparty_name: Option<&'a str>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SaveResult {
    pub job_id: String,
    pub role: String,
    pub path: String,
    pub total_entries: usize,
}

pub fn handle_save(params: &SaveParams<'_>) -> Result<SaveResult> {
    let role = params.role;
    if role != "user" && role != "asp" {
        bail!("--role must be 'user' or 'asp', got '{role}'");
    }

    let src = Path::new(params.file_path);
    if !src.exists() {
        bail!("file not found: {}", params.file_path);
    }

    let file_size = std::fs::metadata(src)?.len();
    if file_size > MAX_FILE_SIZE {
        let size_mb = file_size as f64 / (1024.0 * 1024.0);
        bail!(
            "file too large: {size_mb:.1} MB (max 100 MB). \
             Please compress or resize the file before saving."
        );
    }

    let original_name = src
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| "deliverable".to_string());

    let sanitized = sanitize_title(params.title, params.job_id);
    let now = chrono::Local::now();
    let timestamp = format!("{}{:03}", now.format("%Y%m%d_%H%M%S"), now.timestamp_subsec_millis());
    let ext = src.extension()
        .map(|e| format!(".{}", e.to_string_lossy()))
        .unwrap_or_else(|| ".txt".to_string());
    let dest_name = format!("{sanitized}_{timestamp}{ext}");

    let target_dir_name = format!("{}_{sanitized}", params.job_id);
    let target_dir = deliverables_root()?.join(role).join(&target_dir_name);
    let existing = deliverables_dir(role, params.job_id)?;
    let dir = if existing == target_dir && existing.exists() {
        existing
    } else if existing.exists() {
        // Rename old-style bare-jobId dir to new-style <jobId>_<title>
        let _ = std::fs::rename(&existing, &target_dir);
        target_dir
    } else {
        target_dir
    };
    std::fs::create_dir_all(&dir)?;
    let dest = dir.join(&dest_name);

    // Move: rename first (same partition), fallback to copy + delete
    if std::fs::rename(src, &dest).is_err() {
        std::fs::copy(src, &dest)?;
        let _ = std::fs::remove_file(src);
    }

    let entry = DeliverableEntry {
        filename: dest_name,
        original_name,
        deliverable_type: params.deliverable_type.to_string(),
        file_key: params.file_key.map(|s| s.to_string()),
        saved_at: now.to_rfc3339(),
        size_bytes: file_size,
    };

    let mut manifest = read_manifest(role, params.job_id)?
        .unwrap_or_else(|| Manifest {
            job_id: params.job_id.to_string(),
            role: role.to_string(),
            task: TaskContext {
                short_id: params.short_id.to_string(),
                title: params.title.to_string(),
                token_symbol: params.token_symbol.map(|s| s.to_string()),
                token_amount: params.token_amount.map(|s| s.to_string()),
                counterparty_agent_id: params.counterparty_agent_id.map(|s| s.to_string()),
                counterparty_name: params.counterparty_name.map(|s| s.to_string()),
            },
            entries: Vec::new(),
        });

    manifest.entries.push(entry);
    write_manifest(&manifest)?;

    Ok(SaveResult {
        job_id: params.job_id.to_string(),
        role: role.to_string(),
        path: dest.display().to_string(),
        total_entries: manifest.entries.len(),
    })
}

// ── Review-awaiting-deliverable marker ───────────────────────────────
//
// When `job_submitted` arrives before the XMTP `[intent:deliver]` message,
// the user has no deliverable to review yet. A marker file is written so
// that the later `deliverable_received` event can detect this and directly
// output the review prompt instead of "wait for job_submitted".

fn review_marker_path(job_id: &str) -> Result<PathBuf> {
    Ok(deliverables_dir("user", job_id)?.join("review_awaiting_deliverable"))
}

pub fn write_review_marker(job_id: &str) -> Result<()> {
    let path = review_marker_path(job_id)?;
    std::fs::create_dir_all(path.parent().unwrap())?;
    std::fs::write(&path, "")?;
    Ok(())
}

pub fn has_review_marker(job_id: &str) -> bool {
    review_marker_path(job_id).map(|p| p.exists()).unwrap_or(false)
}

pub fn delete_review_marker(job_id: &str) {
    if let Ok(p) = review_marker_path(job_id) {
        let _ = std::fs::remove_file(p);
    }
}

// ── List (single job) ────────────────────────────────────────────────

pub fn handle_list(job_id: &str, role: &str) -> Result<()> {
    if role != "user" && role != "asp" {
        bail!("--role must be 'user' or 'asp', got '{role}'");
    }
    let manifest = read_manifest(role, job_id)?;
    match manifest {
        Some(m) => {
            let dir = deliverables_dir(role, job_id)?;
            crate::output::success(json!({
                "jobId": m.job_id,
                "shortId": m.task.short_id,
                "title": m.task.title,
                "tokenAmount": m.task.token_amount,
                "tokenSymbol": m.task.token_symbol,
                "counterpartyAgentId": m.task.counterparty_agent_id,
                "counterpartyName": m.task.counterparty_name,
                "deliverables": m.entries.iter().map(|e| {
                    json!({
                        "path": dir.join(&e.filename).display().to_string(),
                        "originalName": e.original_name,
                        "deliverableType": e.deliverable_type,
                        "sizeBytes": e.size_bytes,
                        "savedAt": e.saved_at,
                    })
                }).collect::<Vec<_>>(),
            }));
        }
        None => {
            crate::output::success(json!({ "deliverables": [] }));
        }
    }
    Ok(())
}

// ── List all (with optional search) ──────────────────────────────────

pub fn handle_list_all(role: &str, search: Option<&str>) -> Result<()> {
    if role != "user" && role != "asp" {
        bail!("--role must be 'user' or 'asp', got '{role}'");
    }
    let role_dir = deliverables_root()?.join(role);
    if !role_dir.exists() {
        crate::output::success(json!({ "results": [] }));
        return Ok(());
    }

    let keyword = search.map(|s| s.to_lowercase());
    let mut results = Vec::new();

    let entries = std::fs::read_dir(&role_dir)?;
    for entry in entries.flatten() {
        if !entry.file_type().map(|t| t.is_dir()).unwrap_or(false) {
            continue;
        }
        let job_id = entry.file_name().to_string_lossy().to_string();
        let manifest = match read_manifest(role, &job_id)? {
            Some(m) => m,
            None => continue,
        };

        if let Some(ref kw) = keyword {
            if !manifest.task.title.to_lowercase().contains(kw) {
                continue;
            }
        }

        let dir = deliverables_dir(role, &job_id)?;
        results.push(json!({
            "jobId": manifest.job_id,
            "shortId": manifest.task.short_id,
            "title": manifest.task.title,
            "tokenAmount": manifest.task.token_amount,
            "tokenSymbol": manifest.task.token_symbol,
            "counterpartyAgentId": manifest.task.counterparty_agent_id,
            "counterpartyName": manifest.task.counterparty_name,
            "deliverableCount": manifest.entries.len(),
            "deliverables": manifest.entries.iter().map(|e| {
                json!({
                    "path": dir.join(&e.filename).display().to_string(),
                    "originalName": e.original_name,
                    "deliverableType": e.deliverable_type,
                    "sizeBytes": e.size_bytes,
                    "savedAt": e.saved_at,
                })
            }).collect::<Vec<_>>(),
        }));
    }

    crate::output::success(json!({ "results": results }));
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sanitize_normal_title() {
        assert_eq!(sanitize_title("Polymarket聪明钱信号", "0xabc"), "Polymarket聪明钱信号");
    }

    #[test]
    fn sanitize_strips_non_alphanumeric() {
        assert_eq!(sanitize_title("ETH/BTC 分析: 2026", "0xabc"), "ETHBTC分析2026");
    }

    #[test]
    fn sanitize_removes_symbols() {
        assert_eq!(sanitize_title("a/*?b", "0xabc"), "ab");
    }

    #[test]
    fn sanitize_empty_fallback() {
        assert_eq!(sanitize_title("", "0xabcdef1234"), "job_0xabcdef12");
    }

    #[test]
    fn sanitize_all_illegal_fallback() {
        assert_eq!(sanitize_title("/:*?", "0xabcdef1234"), "job_0xabcdef12");
    }

    #[test]
    fn sanitize_truncates_to_20_chars() {
        let long = "一二三四五六七八九十一二三四五六七八九十额外的字";
        let result = sanitize_title(long, "0xabc");
        assert_eq!(result.chars().count(), 20);
        assert_eq!(result, "一二三四五六七八九十一二三四五六七八九十");
    }

    #[test]
    fn sanitize_strips_spaces() {
        assert_eq!(sanitize_title("  hello world  ", "0xabc"), "helloworld");
    }
}

