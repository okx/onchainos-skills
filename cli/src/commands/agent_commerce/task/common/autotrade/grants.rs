//! FR-8 per-trade authorization grants.
//!
//! One file per `jobId` at `<onchainos_home>/autotrade/grants/<jobId>.json`,
//! written **whole** in one shot (a per-venue partial write would clobber other
//! venues). `check_grant` is the single shared validation core behind the
//! `autotrade-grant-check` subcommand used by model-selected Skills/plugins.

use std::collections::BTreeMap;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use super::amount::Decimal;

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct GrantFile {
    /// This version = 1; a higher version ⇒ reject.
    pub version: u32,
    pub job_id: String,
    /// `"dex" | "defi" | "polymarket" | "trade_kit"` ⇒ caps.
    pub grants: BTreeMap<String, VenueGrant>,
    /// seconds.
    pub created_at: u64,
    /// seconds.
    pub expires_at: u64,
}

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct VenueGrant {
    pub max_buy: Option<String>,
    pub max_sell: Option<String>,
}

/// The current grant-file schema version.
pub const GRANT_VERSION: u32 = 1;

// ── Stable deny reasons (process-level only; NFR-3: never echo file content) ──
pub const DENY_INVALID_JOB_ID: &str = "invalid job id";
pub const DENY_INVALID_VENUE: &str = "invalid venue";
pub const DENY_INVALID_ACTION: &str = "invalid action";
pub const DENY_INVALID_AMOUNT: &str = "invalid amount";
pub const DENY_INVALID_FORMAT: &str = "invalid format";
pub const DENY_NO_GRANT_FILE: &str = "no grant file";
pub const DENY_GRANT_UNREADABLE: &str = "grant file unreadable";
pub const DENY_VERSION_TOO_NEW: &str = "grant version too new";
pub const DENY_JOB_MISMATCH: &str = "grant job mismatch";
pub const DENY_EXPIRED: &str = "grant expired";
pub const DENY_VENUE_NOT_AUTHORIZED: &str = "venue not authorized";
pub const DENY_NO_CAP: &str = "no cap for action";
pub const DENY_OVER_CAP: &str = "per-trade cap exceeded";

const VENUES: &[&str] = &["dex", "defi", "polymarket", "trade_kit"];
const ACTIONS: &[&str] = &["buy", "sell"];

/// Map plugin-specific venue names onto the stable grant namespaces.
///
/// Hyperliquid plugin v0.6 sends `--venue hyperliquid`, while existing consent
/// files deliberately store all DEX execution under the generic `dex` grant.
/// Canonicalizing at the public check boundary keeps old grant files valid and
/// avoids duplicating the same cap under a plugin-specific key.
fn canonical_venue(venue: &str) -> Option<&str> {
    match venue {
        "hyperliquid" => Some("dex"),
        venue if VENUES.contains(&venue) => Some(venue),
        _ => None,
    }
}

/// A grant-check denial carrying a stable, process-level reason.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GrantDeny(pub &'static str);

impl std::fmt::Display for GrantDeny {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.0)
    }
}

impl std::error::Error for GrantDeny {}

impl GrantDeny {
    /// A stable snake_case machine code for this denial, used for the buyer-inbound
    /// notify-only `reason` + audit. Distinct from the human message carried in `.0`,
    /// which is the bespoke grant-check `{ok,reason}` stdout contract (NFR-3). Keeping
    /// the real reason (no-grant-file / expired / venue-not-authorized / no-cap / …)
    /// instead of collapsing every denial onto `over_cap`.
    pub fn code(&self) -> &'static str {
        match self.0 {
            DENY_INVALID_JOB_ID => "grant_invalid_job_id",
            DENY_INVALID_VENUE => "grant_invalid_venue",
            DENY_INVALID_ACTION => "grant_invalid_action",
            DENY_INVALID_AMOUNT => "grant_invalid_amount",
            DENY_INVALID_FORMAT => "grant_invalid_format",
            DENY_NO_GRANT_FILE => "no_grant_file",
            DENY_GRANT_UNREADABLE => "grant_unreadable",
            DENY_VERSION_TOO_NEW => "grant_version_too_new",
            DENY_JOB_MISMATCH => "grant_job_mismatch",
            DENY_EXPIRED => "grant_expired",
            DENY_VENUE_NOT_AUTHORIZED => "venue_not_authorized",
            DENY_NO_CAP => "no_cap",
            DENY_OVER_CAP => "over_cap",
            _ => "grant_denied",
        }
    }
}

