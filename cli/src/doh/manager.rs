use std::net::{IpAddr, SocketAddr};

use super::types::{DohCacheEntry, DohMode, DohNode, FailedNode};
use super::{binary, cache};

/// Per-request DoH failover state.
///
/// Clone semantics: each clone carries its own copy of `mode`, `node`,
/// `retried`, and `resolved_ip`. This enables `ApiClient: Clone` and true
/// parallel HTTP via `tokio::join!` / `JoinSet` without serialising on a
/// shared mutex guard.
///
/// Trade-off on a cold DoH cache: if the first-run network is unreliable,
/// N parallel clones may each independently probe for a proxy node before
/// any of them writes the cache file. The redundancy is bounded — once the
/// first task writes the cache, subsequent runs skip the probe entirely.
/// For the current workflow fan-out (≤10 parallel clones) the extra probe
/// cost is preferable to serialising the entire HTTP path behind a mutex.
#[derive(Clone)]
pub struct DohManager {
    domain: String,
    original_base_url: String,
    mode: Option<DohMode>,
    node: Option<DohNode>,
    /// Resolved IP for the current node. Needed when node.ip is a CNAME domain
    /// instead of an IP address — we DNS-resolve it once and cache the result.
    resolved_ip: Option<IpAddr>,
    resolved: bool,
    retried: bool,
    custom_base_url: bool,
}

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

impl DohManager {
    pub fn new(domain: &str, base_url: &str, custom_base_url: bool) -> Self {
        Self {
            domain: domain.to_string(),
            original_base_url: base_url.to_string(),
            mode: None,
            node: None,
            resolved_ip: None,
            resolved: false,
            retried: false,
            custom_base_url,
        }
    }

    /// Called before first request: reads cache if available.
    pub fn prepare(&mut self) {
        if self.custom_base_url || self.resolved {
            return;
        }
        self.resolved = true;

        if let Some(entry) = cache::read_cache(&self.domain) {
            self.mode = Some(entry.mode.clone());
            if let Some(ref node) = entry.node {
                self.resolved_ip = Self::resolve_node_ip(&node.ip);
            }
            self.node = entry.node.clone();
        }
    }

    /// Called on network failure. Returns true if a new proxy node was found and state updated.
    pub async fn handle_failure(&mut self) -> bool {
        if self.custom_base_url || self.retried {
            return false;
        }

        // Collect failed node IPs from cache for --exclude
        let mut exclude: Vec<String> = cache::read_cache(&self.domain)
            .map(|e| e.failed_nodes.iter().map(|n| n.ip.clone()).collect())
            .unwrap_or_default();

        // If current node failed, add its IP to the failed list and persist
        if let Some(ref current_node) = self.node {
            let ip = current_node.ip.clone();
            if !exclude.contains(&ip) {
                exclude.push(ip.clone());
            }

            // Persist this failed node to cache
            let mut entry = cache::read_cache(&self.domain).unwrap_or(DohCacheEntry {
                mode: DohMode::Proxy,
                node: None,
                failed_nodes: vec![],
                updated_at: now_ms(),
            });
            if !entry.failed_nodes.iter().any(|n| n.ip == ip) {
                entry.failed_nodes.push(FailedNode {
                    ip,
                    failed_at: now_ms(),
                });
            }
            cache::write_cache(&self.domain, &entry);
        }

        // Ensure binary exists (download if needed)
        let bin_exists = binary::binary_path().map(|p| p.exists()).unwrap_or(false);
        if !bin_exists && binary::download_binary().await.is_err() {
            self.retried = true;
            return false;
        }

        // Call exec_doh_binary with domain + exclude list + user-agent
        let ua = self.doh_user_agent();
        if let Some(new_node) = binary::exec_doh_binary(&self.domain, &exclude, Some(&ua)).await {
            let failed_nodes = cache::read_cache(&self.domain)
                .map(|e| e.failed_nodes)
                .unwrap_or_default();

            // classifyAndCache: if binary says ip/host == domain, it means "go direct"
            if new_node.ip == self.domain || new_node.host == self.domain {
                let entry = DohCacheEntry {
                    mode: DohMode::Direct,
                    node: None,
                    failed_nodes,
                    updated_at: now_ms(),
                };
                cache::write_cache(&self.domain, &entry);
                self.mode = Some(DohMode::Direct);
                self.node = None;
                self.resolved_ip = None;
                self.retried = false;
                return true;
            }

            // Proxy node — cache and apply
            let entry = DohCacheEntry {
                mode: DohMode::Proxy,
                node: Some(new_node.clone()),
                failed_nodes,
                updated_at: now_ms(),
            };
            cache::write_cache(&self.domain, &entry);

            self.resolved_ip = Self::resolve_node_ip(&new_node.ip);
            if self.resolved_ip.is_none() {
                // CNAME DNS resolution failed — fall back to direct
                self.retried = true;
                self.mode = None;
                self.node = None;
                return true;
            }
            self.mode = Some(DohMode::Proxy);
            self.node = Some(new_node);
            // Reset retried so long-lived processes (MCP) can failover again later
            self.retried = false;
            true
        } else {
            // All nodes exhausted — clear proxy state, let caller retry with direct.
            // Don't write cache (avoid failedNodes deadloop per spec).
            // retried stays true so this is the last attempt.
            self.retried = true;
            self.mode = None;
            self.node = None;
            self.resolved_ip = None;
            true
        }
    }

