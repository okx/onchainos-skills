//! Persistent deliverable storage for both user and asp roles.
//!
//! Layout: `~/.onchainos/deliverables/<role>/<jobId>/`
//!   - Files are moved (not copied) from the platform download directory.
//!   - A `manifest.json` per job tracks metadata + task context.
//!   - The entire directory can later be packaged as evidence.

use anyhow::{bail, Result};
use fs2::FileExt;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::path::{Path, PathBuf};

use super::deadline;

const MAX_FILE_SIZE: u64 = 100 * 1024 * 1024; // 100 MB

fn deliverables_root() -> Result<PathBuf> {
    Ok(crate::home::onchainos_home()?.join("deliverables"))
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
    // Layer-2 guard: fail closed before joining job_id into a path.
    super::util::validate_job_id_path_component(job_id)?;
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

/// Stable lock path for a `(role, job_id)` pair — depends only on those two
/// values, never on `deliverables_dir`'s resolved (and renameable) per-job
/// directory. `handle_save` renames the bare `<jobId>/` layout to
/// `<jobId>_<title>/` the first time it saves a deliverable for a job; a
/// lock file placed inside that directory would move or be orphaned by that
/// rename. Living under the stable `role_dir` instead means this path is
/// identical before and after any such rename.
fn deliverables_job_lock_path(role: &str, job_id: &str) -> Result<PathBuf> {
    super::util::validate_job_id_path_component(job_id)?;
    Ok(deliverables_root()?.join(role).join(format!(".{job_id}.lock")))
}

/// Acquire an exclusive, process- and thread-safe lock for a `(role, job_id)`
/// pair. Mirrors `autotrade::delivery_queue::acquire_lock`. Every operation
/// that depends on marker state or on the per-job directory's identity —
/// including `handle_save`'s one-time bare→titled rename — must hold this
/// lock for its whole read-decide-act sequence; `rename()`'s own atomicity
/// only protects individual readers from a half-written file, not two
/// concurrent writers from racing on what to write.
fn acquire_deliverables_job_lock(role: &str, job_id: &str) -> Result<std::fs::File> {
    let path = deliverables_job_lock_path(role, job_id)?;
    if let Some(parent) = path.parent() {
        crate::home::ensure_dir_0700(parent)?;
    }
    let mut options = std::fs::OpenOptions::new();
    options.create(true).read(true).write(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let file = options.open(path)?;
    file.lock_exclusive()?;
    Ok(file)
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

    // Held for the rest of this function: the bare→titled rename below must
    // never race with another process resolving the stale path (e.g. a
    // concurrent `write_review_marker`/`mark_review_card_sent` for the same
    // job), and this incidentally also serializes this job's manifest
    // read/write against a concurrent `handle_save` call.
    let _lock = acquire_deliverables_job_lock(role, params.job_id)?;

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
// When `job_submitted` arrives before the A2A `[intent:deliver]` message,
// the user has no deliverable to review yet. A marker file is written so
// that the later `deliverable_received` event can detect this and directly
// output the review prompt instead of "wait for job_submitted". The marker's
// content durably carries the triggering event's `expireTime` (empty =
// unknown — covers both a legacy empty-file marker and an event with no
// valid `expireTime`), so the review deadline survives to whichever arrival
// order resumes the flow. All reads/writes of this marker, and of the
// sent-marker below, go through `acquire_deliverables_job_lock("user", ..)`
// so the "is it already sent? / what's the current value? / what should the
// new value be?" sequence is atomic across processes and threads.

fn review_marker_path(job_id: &str) -> Result<PathBuf> {
    Ok(deliverables_dir("user", job_id)?.join("review_awaiting_deliverable"))
}

/// Only a value that can actually be rendered by the card counts as "valid"
/// here — `parse_timestamp_value` accepts any in-range integer, but
/// `format_local_timestamp_with_offset` can still fail for an out-of-range
/// one (e.g. `i64::MAX`). Without this filter such a value could be durably
/// persisted as "the first valid value" and then block a genuinely valid,
/// displayable `expireTime` arriving afterward.
fn formattable(value: Option<i64>) -> Option<i64> {
    value.filter(|&t| deadline::format_local_timestamp_with_offset(t).is_some())
}

/// Lock-free: safe on its own because the writer below only ever publishes
/// a complete value via an atomic `rename()`, so a reader never observes a
/// half-written file. Taking the lock here too would self-deadlock when
/// called internally by the already-locked `write_review_marker`.
pub fn review_marker_event_expire_time(job_id: &str) -> Option<i64> {
    let path = review_marker_path(job_id).ok()?;
    let contents = std::fs::read_to_string(path).ok()?;
    formattable(contents.trim().parse::<i64>().ok())
}

pub fn has_review_marker(job_id: &str) -> bool {
    review_marker_path(job_id).map(|p| p.exists()).unwrap_or(false)
}

/// Lock-free removal of the marker file itself — callers that already hold
/// `acquire_deliverables_job_lock("user", job_id)` (namely
/// `mark_review_card_sent`) must use this instead of the public
/// `delete_review_marker`, which would try to re-acquire the same lock from
/// the same call stack and deadlock.
fn remove_review_marker_file(job_id: &str) {
    if let Ok(p) = review_marker_path(job_id) {
        let _ = std::fs::remove_file(p);
    }
}

pub fn delete_review_marker(job_id: &str) {
    if let Ok(_lock) = acquire_deliverables_job_lock("user", job_id) {
        remove_review_marker_file(job_id);
    }
}

/// Outcome of attempting to persist a `job_submitted` event's `expireTime`.
pub enum ReviewMarkerOutcome {
    /// The card for this job was already confirmed sent (checked inside the
    /// same lock as the decision below — no gap for a duplicate/replayed
    /// event to resurrect the marker right after `mark_review_card_sent`
    /// cleaned it up). Nothing was written or touched.
    AlreadySent,
    /// `persisted` is the value now durably on disk — always use this, even
    /// when it differs from what was just passed in. `conflicting` is
    /// `Some(x)` only when this call's value was a different valid
    /// timestamp than an already-persisted valid value; `x` was rejected
    /// and `persisted` kept the earlier (first-observed) one.
    Written {
        persisted: Option<i64>,
        conflicting: Option<i64>,
    },
}

/// Persist (or confirm) the out-of-order marker for `job_id`, carrying
/// `event_expire_time` (the triggering `job_submitted` event's parsed
/// `expireTime`, if any) durably alongside it. The whole check-decide-write
/// sequence runs under one exclusive lock acquisition — see
/// `acquire_deliverables_job_lock` — so no other process or thread can
/// observe or act on an intermediate state.
pub fn write_review_marker(job_id: &str, event_expire_time: Option<i64>) -> Result<ReviewMarkerOutcome> {
    let _lock = acquire_deliverables_job_lock("user", job_id)?;

    if has_review_card_sent_marker(job_id) {
        return Ok(ReviewMarkerOutcome::AlreadySent);
    }

    let new = formattable(event_expire_time);
    let existing = review_marker_event_expire_time(job_id);
    let (persisted, conflicting) = match (existing, new) {
        (Some(old), Some(new)) if old != new => (Some(old), Some(new)),
        (Some(old), _) => (Some(old), None),
        (None, new) => (new, None),
    };

    let path = review_marker_path(job_id)?;
    let dir = path.parent().expect("review marker has a parent");
    std::fs::create_dir_all(dir)?;
    let contents = persisted.map(|t| t.to_string()).unwrap_or_default();
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let tmp_path = dir.join(format!(
        "review_awaiting_deliverable.tmp.{}.{now}",
        std::process::id()
    ));
    std::fs::write(&tmp_path, &contents)?;
    std::fs::rename(&tmp_path, &path)?;

    Ok(ReviewMarkerOutcome::Written {
        persisted,
        conflicting,
    })
}

// Durable per-job delivery marker shared by the delivery-first and
// job_submitted-first review paths. Queue-mode runtimes still use it as their
// local idempotency guard; CLI-driver runtimes use okx-a2a's stable database
// idempotency key and retain this marker for compatibility and recovery.
fn review_card_sent_marker_path(job_id: &str) -> Result<PathBuf> {
    Ok(deliverables_dir("user", job_id)?.join("review_card_sent"))
}

pub fn has_review_card_sent_marker(job_id: &str) -> bool {
    review_card_sent_marker_path(job_id).map(|p| p.is_file()).unwrap_or(false)
}

/// Writes the sent-marker and removes the now-moot awaiting-marker inside a
/// single lock acquisition. Two separate locked calls here would reopen the
/// same TOCTOU window `write_review_marker`'s internal sent-check closes,
/// one level down: a `write_review_marker` call from another process could
/// slip into the gap between them and resurrect the awaiting-marker right
/// after it was correctly cleaned up.
pub fn mark_review_card_sent(job_id: &str) -> Result<()> {
    let _lock = acquire_deliverables_job_lock("user", job_id)?;
    let path = review_card_sent_marker_path(job_id)?;
    std::fs::create_dir_all(path.parent().expect("review marker has a parent"))?;
    std::fs::write(path, "")?;
    remove_review_marker_file(job_id);
    Ok(())
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

    #[test]
    fn review_card_sent_marker_is_durable_per_job() {
        let _lock = crate::home::TEST_ENV_MUTEX
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let root = std::env::current_dir().unwrap()
            .join("target").join("deliverables-review-marker-test");
        std::fs::create_dir_all(&root).unwrap();
        let home = tempfile::tempdir_in(root).unwrap();
        std::env::set_var("ONCHAINOS_HOME", home.path());

        assert!(!has_review_card_sent_marker("job-review-marker"));
        mark_review_card_sent("job-review-marker").unwrap();
        assert!(has_review_card_sent_marker("job-review-marker"));
        assert!(!has_review_card_sent_marker("another-job"));

        std::env::remove_var("ONCHAINOS_HOME");
    }

    // ── review_awaiting_deliverable marker (event-only deadline, fixes #1-#6) ──

    fn with_temp_home<F: FnOnce()>(prefix: &str, f: F) {
        let _lock = crate::home::TEST_ENV_MUTEX
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let root = std::env::current_dir()
            .unwrap()
            .join("target")
            .join("deliverables-review-marker-test");
        std::fs::create_dir_all(&root).unwrap();
        let home = tempfile::Builder::new()
            .prefix(prefix)
            .tempdir_in(&root)
            .unwrap();
        std::env::set_var("ONCHAINOS_HOME", home.path());
        f();
        std::env::remove_var("ONCHAINOS_HOME");
    }

    #[test]
    fn write_review_marker_round_trips_a_valid_value() {
        with_temp_home("marker-roundtrip", || {
            let outcome = write_review_marker("job-a", Some(1_700_000_000)).unwrap();
            match outcome {
                ReviewMarkerOutcome::Written { persisted, conflicting } => {
                    assert_eq!(persisted, Some(1_700_000_000));
                    assert!(conflicting.is_none());
                }
                ReviewMarkerOutcome::AlreadySent => panic!("must not be already sent"),
            }
            assert!(has_review_marker("job-a"));
            assert_eq!(review_marker_event_expire_time("job-a"), Some(1_700_000_000));
        });
    }

    #[test]
    fn write_review_marker_none_first_time_marks_present_with_unknown_value() {
        with_temp_home("marker-none-first", || {
            let outcome = write_review_marker("job-b", None).unwrap();
            match outcome {
                ReviewMarkerOutcome::Written { persisted, conflicting } => {
                    assert_eq!(persisted, None);
                    assert!(conflicting.is_none());
                }
                ReviewMarkerOutcome::AlreadySent => panic!("must not be already sent"),
            }
            assert!(has_review_marker("job-b"));
            assert_eq!(review_marker_event_expire_time("job-b"), None);
        });
    }

    #[test]
    fn review_marker_event_expire_time_treats_legacy_empty_marker_as_unknown() {
        with_temp_home("marker-legacy-empty", || {
            let path = review_marker_path("job-c").unwrap();
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(&path, "").unwrap();
            assert!(has_review_marker("job-c"));
            assert_eq!(review_marker_event_expire_time("job-c"), None);
        });
    }

    #[test]
    fn write_review_marker_missing_new_value_does_not_clobber_existing_valid_one() {
        with_temp_home("marker-keep-existing", || {
            write_review_marker("job-d", Some(1_700_000_000)).unwrap();
            let outcome = write_review_marker("job-d", None).unwrap();
            match outcome {
                ReviewMarkerOutcome::Written { persisted, conflicting } => {
                    assert_eq!(persisted, Some(1_700_000_000));
                    assert!(conflicting.is_none());
                }
                ReviewMarkerOutcome::AlreadySent => panic!("must not be already sent"),
            }
            assert_eq!(review_marker_event_expire_time("job-d"), Some(1_700_000_000));
        });
    }

    #[test]
    fn write_review_marker_differing_valid_values_keeps_first_and_reports_conflict() {
        with_temp_home("marker-conflict", || {
            write_review_marker("job-e", Some(1_700_000_000)).unwrap();
            let outcome = write_review_marker("job-e", Some(1_700_000_999)).unwrap();
            match outcome {
                ReviewMarkerOutcome::Written { persisted, conflicting } => {
                    assert_eq!(persisted, Some(1_700_000_000));
                    assert_eq!(conflicting, Some(1_700_000_999));
                }
                ReviewMarkerOutcome::AlreadySent => panic!("must not be already sent"),
            }
            assert_eq!(review_marker_event_expire_time("job-e"), Some(1_700_000_000));
        });
    }

    #[test]
    fn write_review_marker_identical_repeated_value_is_idempotent_no_conflict() {
        with_temp_home("marker-idempotent", || {
            write_review_marker("job-f", Some(1_700_000_000)).unwrap();
            let outcome = write_review_marker("job-f", Some(1_700_000_000)).unwrap();
            match outcome {
                ReviewMarkerOutcome::Written { persisted, conflicting } => {
                    assert_eq!(persisted, Some(1_700_000_000));
                    assert!(conflicting.is_none());
                }
                ReviewMarkerOutcome::AlreadySent => panic!("must not be already sent"),
            }
        });
    }

    #[test]
    fn write_review_marker_filters_unformattable_value_so_it_never_blocks_a_real_one() {
        with_temp_home("marker-unformattable-write", || {
            write_review_marker("job-g", Some(i64::MAX)).unwrap();
            // The out-of-range value must never be treated as "the first
            // valid value" — it should be filtered to None, not recorded.
            assert_eq!(review_marker_event_expire_time("job-g"), None);

            let outcome = write_review_marker("job-g", Some(1_700_000_000)).unwrap();
            match outcome {
                ReviewMarkerOutcome::Written { persisted, conflicting } => {
                    assert_eq!(persisted, Some(1_700_000_000));
                    assert!(
                        conflicting.is_none(),
                        "the unformattable value must not block a real one"
                    );
                }
                ReviewMarkerOutcome::AlreadySent => panic!("must not be already sent"),
            }
        });
    }

    #[test]
    fn review_marker_event_expire_time_filters_unformattable_on_disk_content() {
        with_temp_home("marker-unformattable-read", || {
            let path = review_marker_path("job-h").unwrap();
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(&path, i64::MAX.to_string()).unwrap();
            assert_eq!(review_marker_event_expire_time("job-h"), None);
        });
    }

    #[test]
    fn mark_review_card_sent_clears_the_awaiting_marker_in_one_critical_section() {
        with_temp_home("marker-mark-sent-clears", || {
            write_review_marker("job-i", Some(1_700_000_000)).unwrap();
            assert!(has_review_marker("job-i"));

            mark_review_card_sent("job-i").unwrap();

            assert!(!has_review_marker("job-i"));
            assert!(has_review_card_sent_marker("job-i"));
        });
    }

    #[test]
    fn write_review_marker_leaves_no_temp_file_behind() {
        with_temp_home("marker-no-tmp-leftover", || {
            write_review_marker("job-l", Some(1_700_000_000)).unwrap();
            let dir = deliverables_dir("user", "job-l").unwrap();
            let leftover = std::fs::read_dir(&dir)
                .unwrap()
                .flatten()
                .any(|e| e.file_name().to_string_lossy().contains(".tmp."));
            assert!(!leftover, "no temp file should remain after a successful write");
        });
    }

    /// Deterministic, controlled-order concurrency test for the exact
    /// interleaving the review flagged: a `write_review_marker` call must
    /// never resurrect the awaiting-marker if it overlaps with another
    /// thread/process completing `mark_review_card_sent` for the same job.
    /// We force the overlap by holding the production lock ourselves
    /// (simulating "the other process is mid-critical-section") before
    /// spawning the writer thread, so its call is guaranteed to either
    /// block on the lock or find the sent-marker already written — both
    /// orderings must yield `AlreadySent`, regardless of OS scheduling.
    #[test]
    fn duplicate_event_during_concurrent_send_confirmation_cannot_resurrect_the_marker() {
        with_temp_home("marker-duplicate-concurrent", || {
            let job_id = "job-j";
            write_review_marker(job_id, Some(1_700_000_000)).unwrap();
            assert!(has_review_marker(job_id));

            let held = acquire_deliverables_job_lock("user", job_id).unwrap();

            let (ready_tx, ready_rx) = std::sync::mpsc::channel();
            let writer = std::thread::spawn(move || {
                ready_tx.send(()).unwrap();
                write_review_marker(job_id, Some(1_800_000_000))
            });
            ready_rx.recv().unwrap();
            std::thread::sleep(std::time::Duration::from_millis(50));

            // Simulate the rest of mark_review_card_sent's critical section
            // while still holding the lock, exactly as the real function does.
            let sent_path = review_card_sent_marker_path(job_id).unwrap();
            std::fs::create_dir_all(sent_path.parent().unwrap()).unwrap();
            std::fs::write(&sent_path, "").unwrap();
            remove_review_marker_file(job_id);

            drop(held);

            let outcome = writer.join().unwrap().unwrap();
            assert!(matches!(outcome, ReviewMarkerOutcome::AlreadySent));
            assert!(!has_review_marker(job_id));
        });
    }

    /// Uncontrolled-ordering smoke test: proves the shared flock prevents
    /// corruption under real concurrent access. It does not and cannot
    /// assert a specific thread-scheduling winner.
    #[test]
    fn concurrent_writers_never_corrupt_the_marker_file() {
        with_temp_home("marker-concurrent-smoke", || {
            let job_id = "job-k";
            let handles: Vec<_> = (0..8)
                .map(|i| {
                    let value = if i % 2 == 0 { 1_700_000_000 } else { 1_700_000_001 };
                    std::thread::spawn(move || write_review_marker(job_id, Some(value)))
                })
                .collect();
            for h in handles {
                h.join().unwrap().unwrap();
            }

            let final_value = review_marker_event_expire_time(job_id);
            assert!(
                final_value == Some(1_700_000_000) || final_value == Some(1_700_000_001),
                "final value must be exactly one of the written values, got {final_value:?}"
            );

            let dir = deliverables_dir("user", job_id).unwrap();
            let leftover = std::fs::read_dir(&dir)
                .unwrap()
                .flatten()
                .any(|e| e.file_name().to_string_lossy().contains(".tmp."));
            assert!(!leftover, "no temp file should remain after concurrent writes");
        });
    }

    /// Regression test for the path-stability requirement: the lock and
    /// marker must follow `handle_save`'s one-time bare→titled directory
    /// rename, never leaving an orphaned lock/marker pair behind in a
    /// resurrected bare directory.
    #[test]
    fn marker_and_lock_follow_the_directory_rename_without_orphaning() {
        with_temp_home("marker-rename-stability", || {
            let job_id = "job-rename";
            write_review_marker(job_id, Some(1_700_000_000)).unwrap();
            let bare_dir = deliverables_dir("user", job_id).unwrap();
            assert!(bare_dir.ends_with(job_id), "{bare_dir:?}");

            let test_root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("target")
                .join("test_tmp");
            std::fs::create_dir_all(&test_root).unwrap();
            let src_dir = tempfile::Builder::new()
                .prefix("deliverable-src-")
                .tempdir_in(&test_root)
                .unwrap();
            let src_path = src_dir.path().join("deliverable.txt");
            std::fs::write(&src_path, "hello").unwrap();

            handle_save(&SaveParams {
                job_id,
                role: "user",
                file_path: src_path.to_str().unwrap(),
                deliverable_type: "text",
                title: "Rename Stability",
                short_id: job_id,
                file_key: None,
                token_symbol: None,
                token_amount: None,
                counterparty_agent_id: None,
                counterparty_name: None,
            })
            .unwrap();

            let titled_dir = deliverables_dir("user", job_id).unwrap();
            assert_ne!(titled_dir, bare_dir, "the directory must have been renamed");
            assert!(
                !bare_dir.exists(),
                "the old bare directory must not have been resurrected"
            );

            // The marker moved with the directory, not left behind orphaned.
            assert_eq!(review_marker_event_expire_time(job_id), Some(1_700_000_000));

            // A later write_review_marker call resolves the same lock/marker
            // (keyed by (role, job_id) alone), not a stale path under the
            // now-gone bare directory — confirmed by first-wins semantics.
            write_review_marker(job_id, Some(1_800_000_000)).unwrap();
            assert_eq!(review_marker_event_expire_time(job_id), Some(1_700_000_000));
        });
    }
}
