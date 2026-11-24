//! Persistent x402 payment state cache.
//!
//! Stored at `~/.onchainos/payment_cache.json` (plain JSON — all fields are
//! non-sensitive server-returned data). Written atomically (tmp + rename) so a
//! mid-write crash cannot corrupt the file.

use std::collections::HashMap;
use std::path::PathBuf;

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::payment_notify::{TierState, UserType};

/// User-selected default payment asset. When set, `payment_flow::select_accept`
/// prefers entries matching `(asset, network)` before falling back to the
/// scheme-priority rule.
///
/// `name` is display-only (e.g. `"USDT"`) and never used for matching —
/// same symbol on different chains is not the same asset.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct PaymentDefault {
    /// EVM token contract address, e.g. `"0xUSDG"`.
    pub asset: String,
    /// CAIP-2 network identifier, e.g. `"eip155:196"`.
    pub network: String,
    /// Display name, e.g. `"USDT"`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
}

/// Snapshot of the payment config + live charging state.
///
/// - `endpoints`: path → tier (`"basic"` / `"premium"`) mapping returned from
///   the market config endpoint. Replaces the legacy `basic_paths` /
///   `premium_paths` split.
/// - `accepts`: signing parameters (scheme, network, asset, payTo, tiered
///   `amount`, ...) returned from the same endpoint.
/// - `basic_state` / `premium_state`: per-tier lifecycle, advanced by the
///   `ok-web3-openapi-pay: Basic=1;Premium=0` header and by OVER_QUOTA
///   notifications. Only `charging_confirmed` allows auto pre-signing.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct PaymentCache {
    #[serde(default)]
    pub endpoints: HashMap<String, String>,
    #[serde(default)]
    pub accepts: Option<Value>,
    #[serde(default)]
    pub basic_state: TierState,
    #[serde(default)]
    pub premium_state: TierState,
    /// Unix seconds when this snapshot was written.
    #[serde(default)]
    pub updated_at: u64,

    // ── Notification state ───────────────────────────────────────────────
    /// `1`→New, `0`→Old from the `UserType=` header field.
    #[serde(default)]
    pub user_type: Option<UserType>,
    #[serde(default)]
    pub intro_shown: bool,
    #[serde(default)]
    pub grace_shown: bool,

    /// User-selected default payment asset. Cleared on logout (via
    /// `PaymentCache::delete`) so the preference never crosses accounts.
    #[serde(default)]
    pub default_asset: Option<PaymentDefault>,

    /// Cross-process dedupe for the local-signing disclaimer. Reset on
    /// logout so a new account sees the warning once.
    #[serde(default)]
    pub local_signing_warned: bool,
}

impl PaymentCache {
    /// Path to the cache file (`~/.onchainos/payment_cache.json`).
    pub fn cache_path() -> Result<PathBuf> {
        Ok(crate::home::onchainos_home()?.join("payment_cache.json"))
    }

    /// Load from disk. Returns `None` if the file is missing or unparseable
    /// (we treat a corrupt cache as "no cache" rather than an error — it will
    /// be rewritten on the next successful config fetch).
    pub fn load() -> Option<Self> {
        let path = Self::cache_path().ok()?;
        if !path.exists() {
            return None;
        }
        let data = std::fs::read(&path).ok()?;
        serde_json::from_slice(&data).ok()
    }

    /// Write atomically: write to a sibling `.tmp` file, then rename over the
    /// real path. Prevents partial writes from surviving a crash.
    pub fn save(&self) -> Result<()> {
        let path = Self::cache_path()?;
        crate::home::ensure_onchainos_home()?;
        let tmp = path.with_extension("json.tmp");
        let bytes = serde_json::to_vec(self).context("serialize payment cache")?;
        std::fs::write(&tmp, &bytes).context("write payment cache tmp")?;
        std::fs::rename(&tmp, &path).context("rename payment cache")?;
        Ok(())
    }

    /// `true` if the snapshot is older than `ttl_secs`.
    pub fn is_expired(&self, ttl_secs: u64) -> bool {
        now_secs().saturating_sub(self.updated_at) > ttl_secs
    }

    /// Remove the cache file from disk. No-op if it doesn't exist. Called on
    /// logout so the next session starts without stale charging flags from a
    /// previous account.
    pub fn delete() -> Result<()> {
        let path = Self::cache_path()?;
        if path.exists() {
            std::fs::remove_file(&path).context("failed to delete payment_cache.json")?;
        }
        Ok(())
    }
}

