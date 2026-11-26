//! Per-channel MPP session state for the down-sunk
//! `payment session {open,voucher,topup,close}` decision layer.
//!
//! Persists the channel deposit + latest authorized cumulative, keyed by
//! `channelId`, to `~/.onchainos/sessions/{channel_id}.json`, so:
//! - `voucher` can compute `needsTopUp` from the real deposit,
//! - `close` can compute `refund = deposit - final_cum`,
//!
//! without the agent doing any arithmetic. Like `state.rs`, this file holds NO
//! key material and NO signed voucher — only the amounts needed for the
//! decision math. Reads are best-effort: a missing or corrupt file degrades to
//! "no prior state" rather than failing the command (the seller SDK remains the
//! authority on the true channel balance).

use std::fs::{self, OpenOptions};
use std::path::PathBuf;

use anyhow::{anyhow, Context, Result};
use rand::RngCore;
use serde::{Deserialize, Serialize};

use crate::home;

/// Persisted per-channel state. Atomic-integer strings; never a key or a signature.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, Default)]
pub struct ChannelState {
    pub channel_id: String,
    /// Selected account id that opened the channel.
    pub owner_wallet: String,
    /// Total escrow deposit (atomic units), grown by `topup`.
    pub deposit: String,
    /// Latest authorized cumulative voucher amount (atomic units).
    pub cumulative: String,
    pub created_at: u64,
    pub updated_at: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pending_operation: Option<PendingOperation>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub resolved_operations: Vec<ResolvedOperation>,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum PendingAction {
    Topup,
    Close,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct PendingOperation {
    pub operation_id: String,
    pub action: PendingAction,
    pub amount: String,
    pub offer_id: String,
    pub created_at: u64,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct ResolvedOperation {
    pub operation_id: String,
    pub channel_id: String,
    pub action: PendingAction,
    pub resolution: String,
    pub committed: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub receipt_reference: Option<String>,
    pub resolved_at: u64,
}

/// `~/.onchainos/sessions/`, created (0700 inherited from the home dir) on demand.
fn sessions_dir() -> Result<PathBuf> {
    let dir = home::onchainos_home()?.join("sessions");
    fs::create_dir_all(&dir).context("failed to create ~/.onchainos/sessions")?;
    Ok(dir)
}

/// Sanitize a channelId into a safe filename stem: keep `[0-9a-zA-Z_-]`, drop the
/// rest. channelIds are `0x`-prefixed hex in practice, so this is loss-free for
/// real inputs while foreclosing any path-traversal via a crafted id.
fn sanitize(channel_id: &str) -> String {
    channel_id
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || *c == '_' || *c == '-')
        .collect()
}

/// `~/.onchainos/sessions/{sanitized_channel_id}.json`.
fn state_path(channel_id: &str) -> Result<PathBuf> {
    Ok(sessions_dir()?.join(format!("{}.json", sanitize(channel_id))))
}

fn resolved_dir() -> Result<PathBuf> {
    let dir = sessions_dir()?.join("resolved");
    fs::create_dir_all(&dir).context("failed to create session resolution directory")?;
    Ok(dir)
}

fn receipt_reference_key(reference: &str) -> String {
    use sha2::{Digest, Sha256};
    hex::encode(Sha256::digest(reference.as_bytes()))
}

pub struct ChannelLock {
    _file: fs::File,
}

pub fn lock(channel_id: &str) -> Result<ChannelLock> {
    let dir = sessions_dir()?.join("locks");
    fs::create_dir_all(&dir).context("failed to create session lock directory")?;
    let path = dir.join(format!("{}.lock", sanitize(channel_id)));
    let file = OpenOptions::new()
        .create(true)
        .read(true)
        .write(true)
        .open(&path)
        .with_context(|| format!("open {}", path.display()))?;
    fs2::FileExt::lock_exclusive(&file)
        .with_context(|| format!("lock APP session channel {channel_id}"))?;
    Ok(ChannelLock { _file: file })
}

pub fn lock_receipt_reference(reference: &str) -> Result<ChannelLock> {
    lock(&format!("receipt_{}", receipt_reference_key(reference)))
}

impl ChannelState {
    /// Atomic write (`tmp` → `rename`), mirroring `state.rs`.
    pub fn write(&self) -> Result<()> {
        let path = state_path(&self.channel_id)?;
        let tmp = path.with_extension("json.tmp");
        let body = serde_json::to_string_pretty(self).context("serialize channel state")?;
        fs::write(&tmp, body).with_context(|| format!("write {}", tmp.display()))?;
        fs::rename(&tmp, &path).with_context(|| format!("rename into {}", path.display()))?;
        Ok(())
    }

    pub fn ensure_no_pending(&self) -> Result<()> {
        if let Some(operation) = &self.pending_operation {
            return Err(anyhow!(
                "APP session operation {} is pending for channel {}; resolve it before signing another operation",
                operation.operation_id,
                self.channel_id
            ));
        }
        Ok(())
    }

    pub fn begin_pending(
        &mut self,
        action: PendingAction,
        amount: &str,
        offer_id: &str,
    ) -> Result<String> {
        self.ensure_no_pending()?;
        let mut random = [0u8; 16];
        rand::rngs::OsRng.fill_bytes(&mut random);
        let operation_id = format!("op_{}", hex::encode(random));
        let now = now_unix();
        self.pending_operation = Some(PendingOperation {
            operation_id: operation_id.clone(),
            action,
            amount: amount.to_string(),
            offer_id: offer_id.to_string(),
            created_at: now,
        });
        self.updated_at = now;
        Ok(operation_id)
    }
}

/// Best-effort read; `None` when the file is missing or unparseable.
pub fn read(channel_id: &str) -> Option<ChannelState> {
    let path = state_path(channel_id).ok()?;
    let body = fs::read_to_string(&path).ok()?;
    serde_json::from_str(&body).ok()
}

pub fn find_pending(operation_id: &str) -> Result<Option<ChannelState>> {
    for entry in fs::read_dir(sessions_dir()?).context("read ~/.onchainos/sessions")? {
        let entry = entry.context("read session state entry")?;
        if entry.path().extension().and_then(|value| value.to_str()) != Some("json") {
            continue;
        }
        let Ok(body) = fs::read_to_string(entry.path()) else {
            continue;
        };
        let Ok(state) = serde_json::from_str::<ChannelState>(&body) else {
            continue;
        };
        if state
            .pending_operation
            .as_ref()
            .is_some_and(|pending| pending.operation_id == operation_id)
        {
            return Ok(Some(state));
        }
    }
    Ok(None)
}

pub fn read_resolution(operation_id: &str) -> Option<ResolvedOperation> {
    let active_dir = sessions_dir().ok()?;
    let resolved_dir = active_dir.join("resolved");
    for dir in [&active_dir, &resolved_dir] {
        for entry in fs::read_dir(dir).ok()?.flatten() {
            if entry.path().extension().and_then(|value| value.to_str()) != Some("json") {
                continue;
            }
            let Ok(body) = fs::read_to_string(entry.path()) else {
                continue;
            };
            let Ok(state) = serde_json::from_str::<ChannelState>(&body) else {
                continue;
            };
            if let Some(resolved) = state
                .resolved_operations
                .into_iter()
                .find(|resolved| resolved.operation_id == operation_id)
            {
                return Some(resolved);
            }
            if dir == &resolved_dir {
                if let Some(pending) = state
                    .pending_operation
                    .filter(|pending| pending.operation_id == operation_id)
                {
                    return Some(ResolvedOperation {
                        operation_id: pending.operation_id,
                        channel_id: state.channel_id,
                        action: pending.action,
                        resolution: "success".to_string(),
                        committed: true,
                        receipt_reference: None,
                        resolved_at: pending.created_at,
                    });
                }
            }
        }
    }
    None
}

pub fn receipt_reference_used(reference: &str) -> Result<bool> {
    let key = receipt_reference_key(reference);
    let active_dir = sessions_dir()?;
    let archive_dir = resolved_dir()?;
    for dir in [&active_dir, &archive_dir] {
        for entry in fs::read_dir(dir).with_context(|| format!("read {}", dir.display()))? {
            let entry = entry.with_context(|| format!("read {} entry", dir.display()))?;
            if dir == &archive_dir
                && entry
                    .file_name()
                    .to_string_lossy()
                    .contains(&format!("__{key}.json"))
            {
                return Ok(true);
            }
            if entry.path().extension().and_then(|value| value.to_str()) != Some("json") {
                continue;
            }
            let Ok(body) = fs::read_to_string(entry.path()) else {
                continue;
            };
            let Ok(state) = serde_json::from_str::<ChannelState>(&body) else {
                continue;
            };
            if state.resolved_operations.iter().any(|resolved| {
                resolved.receipt_reference.as_deref() == Some(reference)
            }) {
                return Ok(true);
            }
        }
    }
    Ok(false)
}

/// A successful close atomically moves the active channel state to an archive.
/// The archived pending operation is the idempotency record for that close.
pub fn archive_close_success(
    channel_id: &str,
    operation_id: &str,
    receipt_reference: &str,
) -> Result<()> {
    let source = state_path(channel_id)?;
    let destination = resolved_dir()?.join(format!(
        "{}__{}.json",
        sanitize(operation_id),
        receipt_reference_key(receipt_reference)
    ));
    fs::rename(&source, &destination).with_context(|| {
        format!(
            "archive closed APP session {} to {}",
            source.display(),
            destination.display()
        )
    })
}

/// Best-effort delete — on channel `close`.
pub fn cleanup(channel_id: &str) {
    if let Ok(path) = state_path(channel_id) {
        let _ = fs::remove_file(path);
    }
}

/// Current Unix seconds (never negative).
pub fn now_unix() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
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
    fn write_read_round_trip_and_cleanup() {
        with_home("session_state_round_trip", || {
            let st = ChannelState {
                channel_id: "0xabc123".into(),
                owner_wallet: "acc-1".into(),
                deposit: "100000".into(),
                cumulative: "40000".into(),
                created_at: 1_000,
                updated_at: 1_000,
                pending_operation: None,
                resolved_operations: Vec::new(),
            };
            st.write().unwrap();
            let got = read("0xabc123").expect("state should round-trip");
            assert_eq!(got.deposit, "100000");
            assert_eq!(got.cumulative, "40000");
            cleanup("0xabc123");
            assert!(read("0xabc123").is_none(), "cleanup should remove the file");
        });
    }

    #[test]
    fn read_missing_is_none() {
        with_home("session_state_missing", || {
            assert!(read("0xdoes-not-exist").is_none());
        });
    }

    #[test]
    fn sanitize_strips_path_separators() {
        assert_eq!(sanitize("../../etc/passwd"), "etcpasswd");
        assert_eq!(
            sanitize("0xDEADbeef_01-ff"),
            "0xDEADbeef_01-ff",
            "hex + _ + - are preserved loss-free"
        );
    }

    #[test]
    fn pending_operation_round_trips_and_blocks_a_second_operation() {
        with_home("session_state_pending", || {
            let mut state = ChannelState {
                channel_id: "0xpending".into(),
                owner_wallet: "acc-1".into(),
                deposit: "100".into(),
                cumulative: "10".into(),
                created_at: 1,
                updated_at: 1,
                pending_operation: None,
                resolved_operations: Vec::new(),
            };
            let operation_id = state
                .begin_pending(PendingAction::Topup, "25", "offer-1")
                .unwrap();
            state.write().unwrap();
            let found = find_pending(&operation_id).unwrap().unwrap();
            assert_eq!(found.channel_id, "0xpending");
            assert!(found
                .clone()
                .begin_pending(PendingAction::Close, "10", "offer-1")
                .is_err());

            let resolved = ResolvedOperation {
                operation_id: operation_id.clone(),
                channel_id: "0xpending".into(),
                action: PendingAction::Topup,
                resolution: "success".into(),
                committed: true,
                receipt_reference: Some("tx-1".into()),
                resolved_at: now_unix(),
            };
            let mut state = read("0xpending").unwrap();
            state.resolved_operations.push(resolved.clone());
            state.write().unwrap();
            assert_eq!(read_resolution(&operation_id), Some(resolved));
        });
    }

    #[test]
    fn channel_lock_serializes_pending_writes() {
        with_home("session_state_lock", || {
            let state = ChannelState {
                channel_id: "0xlocked".into(),
                owner_wallet: "wallet-1".into(),
                deposit: "100".into(),
                cumulative: "10".into(),
                created_at: 1,
                updated_at: 1,
                pending_operation: None,
                resolved_operations: Vec::new(),
            };
            state.write().unwrap();
            let (ready_tx, ready_rx) = std::sync::mpsc::channel();
            let first = std::thread::spawn(move || {
                let _lock = lock("0xlocked").unwrap();
                ready_tx.send(()).unwrap();
                std::thread::sleep(std::time::Duration::from_millis(25));
                let mut state = read("0xlocked").unwrap();
                state
                    .begin_pending(PendingAction::Topup, "20", "offer-1")
                    .unwrap();
                state.write().unwrap();
            });
            ready_rx.recv().unwrap();
            let second = std::thread::spawn(move || {
                let _lock = lock("0xlocked").unwrap();
                read("0xlocked").unwrap().ensure_no_pending().is_err()
            });
            first.join().unwrap();
            assert!(second.join().unwrap());
        });
    }
}
