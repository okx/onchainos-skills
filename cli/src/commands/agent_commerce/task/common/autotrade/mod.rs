//! Automatic signal execution shared submodule.
//!
//! The model-driven subscription session reads each saved delivery, selects an
//! appropriate Skill/tool, and applies that tool's safety checks. This module
//! retains local consent, grants, bounded route metadata, and compatibility
//! helpers; it no longer parses or executes delivered signal text.
//!
//! This module is shared by:
//! - the retired ASP `agent deliver --autotrade` argument (accepted but ignored);
//! - the Active-subscription route cache and consent/grant commands used by the
//!   model-selected Skill/tool;
//! - compatibility rendering for decisions produced by earlier releases.

pub(crate) mod amount;
pub(crate) mod card;
pub(crate) mod consent;
pub(crate) mod consent_reply;
pub(crate) mod continuation;
pub(crate) mod delivery_queue;
pub(crate) mod executor;
pub(crate) mod grants;
pub(crate) mod notify;
pub(crate) mod profile;
pub(crate) mod schema;
pub(crate) mod subscription;
pub(crate) mod tooling;
pub(crate) mod trade_kit;

pub const DEFAULT_AUTOTRADE_TTL_SEC: u64 = 31_536_000;

/// Terminal skip reason used when a delivery arrives without a live execution
/// policy. The outcome notifier renders this code as localized guidance for
/// restoring or updating the subscription's execution policy.
pub const EXECUTION_POLICY_NOT_CONFIGURED_REASON: &str = "execution_policy_not_configured";

/// Delivery-time mode/configuration prompts removed from the current flow.
/// Replies from older releases are retained only long enough to fail closed;
/// they must never write policy or produce another configuration card.
pub const RETIRED_MODE_CONFIGURATION_EVENTS: &[&str] =
    &["autotrade_consent", "autotrade_config_required"];

pub fn is_retired_mode_configuration_decision(source_event: Option<&str>) -> bool {
    source_event.is_some_and(|event| RETIRED_MODE_CONFIGURATION_EVENTS.contains(&event))
}

pub const RETIRED_DELIVERY_DECISION_EVENTS: &[&str] = &[
    "autotrade_consent",
    "autotrade_consent_pre_delivery",
    "autotrade_config_required",
    "autotrade_manual_signal",
    "autotrade_over_cap",
    "autotrade_cap_adjust",
    "autotrade_tool_select",
    "autotrade_plugin_install",
];

pub fn is_retired_delivery_decision(source_event: Option<&str>) -> bool {
    source_event.is_some_and(|event| RETIRED_DELIVERY_DECISION_EVENTS.contains(&event))
}

pub fn ensure_default_auto(job_id: &str, ttl_sec: u64) -> anyhow::Result<bool> {
    if let Some(existing) = consent::load_consent(job_id)? {
        match existing.mode {
            consent::ConsentMode::Auto => {
                let now = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|duration| duration.as_secs())
                    .unwrap_or(0);
                grants::write_auto_grant(job_id, existing.expires_at.saturating_sub(now).max(1))?;
            }
            consent::ConsentMode::Manual | consent::ConsentMode::Decline => {
                grants::clear_grant(job_id);
            }
        }
        return Ok(false);
    }
    consent::write_consent_with_trade_amount(
        job_id,
        consent::ConsentMode::Auto,
        None,
        None,
        Some(consent::DEFAULT_QUOTE),
        ttl_sec,
    )?;
    if let Err(error) = grants::write_auto_grant(job_id, ttl_sec) {
        consent::clear_consent(job_id);
        grants::clear_grant(job_id);
        return Err(error);
    }
    Ok(true)
}

// ── Stable audit action names ────────────────────────────────────────────
//
// One source of truth for the audit-log action strings referenced by
// `audit.rs` (grant-check allow/deny) and threaded through the degrade paths.

/// Audit action: buyer inbound auto-trade delivery handling.
pub const ACTION_AUTOTRADE_DELIVER: &str = "user/autotrade_deliver";
/// Audit action: grant-check (allow or deny; deny carries the reason).
pub const ACTION_GRANT_CHECK: &str = "agent/autotrade_grant_check";
/// Audit action: buyer consent record write (auto / manual / decline + cap).
pub const ACTION_AUTOTRADE_CONSENT_SET: &str = "user/autotrade_consent_set";

