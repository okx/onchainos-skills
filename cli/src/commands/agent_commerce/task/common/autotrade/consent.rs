//! Buyer-side auto-trade **consent** record (product revision 2026-07-17).
//!
//! One file per `jobId` at `<onchainos_home>/autotrade/consent/<jobId>.json`,
//! written **whole** in one shot. The consent record is the client-side gate that
//! sits AFTER the exact-Active backend subscription gate. `copyTrade` is ignored;
//! the model-driven session uses this policy when handling an inbound signal:
//!
//! - no record (first time) ⇒ ask the user a three-way decision, then remember it
//! - `Auto` + amount ≤ `capU` ⇒ auto-execute (execution card)
//! - `Auto` + amount > `capU` ⇒ re-ask (over-cap)
//! - `Manual` ⇒ do not auto-execute; ask a bounded one-shot execute/skip decision
//! - `Decline` ⇒ the previous delivery was skipped; a later signal has no authorization
//!
//! Key granularity is **per-job** (product call, 2026-07-17): consent authorizes
//! THIS subscription, not the ASP as a whole (a subscription's `jobId` is stable
//! across renewals, so this does not re-prompt on renew). The per-trade `capU` is a
//! single global buy-side cap in quote-stablecoin units (PRD: USDT); sells are never cap-gated (L1,
//! WBW-13715 — a sell is naturally bounded by the position held).
//!
//! Supersedes the per-venue `grants.rs` cap model: the consent record is now the
//! single source of truth exposed through `autotrade-grant-check`, so the
//! Skill-selected execution tool enforces the same client-side cap.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use super::amount::Decimal;
use super::grants::job_id_is_safe;

/// The current consent-file schema version.
pub const CONSENT_VERSION: u32 = 1;

/// How the buyer wants this subscription's Active signals handled.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum ConsentMode {
    /// Auto-execute within `capU` (buy-side); over-cap re-asks.
    Auto,
    /// Never auto-execute; surface the command for the user to run each time.
    Manual,
    /// Do not execute; notify only.
    Decline,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ConsentFile {
    /// This version = 1; a higher version ⇒ reject (treat as unreadable).
    pub version: u32,
    pub job_id: String,
    pub mode: ConsentMode,
    /// Per-trade buy-side cap in quote-stablecoin units (PRD copy denominates in
    /// USDT). Present (and enforced) only for `Auto`; `None`/absent for
    /// `Manual` / `Decline`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cap_u: Option<String>,
    /// Fixed quote-currency amount used for each parsed-text copy trade. This is
    /// deliberately separate from `capU`: the amount says what to trade, while
    /// the cap remains the maximum the auto-consent permits. Older consent files
    /// predate this field and deserialize it as `None`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub trade_amount_u: Option<String>,
    /// The stablecoin the buyer pays dex buys with / receives on sells
    /// (`usdc` | `usdt`, lowercase alias). Captured from the consent reply
    /// ("A 每笔100 用USDC"); absent ⇒ the default, USDT (PRD denomination).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub quote_token: Option<String>,
    /// seconds since epoch.
    pub created_at: u64,
    /// seconds since epoch.
    pub expires_at: u64,
}

/// Read-only consent context exposed to the model-driven subscription session.
///
/// This deliberately contains only previously persisted policy. Conversation
/// text and `serviceDescription` are never converted into authorization here.
/// A missing/expired record is `NotSet`; a corrupt record is `Unreadable` so the
/// model can fail closed instead of treating it as a first-time prompt.
#[derive(Serialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ConsentSnapshotStatus {
    NotSet,
    Active,
    Unreadable,
}

impl ConsentSnapshotStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::NotSet => "not_set",
            Self::Active => "active",
            Self::Unreadable => "unreadable",
        }
    }
}

#[derive(Serialize, Clone, Debug, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ConsentSnapshot {
    pub status: ConsentSnapshotStatus,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mode: Option<ConsentMode>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cap_u: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub trade_amount_u: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub quote_token: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub created_at: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub expires_at: Option<u64>,
}

