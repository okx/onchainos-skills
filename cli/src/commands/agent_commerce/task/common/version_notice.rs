//! Throttle for the non-blocking protocol-version mismatch notice.
//!
//! Cross-process dedup — persists the last-shown timestamp to disk so that
//! repeated triggers within 48 hours suppress further user prompts.
//! File-based (not in-memory) because `onchainos` runs both as a long-lived
//! MCP server and as a one-shot CLI; the latter forks a fresh process per
//! invocation, defeating any in-memory state.
//!
//! Location: `~/.onchainos/task/version_notice_last_shown.txt` (single line,
//! unix seconds). Lives alongside other task-system state files
//! (`pending-decisions.json`, `<jobId>/negotiate-state.json`, etc.) under
//! `~/.onchainos/task/`. All I/O errors fail open (default to showing the
//! notice) so a broken state file never permanently silences upgrade prompts.

use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

const THROTTLE_SECS: u64 = 2 * 24 * 60 * 60;
const FILE_NAME: &str = "version_notice_last_shown.txt";

fn state_path() -> Option<PathBuf> {
    crate::home::onchainos_home()
        .ok()
        .map(|d| d.join("task").join(FILE_NAME))
}

fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

pub fn should_show() -> bool {
    let Some(path) = state_path() else {
        return true;
    };
    let Ok(raw) = std::fs::read_to_string(&path) else {
        return true;
    };
    let Ok(last_ts) = raw.trim().parse::<u64>() else {
        return true;
    };
    now_secs().saturating_sub(last_ts) >= THROTTLE_SECS
}

pub fn mark_shown() {
    let Some(path) = state_path() else {
        return;
    };
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let _ = std::fs::write(&path, now_secs().to_string());
}