/// Stable machine-readable degrade reasons (buyer inbound path).
///
/// These strings are a contract: they appear in `NotifyOnly.reason` (read by the
/// model) **and** as the audited action discriminator, so they must never drift.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DegradeReason {
    FreshnessExpired,
    SubscriptionNotActive,
    NoActiveWallet,
    StructureReject,
    TypeDegrade,
    OverCap,
    PctHoldingFail,
    HoldingTooSmall,
    HoldingUnavailable,
    ReplaySkip,
    LatchWriteFail,
    LookupOff,
    /// Parsed-text auto consent predates the fixed per-signal amount field.
    MissingTradeAmount,
    /// The selected execution tool is not installed locally.
    ToolMissing,
    /// Current market price is outside the signal's entry interval.
    EntryOutsideRange,
    /// Parsed successfully, but the current runtime supports only one take-profit level.
    MultipleTakeProfitUnsupported,
    SchemaVersionTooNew,
    /// `jobId` failed the entry charset check (path-traversal defense; FR-4).
    InvalidJobId,
    /// The consent record is present but broken (unreadable / version-too-new /
    /// job-mismatch); carries the specific code. Fails closed (notify only).
    ConsentInvalid(&'static str),
    /// A grant-check denial, carrying the specific grant-deny code so the real
    /// reason (no-grant-file / expired / venue-not-authorized / no-cap / over-cap …)
    /// is surfaced instead of being collapsed onto `over_cap`.
    GrantDenied(&'static str),
}

impl DegradeReason {
    /// The stable wire string (matches the FR-9 audit action names).
    pub const fn as_str(self) -> &'static str {
        match self {
            DegradeReason::FreshnessExpired => "freshness_expired",
            DegradeReason::SubscriptionNotActive => "subscription_not_active",
            DegradeReason::NoActiveWallet => "no_active_wallet",
            DegradeReason::StructureReject => "structure_reject",
            DegradeReason::TypeDegrade => "type_degrade",
            DegradeReason::OverCap => "over_cap",
            DegradeReason::PctHoldingFail => "pct_holding_fail",
            DegradeReason::HoldingTooSmall => "holding_too_small",
            DegradeReason::HoldingUnavailable => "holding_unavailable",
            DegradeReason::ReplaySkip => "replay_skip",
            DegradeReason::LatchWriteFail => "latch_write_fail",
            DegradeReason::LookupOff => "lookup_off",
            DegradeReason::MissingTradeAmount => "missing_trade_amount",
            DegradeReason::ToolMissing => "tool_missing",
            DegradeReason::EntryOutsideRange => "entry_outside_range",
            DegradeReason::MultipleTakeProfitUnsupported => "multiple_take_profit_unsupported",
            DegradeReason::SchemaVersionTooNew => "schema_version_too_new",
            DegradeReason::InvalidJobId => "invalid_job_id",
            DegradeReason::ConsentInvalid(code) => code,
            DegradeReason::GrantDenied(code) => code,
        }
    }
}

impl std::fmt::Display for DegradeReason {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// The two auto-trade outcome classes.
#[derive(Debug)]
pub enum AutoTradeError {
    /// Structural / schema violation.
    /// Retained for compatibility helpers and their tests; delivered text is no
    /// longer parsed into this schema on either side.
    Reject(String),

    /// Runtime fail-safe (query fail, not-active, no wallet, over-cap, holding-fail,
    /// replay, latch-fail, lookup-fail). Always ⇒ notify-only + machine-readable reason
    /// + audit; never errors, never retries into execution.
    Degrade(DegradeReason),
}

impl std::fmt::Display for AutoTradeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            // Prefix matches the outbound `output::error` contract verbatim.
            AutoTradeError::Reject(s) => write!(f, "signal rejected: {s}"),
            AutoTradeError::Degrade(r) => write!(f, "{r}"),
        }
    }
}

impl std::error::Error for AutoTradeError {}

// ── Bespoke CLI exit ──────────────────────────────────────────────────────

/// Typed error carrying a process exit code for handlers that print their own
/// bespoke stdout (the `autotrade-grant-check` `{ok,reason?}` contract).
///
/// `main.rs run()` downcasts to this **after** `audit::log` fires (so both allow
/// and deny are audited) and calls `std::process::exit(code)` **without** printing
/// the standard `output::error` envelope — the handler already `println!`'d its
/// bespoke JSON. This keeps exit-code centralization in `main.rs` intact.
#[derive(Debug)]
pub struct CliBespokeExit(pub i32);

impl std::fmt::Display for CliBespokeExit {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "bespoke exit: {}", self.0)
    }
}

impl std::error::Error for CliBespokeExit {}