    pub fn base_url(&self) -> &str {
        &self.original_base_url
    }

    pub fn proxy_base_url(&self) -> Option<String> {
        if self.mode.as_ref() == Some(&DohMode::Proxy) {
            if let Some(ref node) = self.node {
                return Some(format!("https://{}", node.host));
            }
        }
        None
    }

    pub fn resolve_override(&self) -> Option<(String, SocketAddr)> {
        if self.mode.as_ref() == Some(&DohMode::Proxy) {
            if let Some(ref node) = self.node {
                let ip = self.resolved_ip?;
                return Some((node.host.clone(), SocketAddr::new(ip, 443)));
            }
        }
        None
    }

    /// If not custom and mode is None, write cache entry with mode=Direct.
    pub fn cache_direct_if_needed(&self) {
        if !self.custom_base_url && self.mode.is_none() {
            let entry = DohCacheEntry {
                mode: DohMode::Direct,
                node: None,
                failed_nodes: vec![],
                updated_at: now_ms(),
            };
            cache::write_cache(&self.domain, &entry);
        }
    }

    pub fn is_proxy(&self) -> bool {
        self.mode.as_ref() == Some(&DohMode::Proxy) && self.node.is_some()
    }

    /// Resolve node.ip to an IpAddr. Handles two cases:
    /// 1. node.ip is a real IP like "8.212.1.102" → parse directly
    /// 2. node.ip is a CNAME domain like "xyz.aliyunddos.com" → DNS lookup
    fn resolve_node_ip(ip_or_domain: &str) -> Option<IpAddr> {
        // Try direct IP parse first
        if let Ok(ip) = ip_or_domain.parse::<IpAddr>() {
            return Some(ip);
        }
        // CNAME domain — do blocking DNS lookup (only happens once per node switch)
        use std::net::ToSocketAddrs;
        let addr = format!("{}:443", ip_or_domain);
        match addr.to_socket_addrs() {
            Ok(mut addrs) => addrs.next().map(|a| a.ip()),
            Err(_) => {
                eprintln!("[doh] proxy node {ip_or_domain} unavailable, falling back to direct connection");
                None
            }
        }
    }