/// `jobId` charset for use as a filename: `[A-Za-z0-9_-]` (path-traversal defense).
pub(crate) fn job_id_is_safe(job_id: &str) -> bool {
    !job_id.is_empty()
        && job_id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
}

/// `<onchainos_home>/autotrade/grants/<jobId>.json`. Caller MUST have charset-checked
/// `job_id` first.
fn grant_path(job_id: &str) -> Result<PathBuf, GrantDeny> {
    let home = crate::home::onchainos_home().map_err(|_| GrantDeny(DENY_GRANT_UNREADABLE))?;
    Ok(home
        .join("autotrade")
        .join("grants")
        .join(format!("{job_id}.json")))
}

/// Current wall-clock time in seconds since the Unix epoch.
fn now_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// The single shared grant-check core (FR-8). Runs the fixed validation chain in
/// order; any failure ⇒ `GrantDeny(reason)`.
///
/// `venue` ∈ {dex,hyperliquid,defi,polymarket,trade_kit}, where `hyperliquid`
/// is an alias of `dex`; `action` ∈ {buy,sell}. `buy` checks against `maxBuy`,
/// `sell` against `maxSell`.
pub fn check_grant(job_id: &str, venue: &str, action: &str, amount: &str) -> Result<(), GrantDeny> {
    // 1. jobId charset (path-traversal defense) — before any filesystem use.
    if !job_id_is_safe(job_id) {
        return Err(GrantDeny(DENY_INVALID_JOB_ID));
    }
    // 2. venue enum.
    let venue = canonical_venue(venue).ok_or(GrantDeny(DENY_INVALID_VENUE))?;
    // 3. action enum.
    if !ACTIONS.contains(&action) {
        return Err(GrantDeny(DENY_INVALID_ACTION));
    }
    // 4. amount parseable (overflow / non-decimal ⇒ deny).
    let amount = Decimal::parse(amount).map_err(|_| GrantDeny(DENY_INVALID_AMOUNT))?;
    // 5. grant file. DENY-BY-DEFAULT (consent flow, product 2026-07-17): a MISSING grant
    // file ⇒ DENY. The client-side per-trade cap now lives in the consent record and is
    // mirrored to the grant file by `autotrade-consent-set` ONLY for `Auto` mode. A
    // missing file therefore means "not consented to auto-execute" (first-time / manual /
    // declined), which must not silently pass this out-of-process (plugin) check. In the
    // in-process pipeline the consent gate already decided this before we reach here; this
    // is defense-in-depth + the plugin's only enforcement point. (Was permissive-by-
    // default under WBW-13715, when there was no consent record.)
    let path = grant_path(job_id)?;
    if !path.exists() {
        return Err(GrantDeny(DENY_NO_GRANT_FILE));
    }
    // 6. JSON parseable.
    let raw = std::fs::read_to_string(&path).map_err(|_| GrantDeny(DENY_GRANT_UNREADABLE))?;
    let grant: GrantFile =
        serde_json::from_str(&raw).map_err(|_| GrantDeny(DENY_GRANT_UNREADABLE))?;
    // 7. version <= current.
    if grant.version > GRANT_VERSION {
        return Err(GrantDeny(DENY_VERSION_TOO_NEW));
    }
    // 8. file jobId matches input.
    if grant.job_id != job_id {
        return Err(GrantDeny(DENY_JOB_MISMATCH));
    }
    // 9. not expired.
    if grant.expires_at <= now_secs() {
        return Err(GrantDeny(DENY_EXPIRED));
    }
    // 10. venue authorized.
    let venue_grant = grant
        .grants
        .get(venue)
        .ok_or(GrantDeny(DENY_VENUE_NOT_AUTHORIZED))?;
    // 11. that action has a cap.
    let cap_str = match action {
        "buy" => venue_grant.max_buy.as_deref(),
        _ => venue_grant.max_sell.as_deref(),
    }
    .ok_or(GrantDeny(DENY_NO_CAP))?;
    let cap = Decimal::parse(cap_str).map_err(|_| GrantDeny(DENY_OVER_CAP))?;
    // 12. amount <= cap (exact decimal compare; overflow already ⇒ deny above).
    if amount.le(&cap) {
        Ok(())
    } else {
        Err(GrantDeny(DENY_OVER_CAP))
    }
}