impl ConsentSnapshot {
    fn not_set() -> Self {
        Self {
            status: ConsentSnapshotStatus::NotSet,
            mode: None,
            cap_u: None,
            trade_amount_u: None,
            quote_token: None,
            created_at: None,
            expires_at: None,
        }
    }

    fn unreadable() -> Self {
        Self {
            status: ConsentSnapshotStatus::Unreadable,
            ..Self::not_set()
        }
    }
}

/// The default quote stablecoin (PRD denominates the whole flow in USDT).
pub const DEFAULT_QUOTE: &str = "usdt";
/// Quote stablecoins a consent may select. Whitelisted so the "per-trade cap in
/// stablecoin dollars" semantics can't be bent onto an arbitrary token.
pub const QUOTE_WHITELIST: [&str; 2] = ["usdc", "usdt"];

/// The quote stablecoin alias in effect for `job_id`: the consent record's
/// choice, else [`DEFAULT_QUOTE`]. Always lowercase, always whitelisted.
pub fn quote_token(job_id: &str) -> String {
    load_consent(job_id)
        .ok()
        .flatten()
        .and_then(|c| c.quote_token)
        .filter(|q| QUOTE_WHITELIST.contains(&q.as_str()))
        .unwrap_or_else(|| DEFAULT_QUOTE.to_string())
}

// ── Stable, process-level failure reasons (never echo file content) ──
pub const CONSENT_UNREADABLE: &str = "consent_unreadable";
pub const CONSENT_VERSION_TOO_NEW: &str = "consent_version_too_new";
pub const CONSENT_JOB_MISMATCH: &str = "consent_job_mismatch";

/// A hard consent read failure (present-but-broken record). Distinct from the
/// normal "no record / declined / manual / auto" states carried by
/// [`ConsentDecision`]. Fails closed at the pipeline (degrade → notify).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConsentError(pub &'static str);

impl std::fmt::Display for ConsentError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.0)
    }
}
impl std::error::Error for ConsentError {}

/// The client-side consent verdict for one inbound Active signal.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConsentDecision {
    /// No record yet — ask the three-way decision.
    FirstTime,
    /// `Decline` (C = "don't execute THIS one") — re-ask the three-way on every signal (auto
    /// stays off; C only rejects the current signal, it is not a persistent decline).
    Declined,
    /// `Manual` (B = "execute this one, then ask every time") — re-ask the three-way on every
    /// signal (auto stays off).
    Manual,
    /// `Auto`, and this action is allowed to auto-execute (sell, or buy within cap).
    AutoAllow,
    /// `Auto`, but a buy exceeds `capU` (or `Auto` with no cap on a buy) — re-ask.
    AutoOverCap,
}

/// `<onchainos_home>/autotrade/consent/<jobId>.json`. Caller MUST have
/// charset-checked `job_id` first (path-traversal defense).
fn consent_path(job_id: &str) -> Result<PathBuf, ConsentError> {
    let home = crate::home::onchainos_home().map_err(|_| ConsentError(CONSENT_UNREADABLE))?;
    Ok(home
        .join("autotrade")
        .join("consent")
        .join(format!("{job_id}.json")))
}

/// Current wall-clock time in seconds since the Unix epoch.
fn now_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// Load + validate the consent record for `job_id`.
///
/// Returns `Ok(None)` when there is no record OR the record has **expired**
/// (both ⇒ treat as first-time and re-ask). Returns `Err` only for a
/// present-but-broken record (unreadable / version-too-new / job mismatch), which
/// the pipeline fails closed on. `Ok(Some(_))` is a live, valid record.
pub fn load_consent(job_id: &str) -> Result<Option<ConsentFile>, ConsentError> {
    if !job_id_is_safe(job_id) {
        // Charset failure is handled by the pipeline's entry guard; be defensive.
        return Err(ConsentError(CONSENT_UNREADABLE));
    }
    let path = consent_path(job_id)?;
    if !path.exists() {
        return Ok(None);
    }
    let raw = std::fs::read_to_string(&path).map_err(|_| ConsentError(CONSENT_UNREADABLE))?;
    let file: ConsentFile =
        serde_json::from_str(&raw).map_err(|_| ConsentError(CONSENT_UNREADABLE))?;
    if file.version > CONSENT_VERSION {
        return Err(ConsentError(CONSENT_VERSION_TOO_NEW));
    }
    if file.job_id != job_id {
        return Err(ConsentError(CONSENT_JOB_MISMATCH));
    }
    // Expired ⇒ re-ask (first-time), not a hard error.
    if file.expires_at <= now_secs() {
        return Ok(None);
    }
    Ok(Some(file))
}