    /// Returns the User-Agent string for DoH operations.
    /// Always returns the UA regardless of current mode — the binary and proxy
    /// nodes both need it.
    pub fn doh_user_agent(&self) -> String {
        format!("OKX/@okx_ai/onchainos-cli/{}", env!("CARGO_PKG_VERSION"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::home::TEST_ENV_MUTEX;
    use std::fs;
    use std::path::PathBuf;

    fn test_dir(name: &str) -> PathBuf {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("target")
            .join("test_tmp")
            .join("doh_manager")
            .join(name);
        if dir.exists() {
            fs::remove_dir_all(&dir).ok();
        }
        fs::create_dir_all(&dir).ok();
        dir
    }

    #[test]
    fn new_manager_starts_unresolved() {
        let mgr = DohManager::new("web3.okx.com", "https://web3.okx.com", false);
        assert!(mgr.mode.is_none());
        assert!(!mgr.resolved);
        assert!(mgr.node.is_none());
    }

    #[test]
    fn prepare_with_no_cache_stays_none() {
        let _lock = TEST_ENV_MUTEX.lock().unwrap();
        let dir = test_dir("prepare_no_cache");
        std::env::set_var("ONCHAINOS_HOME", &dir);

        let mut mgr = DohManager::new("web3.okx.com", "https://web3.okx.com", false);
        mgr.prepare();

        assert!(mgr.mode.is_none());
        assert!(mgr.resolved);

        std::env::remove_var("ONCHAINOS_HOME");
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn prepare_reads_cached_proxy() {
        let _lock = TEST_ENV_MUTEX.lock().unwrap();
        let dir = test_dir("prepare_cached_proxy");
        std::env::set_var("ONCHAINOS_HOME", &dir);

        let entry = DohCacheEntry {
            mode: DohMode::Proxy,
            node: Some(DohNode {
                ip: "1.2.3.4".to_string(),
                host: "proxy.example.com".to_string(),
                ttl: 300,
            }),
            failed_nodes: vec![],
            updated_at: now_ms(),
        };
        cache::write_cache("web3.okx.com", &entry);

        let mut mgr = DohManager::new("web3.okx.com", "https://web3.okx.com", false);
        mgr.prepare();

        assert_eq!(mgr.mode, Some(DohMode::Proxy));
        assert_eq!(mgr.node.as_ref().unwrap().ip, "1.2.3.4");
        assert_eq!(mgr.node.as_ref().unwrap().host, "proxy.example.com");

        std::env::remove_var("ONCHAINOS_HOME");
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn prepare_reads_cached_direct() {
        let _lock = TEST_ENV_MUTEX.lock().unwrap();
        let dir = test_dir("prepare_cached_direct");
        std::env::set_var("ONCHAINOS_HOME", &dir);

        let entry = DohCacheEntry {
            mode: DohMode::Direct,
            node: None,
            failed_nodes: vec![],
            updated_at: now_ms(),
        };
        cache::write_cache("web3.okx.com", &entry);

        let mut mgr = DohManager::new("web3.okx.com", "https://web3.okx.com", false);
        mgr.prepare();

        assert_eq!(mgr.mode, Some(DohMode::Direct));
        assert!(mgr.node.is_none());

        std::env::remove_var("ONCHAINOS_HOME");
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn custom_base_url_skips_everything() {
        let mgr_custom = DohManager::new("web3.okx.com", "https://custom.example.com", true);
        // prepare does nothing when custom_base_url is true
        let mut mgr = mgr_custom;
        mgr.prepare();
        assert!(!mgr.resolved);
        assert!(mgr.mode.is_none());
    }

    #[test]
    fn resolve_override_returns_none_when_direct() {
        let mgr = DohManager::new("web3.okx.com", "https://web3.okx.com", false);
        assert!(mgr.resolve_override().is_none());
    }

    #[test]
    fn resolve_override_returns_addr_when_proxy() {
        let mut mgr = DohManager::new("web3.okx.com", "https://web3.okx.com", false);
        mgr.mode = Some(DohMode::Proxy);
        mgr.node = Some(DohNode {
            ip: "93.184.216.34".to_string(),
            host: "proxy.example.com".to_string(),
            ttl: 300,
        });
        mgr.resolved_ip = Some("93.184.216.34".parse().unwrap());

        let (host, addr) = mgr.resolve_override().expect("should return Some");
        assert_eq!(host, "proxy.example.com");
        assert_eq!(addr.ip().to_string(), "93.184.216.34");
        assert_eq!(addr.port(), 443);
    }

    #[test]
    fn cache_direct_if_needed_writes_on_first_success() {
        let _lock = TEST_ENV_MUTEX.lock().unwrap();
        let dir = test_dir("cache_direct_first_success");
        std::env::set_var("ONCHAINOS_HOME", &dir);

        let mgr = DohManager::new("web3.okx.com", "https://web3.okx.com", false);
        assert!(mgr.mode.is_none());

        mgr.cache_direct_if_needed();

        let cached = cache::read_cache("web3.okx.com").expect("should have cache entry");
        assert_eq!(cached.mode, DohMode::Direct);
        assert!(cached.node.is_none());

        std::env::remove_var("ONCHAINOS_HOME");
        fs::remove_dir_all(&dir).ok();
    }
}
