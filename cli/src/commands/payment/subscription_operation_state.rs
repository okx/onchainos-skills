//! Durable two-phase state for APP/1 subscription cancellation operations.
//!
//! Signing creates a pending operation without changing the subscription cache.
//! A later `subscription resolve` commits local state only from a matching
//! successful `Payment-Receipt`.

use std::fs::{self, OpenOptions};
use std::path::PathBuf;

use anyhow::{anyhow, Context, Result};
use rand::RngCore;
use serde::{Deserialize, Serialize};

use crate::home;

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum PendingAction {
    Cancel,
    CancelPending,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct OperationState {
    pub operation_id: String,
    pub action: PendingAction,
    pub sub_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub new_sub_id: Option<String>,
    pub offer_id: String,
    pub payer: String,
    pub created_at: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resolution: Option<String>,
    #[serde(default)]
    pub committed: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub receipt_reference: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resolved_at: Option<u64>,
}

fn operations_dir() -> Result<PathBuf> {
    let dir = home::onchainos_home()?.join("subscription_operations");
    fs::create_dir_all(&dir).context("failed to create subscription operation directory")?;
    Ok(dir)
}

fn resolved_dir() -> Result<PathBuf> {
    let dir = operations_dir()?.join("resolved");
    fs::create_dir_all(&dir)
        .context("failed to create subscription operation resolution directory")?;
    Ok(dir)
}

fn sanitize(value: &str) -> String {
    value
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || *c == '_' || *c == '-')
        .collect()
}

fn validate_operation_id(operation_id: &str) -> Result<()> {
    let suffix = operation_id
        .strip_prefix("op_")
        .ok_or_else(|| anyhow!("invalid APP subscription operation id"))?;
    if suffix.len() != 32 || !suffix.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(anyhow!("invalid APP subscription operation id"));
    }
    Ok(())
}

fn pending_path(operation_id: &str) -> Result<PathBuf> {
    validate_operation_id(operation_id)?;
    Ok(operations_dir()?.join(format!("{}.json", sanitize(operation_id))))
}

fn resolved_path(operation_id: &str) -> Result<PathBuf> {
    validate_operation_id(operation_id)?;
    Ok(resolved_dir()?.join(format!("{}.json", sanitize(operation_id))))
}

fn receipt_reference_key(reference: &str) -> String {
    use sha2::{Digest, Sha256};
    hex::encode(Sha256::digest(reference.as_bytes()))
}

pub struct OperationLock {
    _file: fs::File,
}

fn lock(name: &str) -> Result<OperationLock> {
    let dir = operations_dir()?.join("locks");
    fs::create_dir_all(&dir).context("failed to create subscription operation lock directory")?;
    let path = dir.join(format!("{}.lock", sanitize(name)));
    let file = OpenOptions::new()
        .create(true)
        .read(true)
        .write(true)
        .open(&path)
        .with_context(|| format!("open {}", path.display()))?;
    fs2::FileExt::lock_exclusive(&file)
        .with_context(|| format!("lock APP subscription operation {name}"))?;
    Ok(OperationLock { _file: file })
}

pub fn lock_subscription(sub_id: &str) -> Result<OperationLock> {
    lock(&format!("subscription_{}", sub_id.to_ascii_lowercase()))
}

pub fn lock_receipt_reference(reference: &str) -> Result<OperationLock> {
    lock(&format!("receipt_{}", receipt_reference_key(reference)))
}

fn write_atomic(path: &PathBuf, state: &OperationState) -> Result<()> {
    let tmp = path.with_extension("json.tmp");
    let body = serde_json::to_string_pretty(state).context("serialize subscription operation")?;
    fs::write(&tmp, body).with_context(|| format!("write {}", tmp.display()))?;
    fs::rename(&tmp, path).with_context(|| format!("rename into {}", path.display()))?;
    Ok(())
}

pub fn ensure_no_pending(sub_id: &str) -> Result<()> {
    for entry in fs::read_dir(operations_dir()?).context("read subscription operations")? {
        let entry = entry.context("read subscription operation entry")?;
        if entry.path().extension().and_then(|value| value.to_str()) != Some("json") {
            continue;
        }
        let Ok(body) = fs::read_to_string(entry.path()) else {
            continue;
        };
        let Ok(state) = serde_json::from_str::<OperationState>(&body) else {
            continue;
        };
        if resolved_path(&state.operation_id)?.exists() {
            let _ = fs::remove_file(entry.path());
            continue;
        }
        if state.sub_id.eq_ignore_ascii_case(sub_id) && state.resolution.is_none() {
            return Err(anyhow!(
                "APP subscription operation {} is pending for subId {}; resolve it before signing another cancellation",
                state.operation_id,
                state.sub_id
            ));
        }
    }
    Ok(())
}

pub fn begin_pending(
    action: PendingAction,
    sub_id: &str,
    new_sub_id: Option<&str>,
    offer_id: &str,
    payer: &str,
) -> Result<OperationState> {
    ensure_no_pending(sub_id)?;
    let mut random = [0u8; 16];
    rand::rngs::OsRng.fill_bytes(&mut random);
    let state = OperationState {
        operation_id: format!("op_{}", hex::encode(random)),
        action,
        sub_id: sub_id.to_string(),
        new_sub_id: new_sub_id.map(str::to_string),
        offer_id: offer_id.to_string(),
        payer: payer.to_string(),
        created_at: now_unix(),
        resolution: None,
        committed: false,
        receipt_reference: None,
        resolved_at: None,
    };
    write_atomic(&pending_path(&state.operation_id)?, &state)?;
    Ok(state)
}