/// Return a bounded, serialization-safe view of the current local policy.
/// Errors are represented explicitly and never expose file content or paths.
pub fn consent_snapshot(job_id: &str) -> ConsentSnapshot {
    match load_consent(job_id) {
        Ok(Some(file)) => ConsentSnapshot {
            status: ConsentSnapshotStatus::Active,
            mode: Some(file.mode),
            cap_u: file.cap_u,
            trade_amount_u: file.trade_amount_u,
            quote_token: file.quote_token,
            created_at: Some(file.created_at),
            expires_at: Some(file.expires_at),
        },
        Ok(None) => ConsentSnapshot::not_set(),
        Err(_) => ConsentSnapshot::unreadable(),
    }
}

/// The client-side consent verdict for one inbound Active signal.
///
/// `buy_amount` is `Some` only for a cap-relevant **buy** (dex buy / defi deposit /
/// polymarket buy — spend in quote-stablecoin units); `None` for sells and no-spend
/// actions, which are never cap-gated. The mode dispatch (first-time / declined /
/// manual / auto) is independent of `buy_amount`; only the `Auto` cap check uses it.
pub fn evaluate_consent(
    job_id: &str,
    buy_amount: Option<&Decimal>,
) -> Result<ConsentDecision, ConsentError> {
    let Some(file) = load_consent(job_id)? else {
        return Ok(ConsentDecision::FirstTime);
    };
    match file.mode {
        ConsentMode::Decline => Ok(ConsentDecision::Declined),
        ConsentMode::Manual => Ok(ConsentDecision::Manual),
        ConsentMode::Auto => match buy_amount {
            // Sell / no-spend under Auto ⇒ always allowed (never cap-gated).
            None => Ok(ConsentDecision::AutoAllow),
            // Buy under Auto ⇒ enforce the per-trade cap. A missing/unparseable cap
            // on an Auto buy fails closed to a re-ask (never an uncapped auto-buy).
            Some(amount) => {
                let within = file
                    .cap_u
                    .as_deref()
                    .and_then(|c| Decimal::parse(c).ok())
                    .map(|cap| amount.le(&cap))
                    .unwrap_or(false);
                Ok(if within {
                    ConsentDecision::AutoAllow
                } else {
                    ConsentDecision::AutoOverCap
                })
            }
        },
    }
}

/// Persist a consent record for `job_id` (release-mode; replaces the debug-only
/// `grants::write_grant` seeding). `cap_u` is required and validated for `Auto`,
/// and must be absent for `Manual` / `Decline`.
pub fn write_consent(
    job_id: &str,
    mode: ConsentMode,
    cap_u: Option<&str>,
    quote: Option<&str>,
    ttl_sec: u64,
) -> anyhow::Result<()> {
    write_consent_with_trade_amount(job_id, mode, cap_u, None, quote, ttl_sec)
}

