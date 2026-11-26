//! Local subId cache for the `period` scheme.
//!
//! Plain JSON at `$ONCHAINOS_HOME/subscriptions.json`; maps resource host →
//! buyer's active subId. Convenience index only, never authoritative — the
//! chain / SA are the source of truth. Updated write-through on subscribe and
//! change, from a receipt-confirmed APP cancellation, or from an authoritative
//! subscription-list read.

use std::collections::HashMap;
use std::path::PathBuf;

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

use crate::payment::subscription::types::{BuyerSubscriptionItem, SubscriptionCacheEntry};

/// `SubscriptionState` integer → cache state string.
fn state_label(state: u8) -> &'static str {
    match state {
        0 => "pending",
        1 => "active",
        3 => "canceled",
        4 => "changed",
        2 => "completed",
        // 99 = failed (SA-local submit failure); any other value is unknown.
        _ => "inactive",
    }
}

/// Extract the host (`example.com:8443`) from a URL; strips scheme, path,
/// query, fragment, and userinfo.
pub fn host_of(url: &str) -> String {
    let after_scheme = url.split_once("://").map(|(_, rest)| rest).unwrap_or(url);
    let authority = after_scheme
        .split(['/', '?', '#'])
        .next()
        .unwrap_or(after_scheme);
    // Drop any `user:pass@` userinfo prefix.
    let host = authority
        .rsplit_once('@')
        .map(|(_, h)| h)
        .unwrap_or(authority);
    host.to_ascii_lowercase()
}