pub fn now_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn tmp_home(sub: &str) -> PathBuf {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("target")
            .join("test_tmp")
            .join(sub);
        if dir.exists() {
            fs::remove_dir_all(&dir).ok();
        }
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn load_missing_returns_none() {
        let _lock = crate::home::TEST_ENV_MUTEX.lock().unwrap();
        let dir = tmp_home("payment_cache_missing");
        std::env::set_var("ONCHAINOS_HOME", &dir);
        assert!(PaymentCache::load().is_none());
        std::env::remove_var("ONCHAINOS_HOME");
    }

    #[test]
    fn save_then_load_roundtrip() {
        let _lock = crate::home::TEST_ENV_MUTEX.lock().unwrap();
        let dir = tmp_home("payment_cache_roundtrip");
        std::env::set_var("ONCHAINOS_HOME", &dir);

        let cache = PaymentCache {
            endpoints: [
                ("/api/v6/dex/market/price".to_string(), "basic".to_string()),
                ("/api/v6/dex/market/k".to_string(), "premium".to_string()),
            ]
            .into_iter()
            .collect(),
            accepts: Some(serde_json::json!([{"scheme": "exact"}])),
            basic_state: TierState::ChargingConfirmed,
            premium_state: TierState::Free,
            updated_at: 1_700_000_000,
            ..Default::default()
        };
        cache.save().unwrap();

        let loaded = PaymentCache::load().unwrap();
        assert_eq!(
            loaded
                .endpoints
                .get("/api/v6/dex/market/price")
                .map(String::as_str),
            Some("basic")
        );
        assert_eq!(
            loaded
                .endpoints
                .get("/api/v6/dex/market/k")
                .map(String::as_str),
            Some("premium")
        );
        assert_eq!(loaded.basic_state, TierState::ChargingConfirmed);
        assert_eq!(loaded.premium_state, TierState::Free);
        assert_eq!(loaded.updated_at, 1_700_000_000);

        std::env::remove_var("ONCHAINOS_HOME");
    }

    #[test]
    fn default_asset_roundtrips() {
        let _lock = crate::home::TEST_ENV_MUTEX.lock().unwrap();
        let dir = tmp_home("payment_cache_default_asset");
        std::env::set_var("ONCHAINOS_HOME", &dir);

        let cache = PaymentCache {
            default_asset: Some(PaymentDefault {
                asset: "0xUSDG".to_string(),
                network: "eip155:196".to_string(),
                name: Some("USDG".to_string()),
            }),
            updated_at: 1_700_000_000,
            ..Default::default()
        };
        cache.save().unwrap();

        let loaded = PaymentCache::load().unwrap();
        let def = loaded.default_asset.expect("default_asset persisted");
        assert_eq!(def.asset, "0xUSDG");
        assert_eq!(def.network, "eip155:196");
        assert_eq!(def.name.as_deref(), Some("USDG"));

        std::env::remove_var("ONCHAINOS_HOME");
    }

    #[test]
    fn is_expired_respects_ttl() {
        let cache = PaymentCache {
            updated_at: now_secs(),
            ..Default::default()
        };
        assert!(!cache.is_expired(3600));
        let stale = PaymentCache {
            updated_at: now_secs().saturating_sub(7200),
            ..Default::default()
        };
        assert!(stale.is_expired(3600));
    }

    #[test]
    fn delete_removes_existing_cache_file() {
        let _lock = crate::home::TEST_ENV_MUTEX.lock().unwrap();
        let dir = tmp_home("payment_cache_delete");
        std::env::set_var("ONCHAINOS_HOME", &dir);

        PaymentCache::default().save().unwrap();
        let path = dir.join("payment_cache.json");
        assert!(path.exists());

        PaymentCache::delete().unwrap();
        assert!(!path.exists());
        std::env::remove_var("ONCHAINOS_HOME");
    }

    #[test]
    fn delete_is_noop_when_cache_missing() {
        let _lock = crate::home::TEST_ENV_MUTEX.lock().unwrap();
        let dir = tmp_home("payment_cache_delete_missing");
        std::env::set_var("ONCHAINOS_HOME", &dir);

        assert!(PaymentCache::delete().is_ok());
        std::env::remove_var("ONCHAINOS_HOME");
    }

    #[test]
    fn save_is_atomic_no_tmp_leftover() {
        let _lock = crate::home::TEST_ENV_MUTEX.lock().unwrap();
        let dir = tmp_home("payment_cache_atomic");
        std::env::set_var("ONCHAINOS_HOME", &dir);

        let cache = PaymentCache::default();
        cache.save().unwrap();

        let real = dir.join("payment_cache.json");
        let tmp = dir.join("payment_cache.json.tmp");
        assert!(real.exists());
        assert!(!tmp.exists());
        std::env::remove_var("ONCHAINOS_HOME");
    }
}