/// Persist consent together with an optional fixed per-trade amount.
///
/// `trade_amount_u = None` preserves an existing configured amount, matching
/// the existing `quote = None` behavior. This is important for cap-only rewrites
/// after an over-cap decision: raising the cap must not silently erase the
/// subscription's configured execution amount. An explicitly supplied amount
/// must be a positive decimal.
pub fn write_consent_with_trade_amount(
    job_id: &str,
    mode: ConsentMode,
    cap_u: Option<&str>,
    trade_amount_u: Option<&str>,
    quote: Option<&str>,
    ttl_sec: u64,
) -> anyhow::Result<()> {
    if !job_id_is_safe(job_id) {
        anyhow::bail!("invalid job id");
    }
    if ttl_sec == 0 {
        anyhow::bail!("--ttl-sec must be > 0");
    }
    let cap_u = match mode {
        ConsentMode::Auto => {
            let cap = cap_u.ok_or_else(|| anyhow::anyhow!("--cap is required for --mode auto"))?;
            let parsed =
                Decimal::parse(cap).map_err(|_| anyhow::anyhow!("--cap is not a valid decimal"))?;
            if parsed.is_zero() {
                anyhow::bail!("--cap must be greater than 0");
            }
            Some(cap.to_string())
        }
        ConsentMode::Manual | ConsentMode::Decline => {
            if cap_u.is_some() {
                anyhow::bail!("--cap is only valid with --mode auto");
            }
            None
        }
    };
    // Best-effort compatibility read. This intentionally retains the historical
    // behavior of allowing a valid new write to replace a broken old record.
    let existing = load_consent(job_id).ok().flatten();
    let trade_amount_u = match trade_amount_u {
        Some(amount) => {
            let parsed = Decimal::parse(amount)
                .map_err(|_| anyhow::anyhow!("--trade-amount is not a valid decimal"))?;
            if parsed.is_zero() {
                anyhow::bail!("--trade-amount must be greater than 0");
            }
            Some(amount.to_string())
        }
        None => existing.as_ref().and_then(|c| c.trade_amount_u.clone()),
    };
    // Quote preference: explicit value must be whitelisted; absent ⇒ KEEP the
    // existing record's choice (an over-cap raise re-writes the record and must
    // not silently reset a stored preference back to the default).
    let quote_token = match quote {
        Some(q) => {
            let q = q.to_ascii_lowercase();
            if !QUOTE_WHITELIST.contains(&q.as_str()) {
                anyhow::bail!("--quote must be one of: usdc | usdt");
            }
            Some(q)
        }
        None => existing.as_ref().and_then(|c| c.quote_token.clone()),
    };

    let created_at = now_secs();
    let file = ConsentFile {
        version: CONSENT_VERSION,
        job_id: job_id.to_string(),
        mode,
        cap_u,
        trade_amount_u,
        quote_token,
        created_at,
        expires_at: created_at + ttl_sec,
    };

    let path = consent_path(job_id).map_err(|d| anyhow::anyhow!("{}", d.0))?;
    let body = serde_json::to_string_pretty(&file)?;
    crate::home::write_secure(&path, body.as_bytes())?;
    Ok(())
}

/// Remove a pending file written by a pre-model-routing client. New deliveries
/// stay in their persistent subscription session and are never serialized here.
pub fn clear_pending_signal(job_id: &str) {
    if !job_id_is_safe(job_id) {
        return;
    }
    if let Ok(home) = crate::home::onchainos_home() {
        let path = home.join("autotrade").join("pending").join(format!("{job_id}.json"));
        let _ = std::fs::remove_file(path);
    }
}
/// Pause auto copy-trade for this job: delete the consent record so `evaluate_consent`
/// falls back to `FirstTime` — the next signal re-shows the three-way prompt. Best-effort
/// (already-absent is fine). Grant file + pending signal are cleared by the caller.
pub fn clear_consent(job_id: &str) {
    if let Ok(path) = consent_path(job_id) {
        let _ = std::fs::remove_file(path);
    }
}

// ── Plugin-install approval (per job + plugin) ──────────────────────────────
//
// A copy-trade command that needs a plugin (e.g. `polymarket-plugin`) must NOT be
// silently installed inside the headless sub session: compliance requires the first
// install to be user-confirmed, and a sub-side `okx-dapp-discovery` install prompt is
// invisible to the human. Instead the pipeline defers with a plugin-install decision;
// once the user approves and the user session installs the plugin, this marker is
// written so subsequent signals for the same (job, plugin) execute without re-asking.