pub fn read_pending(operation_id: &str) -> Option<OperationState> {
    let body = fs::read_to_string(pending_path(operation_id).ok()?).ok()?;
    let state = serde_json::from_str::<OperationState>(&body).ok()?;
    state.resolution.is_none().then_some(state)
}

pub fn read_resolution(operation_id: &str) -> Option<OperationState> {
    let body = fs::read_to_string(resolved_path(operation_id).ok()?).ok()?;
    let state = serde_json::from_str::<OperationState>(&body).ok()?;
    if state.resolution.is_some() {
        // A crash after persisting the resolution but before removing pending
        // must not block this subscription forever.
        let _ = fs::remove_file(pending_path(operation_id).ok()?);
        Some(state)
    } else {
        None
    }
}

pub fn receipt_reference_used(reference: &str) -> Result<bool> {
    let dir = resolved_dir()?;
    for entry in fs::read_dir(&dir).with_context(|| format!("read {}", dir.display()))? {
        let entry = entry.with_context(|| format!("read {} entry", dir.display()))?;
        if entry.path().extension().and_then(|value| value.to_str()) != Some("json") {
            continue;
        }
        let Ok(body) = fs::read_to_string(entry.path()) else {
            continue;
        };
        let Ok(state) = serde_json::from_str::<OperationState>(&body) else {
            continue;
        };
        if state.receipt_reference.as_deref() == Some(reference) {
            return Ok(true);
        }
    }
    Ok(false)
}

pub fn resolve(
    mut state: OperationState,
    resolution: &str,
    committed: bool,
    receipt_reference: Option<&str>,
) -> Result<OperationState> {
    state.resolution = Some(resolution.to_string());
    state.committed = committed;
    state.receipt_reference = receipt_reference.map(str::to_string);
    state.resolved_at = Some(now_unix());
    let destination = resolved_path(&state.operation_id)?;
    write_atomic(&destination, &state)?;
    let source = pending_path(&state.operation_id)?;
    // The resolved record is the durable commit point. Cleanup is retried by
    // read_resolution/ensure_no_pending if the process exits here.
    let _ = fs::remove_file(&source);
    Ok(state)
}

pub fn now_unix() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn with_home<F: FnOnce()>(sub: &str, f: F) {
        let _lock = home::TEST_ENV_MUTEX.lock().unwrap();
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("target")
            .join("test_tmp")
            .join(sub);
        let _ = fs::remove_dir_all(&dir);
        std::env::set_var("ONCHAINOS_HOME", &dir);
        f();
        std::env::remove_var("ONCHAINOS_HOME");
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn pending_operation_round_trips_and_blocks_same_subscription() {
        with_home("subscription_operation_pending", || {
            let _lock = lock_subscription("0xsub").unwrap();
            ensure_no_pending("0xsub").unwrap();
            let state =
                begin_pending(PendingAction::Cancel, "0xsub", None, "offer-1", "0xpayer").unwrap();
            assert_eq!(read_pending(&state.operation_id), Some(state.clone()));
            assert!(ensure_no_pending("0xsub").is_err());
            ensure_no_pending("0xother").unwrap();
        });
    }

    #[test]
    fn resolved_operation_is_idempotency_record() {
        with_home("subscription_operation_resolved", || {
            let state = begin_pending(
                PendingAction::CancelPending,
                "0xsub",
                Some("0xnew"),
                "offer-1",
                "0xpayer",
            )
            .unwrap();
            let resolved = resolve(state, "success", true, Some("0xtx1")).unwrap();
            assert!(read_pending(&resolved.operation_id).is_none());
            assert_eq!(read_resolution(&resolved.operation_id), Some(resolved));
            assert!(receipt_reference_used("0xtx1").unwrap());
        });
    }

    #[test]
    fn resolved_record_cleans_a_crash_leftover_pending_file() {
        with_home("subscription_operation_crash_cleanup", || {
            let state =
                begin_pending(PendingAction::Cancel, "0xsub", None, "offer-1", "0xpayer").unwrap();
            let pending = pending_path(&state.operation_id).unwrap();
            let pending_body = fs::read(&pending).unwrap();
            let resolved = resolve(state, "success", true, Some("0xtx1")).unwrap();

            // Simulate the crash window by restoring the old pending record.
            fs::write(&pending, pending_body).unwrap();
            assert!(pending.exists());
            assert_eq!(read_resolution(&resolved.operation_id), Some(resolved));
            assert!(!pending.exists());
            ensure_no_pending("0xsub").unwrap();
        });
    }

    #[test]
    fn operation_paths_reject_noncanonical_ids() {
        for operation_id in [
            "",
            "op_1",
            "../op_00000000000000000000000000000000",
            "op_gggggggggggggggggggggggggggggggg",
        ] {
            assert!(pending_path(operation_id).is_err());
            assert!(resolved_path(operation_id).is_err());
        }
    }
}