/// Write a whole grant file for one venue's caps (`version = 1`). Dev-seeding only
/// today — no release caller until the external create-subscription flow exists,
/// hence `allow(dead_code)` in release to avoid `-D warnings`.
#[cfg_attr(not(debug_assertions), allow(dead_code))]
pub fn write_grant(
    job_id: &str,
    venue: &str,
    max_buy: Option<&str>,
    max_sell: Option<&str>,
    ttl_sec: u64,
) -> anyhow::Result<()> {
    if !job_id_is_safe(job_id) {
        anyhow::bail!("invalid job id");
    }
    let venue = canonical_venue(venue).ok_or_else(|| {
        anyhow::anyhow!("invalid venue (must be dex|hyperliquid|defi|polymarket|trade_kit)")
    })?;
    if ttl_sec == 0 {
        anyhow::bail!("--ttl-sec must be > 0");
    }
    if max_buy.is_none() && max_sell.is_none() {
        anyhow::bail!("at least one of --max-buy / --max-sell is required");
    }
    // Caps must parse for the comparator.
    if let Some(b) = max_buy {
        Decimal::parse(b).map_err(|_| anyhow::anyhow!("--max-buy is not a valid decimal"))?;
    }
    if let Some(s) = max_sell {
        Decimal::parse(s).map_err(|_| anyhow::anyhow!("--max-sell is not a valid decimal"))?;
    }

    let created_at = now_secs();
    let mut grants = BTreeMap::new();
    grants.insert(
        venue.to_string(),
        VenueGrant {
            max_buy: max_buy.map(str::to_string),
            max_sell: max_sell.map(str::to_string),
        },
    );
    let grant = GrantFile {
        version: GRANT_VERSION,
        job_id: job_id.to_string(),
        grants,
        created_at,
        expires_at: created_at + ttl_sec,
    };

    let path = grant_path(job_id).map_err(|d| anyhow::anyhow!("{}", d.0))?;
    let body = serde_json::to_string_pretty(&grant)?;
    crate::home::write_secure(&path, body.as_bytes())?;
    Ok(())
}

/// Sells are not cap-gated under the consent model (a sell is naturally bounded by the
/// position held — L1, WBW-13715). But the out-of-process plugin grant-check denies a
/// sell whose venue has no `maxSell` (`no cap for action`), so the grant must mirror an
/// effectively-unlimited sell allowance, NOT leave it unset — otherwise auto-mode
/// polymarket sells get blocked by the plugin.
const CONSENT_SELL_UNLIMITED: &str = "1000000000000000000"; // 1e18, far above any real sell

/// Mirror the consent per-trade cap into the grant file so model-selected execution
/// tools enforce the same buy-side cap as the in-process pipeline. Existing DEX/DeFi/
/// Polymarket routes retain their reduce-position sell allowance. Trade Kit is kept in
/// a separate authorization domain and caps both sides because a CEX derivative
/// `sell` may open or increase a short rather than reduce an existing position.
/// Written by `autotrade-consent-set` for `Auto` mode.
pub fn write_cap_grant(job_id: &str, cap_u: &str, ttl_sec: u64) -> anyhow::Result<()> {
    if !job_id_is_safe(job_id) {
        anyhow::bail!("invalid job id");
    }
    if ttl_sec == 0 {
        anyhow::bail!("ttl must be > 0");
    }
    Decimal::parse(cap_u).map_err(|_| anyhow::anyhow!("cap is not a valid decimal"))?;

    let created_at = now_secs();
    let mut grants = BTreeMap::new();
    for venue in VENUES {
        let max_sell = if *venue == "trade_kit" {
            cap_u
        } else {
            CONSENT_SELL_UNLIMITED
        };
        grants.insert(
            (*venue).to_string(),
            VenueGrant {
                max_buy: Some(cap_u.to_string()),
                max_sell: Some(max_sell.to_string()),
            },
        );
    }
    let grant = GrantFile {
        version: GRANT_VERSION,
        job_id: job_id.to_string(),
        grants,
        created_at,
        expires_at: created_at + ttl_sec,
    };
    let path = grant_path(job_id).map_err(|d| anyhow::anyhow!("{}", d.0))?;
    crate::home::write_secure(&path, serde_json::to_string_pretty(&grant)?.as_bytes())?;
    Ok(())
}

