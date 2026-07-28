//! Auto copy-trade (`copyTrade`) shared submodule.
//!
//! Core safety model: *the model executes; the CLI decides what to execute.*
//! All parsing / validation / decision-making happens here in-process; the model
//! only copies one verbatim command (an [`card::ExecutionCard`]) and narrates the
//! result — it never reads the raw deliverable.
//!
//! This module is shared by:
//! - the **ASP outbound** path (`agent deliver --autotrade`) — structure validation +
//!   `signalTime` stamping (see [`schema::validate_structure`]).
//! - the **buyer inbound** path (`deliverable_received_cli`) — the full fixed-order
//!   runtime chain ([`pipeline`]) producing an [`card::ExecutionCard`] or
//!   [`card::NotifyOnly`].
//! - the **grant-check** subcommand (`agent autotrade-grant-check`) — the single shared
//!   [`grants::check_grant`] core (also invoked by the polymarket plugin as a subprocess).

pub(crate) mod amount;
pub(crate) mod card;
pub(crate) mod consent;
pub(crate) mod grants;
pub(crate) mod holding;
pub(crate) mod notify;
pub(crate) mod pipeline;
pub(crate) mod schema;
pub(crate) mod subscription;

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
    /// - Outbound (`deliver --autotrade`): `output::error("signal rejected: …")`, exit 1, nothing sent.
    /// - Inbound: notify-only with this reason (a *reject*, not a silent ignore).
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
