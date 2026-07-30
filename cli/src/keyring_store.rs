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
/// If file_keyring fails (corrupted / undecryptable), we surface an actionable
/// `Err` to the caller instead of silently purging every credential — silently
/// clearing hides the fault and traps the user in an "expired → re-login →
/// still expired" loop with no explanation (spec §3 / §8.5 #7). The caller maps
/// the error to exit code 1.
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
        Err(_) => {
            // Credential corruption is surfaced to the caller instead of being
            // silently purged: silently clearing all credentials hides the fault
            // and forces the user through an "expired → re-login → still expired"
            // loop with no explanation. See spec §3 / §8.5 #7.
            return Err(anyhow::anyhow!(
                "Credentials corrupted. Please login again: onchainos wallet login"
            ));
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
        Err(_) => {
            // Same as the Linux path: surface corruption to the caller rather
            // than silently purging every credential. See spec §3 / §8.5 #7.
            Err(anyhow::anyhow!(
                "Credentials corrupted. Please login again: onchainos wallet login"
            ))
        }
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
/// On an unreadable/corrupted store, overwrites from empty instead of erroring,
/// so the login write path can always re-establish credentials.
pub fn store(credentials: &[(&str, &str)]) -> Result<()> {
    let mut map = read_blob().unwrap_or_default();
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    /// Run `f` inside a sandboxed `ONCHAINOS_HOME` so credential files live in a
    /// throwaway dir. Mirrors `file_keyring::tests::with_temp_home` and shares the
    /// same `TEST_ENV_MUTEX` so env-var mutation is serialized across modules.
    fn with_temp_home<F: FnOnce()>(name: &str, f: F) {
        let _lock = crate::home::TEST_ENV_MUTEX
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("target")
            .join("test_tmp")
            .join(format!("ks_{name}"));
        if dir.exists() {
            fs::remove_dir_all(&dir).ok();
        }
        fs::create_dir_all(&dir).unwrap();
        std::env::set_var("ONCHAINOS_HOME", &dir);
        f();
        std::env::remove_var("ONCHAINOS_HOME");
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn read_blob_returns_err_when_file_keyring_corrupted() {
        with_temp_home("corrupt_returns_err", || {
            // Write a valid blob so the machine-identity + keyring.enc exist.
            let mut map = HashMap::new();
            map.insert("access_token".to_string(), "tok-123".to_string());
            file_keyring::write_blob(&map).unwrap();

            // Corrupt keyring.enc: long enough to clear the salt+nonce length
            // check but undecryptable, so file_keyring::read_blob() returns Err.
            let path = crate::home::onchainos_home().unwrap().join("keyring.enc");
            fs::write(
                &path,
                b"this is not valid encrypted data at all, needs to be long enough for salt+nonce",
            )
            .unwrap();

            let err = read_blob().expect_err("corrupted keyring must return Err");
            let msg = format!("{err:#}").to_lowercase();
            assert!(
                msg.contains("please login again"),
                "error message should guide the user to re-login, got: {err:#}"
            );
        });
    }

    #[test]
    fn store_recovers_from_corrupted_keyring() {
        // read_blob() hard-errors on corruption, but store() (the login write
        // path) must self-heal by overwriting rather than trap the user.
        with_temp_home("store_recovers_corrupt", || {
            let mut map = HashMap::new();
            map.insert("access_token".to_string(), "stale-tok".to_string());
            file_keyring::write_blob(&map).unwrap();

            // Corrupt keyring.enc so read_blob() returns Err.
            let path = crate::home::onchainos_home().unwrap().join("keyring.enc");
            fs::write(
                &path,
                b"this is not valid encrypted data at all, needs to be long enough for salt+nonce",
            )
            .unwrap();
            read_blob().expect_err("precondition: corrupted keyring must read as Err");

            // store() (the login write path) must succeed and overwrite.
            store(&[("access_token", "fresh-tok")]).expect("store must self-heal over corruption");

            let loaded = read_blob().expect("blob must be readable after store recovery");
            assert_eq!(loaded.get("access_token").unwrap(), "fresh-tok");
        });
    }

    #[test]
    fn read_blob_returns_ok_for_valid_blob() {
        with_temp_home("valid_ok", || {
            let mut map = HashMap::new();
            map.insert("access_token".to_string(), "tok-123".to_string());
            map.insert("refresh_token".to_string(), "ref-456".to_string());
            file_keyring::write_blob(&map).unwrap();

            let loaded = read_blob().expect("valid blob must return Ok");
            assert_eq!(loaded.get("access_token").unwrap(), "tok-123");
            assert_eq!(loaded.get("refresh_token").unwrap(), "ref-456");
        });
    }
}
