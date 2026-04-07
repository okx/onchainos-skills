//! Keyring store for onchainos.
//!
//! Sensitive credentials (tokens, session key) are stored as a single JSON blob
//! under one keyring entry ("agentic-wallet"). Non-sensitive session metadata
//! lives in `~/.onchainos/session.json` (see `wallet_store::SessionJson`).
//!
//! On systems where the OS keyring is unavailable (headless Linux, Docker,
//! minimal distros), we silently fall back to an encrypted local file
//! (`~/.onchainos/keyring.enc`) via the `file_keyring` module.

use anyhow::{Context, Result};
use std::collections::HashMap;

use crate::file_keyring;
use crate::wallet_store;

const SERVICE: &str = "onchainos";
const UNIFIED_KEY: &str = "agentic-wallet";

// --------------- internal helpers ---------------

/// Read the entire JSON blob from the keyring.
/// Public so callers can batch-read multiple keys in a single access.
///
/// Priority: OS keyring first (macOS/Windows always work); fall back to
/// file_keyring only when OS returns empty or errors (headless Linux, Docker).
/// This keeps macOS/Windows behaviour identical to the original code —
/// file_keyring is never touched when the OS keyring is healthy.
///
/// If file_keyring also fails (corrupted / undecryptable), we purge stale
/// data and return Ok(empty) so callers like `store()` can still write
/// fresh credentials — breaking the "expired → re-login → still expired"
/// loop.
pub fn read_blob() -> Result<HashMap<String, String>> {
    if cfg!(target_os = "linux") {
        // Linux: file_keyring is the durable cross-process store.
        // Fall back to OS keyring only if file is empty (e.g. first run
        // before any write, or migrating from an OS-keyring-only install).
        return read_blob_linux();
    }
    // macOS / Windows: OS keyring is reliable and cross-process.
    read_blob_os_first()
}

/// Linux read strategy: file_keyring first, OS keyring fallback.
fn read_blob_linux() -> Result<HashMap<String, String>> {
    match file_keyring::read_blob() {
        Ok(map) if !map.is_empty() => return Ok(map),
        Ok(_) => {} // file empty/missing — try OS keyring
        Err(e) => {
            eprintln!(
                "Warning: failed to read credentials ({}). \
                 Clearing corrupted data — please login again: onchainos wallet login",
                e
            );
            purge_stale_credentials();
            return Ok(HashMap::new());
        }
    }
    // File was empty — try OS keyring (in-session data or legacy install).
    match os_read_blob() {
        Ok(map) if !map.is_empty() => Ok(map),
        Ok(_) => Ok(HashMap::new()),
        Err(_) => Ok(HashMap::new()),
    }
}

/// macOS/Windows read strategy: OS keyring first, file_keyring fallback.
fn read_blob_os_first() -> Result<HashMap<String, String>> {
    match os_read_blob() {
        Ok(map) if !map.is_empty() => return Ok(map),
        Ok(_) => {}
        Err(e) => {
            eprintln!("Warning: OS keyring read failed ({e}), trying file fallback");
        }
    }
    match file_keyring::read_blob() {
        Ok(map) => Ok(map),
        Err(e) => {
            eprintln!(
                "Warning: failed to read credentials ({}). \
                 Clearing corrupted data — please login again: onchainos wallet login",
                e
            );
            purge_stale_credentials();
            Ok(HashMap::new())
        }
    }
}

/// Remove all credential artifacts (OS keyring, file keyring, session.json)
/// so the user can start fresh with `onchainos wallet login`.
fn purge_stale_credentials() {
    if let Err(e) = os_clear_all() {
        eprintln!("Warning: failed to clear OS keyring: {e}");
    }
    if let Err(e) = file_keyring::clear_all() {
        eprintln!("Warning: failed to clear file keyring: {e}");
    }
    if let Err(e) = wallet_store::delete_session() {
        eprintln!("Warning: failed to delete session.json: {e}");
    }
}

/// Write the entire JSON blob to the keyring.
///
/// - macOS/Windows: OS keyring only; file_keyring on failure.
/// - Linux: always write file_keyring (keyutils session keyring is not
///   reliably shared across processes — e.g. a Telegram bot runs in a
///   different session than the user's SSH shell). OS keyring is also
///   attempted best-effort for in-session convenience.
fn write_blob(map: &HashMap<String, String>) -> Result<()> {
    if cfg!(target_os = "linux") {
        // Linux: file_keyring is the durable store; OS keyring best-effort.
        let result = file_keyring::write_blob(map);
        let _ = os_write_blob(map);
        return result;
    }
    // macOS / Windows: OS keyring is reliable, no need to touch file.
    match os_write_blob(map) {
        Ok(()) => Ok(()),
        Err(e) => {
            eprintln!("Warning: OS keyring write failed ({e}), using file fallback");
            file_keyring::write_blob(map)
        }
    }
}

/// Read from OS keyring only.
fn os_read_blob() -> Result<HashMap<String, String>> {
    let e = keyring::Entry::new(SERVICE, UNIFIED_KEY).context("failed to create keyring entry")?;
    match e.get_password() {
        Ok(json) => {
            let map: HashMap<String, String> =
                serde_json::from_str(&json).context("failed to parse keyring blob")?;
            Ok(map)
        }
        Err(keyring::Error::NoEntry) => Ok(HashMap::new()),
        Err(err) => Err(err).context("failed to read keyring blob"),
    }
}

/// Write to OS keyring only.
fn os_write_blob(map: &HashMap<String, String>) -> Result<()> {
    let e = keyring::Entry::new(SERVICE, UNIFIED_KEY).context("failed to create keyring entry")?;
    let json = serde_json::to_string(map).context("failed to serialize keyring blob")?;
    e.set_password(&json)
        .context("failed to write keyring blob")
}

// --------------- public API ---------------

pub fn get(key: &str) -> Result<String> {
    let map = read_blob()?;
    match map.get(key) {
        Some(v) => Ok(v.clone()),
        None => anyhow::bail!("keyring key '{}' not found", key),
    }
}

pub fn get_opt(key: &str) -> Option<String> {
    get(key).ok()
}

pub fn set(key: &str, value: &str) -> Result<()> {
    let mut map = read_blob()?;
    map.insert(key.to_string(), value.to_string());
    write_blob(&map)
}

pub fn delete(key: &str) -> Result<()> {
    let mut map = read_blob()?;
    map.remove(key);
    write_blob(&map)
}

/// Store multiple credentials at once (single read + single write).
pub fn store(credentials: &[(&str, &str)]) -> Result<()> {
    let mut map = read_blob()?;
    for (key, value) in credentials {
        map.insert(key.to_string(), value.to_string());
    }
    write_blob(&map)
}

/// Clear all credentials by deleting the single keyring entry.
/// Also clears the file fallback to ensure no stale credentials remain.
pub fn clear_all() -> Result<()> {
    let _ = os_clear_all();
    file_keyring::clear_all()
}

fn os_clear_all() -> Result<()> {
    let e = keyring::Entry::new(SERVICE, UNIFIED_KEY).context("failed to create keyring entry")?;
    match e.delete_credential() {
        Ok(()) => Ok(()),
        Err(keyring::Error::NoEntry) => Ok(()),
        Err(err) => Err(err).context("failed to clear keyring"),
    }
}
