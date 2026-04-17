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

/// Snapshot of the payment config + live charging state.
///
/// - `endpoints`: path → tier (`"basic"` / `"premium"`) mapping returned from
///   the market config endpoint. Replaces the legacy `basic_paths` /
///   `premium_paths` split.
/// - `accepts`: signing parameters (scheme, network, asset, payTo, tiered
///   `amount`, ...) returned from the same endpoint.
/// - `basic_charging` / `premium_charging`: flipped by the
///   `ok-web3-openapi-pay: Basic=1;Premium=0` response header. `true` means the
///   next request on that tier must be pre-signed.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct PaymentCache {
    #[serde(default)]
    pub endpoints: HashMap<String, String>,
    #[serde(default)]
    pub accepts: Option<Value>,
    #[serde(default)]
    pub basic_charging: bool,
    #[serde(default)]
    pub premium_charging: bool,
    /// Unix seconds when this snapshot was written.
    #[serde(default)]
    pub updated_at: u64,
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
            basic_charging: true,
            premium_charging: false,
            updated_at: 1_700_000_000,
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
        assert!(loaded.basic_charging);
        assert!(!loaded.premium_charging);
        assert_eq!(loaded.updated_at, 1_700_000_000);

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