/// `<onchainos_home>/autotrade/plugin-approved/<jobId>/<plugin>`.
fn plugin_approved_path(job_id: &str, plugin: &str) -> Result<PathBuf, ConsentError> {
    if !job_id_is_safe(job_id) {
        return Err(ConsentError(CONSENT_UNREADABLE));
    }
    let safe_plugin: String = plugin
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || *c == '-' || *c == '_')
        .collect();
    if safe_plugin.is_empty() {
        return Err(ConsentError(CONSENT_UNREADABLE));
    }
    let home = crate::home::onchainos_home().map_err(|_| ConsentError(CONSENT_UNREADABLE))?;
    Ok(home
        .join("autotrade")
        .join("plugin-approved")
        .join(job_id)
        .join(safe_plugin))
}

/// True when the user has approved installing `plugin` for `job_id`.
pub fn plugin_approved(job_id: &str, plugin: &str) -> bool {
    plugin_approved_path(job_id, plugin)
        .map(|p| p.exists())
        .unwrap_or(false)
}

/// Record the user's approval to install `plugin` for `job_id` (best-effort marker).
pub fn write_plugin_approved(job_id: &str, plugin: &str) -> Result<(), ConsentError> {
    let path = plugin_approved_path(job_id, plugin)?;
    crate::home::write_secure(&path, b"1").map_err(|_| ConsentError(CONSENT_UNREADABLE))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dec(s: &str) -> Decimal {
        Decimal::parse(s).unwrap()
    }

    /// Set ONCHAINOS_HOME to an isolated temp dir for the duration of a test.
    fn with_home<F: FnOnce()>(f: F) {
        // macOS may deny access to the process-wide temporary directory in
        // hardened test environments. Keep these fixtures under Cargo's local
        // target directory instead, which is already known to be writable.
        // Recover a poisoned mutex too, so one failed fixture does not turn all
        // subsequent tests into unrelated `PoisonError` failures.
        let _lock = crate::home::TEST_ENV_MUTEX
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let temp_root = std::env::current_dir()
            .expect("current working directory")
            .join("target")
            .join("consent-test-home");
        std::fs::create_dir_all(&temp_root).expect("create consent test temp root");
        let tmp = tempfile::tempdir_in(temp_root).expect("create consent test home");
        std::env::set_var("ONCHAINOS_HOME", tmp.path());
        f();
        std::env::remove_var("ONCHAINOS_HOME");
    }

    #[test]
    fn no_record_is_first_time() {
        with_home(|| {
            assert_eq!(
                evaluate_consent("job1", Some(&dec("10"))).unwrap(),
                ConsentDecision::FirstTime
            );
            assert_eq!(
                consent_snapshot("job1"),
                ConsentSnapshot {
                    status: ConsentSnapshotStatus::NotSet,
                    mode: None,
                    cap_u: None,
                    trade_amount_u: None,
                    quote_token: None,
                    created_at: None,
                    expires_at: None,
                }
            );
        });
    }

    #[test]
    fn snapshot_exposes_only_persisted_policy() {
        with_home(|| {
            write_consent_with_trade_amount(
                "job1",
                ConsentMode::Auto,
                Some("50"),
                Some("12.5"),
                Some("USDC"),
                3600,
            )
            .unwrap();

            let snapshot = consent_snapshot("job1");
            assert_eq!(snapshot.status, ConsentSnapshotStatus::Active);
            assert_eq!(snapshot.mode, Some(ConsentMode::Auto));
            assert_eq!(snapshot.cap_u.as_deref(), Some("50"));
            assert_eq!(snapshot.trade_amount_u.as_deref(), Some("12.5"));
            assert_eq!(snapshot.quote_token.as_deref(), Some("usdc"));
            assert!(snapshot.created_at.is_some());
            assert!(snapshot.expires_at.is_some());
        });
    }

    #[test]
    fn snapshot_marks_broken_policy_unreadable() {
        with_home(|| {
            let path = consent_path("job1").unwrap();
            crate::home::write_secure(&path, b"not-json").unwrap();
            assert_eq!(
                consent_snapshot("job1").status,
                ConsentSnapshotStatus::Unreadable
            );
        });
    }

    #[test]
    fn quote_preference_defaults_persists_and_survives_cap_rewrite() {
        with_home(|| {
            // No record ⇒ the default (PRD denominates in USDT).
            assert_eq!(quote_token("job1"), "usdt");
            // Explicit choice is stored, case-normalized.
            write_consent("job1", ConsentMode::Auto, Some("50"), Some("USDC"), 3600).unwrap();
            assert_eq!(quote_token("job1"), "usdc");
            // Over-cap raise re-writes the record WITHOUT --quote ⇒ choice survives.
            write_consent("job1", ConsentMode::Auto, Some("200"), None, 3600).unwrap();
            assert_eq!(quote_token("job1"), "usdc");
            // Whitelist: arbitrary tokens can't bend the cap semantics.
            assert!(write_consent("job1", ConsentMode::Auto, Some("50"), Some("dai"), 3600).is_err());
            // Manual (B) can carry a preference too.
            write_consent("job1", ConsentMode::Manual, None, Some("usdt"), 3600).unwrap();
            assert_eq!(quote_token("job1"), "usdt");
        });
    }

    #[test]
    fn trade_amount_is_optional_backward_compatible_and_survives_legacy_rewrite() {
        with_home(|| {
            // A v1 file written before tradeAmountU existed remains readable.
            let created_at = now_secs();
            let legacy = serde_json::json!({
                "version": CONSENT_VERSION,
                "jobId": "job1",
                "mode": "auto",
                "capU": "50",
                "createdAt": created_at,
                "expiresAt": created_at + 3600
            });
            let path = consent_path("job1").unwrap();
            crate::home::write_secure(&path, legacy.to_string().as_bytes()).unwrap();
            assert_eq!(load_consent("job1").unwrap().unwrap().trade_amount_u, None);

            write_consent_with_trade_amount(
                "job1",
                ConsentMode::Auto,
                Some("50"),
                Some("12.5"),
                None,
                3600,
            )
            .unwrap();
            assert_eq!(
                load_consent("job1")
                    .unwrap()
                    .unwrap()
                    .trade_amount_u
                    .as_deref(),
                Some("12.5")
            );

            // Existing callers use write_consent (no amount argument). A cap-only
            // rewrite must preserve the fixed amount rather than erasing it.
            write_consent("job1", ConsentMode::Auto, Some("100"), None, 3600).unwrap();
            let saved = load_consent("job1").unwrap().unwrap();
            assert_eq!(saved.cap_u.as_deref(), Some("100"));
            assert_eq!(saved.trade_amount_u.as_deref(), Some("12.5"));
        });
    }

    #[test]
    fn write_consent_validates_fixed_trade_amount() {
        with_home(|| {
            for invalid in ["", "abc", "0"] {
                assert!(write_consent_with_trade_amount(
                    "job1",
                    ConsentMode::Auto,
                    Some("50"),
                    Some(invalid),
                    None,
                    3600,
                )
                .is_err());
            }
            assert!(write_consent_with_trade_amount(
                "job1",
                ConsentMode::Auto,
                Some("50"),
                Some("0.01"),
                None,
                3600,
            )
            .is_ok());
        });
    }

    #[test]
    fn decline_maps_to_declined_decision() {
        with_home(|| {
            // A Decline record maps to ConsentDecision::Declined; the pipeline re-asks on it
            // (C rejects only the current signal, not the whole subscription).
            write_consent("job1", ConsentMode::Decline, None, None, 3600).unwrap();
            assert_eq!(
                evaluate_consent("job1", None).unwrap(),
                ConsentDecision::Declined
            );
        });
    }

    #[test]
    fn manual_surfaces_command() {
        with_home(|| {
            write_consent("job1", ConsentMode::Manual, None, None, 3600).unwrap();
            assert_eq!(
                evaluate_consent("job1", Some(&dec("10"))).unwrap(),
                ConsentDecision::Manual
            );
        });
    }

    #[test]
    fn clear_consent_pauses_back_to_first_time() {
        with_home(|| {
            // Auto authorized within cap → auto-executes.
            write_consent("job1", ConsentMode::Auto, Some("10"), None, 3600).unwrap();
            assert_eq!(
                evaluate_consent("job1", Some(&dec("5"))).unwrap(),
                ConsentDecision::AutoAllow
            );
            // Pause ("暂停自动跟单"): clearing the consent record reverts to FirstTime,
            // so the next signal re-shows the three-way prompt.
            clear_consent("job1");
            assert!(load_consent("job1").unwrap().is_none());
            assert_eq!(
                evaluate_consent("job1", Some(&dec("5"))).unwrap(),
                ConsentDecision::FirstTime
            );
        });
    }

    #[test]
    fn auto_within_cap_allows_and_over_cap_reasks() {
        with_home(|| {
            write_consent("job1", ConsentMode::Auto, Some("100"), None, 3600).unwrap();
            // buy within cap ⇒ allow (boundary inclusive)
            assert_eq!(
                evaluate_consent("job1", Some(&dec("100"))).unwrap(),
                ConsentDecision::AutoAllow
            );
            // buy over cap ⇒ re-ask
            assert_eq!(
                evaluate_consent("job1", Some(&dec("100.01"))).unwrap(),
                ConsentDecision::AutoOverCap
            );
            // sell (no buy amount) ⇒ always allowed under Auto
            assert_eq!(
                evaluate_consent("job1", None).unwrap(),
                ConsentDecision::AutoAllow
            );
        });
    }

    #[test]
    fn expired_record_is_first_time() {
        with_home(|| {
            write_consent("job1", ConsentMode::Auto, Some("100"), None, 1).unwrap();
            // Force expiry into the past.
            let path = consent_path("job1").unwrap();
            let mut file: ConsentFile =
                serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
            file.expires_at = 1;
            std::fs::write(&path, serde_json::to_string(&file).unwrap()).unwrap();
            assert_eq!(
                evaluate_consent("job1", Some(&dec("10"))).unwrap(),
                ConsentDecision::FirstTime
            );
        });
    }

    #[test]
    fn version_too_new_and_job_mismatch_fail_closed() {
        with_home(|| {
            write_consent("job1", ConsentMode::Auto, Some("100"), None, 3600).unwrap();
            let path = consent_path("job1").unwrap();
            let good: ConsentFile =
                serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();

            let mut newer = good.clone();
            newer.version = CONSENT_VERSION + 1;
            std::fs::write(&path, serde_json::to_string(&newer).unwrap()).unwrap();
            assert_eq!(
                load_consent("job1").unwrap_err(),
                ConsentError(CONSENT_VERSION_TOO_NEW)
            );

            let mut mism = good;
            mism.job_id = "other".into();
            std::fs::write(&path, serde_json::to_string(&mism).unwrap()).unwrap();
            assert_eq!(
                load_consent("job1").unwrap_err(),
                ConsentError(CONSENT_JOB_MISMATCH)
            );
        });
    }

    #[test]
    fn write_consent_validates_cap_and_mode() {
        with_home(|| {
            // Auto requires a positive, parseable cap.
            assert!(write_consent("job1", ConsentMode::Auto, None, None, 3600).is_err());
            assert!(write_consent("job1", ConsentMode::Auto, Some("abc"), None, 3600).is_err());
            assert!(write_consent("job1", ConsentMode::Auto, Some("0"), None, 3600).is_err());
            assert!(write_consent("job1", ConsentMode::Auto, Some("50"), None, 3600).is_ok());
            // Manual / Decline must NOT carry a cap.
            assert!(write_consent("job1", ConsentMode::Manual, Some("50"), None, 3600).is_err());
            assert!(write_consent("job1", ConsentMode::Manual, None, None, 3600).is_ok());
            assert!(write_consent("job1", ConsentMode::Decline, None, None, 3600).is_ok());
            // ttl must be > 0.
            assert!(write_consent("job1", ConsentMode::Decline, None, None, 0).is_err());
        });
    }

    #[test]
    fn plugin_approved_marker_is_per_job_and_plugin() {
        with_home(|| {
            assert!(!plugin_approved("job1", "polymarket-plugin"));
            write_plugin_approved("job1", "polymarket-plugin").unwrap();
            assert!(plugin_approved("job1", "polymarket-plugin"));
            // Independent per (job, plugin): a different plugin or job is NOT approved.
            assert!(!plugin_approved("job1", "hyperliquid-plugin"));
            assert!(!plugin_approved("job2", "polymarket-plugin"));
        });
    }
}