/// Remove the grant file (Manual / Decline — there is no auto-execute cap to enforce).
pub fn clear_grant(job_id: &str) {
    if let Ok(path) = grant_path(job_id) {
        let _ = std::fs::remove_file(path);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Set ONCHAINOS_HOME to an isolated temp dir for the duration of a test.
    fn with_home<F: FnOnce()>(f: F) {
        let _lock = crate::home::TEST_ENV_MUTEX.lock().unwrap();
        let tmp = tempfile::tempdir().unwrap();
        std::env::set_var("ONCHAINOS_HOME", tmp.path());
        f();
        std::env::remove_var("ONCHAINOS_HOME");
    }

    #[test]
    fn no_file_denies_by_default() {
        with_home(|| {
            // Deny-by-default (consent flow, 2026-07-17): no grant file ⇒ deny. The cap
            // lives in the consent record and is mirrored to the grant file only for Auto
            // mode, so a missing file means "not consented to auto-execute".
            assert_eq!(
                check_grant("nogrant", "dex", "buy", "1").unwrap_err().0,
                DENY_NO_GRANT_FILE
            );
        });
    }

    #[test]
    fn write_cap_grant_mirrors_cap_for_all_venues() {
        with_home(|| {
            write_cap_grant("job1", "100", 3600).unwrap();
            // within cap on every venue's buy
            assert!(check_grant("job1", "dex", "buy", "100").is_ok());
            assert!(check_grant("job1", "hyperliquid", "buy", "100").is_ok());
            assert!(check_grant("job1", "defi", "buy", "50").is_ok());
            assert!(check_grant("job1", "polymarket", "buy", "99.99").is_ok());
            assert!(check_grant("job1", "trade_kit", "buy", "100").is_ok());
            assert!(check_grant("job1", "trade_kit", "sell", "100").is_ok());
            // over cap denies
            assert_eq!(
                check_grant("job1", "dex", "buy", "100.01").unwrap_err().0,
                DENY_OVER_CAP
            );
            assert_eq!(
                check_grant("job1", "hyperliquid", "buy", "100.01")
                    .unwrap_err()
                    .0,
                DENY_OVER_CAP
            );
            assert_eq!(
                check_grant("job1", "trade_kit", "buy", "100.01")
                    .unwrap_err()
                    .0,
                DENY_OVER_CAP
            );
            assert_eq!(
                check_grant("job1", "trade_kit", "sell", "100.01")
                    .unwrap_err()
                    .0,
                DENY_OVER_CAP
            );
            // sells are NOT cap-gated under consent — any sell passes (regression fix:
            // maxSell was unset → DENY_NO_CAP blocked auto-mode polymarket sells).
            assert!(check_grant("job1", "polymarket", "sell", "5").is_ok());
            assert!(check_grant("job1", "polymarket", "sell", "999999999").is_ok());
            assert!(check_grant("job1", "dex", "sell", "12345").is_ok());
            // clear ⇒ back to deny-by-default
            clear_grant("job1");
            assert_eq!(
                check_grant("job1", "dex", "buy", "1").unwrap_err().0,
                DENY_NO_GRANT_FILE
            );
        });
    }

    #[test]
    fn ac9_path_traversal_job_id_denied() {
        with_home(|| {
            let err = check_grant("../../etc/passwd", "dex", "buy", "1").unwrap_err();
            assert_eq!(err.0, DENY_INVALID_JOB_ID);
        });
    }

    #[test]
    fn ac9_write_then_within_cap_allows() {
        with_home(|| {
            write_grant("job1", "dex", Some("100"), None, 3600).unwrap();
            assert!(check_grant("job1", "dex", "buy", "100").is_ok());
            assert!(check_grant("job1", "dex", "buy", "99.99").is_ok());
        });
    }

    #[test]
    fn ac9_over_cap_denies() {
        with_home(|| {
            write_grant("job1", "dex", Some("100"), None, 3600).unwrap();
            let err = check_grant("job1", "dex", "buy", "100.01").unwrap_err();
            assert_eq!(err.0, DENY_OVER_CAP);
        });
    }

    #[test]
    fn ac9_no_cap_for_action_denies() {
        with_home(|| {
            // maxBuy set but maxSell absent → sell denied.
            write_grant("job1", "dex", Some("100"), None, 3600).unwrap();
            let err = check_grant("job1", "dex", "sell", "1").unwrap_err();
            assert_eq!(err.0, DENY_NO_CAP);
        });
    }

    #[test]
    fn ac9_wrong_venue_denies() {
        with_home(|| {
            write_grant("job1", "dex", Some("100"), None, 3600).unwrap();
            let err = check_grant("job1", "polymarket", "buy", "1").unwrap_err();
            assert_eq!(err.0, DENY_VENUE_NOT_AUTHORIZED);
        });
    }

    #[test]
    fn ac9_expired_denies() {
        with_home(|| {
            // ttl 1s; write then force expiry by rewriting expires_at in the past.
            write_grant("job1", "dex", Some("100"), None, 1).unwrap();
            let path = grant_path("job1").unwrap();
            let mut grant: GrantFile =
                serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
            grant.expires_at = 1; // far in the past
            std::fs::write(&path, serde_json::to_string(&grant).unwrap()).unwrap();
            let err = check_grant("job1", "dex", "buy", "1").unwrap_err();
            assert_eq!(err.0, DENY_EXPIRED);
        });
    }

    #[test]
    fn ac9_version_too_new_denies() {
        with_home(|| {
            write_grant("job1", "dex", Some("100"), None, 3600).unwrap();
            let path = grant_path("job1").unwrap();
            let mut grant: GrantFile =
                serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
            grant.version = GRANT_VERSION + 1;
            std::fs::write(&path, serde_json::to_string(&grant).unwrap()).unwrap();
            let err = check_grant("job1", "dex", "buy", "1").unwrap_err();
            assert_eq!(err.0, DENY_VERSION_TOO_NEW);
        });
    }

    #[test]
    fn invalid_venue_and_action_denied() {
        with_home(|| {
            assert_eq!(
                check_grant("job1", "nasdaq", "buy", "1").unwrap_err().0,
                DENY_INVALID_VENUE
            );
            assert_eq!(
                check_grant("job1", "dex", "hodl", "1").unwrap_err().0,
                DENY_INVALID_ACTION
            );
            assert_eq!(
                check_grant("job1", "dex", "buy", "abc").unwrap_err().0,
                DENY_INVALID_AMOUNT
            );
        });
    }

    #[test]
    fn write_grant_rejects_bad_input() {
        with_home(|| {
            assert!(write_grant("job1", "dex", None, None, 3600).is_err()); // no cap
            assert!(write_grant("job1", "dex", Some("100"), None, 0).is_err()); // ttl 0
            assert!(write_grant("job1", "bogus", Some("100"), None, 3600).is_err()); // venue
            assert!(write_grant("job1", "dex", Some("abc"), None, 3600).is_err());
            // unparseable cap
        });
    }

    // ── FR-8: GrantDeny → stable snake_case code (no over_cap collapsing) ──
    #[test]
    fn grant_deny_code_maps_reasons() {
        assert_eq!(GrantDeny(DENY_NO_GRANT_FILE).code(), "no_grant_file");
        assert_eq!(GrantDeny(DENY_EXPIRED).code(), "grant_expired");
        assert_eq!(
            GrantDeny(DENY_VENUE_NOT_AUTHORIZED).code(),
            "venue_not_authorized"
        );
        assert_eq!(GrantDeny(DENY_NO_CAP).code(), "no_cap");
        assert_eq!(GrantDeny(DENY_OVER_CAP).code(), "over_cap");
        assert_eq!(
            GrantDeny(DENY_INVALID_JOB_ID).code(),
            "grant_invalid_job_id"
        );
        assert_eq!(
            GrantDeny(DENY_VERSION_TOO_NEW).code(),
            "grant_version_too_new"
        );
        // Distinct reasons must not all collapse onto the same code.
        assert_ne!(
            GrantDeny(DENY_NO_GRANT_FILE).code(),
            GrantDeny(DENY_OVER_CAP).code()
        );
    }
}