/// Composite cache key = `protocol \u{1} host`. Keeps an x402 `period` sub and an
/// APP/1 sub on the SAME host in separate slots (they previously collided on a
/// host-only key, so the second subscribe overwrote the first).
fn cache_key(protocol: &str, host: &str) -> String {
    format!("{}\u{1}{}", protocol, host.to_ascii_lowercase())
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SubscriptionCache {
    /// `cache_key(protocol, host)` → entry. (Field name kept for JSON compat; the
    /// key is protocol-scoped since the protocol discriminator was added.)
    #[serde(default)]
    by_host: HashMap<String, SubscriptionCacheEntry>,
}

impl SubscriptionCache {
    pub fn cache_path() -> Result<PathBuf> {
        Ok(crate::home::onchainos_home()?.join("subscriptions.json"))
    }

    /// Load from disk; a missing or corrupt file is treated as an empty cache.
    /// Legacy files keyed by bare host are re-keyed to `cache_key(protocol, host)`
    /// (entries default `protocol = x402`), so old caches keep resolving.
    pub fn load() -> Self {
        let Ok(path) = Self::cache_path() else {
            return Self::default();
        };
        let mut cache: Self = std::fs::read(&path)
            .ok()
            .and_then(|data| serde_json::from_slice(&data).ok())
            .unwrap_or_default();
        cache.migrate_keys();
        cache
    }

    /// Rebuild the map keys from each entry's `(protocol, resource_host)` when they
    /// don't already match (bare-host legacy keys). Idempotent.
    fn migrate_keys(&mut self) {
        let stale = self
            .by_host
            .iter()
            .any(|(k, e)| *k != cache_key(&e.protocol, &e.resource_host));
        if !stale {
            return;
        }
        for e in std::mem::take(&mut self.by_host).into_values() {
            self.by_host
                .insert(cache_key(&e.protocol, &e.resource_host), e);
        }
    }

    /// Atomic write (tmp + rename).
    pub fn save(&self) -> Result<()> {
        let path = Self::cache_path()?;
        crate::home::ensure_onchainos_home()?;
        let tmp = path.with_extension("json.tmp");
        let bytes = serde_json::to_vec_pretty(self).context("serialize subscription cache")?;
        std::fs::write(&tmp, &bytes).context("write subscription cache tmp")?;
        std::fs::rename(&tmp, &path).context("rename subscription cache")?;
        Ok(())
    }

    /// Remove the cache file from disk (no-op if absent). Called on logout so
    /// the next session can't resolve a stale host→subId mapping.
    pub fn delete() -> Result<()> {
        let path = Self::cache_path()?;
        if path.exists() {
            std::fs::remove_file(&path).context("failed to delete subscriptions.json")?;
        }
        Ok(())
    }

    pub fn put(&mut self, entry: SubscriptionCacheEntry) {
        self.by_host
            .insert(cache_key(&entry.protocol, &entry.resource_host), entry);
    }

    pub fn get(&self, protocol: &str, host: &str) -> Option<&SubscriptionCacheEntry> {
        self.by_host.get(&cache_key(protocol, host))
    }

    /// Resolve the active subId for a URL's host under `protocol`. Returns `None`
    /// when there is no entry or the cached entry is not `active`.
    pub fn resolve(&self, protocol: &str, url: &str) -> Option<&SubscriptionCacheEntry> {
        self.by_host
            .get(&cache_key(protocol, &host_of(url)))
            .filter(|e| e.state == "active")
    }

    /// Commit a receipt-confirmed APP/1 cancellation. Returns the number of
    /// matching host entries changed.
    pub fn mark_app_canceled(&mut self, sub_id: &str) -> usize {
        let mut changed = 0;
        for entry in self.by_host.values_mut() {
            if entry.protocol == "app/1" && entry.sub_id.eq_ignore_ascii_case(sub_id) {
                entry.state = "canceled".to_string();
                changed += 1;
            }
        }
        changed
    }

    /// Record a change: the old subId becomes `changed` (with
    /// `changed_to_sub_id`), and the new entry replaces the host mapping.
    pub fn mark_changed(&mut self, old_sub_id: &str, new_entry: SubscriptionCacheEntry) {
        for e in self.by_host.values_mut() {
            if e.sub_id == old_sub_id {
                e.state = "changed".to_string();
                e.changed_to_sub_id = Some(new_entry.sub_id.clone());
            }
        }
        self.put(new_entry);
    }

    /// Full reconcile from `GET /buyers/{buyer}/subscriptions`. Replaces cached
    /// state per subId and follows `changedToSubId` so each host points at its
    /// current subId. Entries absent from the listing are left untouched.
    pub fn reconcile_from(&mut self, items: &[BuyerSubscriptionItem]) {
        // subId → authoritative state, for updating existing host entries.
        let by_sub_id: HashMap<&str, &BuyerSubscriptionItem> =
            items.iter().map(|i| (i.sub_id.as_str(), i)).collect();

        for entry in self.by_host.values_mut() {
            if let Some(item) = by_sub_id.get(entry.sub_id.as_str()) {
                entry.state = state_label(item.state).to_string();
                entry.plan_tier = item.plan_tier;
                entry.max_periods = item.max_periods;
                entry.changed_to_sub_id = item.changed_to_sub_id.clone();
                entry.plan_id = item.plan_id.clone();
                // Follow the change chain so the host points at the live subId.
                if item.state == 4 {
                    if let Some(new_id) = &item.changed_to_sub_id {
                        if let Some(new_item) = by_sub_id.get(new_id.as_str()) {
                            entry.sub_id = new_item.sub_id.clone();
                            entry.state = state_label(new_item.state).to_string();
                            entry.plan_tier = new_item.plan_tier;
                            entry.max_periods = new_item.max_periods;
                            entry.plan_id = new_item.plan_id.clone();
                            entry.changed_to_sub_id = new_item.changed_to_sub_id.clone();
                        }
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(host: &str, sub_id: &str, state: &str) -> SubscriptionCacheEntry {
        entry_proto("x402", host, sub_id, state)
    }

    fn entry_proto(
        protocol: &str,
        host: &str,
        sub_id: &str,
        state: &str,
    ) -> SubscriptionCacheEntry {
        SubscriptionCacheEntry {
            sub_id: sub_id.to_string(),
            resource_host: host.to_string(),
            merchant: "0xmerchant".to_string(),
            plan_id: "pro_monthly".to_string(),
            plan_tier: 2,
            max_periods: 12,
            state: state.to_string(),
            changed_to_sub_id: None,
            protocol: protocol.to_string(),
        }
    }

    #[test]
    fn host_of_strips_scheme_path_and_userinfo() {
        assert_eq!(
            host_of("https://api.example.com/v1/data?x=1"),
            "api.example.com"
        );
        assert_eq!(host_of("http://user:pass@host.io:8443/x"), "host.io:8443");
        assert_eq!(host_of("example.com/path"), "example.com");
        assert_eq!(host_of("HTTPS://API.EXAMPLE.COM"), "api.example.com");
    }

    #[test]
    fn resolve_returns_only_active() {
        let mut c = SubscriptionCache::default();
        c.put(entry("api.example.com", "0xsub", "active"));
        assert!(c.resolve("x402", "https://api.example.com/data").is_some());

        c.put(entry("api.example.com", "0xsub", "canceled"));
        assert!(c.resolve("x402", "https://api.example.com/data").is_none());
    }

    #[test]
    fn mark_changed_links_old_to_new() {
        let mut c = SubscriptionCache::default();
        c.put(entry("api.example.com", "0xold", "active"));
        let mut new = entry("api.example.com", "0xnew", "active");
        new.plan_tier = 3;
        c.mark_changed("0xold", new);
        let cur = c.get("x402", "api.example.com").unwrap();
        assert_eq!(cur.sub_id, "0xnew");
        assert_eq!(cur.plan_tier, 3);
        assert_eq!(cur.state, "active");
    }

    #[test]
    fn same_host_two_protocols_do_not_collide() {
        let mut c = SubscriptionCache::default();
        c.put(entry_proto("x402", "api.example.com", "0xx402", "active"));
        c.put(entry_proto("app/1", "api.example.com", "0xapp1", "active"));
        // Both survive; each protocol resolves to its own subId (no overwrite).
        assert_eq!(
            c.resolve("x402", "https://api.example.com/x").unwrap().sub_id,
            "0xx402"
        );
        assert_eq!(
            c.resolve("app/1", "https://api.example.com/x").unwrap().sub_id,
            "0xapp1"
        );
    }

    #[test]
    fn receipt_confirmed_cancel_only_updates_app_entry() {
        let mut c = SubscriptionCache::default();
        c.put(entry_proto("x402", "x402.example.com", "0xsame", "active"));
        c.put(entry_proto("app/1", "app.example.com", "0xsame", "active"));

        assert_eq!(c.mark_app_canceled("0xsame"), 1);

        assert_eq!(
            c.get("x402", "x402.example.com").unwrap().state,
            "active"
        );
        assert_eq!(c.get("app/1", "app.example.com").unwrap().state, "canceled");
    }

    #[test]
    fn reconcile_follows_changed_to_sub_id() {
        let mut c = SubscriptionCache::default();
        c.put(entry("api.example.com", "0xold", "active"));

        let mut old_item = BuyerSubscriptionItem {
            chain_index: 196,
            offer_id: "offer-old".to_string(),
            sub_id: "0xold".to_string(),
            state: 4, // changed
            payer: "0xbuyer".to_string(),
            token: "0xtoken".to_string(),
            amount_per_period: "5000000".to_string(),
            period_sec: 2_592_000,
            period_mode: 0,
            billing_anchor_at: 0,
            max_periods: 12,
            start_at: 0,
            initial_charge_periods: 1,
            initial_charge_amount: "5000000".to_string(),
            last_charged_period: 1,
            total_pulled: "5000000".to_string(),
            plan_id: "basic".to_string(),
            plan_tier: 1,
            changed_to_sub_id: Some("0xnew".to_string()),
            pending_change: None,
            is_active: false,
            service_ended: false,
            current_period: 1,
            next_chargeable_at: None,
        };
        let mut new_item = old_item.clone();
        new_item.sub_id = "0xnew".to_string();
        new_item.state = 1; // active
        new_item.plan_tier = 3;
        new_item.changed_to_sub_id = None;
        old_item.changed_to_sub_id = Some("0xnew".to_string());

        c.reconcile_from(&[old_item, new_item]);
        let cur = c.get("x402", "api.example.com").unwrap();
        assert_eq!(cur.sub_id, "0xnew");
        assert_eq!(cur.state, "active");
        assert_eq!(cur.plan_tier, 3);
    }

    #[test]
    fn app_reconcile_marks_canceled_only_from_authoritative_state() {
        let mut c = SubscriptionCache::default();
        c.put(entry_proto("app/1", "api.example.com", "0xsub", "active"));
        assert!(c.resolve("app/1", "https://api.example.com/data").is_some());

        let canceled_item = BuyerSubscriptionItem {
            chain_index: 196,
            offer_id: "offer-canceled".to_string(),
            sub_id: "0xsub".to_string(),
            state: 3,
            payer: "0xbuyer".to_string(),
            token: "0xtoken".to_string(),
            amount_per_period: "5000000".to_string(),
            period_sec: 2_592_000,
            period_mode: 0,
            billing_anchor_at: 0,
            max_periods: 12,
            start_at: 0,
            initial_charge_periods: 1,
            initial_charge_amount: "5000000".to_string(),
            last_charged_period: 1,
            total_pulled: "5000000".to_string(),
            plan_id: "basic".to_string(),
            plan_tier: 1,
            changed_to_sub_id: None,
            pending_change: None,
            is_active: false,
            service_ended: true,
            current_period: 1,
            next_chargeable_at: None,
        };

        c.reconcile_from(&[canceled_item]);

        let cached = c.get("app/1", "api.example.com").unwrap();
        assert_eq!(cached.state, "canceled");
        assert!(c.resolve("app/1", "https://api.example.com/data").is_none());
    }
}
