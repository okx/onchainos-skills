//! Keyring store for onchainos.
//!
//! Sensitive credentials (tokens, session key) are stored as a single JSON blob
//! under one keyring entry ("agentic-wallet"). Non-sensitive session metadata
//! lives in `~/.onchainos/session.json` (see `wallet_store::SessionJson`).

use anyhow::{Context, Result};
use std::collections::HashMap;

const SERVICE: &str = "onchainos";
const UNIFIED_KEY: &str = "agentic-wallet";

// --------------- internal helpers ---------------

/// Read the entire JSON blob from the single keyring entry.
/// Public so callers can batch-read multiple keys in a single OS keyring access.
pub fn read_blob() -> Result<HashMap<String, String>> {
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

/// Write the entire JSON blob back to the single keyring entry.
fn write_blob(map: &HashMap<String, String>) -> Result<()> {
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
pub fn clear_all() -> Result<()> {
    let e = keyring::Entry::new(SERVICE, UNIFIED_KEY).context("failed to create keyring entry")?;
    match e.delete_credential() {
        Ok(()) => Ok(()),
        Err(keyring::Error::NoEntry) => Ok(()),
        Err(err) => Err(err).context("failed to clear keyring"),
    }
}
