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

use std::fs;
use std::path::PathBuf;

use anyhow::{Context, Result};
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
}

/// Best-effort read; `None` when the file is missing or unparseable.
pub fn read(channel_id: &str) -> Option<ChannelState> {
    let path = state_path(channel_id).ok()?;
    let body = fs::read_to_string(&path).ok()?;
    serde_json::from_str(&body).ok()
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
}
