use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

use crate::home::onchainos_home;

// ── wallets.json ────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct WalletsJson {
    #[serde(default)]
    pub email: String,
    #[serde(default)]
    pub is_new: bool,
    #[serde(default)]
    pub project_id: String,
    #[serde(default)]
    pub selected_account_id: String,
    #[serde(default)]
    pub accounts_map: HashMap<String, AccountMapEntry>,
    #[serde(default)]
    pub accounts: Vec<AccountInfo>,
    #[serde(default)]
    pub is_ak: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct AccountMapEntry {
    pub address_list: Vec<AddressInfo>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AddressInfo {
    #[serde(default)]
    pub account_id: String,
    pub address: String,
    pub chain_index: String,
    #[serde(default)]
    pub chain_name: String,
    #[serde(default)]
    pub address_type: String,
    #[serde(default)]
    pub chain_path: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AccountInfo {
    pub project_id: String,
    pub account_id: String,
    pub account_name: String,
    #[serde(default)]
    pub is_default: bool,
}

// ── cache.json ──────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct CacheJson {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub login: Option<LoginCache>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub swap_trace_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LoginCache {
    pub email: String,
    pub flow_id: String,
}

// ── balance_cache.json
/// ```json
/// {
///   "batch_updated_at": 1773370520,
///   "accounts": {
///     "<account_id>": { "updated_at": 1773370530, "data": [...], "total_value_usd": "100.00" },
///     ...
///   }
/// }
/// ```
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct BalanceCacheJson {
    #[serde(default)]
    pub batch_updated_at: i64,
    #[serde(default)]
    pub accounts: HashMap<String, BalanceCacheEntry>,
}

/// Per-account balance cache entry.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BalanceCacheEntry {
    pub updated_at: i64,
    pub data: serde_json::Value,
    pub total_value_usd: String,
}

// ── chain_cache.json ─────────────────────────────────────────────────
/// ```json
/// {
///   "updated_at": 1773370520,
///   "chains": [...]
/// }
/// ```
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ChainCacheJson {
    #[serde(default)]
    pub updated_at: i64,
    #[serde(default)]
    pub chains: Vec<serde_json::Value>,
}

// ── session.json ────────────────────────────────────────────────────
/// Non-sensitive session metadata stored on disk (moved out of OS keyring
/// to stay within Windows Credential Manager's 2560-byte limit).
///
/// Sensitive secrets (`refresh_token`, `access_token`, `session_key`)
/// remain in the OS keyring via `keyring_store`.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct SessionJson {
    #[serde(default)]
    pub tee_id: String,
    #[serde(default)]
    pub session_cert: String,
    #[serde(default)]
    pub encrypted_session_sk: String,
    #[serde(default)]
    pub session_key_expire_at: String,
    #[serde(default)]
    pub api_key: String,
}

// ── Path helpers ────────────────────────────────────────────────────

fn wallets_path() -> Result<PathBuf> {
    Ok(onchainos_home()?.join("wallets.json"))
}

fn cache_path() -> Result<PathBuf> {
    Ok(onchainos_home()?.join("cache.json"))
}

fn balance_cache_path() -> Result<PathBuf> {
    Ok(onchainos_home()?.join("balance_cache.json"))
}

fn chain_cache_path() -> Result<PathBuf> {
    Ok(onchainos_home()?.join("chain_cache.json"))
}

fn session_path() -> Result<PathBuf> {
    Ok(onchainos_home()?.join("session.json"))
}

fn ensure_home_dir() -> Result<()> {
    crate::home::ensure_onchainos_home()?;
    Ok(())
}

// ── wallets.json operations ─────────────────────────────────────────

pub fn load_wallets() -> Result<Option<WalletsJson>> {
    let path = wallets_path()?;
    if !path.exists() {
        return Ok(None);
    }
    let data = fs::read_to_string(&path).context("failed to read wallets.json")?;
    let w: WalletsJson = serde_json::from_str(&data).context("failed to parse wallets.json")?;
    Ok(Some(w))
}

