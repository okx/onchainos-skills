use std::fs;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use crate::home::onchainos_home;

use super::types::{DohCacheEntry, DohCacheFile};

const CACHE_FILENAME: &str = "doh-cache.json";
const FAILED_NODE_TTL_MS: u64 = 3_600_000; // 1 hour

/// Returns the path to `~/.onchainos/doh-cache.json`.
fn cache_path() -> Option<PathBuf> {
    onchainos_home().ok().map(|h| h.join(CACHE_FILENAME))
}

/// Current unix timestamp in milliseconds.
fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

/// Read cache file, parse JSON, return entry for domain.
/// Expired failed nodes (older than 1 hour) are cleaned up.
/// Returns `None` on any error.
pub fn read_cache(domain: &str) -> Option<DohCacheEntry> {
    let path = cache_path()?;
    let data = fs::read_to_string(&path).ok()?;
    let mut file: DohCacheFile = serde_json::from_str(&data).ok()?;
    let entry = file.get_mut(domain)?;

    // Clean up expired failed nodes
    let now = now_ms();
    entry
        .failed_nodes
        .retain(|n| now.saturating_sub(n.failed_at) < FAILED_NODE_TTL_MS);

    Some(entry.clone())
}

/// Read existing file, merge domain entry, atomic write (.tmp + rename).
/// Best-effort: silently ignores all errors.
pub fn write_cache(domain: &str, entry: &DohCacheEntry) {
    let _ = write_cache_inner(domain, entry);
}

fn write_cache_inner(domain: &str, entry: &DohCacheEntry) -> Option<()> {
    let path = cache_path()?;

    // Ensure parent directory exists
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).ok()?;
    }

    // Read existing cache or start fresh
    let mut file: DohCacheFile = path
        .exists()
        .then(|| {
            fs::read_to_string(&path)
                .ok()
                .and_then(|d| serde_json::from_str(&d).ok())
        })
        .flatten()
        .unwrap_or_default();

    file.insert(domain.to_string(), entry.clone());

    let json = serde_json::to_string_pretty(&file).ok()?;
    let tmp_path = path.with_extension("tmp");
    fs::write(&tmp_path, json).ok()?;
    fs::rename(&tmp_path, &path).ok()?;

    Some(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::home::TEST_ENV_MUTEX;

    use super::super::types::{DohMode, DohNode, FailedNode};

    /// Create a unique temp directory under `target/test_tmp/doh_cache/`.
    fn test_dir(name: &str) -> PathBuf {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("target")
            .join("test_tmp")
            .join("doh_cache")
            .join(name);
        if dir.exists() {
            fs::remove_dir_all(&dir).ok();
        }
        fs::create_dir_all(&dir).ok();
        dir
    }

    fn make_proxy_entry() -> DohCacheEntry {
        DohCacheEntry {
            mode: DohMode::Proxy,
            node: Some(DohNode {
                ip: "1.2.3.4".to_string(),
                host: "proxy.example.com".to_string(),
                ttl: 300,
            }),
            failed_nodes: vec![],
            updated_at: now_ms(),
        }
    }

    #[test]
    fn read_returns_none_when_no_file() {
        let _lock = TEST_ENV_MUTEX.lock().unwrap();
        let dir = test_dir("read_none");
        std::env::set_var("ONCHAINOS_HOME", &dir);

        assert!(read_cache("example.com").is_none());

        std::env::remove_var("ONCHAINOS_HOME");
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn write_then_read_roundtrip() {
        let _lock = TEST_ENV_MUTEX.lock().unwrap();
        let dir = test_dir("roundtrip");
        std::env::set_var("ONCHAINOS_HOME", &dir);

        let entry = make_proxy_entry();
        write_cache("example.com", &entry);

        let read_back = read_cache("example.com").expect("should read back entry");
        assert_eq!(read_back.mode, DohMode::Proxy);
        assert_eq!(read_back.node.as_ref().unwrap().ip, "1.2.3.4");
        assert_eq!(read_back.node.as_ref().unwrap().host, "proxy.example.com");

        std::env::remove_var("ONCHAINOS_HOME");
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn write_preserves_other_domains() {
        let _lock = TEST_ENV_MUTEX.lock().unwrap();
        let dir = test_dir("preserves");
        std::env::set_var("ONCHAINOS_HOME", &dir);

        let entry1 = make_proxy_entry();
        let entry2 = DohCacheEntry {
            mode: DohMode::Direct,
            node: None,
            failed_nodes: vec![],
            updated_at: now_ms(),
        };

        write_cache("a.com", &entry1);
        write_cache("b.com", &entry2);

        let a = read_cache("a.com").expect("a.com should exist");
        let b = read_cache("b.com").expect("b.com should exist");
        assert_eq!(a.mode, DohMode::Proxy);
        assert_eq!(b.mode, DohMode::Direct);

        std::env::remove_var("ONCHAINOS_HOME");
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn expired_failed_nodes_are_cleaned() {
        let _lock = TEST_ENV_MUTEX.lock().unwrap();
        let dir = test_dir("expired");
        std::env::set_var("ONCHAINOS_HOME", &dir);

        let entry = DohCacheEntry {
            mode: DohMode::Proxy,
            node: Some(DohNode {
                ip: "1.2.3.4".to_string(),
                host: "proxy.example.com".to_string(),
                ttl: 300,
            }),
            failed_nodes: vec![FailedNode {
                ip: "5.6.7.8".to_string(),
                failed_at: 1000, // ancient timestamp
            }],
            updated_at: now_ms(),
        };

        write_cache("example.com", &entry);

        let read_back = read_cache("example.com").expect("should read back entry");
        assert!(
            read_back.failed_nodes.is_empty(),
            "expired failed nodes should be cleaned"
        );

        std::env::remove_var("ONCHAINOS_HOME");
        fs::remove_dir_all(&dir).ok();
    }
}