pub fn save_wallets(w: &WalletsJson) -> Result<()> {
    ensure_home_dir()?;
    let path = wallets_path()?;
    let json = serde_json::to_string_pretty(w)?;
    let tmp = path.with_extension("json.tmp");
    fs::write(&tmp, &json)?;
    fs::rename(&tmp, &path)?;
    Ok(())
}

pub fn delete_wallets() -> Result<()> {
    let path = wallets_path()?;
    if path.exists() {
        fs::remove_file(&path).context("failed to delete wallets.json")?;
    }
    Ok(())
}

// ── cache.json operations ───────────────────────────────────────────

pub fn load_cache() -> Result<CacheJson> {
    let path = cache_path()?;
    if !path.exists() {
        return Ok(CacheJson::default());
    }
    let data = fs::read_to_string(&path).context("failed to read cache.json")?;
    let c: CacheJson = serde_json::from_str(&data).context("failed to parse cache.json")?;
    Ok(c)
}

pub fn save_cache(c: &CacheJson) -> Result<()> {
    ensure_home_dir()?;
    let path = cache_path()?;
    let json = serde_json::to_string_pretty(c)?;
    let tmp = path.with_extension("json.tmp");
    fs::write(&tmp, &json)?;
    fs::rename(&tmp, &path)?;
    Ok(())
}

pub fn delete_cache() -> Result<()> {
    let path = cache_path()?;
    if path.exists() {
        fs::remove_file(&path).context("failed to delete cache.json")?;
    }
    Ok(())
}

/// Remove the login field from cache.json.
pub fn clear_login_cache() -> Result<()> {
    let mut cache = load_cache()?;
    cache.login = None;
    save_cache(&cache)
}

/// Save the swap trace ID to cache.json (preserves other fields).
pub fn set_swap_trace_id(tid: &str) -> Result<()> {
    let mut cache = load_cache()?;
    cache.swap_trace_id = Some(tid.to_string());
    save_cache(&cache)
}

/// Read the swap trace ID from cache.json. Returns None if not set.
pub fn get_swap_trace_id() -> Result<Option<String>> {
    let cache = load_cache()?;
    Ok(cache.swap_trace_id)
}

/// Clear the swap trace ID from cache.json (preserves other fields).
pub fn clear_swap_trace_id() -> Result<()> {
    let mut cache = load_cache()?;
    cache.swap_trace_id = None;
    save_cache(&cache)
}

// ── balance_cache.json operations ────────────────────────────────────

pub fn load_balance_cache() -> Result<BalanceCacheJson> {
    let path = balance_cache_path()?;
    if !path.exists() {
        return Ok(BalanceCacheJson::default());
    }
    let data = fs::read_to_string(&path).context("failed to read balance_cache.json")?;
    let c: BalanceCacheJson =
        serde_json::from_str(&data).context("failed to parse balance_cache.json")?;
    Ok(c)
}

pub fn save_balance_cache(c: &BalanceCacheJson) -> Result<()> {
    ensure_home_dir()?;
    let path = balance_cache_path()?;
    let json = serde_json::to_string_pretty(c)?;
    let tmp = path.with_extension("json.tmp");
    fs::write(&tmp, &json)?;
    fs::rename(&tmp, &path)?;
    Ok(())
}

pub fn delete_balance_cache() -> Result<()> {
    let path = balance_cache_path()?;
    if path.exists() {
        fs::remove_file(&path).context("failed to delete balance_cache.json")?;
    }
    Ok(())
}

/// Check if the batch cache is fresh (`batch_updated_at` within `ttl_secs`).
/// Returns None if cache has no accounts or the batch update time is expired.
pub fn get_batch_balance_cache(ttl_secs: i64) -> Result<Option<BalanceCacheJson>> {
    let cache = load_balance_cache()?;
    if cache.accounts.is_empty() {
        return Ok(None);
    }
    let now = chrono::Utc::now().timestamp();
    if now - cache.batch_updated_at >= ttl_secs {
        return Ok(None);
    }
    Ok(Some(cache))
}

/// Bulk-write all accounts into balance_cache.json (updates `batch_updated_at`).
pub fn set_batch_balance_cache(entries: &[(String, BalanceCacheEntry)]) -> Result<()> {
    let now = chrono::Utc::now().timestamp();
    let mut cache = load_balance_cache()?;
    cache.batch_updated_at = now;
    for (account_id, entry) in entries {
        cache.accounts.insert(account_id.clone(), entry.clone());
    }
    save_balance_cache(&cache)
}

/// Get a single account's cache entry if within `ttl_secs`.
pub fn get_account_balance_cache(
    account_id: &str,
    ttl_secs: i64,
) -> Result<Option<BalanceCacheEntry>> {
    let cache = load_balance_cache()?;
    let entry = match cache.accounts.get(account_id) {
        Some(e) => e,
        None => return Ok(None),
    };
    let now = chrono::Utc::now().timestamp();
    if now - entry.updated_at >= ttl_secs {
        return Ok(None);
    }
    Ok(Some(entry.clone()))
}

/// Update a single account's cache entry (does not touch `batch_updated_at`).
pub fn set_account_balance_cache(account_id: &str, entry: BalanceCacheEntry) -> Result<()> {
    let mut cache = load_balance_cache()?;
    cache.accounts.insert(account_id.to_string(), entry);
    save_balance_cache(&cache)
}

// ── chain_cache.json operations ──────────────────────────────────────

pub fn load_chain_cache() -> Result<ChainCacheJson> {
    let path = chain_cache_path()?;
    if !path.exists() {
        return Ok(ChainCacheJson::default());
    }
    let data = fs::read_to_string(&path).context("failed to read chain_cache.json")?;
    let c: ChainCacheJson =
        serde_json::from_str(&data).context("failed to parse chain_cache.json")?;
    Ok(c)
}

pub fn save_chain_cache(c: &ChainCacheJson) -> Result<()> {
    ensure_home_dir()?;
    let path = chain_cache_path()?;
    let json = serde_json::to_string_pretty(c)?;
    let tmp = path.with_extension("json.tmp");
    fs::write(&tmp, &json)?;
    fs::rename(&tmp, &path)?;
    Ok(())
}

/// Return cached chain list if within `ttl_secs`, otherwise return None.
pub fn get_chain_cache(ttl_secs: i64) -> Result<Option<ChainCacheJson>> {
    let cache = load_chain_cache()?;
    if cache.chains.is_empty() {
        return Ok(None);
    }
    let now = chrono::Utc::now().timestamp();
    if now - cache.updated_at >= ttl_secs {
        return Ok(None);
    }
    Ok(Some(cache))
}

/// Persist a fresh chain list with the current timestamp.
pub fn set_chain_cache(chains: Vec<serde_json::Value>) -> Result<()> {
    let now = chrono::Utc::now().timestamp();
    save_chain_cache(&ChainCacheJson {
        updated_at: now,
        chains,
    })
}

// ── session.json operations ──────────────────────────────────────────

pub fn load_session() -> Result<Option<SessionJson>> {
    let path = session_path()?;
    if !path.exists() {
        return Ok(None);
    }
    let data = fs::read_to_string(&path).context("failed to read session.json")?;
    let s: SessionJson = serde_json::from_str(&data).context("failed to parse session.json")?;
    Ok(Some(s))
}

pub fn save_session(s: &SessionJson) -> Result<()> {
    ensure_home_dir()?;
    let path = session_path()?;
    let json = serde_json::to_string_pretty(s)?;
    let tmp = path.with_extension("json.tmp");
    fs::write(&tmp, &json)?;
    fs::rename(&tmp, &path)?;
    Ok(())
}

pub fn delete_session() -> Result<()> {
    let path = session_path()?;
    if path.exists() {
        fs::remove_file(&path).context("failed to delete session.json")?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    /// Helper: set ONCHAINOS_HOME to a unique dir under target/ for the closure.
    fn with_temp_home<F: FnOnce()>(name: &str, f: F) {
        let _lock = crate::home::TEST_ENV_MUTEX
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let dir = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("target")
            .join("test_tmp")
            .join(name);
        if dir.exists() {
            fs::remove_dir_all(&dir).ok();
        }
        fs::create_dir_all(&dir).unwrap();
        std::env::set_var("ONCHAINOS_HOME", &dir);
        f();
        std::env::remove_var("ONCHAINOS_HOME");
        fs::remove_dir_all(&dir).ok();
    }

    // ── Serde round-trip tests ────────────────────────────────────────

    #[test]
    fn wallets_json_serde_roundtrip() {
        let mut accounts_map = HashMap::new();
        accounts_map.insert(
            "acc-1".to_string(),
            AccountMapEntry {
                address_list: vec![AddressInfo {
                    account_id: "acc-1".to_string(),
                    address: "0xabc".to_string(),
                    chain_index: "1".to_string(),
                    chain_name: "eth".to_string(),
                    address_type: "eoa".to_string(),
                    chain_path: "/evm/1".to_string(),
                }],
            },
        );

        let w = WalletsJson {
            email: "test@example.com".to_string(),
            is_new: true,
            project_id: "proj-1".to_string(),
            selected_account_id: "acc-1".to_string(),
            accounts_map,
            accounts: vec![AccountInfo {
                project_id: "proj-1".to_string(),
                account_id: "acc-1".to_string(),
                account_name: "My Wallet".to_string(),
                is_default: true,
            }],
            is_ak: false,
        };

        let json = serde_json::to_string(&w).unwrap();
        let parsed: WalletsJson = serde_json::from_str(&json).unwrap();

        assert_eq!(parsed.email, "test@example.com");
        assert!(parsed.is_new);
        assert_eq!(parsed.project_id, "proj-1");
        assert_eq!(parsed.selected_account_id, "acc-1");
        assert_eq!(parsed.accounts.len(), 1);
        assert_eq!(parsed.accounts[0].account_name, "My Wallet");
        assert_eq!(
            parsed.accounts_map["acc-1"].address_list[0].address,
            "0xabc"
        );
    }

    #[test]
    fn wallets_json_camel_case_field_names() {
        let w = WalletsJson {
            project_id: "p1".to_string(),
            selected_account_id: "a1".to_string(),
            ..Default::default()
        };
        let json = serde_json::to_string(&w).unwrap();
        assert!(json.contains("\"projectId\""));
        assert!(json.contains("\"selectedAccountId\""));
        assert!(json.contains("\"isNew\""));
        assert!(json.contains("\"accountsMap\""));
    }

    #[test]
    fn wallets_json_default_is_empty() {
        let w = WalletsJson::default();
        assert!(w.email.is_empty());
        assert!(!w.is_new);
        assert!(w.accounts.is_empty());
        assert!(w.accounts_map.is_empty());
    }

    #[test]
    fn cache_json_serde_roundtrip() {
        let c = CacheJson {
            login: Some(LoginCache {
                email: "user@test.com".to_string(),
                flow_id: "flow-123".to_string(),
            }),
            ..Default::default()
        };
        let json = serde_json::to_string(&c).unwrap();
        let parsed: CacheJson = serde_json::from_str(&json).unwrap();
        let login = parsed.login.unwrap();
        assert_eq!(login.email, "user@test.com");
        assert_eq!(login.flow_id, "flow-123");
    }

    #[test]
    fn cache_json_login_none_skips_field() {
        let c = CacheJson {
            login: None,
            ..Default::default()
        };
        let json = serde_json::to_string(&c).unwrap();
        assert!(!json.contains("login"));
    }

    #[test]
    fn cache_json_deserialize_empty_object() {
        let c: CacheJson = serde_json::from_str("{}").unwrap();
        assert!(c.login.is_none());
    }

    // ── File I/O tests ───────────────────────────────────────────────

    #[test]
    fn save_and_load_wallets() {
        with_temp_home("save_load_wallets", || {
            let w = WalletsJson {
                email: "io@test.com".to_string(),
                project_id: "p1".to_string(),
                ..Default::default()
            };
            save_wallets(&w).unwrap();
            let loaded = load_wallets().unwrap().unwrap();
            assert_eq!(loaded.email, "io@test.com");
            assert_eq!(loaded.project_id, "p1");
        });
    }

    #[test]
    fn load_wallets_returns_none_when_missing() {
        with_temp_home("load_wallets_none", || {
            let result = load_wallets().unwrap();
            assert!(result.is_none());
        });
    }

    #[test]
    fn delete_wallets_removes_file() {
        with_temp_home("delete_wallets", || {
            save_wallets(&WalletsJson::default()).unwrap();
            assert!(load_wallets().unwrap().is_some());
            delete_wallets().unwrap();
            assert!(load_wallets().unwrap().is_none());
        });
    }

    #[test]
    fn delete_wallets_noop_when_missing() {
        with_temp_home("delete_wallets_noop", || {
            // Should not error when file doesn't exist
            delete_wallets().unwrap();
        });
    }

    #[test]
    fn save_and_load_cache() {
        with_temp_home("save_load_cache", || {
            let c = CacheJson {
                login: Some(LoginCache {
                    email: "cache@test.com".to_string(),
                    flow_id: "f1".to_string(),
                }),
                ..Default::default()
            };
            save_cache(&c).unwrap();
            let loaded = load_cache().unwrap();
            assert_eq!(loaded.login.unwrap().email, "cache@test.com");
        });
    }

    #[test]
    fn load_cache_returns_default_when_missing() {
        with_temp_home("load_cache_default", || {
            let c = load_cache().unwrap();
            assert!(c.login.is_none());
        });
    }

    #[test]
    fn delete_cache_removes_file() {
        with_temp_home("delete_cache", || {
            save_cache(&CacheJson::default()).unwrap();
            delete_cache().unwrap();
            // load_cache should return default (no file)
            let c = load_cache().unwrap();
            assert!(c.login.is_none());
        });
    }

    #[test]
    fn clear_login_cache_removes_login_only() {
        with_temp_home("clear_login_cache", || {
            let c = CacheJson {
                login: Some(LoginCache {
                    email: "clear@test.com".to_string(),
                    flow_id: "f2".to_string(),
                }),
                ..Default::default()
            };
            save_cache(&c).unwrap();
            clear_login_cache().unwrap();
            let loaded = load_cache().unwrap();
            assert!(loaded.login.is_none());
        });
    }

    // ── chain_cache tests ─────────────────────────────────────────────

    #[test]
    fn chain_cache_json_default_is_empty() {
        let c = ChainCacheJson::default();
        assert_eq!(c.updated_at, 0);
        assert!(c.chains.is_empty());
    }

    #[test]
    fn chain_cache_serde_roundtrip() {
        let c = ChainCacheJson {
            updated_at: 1_700_000_000,
            chains: vec![
                serde_json::json!({"chainIndex": "1", "chainName": "Ethereum"}),
                serde_json::json!({"chainIndex": "56", "chainName": "BNB Chain"}),
            ],
        };
        let json = serde_json::to_string(&c).unwrap();
        let parsed: ChainCacheJson = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.updated_at, 1_700_000_000);
        assert_eq!(parsed.chains.len(), 2);
        assert_eq!(parsed.chains[0]["chainIndex"], "1");
    }

    #[test]
    fn save_and_load_chain_cache() {
        with_temp_home("chain_cache_io", || {
            let chains = vec![serde_json::json!({"chainIndex": "1", "chainName": "Ethereum"})];
            set_chain_cache(chains).unwrap();
            let loaded = load_chain_cache().unwrap();
            assert_eq!(loaded.chains.len(), 1);
            assert!(loaded.updated_at > 0);
        });
    }

    #[test]
    fn load_chain_cache_returns_default_when_missing() {
        with_temp_home("chain_cache_missing", || {
            let c = load_chain_cache().unwrap();
            assert!(c.chains.is_empty());
            assert_eq!(c.updated_at, 0);
        });
    }

    #[test]
    fn get_chain_cache_returns_none_when_missing() {
        with_temp_home("chain_cache_none_missing", || {
            let result = get_chain_cache(600).unwrap();
            assert!(result.is_none());
        });
    }

    #[test]
    fn get_chain_cache_returns_none_when_expired() {
        with_temp_home("chain_cache_expired", || {
            // Save cache with updated_at in the past (past TTL)
            let c = ChainCacheJson {
                updated_at: 1_000_000, // far in the past
                chains: vec![serde_json::json!({"chainIndex": "1"})],
            };
            save_chain_cache(&c).unwrap();
            let result = get_chain_cache(600).unwrap();
            assert!(result.is_none());
        });
    }

    #[test]
    fn get_chain_cache_returns_some_when_fresh() {
        with_temp_home("chain_cache_fresh", || {
            let chains = vec![serde_json::json!({"chainIndex": "1", "chainName": "Ethereum"})];
            set_chain_cache(chains).unwrap();
            // TTL = 600s, just saved → should be fresh
            let result = get_chain_cache(600).unwrap();
            assert!(result.is_some());
            assert_eq!(result.unwrap().chains.len(), 1);
        });
    }

    #[test]
    fn set_chain_cache_overwrites_previous() {
        with_temp_home("chain_cache_overwrite", || {
            set_chain_cache(vec![serde_json::json!({"chainIndex": "1"})]).unwrap();
            set_chain_cache(vec![
                serde_json::json!({"chainIndex": "56"}),
                serde_json::json!({"chainIndex": "137"}),
            ])
            .unwrap();
            let loaded = load_chain_cache().unwrap();
            assert_eq!(loaded.chains.len(), 2);
            assert_eq!(loaded.chains[0]["chainIndex"], "56");
        });
    }

    // ── session.json tests ───────────────────────────────────────────

    #[test]
    fn session_json_serde_roundtrip() {
        let s = SessionJson {
            tee_id: "tee-123".to_string(),
            session_cert: "cert-abc".to_string(),
            encrypted_session_sk: "esk-xyz".to_string(),
            session_key_expire_at: "1700000000".to_string(),
            api_key: "ak-key".to_string(),
        };
        let json = serde_json::to_string(&s).unwrap();
        let parsed: SessionJson = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.tee_id, "tee-123");
        assert_eq!(parsed.session_cert, "cert-abc");
        assert_eq!(parsed.encrypted_session_sk, "esk-xyz");
        assert_eq!(parsed.session_key_expire_at, "1700000000");
        assert_eq!(parsed.api_key, "ak-key");
    }

    #[test]
    fn session_json_camel_case_field_names() {
        let s = SessionJson {
            tee_id: "t".to_string(),
            encrypted_session_sk: "e".to_string(),
            session_key_expire_at: "1".to_string(),
            ..Default::default()
        };
        let json = serde_json::to_string(&s).unwrap();
        assert!(json.contains("\"teeId\""));
        assert!(json.contains("\"sessionCert\""));
        assert!(json.contains("\"encryptedSessionSk\""));
        assert!(json.contains("\"sessionKeyExpireAt\""));
        assert!(json.contains("\"apiKey\""));
    }

    #[test]
    fn session_json_default_is_empty() {
        let s = SessionJson::default();
        assert!(s.tee_id.is_empty());
        assert!(s.session_cert.is_empty());
        assert!(s.encrypted_session_sk.is_empty());
        assert!(s.session_key_expire_at.is_empty());
        assert!(s.api_key.is_empty());
    }

    #[test]
    fn save_and_load_session() {
        with_temp_home("save_load_session", || {
            let s = SessionJson {
                tee_id: "tee1".to_string(),
                session_cert: "cert1".to_string(),
                encrypted_session_sk: "esk1".to_string(),
                session_key_expire_at: "999".to_string(),
                api_key: "ak1".to_string(),
            };
            save_session(&s).unwrap();
            let loaded = load_session().unwrap().unwrap();
            assert_eq!(loaded.tee_id, "tee1");
            assert_eq!(loaded.api_key, "ak1");
        });
    }

    #[test]
    fn load_session_returns_none_when_missing() {
        with_temp_home("load_session_none", || {
            let result = load_session().unwrap();
            assert!(result.is_none());
        });
    }

    #[test]
    fn delete_session_removes_file() {
        with_temp_home("delete_session", || {
            save_session(&SessionJson::default()).unwrap();
            assert!(load_session().unwrap().is_some());
            delete_session().unwrap();
            assert!(load_session().unwrap().is_none());
        });
    }

    #[test]
    fn delete_session_noop_when_missing() {
        with_temp_home("delete_session_noop", || {
            delete_session().unwrap();
        });
    }
}
