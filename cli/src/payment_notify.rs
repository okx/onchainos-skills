//! Payment-related user notifications.
//!
//! Emits structured JSON events on stderr when the server signals a
//! pricing state transition the user should know about (charging rollout
//! intro, old-user grace period, per-tier over-quota). The renderer
//! (skill / Claude) consumes each `::onchainos:notify::{json}` line and
//! renders human-facing copy.
//!
//! This module exports a pure decision function `compute_events` so the
//! trigger logic is unit-testable independently of the client's mutex
//! state and IO.

use std::sync::Mutex;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::commands::agentic_wallet::chain::show_name_for_real_id_sync;
use crate::commands::agentic_wallet::payment_flow::{parse_eip155_chain_id, PaymentTier};

/// Hardcoded fractional precision used to render `amount` as a display
/// decimal. Placeholder until per-asset decimals land on the `/config`
/// / `accepts` schema.
///
/// NOTE: assumes 6-decimal stables (USDC / USDT). Non-stable payment
/// assets — ETH (18), BNB (18), SOL (9), etc. — will render the wrong
/// amount. Today the Market API only bills in USDC/USDT so this is
/// correct in practice; widening the set of accepted assets requires
/// fixing this first.
const DISPLAY_DECIMALS: usize = 6;

// ── Pricing constants (CLI-side, filled in later) ───────────────────────
// Embedded in intro / grace notifications so the skill can render the
// monthly free quota, doc URL, and grace-period length without reaching
// into `/config`. Placeholders until the product copy lands.
pub const BASIC_FREE_QUOTA: u64 = 1000000;
pub const PREMIUM_FREE_QUOTA: u64 = 100000;
pub const DOC_URL: &str = "https://web3.okx.com/onchainos/dev-docs/market/market-api-fee";

/// Whole-day length of the old-user grace window, derived from the two
/// anchor dates so the displayed copy stays consistent with
/// `new_user_intro_start_at()` and `grace_expires_at()`. With the
/// current literals (2026-04-30 → 2026-05-30) this is 30.
pub fn grace_days() -> u32 {
    ((grace_expires_at() - new_user_intro_start_at()) / 86_400) as u32
}

/// Process-global buffer of notification events emitted by `ApiClient`
/// during a request. Drained by the CLI output layer (`output::success`
/// / MCP `ok`) and attached to the response envelope. Populated by
/// `dispatch_notifications` rather than stderr so that Claude always
/// receives the notice in-band with the data, regardless of how the
/// CLI is invoked (subprocess, MCP stdio, etc.).
///
/// TEST CONTRACT: `PENDING` is process-global. Tests that either push
/// events via `dispatch_notifications` or assert on `drain_events()`
/// must hold `crate::home::TEST_ENV_MUTEX` (the same mutex the
/// `dispatch_*` tests in `client.rs` use) for the duration of the
/// push/drain pair — otherwise a concurrent test case will see foreign
/// events or lose its own to another `drain_events`.
static PENDING: Mutex<Vec<serde_json::Value>> = Mutex::new(Vec::new());

/// Push a notification event onto the global buffer. Called from the
/// response handler once per unique (deduped) event. See the `PENDING`
/// doc comment for the test-isolation contract.
pub fn push_event(event: Event) {
    if let (Ok(mut g), Ok(v)) = (PENDING.lock(), serde_json::to_value(&event)) {
        g.push(v);
    }
}

/// Take and return all pending events, clearing the buffer. Called by
/// the output layer right before writing the final response.
pub fn drain_events() -> Vec<serde_json::Value> {
    PENDING
        .lock()
        .map(|mut g| std::mem::take(&mut *g))
        .unwrap_or_default()
}

/// Stable JSON tag for `UserType` header value: `1` → New, `0` → Old.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum UserType {
    New,
    Old,
}

impl UserType {
    pub fn from_header_value(s: &str) -> Option<Self> {
        match s.trim() {
            "1" => Some(Self::New),
            "0" => Some(Self::Old),
            _ => None,
        }
    }
}

/// Per-tier payment lifecycle. `maybe_sign_payment` pre-signs only on
/// `ChargingConfirmed`, so the `Unconfirmed` step forces one 402 →
/// `confirming` round-trip per charging window. Header `X=0` collapses
/// to `Free`, erasing prior confirmation.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TierState {
    #[default]
    Free,
    ChargingUnconfirmed,
    ChargingConfirmed,
}

impl TierState {
    /// `true` when the server is billing this tier (either before or
    /// after the user has seen the confirmation prompt).
    pub fn is_charging(self) -> bool {
        matches!(self, Self::ChargingUnconfirmed | Self::ChargingConfirmed)
    }

    /// `true` exactly on the one state that should emit OVER_QUOTA and
    /// block pre-sign.
    pub fn is_unconfirmed(self) -> bool {
        matches!(self, Self::ChargingUnconfirmed)
    }

    /// `true` only after the user has seen the OVER_QUOTA notification
    /// for this charging window.
    pub fn is_confirmed(self) -> bool {
        matches!(self, Self::ChargingConfirmed)
    }

    /// Fold a `Basic=0|1` / `Premium=0|1` bit from `ok-web3-openapi-pay`
    /// into the existing state. Only `Free → ChargingUnconfirmed` carries
    /// new information — an already-charging tier keeps its confirmation
    /// status across refresh pings.
    pub fn apply_header_flag(self, charging: bool) -> Self {
        match (charging, self) {
            (false, _) => Self::Free,
            (true, Self::Free) => Self::ChargingUnconfirmed,
            (true, s) => s,
        }
    }
}

/// Dedupe flag — identifies which persisted "shown" bit to flip after
/// the corresponding event has been emitted.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Flag {
    Grace,
    Intro,
    BasicOver,
    PremiumOver,
}

/// One of the 5 user-facing pricing events the CLI emits. Serializes
/// adjacently tagged as `{ "code": "...", "data": { ... } }` — the
/// schema the skill consumes.
///
/// Unit-like variants use struct syntax (`NewUserIntro {}`) so serde
/// still emits `data: {}`, keeping the shape uniform across variants.
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "code", content = "data")]
pub enum Event {
    #[serde(rename = "MARKET_API_NEW_USER_INTRO", rename_all = "camelCase")]
    NewUserIntro {
        basic_free_quota: u64,
        premium_free_quota: u64,
        doc_url: String,
    },

    #[serde(rename = "MARKET_API_OLD_USER_GRACE", rename_all = "camelCase")]
    OldUserGrace {
        grace_days: u32,
        /// RFC3339 UTC timestamp, e.g. `"2026-05-31T00:00:00+00:00"`.
        grace_expires_at: String,
        basic_free_quota: u64,
        premium_free_quota: u64,
        doc_url: String,
    },

    #[serde(
        rename = "MARKET_API_OLD_USER_POST_GRACE_INTRO",
        rename_all = "camelCase"
    )]
    OldUserPostGraceIntro {
        grace_days: u32,
        basic_free_quota: u64,
        premium_free_quota: u64,
        doc_url: String,
    },

    #[serde(rename = "MARKET_API_NEW_USER_OVER_QUOTA")]
    NewUserOverQuota {
        tier: PaymentTier,
        #[serde(skip_serializing_if = "Vec::is_empty")]
        payment: Vec<serde_json::Value>,
    },

    #[serde(rename = "MARKET_API_OLD_USER_POST_GRACE_OVER_QUOTA")]
    OldUserPostGraceOverQuota {
        tier: PaymentTier,
        #[serde(skip_serializing_if = "Vec::is_empty")]
        payment: Vec<serde_json::Value>,
    },
}

/// Snapshot of the fields `compute_events` needs. Decouples the pure
/// decision logic from `PaymentState`'s mutex / private visibility.
#[derive(Debug, Clone)]
pub struct NotifyInput {
    pub user_type: Option<UserType>,
    pub grace_expires_at: i64,
    /// Unix seconds "now" passed in by the caller. Kept on the input
    /// (rather than read via `now_secs()` inside `compute_events`) so
    /// the function remains a pure decision of its inputs — tests can
    /// set `now` deterministically alongside `grace_expires_at`, and
    /// the clock dependency is visible at the construction site.
    pub now: i64,
    pub basic_state: TierState,
    pub premium_state: TierState,
    pub intro_shown: bool,
    pub grace_shown: bool,
    /// Signing params (asset, amount, payTo, ...) attached to
    /// over-quota events. Prefers this response's `PAYMENT-REQUIRED`
    /// header when present, else the cached `/config` value.
    pub accepts: Option<serde_json::Value>,
    /// Tier of the current request path. `Some` emits OVER_QUOTA only
    /// for the matching tier; `None` (endpoints map not yet loaded)
    /// emits for every unconfirmed tier.
    pub path_tier: Option<PaymentTier>,
    /// `(asset, network)` of the user's saved default. Used only to mark
    /// the matching entry in OVER_QUOTA's `payment[]` with
    /// `isDefault: true` so the skill can highlight it in the picker;
    /// the list itself is never narrowed.
    pub preferred_asset: Option<(String, String)>,
}

/// Format a Unix-seconds timestamp as an RFC3339 UTC string. Returns an
/// empty string if the timestamp is out of the representable range (no
/// panic — the notification is best-effort).
fn format_rfc3339_utc(unix_secs: i64) -> String {
    DateTime::<Utc>::from_timestamp(unix_secs, 0)
        .map(|dt| dt.to_rfc3339())
        .unwrap_or_default()
}

/// Grace expiry for old users: 2026-05-30T00:00:00Z. Global constant —
/// the server doesn't return this, so every old user gets the same
/// cutoff. Paired with `new_user_intro_start_at()` (2026-04-30) this
/// gives a clean 30-day grace window via `grace_days()`.
pub fn grace_expires_at() -> i64 {
    DateTime::parse_from_rfc3339("2026-05-30T00:00:00Z")
        .expect("hardcoded RFC3339 literal")
        .timestamp()
}

/// Start of `MARKET_API_NEW_USER_INTRO` emission: 2026-04-30T00:00:00Z.
/// Before this point the event is suppressed without flipping
/// `intro_shown`, so the first eligible request after still emits.
pub fn new_user_intro_start_at() -> i64 {
    DateTime::parse_from_rfc3339("2026-04-30T00:00:00Z")
        .expect("hardcoded RFC3339 literal")
        .timestamp()
}

/// Decide which notifications fire on this response. Returns `(Event,
/// Flag)` pairs in print order; caller persists the flags. Copy lives in
/// the skill — CLI only names the state.
///
/// | User type | Window           | Quota       | Variant                            |
/// |-----------|------------------|-------------|------------------------------------|
/// | New       | after 2026-04-30 | within      | `Event::NewUserIntro`              |
/// | New       | —                | over (tier) | `Event::NewUserOverQuota`          |
/// | Old       | in grace         | —           | `Event::OldUserGrace`              |
/// | Old       | post grace       | within      | `Event::OldUserPostGraceIntro`     |
/// | Old       | post grace       | over (tier) | `Event::OldUserPostGraceOverQuota` |
pub fn compute_events(input: &NotifyInput) -> Vec<(Event, Flag)> {
    let mut events = Vec::new();
    let Some(user_type) = input.user_type else {
        return events;
    };
    // Server signals drive the normal path (both tiers Free for an Old
    // user = still in grace). The explicit clock guard is a belt for the
    // suspenders case where the dedupe cache has been deleted *after*
    // the grace window closed: without it, a Free/Free snapshot from a
    // post-grace Old user would replay `OldUserGrace` instead of
    // `OldUserPostGraceIntro`. `input.now` is caller-provided so this
    // function stays pure.
    let in_grace = matches!(user_type, UserType::Old)
        && input.basic_state == TierState::Free
        && input.premium_state == TierState::Free
        && input.now < input.grace_expires_at;

    if in_grace {
        if !input.grace_shown {
            events.push((
                Event::OldUserGrace {
                    grace_days: grace_days(),
                    grace_expires_at: format_rfc3339_utc(input.grace_expires_at),
                    basic_free_quota: BASIC_FREE_QUOTA,
                    premium_free_quota: PREMIUM_FREE_QUOTA,
                    doc_url: DOC_URL.to_string(),
                },
                Flag::Grace,
            ));
        }
        return events;
    }

    if !input.intro_shown {
        let event = match user_type {
            // Suppressed before the window opens; `intro_shown` stays
            // false so the first request at/after the cutoff emits.
            UserType::New if input.now < new_user_intro_start_at() => None,
            UserType::New => Some(Event::NewUserIntro {
                basic_free_quota: BASIC_FREE_QUOTA,
                premium_free_quota: PREMIUM_FREE_QUOTA,
                doc_url: DOC_URL.to_string(),
            }),
            UserType::Old => Some(Event::OldUserPostGraceIntro {
                grace_days: grace_days(),
                basic_free_quota: BASIC_FREE_QUOTA,
                premium_free_quota: PREMIUM_FREE_QUOTA,
                doc_url: DOC_URL.to_string(),
            }),
        };
        if let Some(event) = event {
            events.push((event, Flag::Intro));
        }
    }

    let over_quota = |tier: PaymentTier| -> Event {
        let payment = input
            .accepts
            .as_ref()
            .map(|a| payment_options_for_tier(a, tier, input.preferred_asset.as_ref()))
            .unwrap_or_default();
        match user_type {
            UserType::New => Event::NewUserOverQuota { tier, payment },
            UserType::Old => Event::OldUserPostGraceOverQuota { tier, payment },
        }
    };

    let matches_path = |tier: PaymentTier| -> bool {
        match input.path_tier {
            Some(t) => t == tier,
            None => true,
        }
    };
    if input.basic_state.is_unconfirmed() && matches_path(PaymentTier::Basic) {
        events.push((over_quota(PaymentTier::Basic), Flag::BasicOver));
    }
    if input.premium_state.is_unconfirmed() && matches_path(PaymentTier::Premium) {
        events.push((over_quota(PaymentTier::Premium), Flag::PremiumOver));
    }

    events
}

/// Project the `accepts` array into the display-ready form used by the
/// skill. Each surviving entry is reduced to a fixed six-field shape:
/// `{amount, asset, name, network, payTo, isDefault}`. Entries whose
/// `amount` is a tiered object but doesn't carry the requested tier are
/// dropped.
///
/// `preferred` never filters — it only flips `isDefault: true` on the
/// matching entry so the skill can highlight it in the picker.
fn payment_options_for_tier(
    accepts: &serde_json::Value,
    tier: PaymentTier,
    preferred: Option<&(String, String)>,
) -> Vec<serde_json::Value> {
    let tier_key = tier.as_key();
    let Some(arr) = accepts.as_array() else {
        return Vec::new();
    };
    arr.iter()
        .filter_map(|entry| {
            let is_default = preferred.is_some_and(|(asset, network)| {
                entry.get("asset").and_then(|v| v.as_str()) == Some(asset.as_str())
                    && entry.get("network").and_then(|v| v.as_str()) == Some(network.as_str())
            });
            transform_payment_entry(entry, tier_key, is_default)
        })
        .collect()
}

/// Per-entry projection. Picks the tier-resolved amount, renders it as
/// a display decimal with `DISPLAY_DECIMALS` fractional digits, hoists
/// `extra.name` to `name`, resolves `network` from CAIP-2 to its chain
/// `showName` (falls back to the raw CAIP-2 on cache miss), parses the
/// numeric `chainId` from the CAIP-2 string, and drops everything else
/// (`scheme`, `maxTimeoutSeconds`, `extra.*`).
fn transform_payment_entry(
    entry: &serde_json::Value,
    tier_key: &str,
    is_default: bool,
) -> Option<serde_json::Value> {
    let obj = entry.as_object()?;
    let raw_amount = match obj.get("amount")? {
        serde_json::Value::String(s) => s.clone(),
        serde_json::Value::Object(o) => o.get(tier_key).and_then(|v| v.as_str())?.to_string(),
        _ => return None,
    };
    let amount = amount_minimal_to_display(&raw_amount)?;
    // `extra.name` carries the full human-readable asset name (e.g.
    // "Global Dollar"); `extra.symbol` carries the short ticker (e.g.
    // "USDG"). Older servers only returned the ticker in `extra.name`,
    // so we pass both through and let the skill render `<symbol> (<name>)`
    // when they differ, or fall back to `<name>` alone otherwise.
    let extra_str = |key: &str| -> String {
        obj.get("extra")
            .and_then(|e| e.get(key))
            .and_then(|v| v.as_str())
            .unwrap_or_default()
            .to_string()
    };
    let name = extra_str("name");
    let symbol = extra_str("symbol");
    let network_raw = obj.get("network").and_then(|v| v.as_str()).unwrap_or("");
    let chain_id = parse_eip155_chain_id(network_raw).ok();
    let network = if network_raw.is_empty() {
        String::new()
    } else {
        display_network(network_raw)
    };
    let asset = obj.get("asset").cloned().unwrap_or(serde_json::Value::Null);
    let pay_to = obj.get("payTo").cloned().unwrap_or(serde_json::Value::Null);
    Some(serde_json::json!({
        "amount": amount,
        "asset": asset,
        "name": name,
        "symbol": symbol,
        "network": network,
        "chainId": chain_id,
        "payTo": pay_to,
        "isDefault": is_default,
    }))
}

/// Convert a non-negative integer string in minimal units to a decimal
/// display string with `DISPLAY_DECIMALS` fractional digits, trimming
/// trailing zeros. `"500"` → `"0.0005"`, `"1500000"` → `"1.5"`,
/// `"1000000"` → `"1"`, `"0"` → `"0"`. Returns `None` if the input is
/// empty or contains non-digits.
fn amount_minimal_to_display(minimal: &str) -> Option<String> {
    let s = minimal.trim();
    if s.is_empty() || !s.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let padded = if s.len() <= DISPLAY_DECIMALS {
        let zeros = DISPLAY_DECIMALS + 1 - s.len();
        format!("{}{s}", "0".repeat(zeros))
    } else {
        s.to_string()
    };
    let split = padded.len() - DISPLAY_DECIMALS;
    let int_part = &padded[..split];
    let frac_part = padded[split..].trim_end_matches('0');
    Some(if frac_part.is_empty() {
        int_part.to_string()
    } else {
        format!("{int_part}.{frac_part}")
    })
}

/// Resolve a CAIP-2 `eip155:<id>` network identifier to its display
/// `showName` via the chain cache. Returns the raw CAIP-2 string when
/// the cache is missing or the chain isn't in it.
fn display_network(caip2: &str) -> String {
    parse_eip155_chain_id(caip2)
        .ok()
        .and_then(show_name_for_real_id_sync)
        .unwrap_or_else(|| caip2.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    // Fixed "now" used by every `base()` test — tests are stable
    // regardless of wall clock. Set past `new_user_intro_start_at()`
    // so the New-user intro gate is open by default; tests for the
    // gated case override `now` explicitly. `grace_expires_at`
    // defaults to `TEST_NOW + 1y` so the "in grace" path is active by
    // default; tests that exercise the post-grace path override both
    // fields with their own relative arithmetic.
    const TEST_NOW: i64 = 1_777_593_600; // 2026-05-01T00:00:00Z — past intro gate, pre-grace cutoff

    fn base() -> NotifyInput {
        NotifyInput {
            user_type: None,
            grace_expires_at: TEST_NOW + 365 * 24 * 3600,
            now: TEST_NOW,
            basic_state: TierState::Free,
            premium_state: TierState::Free,
            intro_shown: false,
            grace_shown: false,
            accepts: None,
            path_tier: None,
            preferred_asset: None,
        }
    }

    fn sample_accepts() -> serde_json::Value {
        serde_json::json!([
            {
                "amount": { "basic": "100", "premium": "500" },
                "asset": "0xUSDG",
                "network": "eip155:196",
                "payTo": "0xPAYTO",
                "extra": { "name": "USDG" },
                "scheme": "exact",
                "maxTimeoutSeconds": 86400
            },
            {
                "amount": { "basic": "100", "premium": "500" },
                "asset": "0xUSDT",
                "network": "eip155:196",
                "payTo": "0xPAYTO",
                "extra": { "name": "USDT" },
                "scheme": "exact",
                "maxTimeoutSeconds": 86400
            }
        ])
    }

    #[test]
    fn grace_expires_at_is_2026_05_30_utc() {
        use chrono::TimeZone;
        let expected = Utc
            .with_ymd_and_hms(2026, 5, 30, 0, 0, 0)
            .unwrap()
            .timestamp();
        assert_eq!(grace_expires_at(), expected);
    }

    #[test]
    fn grace_days_is_thirty_from_anchors() {
        assert_eq!(grace_days(), 30);
    }

    #[test]
    fn new_user_intro_start_at_is_2026_04_30_utc() {
        use chrono::TimeZone;
        let expected = Utc
            .with_ymd_and_hms(2026, 4, 30, 0, 0, 0)
            .unwrap()
            .timestamp();
        assert_eq!(new_user_intro_start_at(), expected);
    }

    #[test]
    fn new_user_intro_suppressed_before_start() {
        let mut i = base();
        i.user_type = Some(UserType::New);
        i.now = new_user_intro_start_at() - 1;
        assert!(compute_events(&i).is_empty());
    }

    #[test]
    fn new_user_intro_emits_at_start() {
        let mut i = base();
        i.user_type = Some(UserType::New);
        i.now = new_user_intro_start_at();
        let events = compute_events(&i);
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].1, Flag::Intro);
        assert!(matches!(events[0].0, Event::NewUserIntro { .. }));
    }

    #[test]
    fn old_user_post_grace_intro_not_gated_by_new_user_start() {
        // The cutoff only gates `NewUserIntro`. An Old user past their
        // grace window must still emit `OldUserPostGraceIntro` even if
        // `now` predates 2026-04-30.
        let mut i = base();
        i.user_type = Some(UserType::Old);
        i.now = new_user_intro_start_at() - 24 * 3600;
        i.grace_expires_at = i.now - 24 * 3600;
        let events = compute_events(&i);
        assert_eq!(events.len(), 1);
        assert!(matches!(events[0].0, Event::OldUserPostGraceIntro { .. }));
    }

    #[test]
    fn no_events_when_user_type_unknown() {
        assert!(compute_events(&base()).is_empty());
    }

    #[test]
    fn user_type_from_header_value() {
        assert_eq!(UserType::from_header_value("1"), Some(UserType::New));
        assert_eq!(UserType::from_header_value("0"), Some(UserType::Old));
        assert_eq!(UserType::from_header_value(" 1 "), Some(UserType::New));
        assert_eq!(UserType::from_header_value("2"), None);
        assert_eq!(UserType::from_header_value(""), None);
    }

    #[test]
    fn new_user_first_request_emits_intro() {
        let mut i = base();
        i.user_type = Some(UserType::New);
        let events = compute_events(&i);
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].1, Flag::Intro);
        assert!(matches!(events[0].0, Event::NewUserIntro { .. }));
    }

    #[test]
    fn old_user_in_grace_emits_grace_only() {
        let mut i = base();
        i.user_type = Some(UserType::Old);
        let events = compute_events(&i);
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].1, Flag::Grace);
        assert!(matches!(events[0].0, Event::OldUserGrace { .. }));
    }

    #[test]
    fn old_user_free_free_post_grace_cutoff_falls_through_to_post_grace_intro() {
        // Edge case the review bot flagged: an Old user whose
        // `grace_shown` flag has been reset (cache deleted) and whose
        // tier state still reports Free/Free (server hasn't flipped
        // charging yet) — post the calendar cutoff, we must NOT replay
        // the grace notice. The clock guard on `in_grace` forces the
        // fallthrough so the user sees `OldUserPostGraceIntro` instead.
        let mut i = base();
        i.user_type = Some(UserType::Old);
        i.grace_expires_at = i.now - 24 * 3600;
        let events = compute_events(&i);
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].1, Flag::Intro);
        assert!(matches!(events[0].0, Event::OldUserPostGraceIntro { .. }));
    }

    #[test]
    fn old_user_with_charging_flag_emits_over_quota_not_grace() {
        // Regression: server may flip a tier to charging before the
        // calendar grace cutoff. `in_grace` is decided by tier state, not
        // by the clock, so the user sees intro + OVER_QUOTA (and the 402
        // retry wrapper can block the first auto-sign) instead of being
        // silently charged while the grace notice masks the event.
        let mut i = base();
        i.user_type = Some(UserType::Old);
        i.basic_state = TierState::ChargingUnconfirmed;
        let events = compute_events(&i);
        assert_eq!(events.len(), 2);
        assert_eq!(events[0].1, Flag::Intro);
        assert!(matches!(events[0].0, Event::OldUserPostGraceIntro { .. }));
        assert_eq!(events[1].1, Flag::BasicOver);
        assert!(matches!(
            events[1].0,
            Event::OldUserPostGraceOverQuota {
                tier: PaymentTier::Basic,
                ..
            }
        ));
    }

    #[test]
    fn old_user_post_grace_emits_post_grace_intro() {
        // "Post grace" here means a tier is charging (even if it was
        // already confirmed in a prior window). Intro fires once; no
        // OVER_QUOTA because Confirmed != Unconfirmed.
        let mut i = base();
        i.user_type = Some(UserType::Old);
        i.grace_shown = true;
        i.basic_state = TierState::ChargingConfirmed;
        let events = compute_events(&i);
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].1, Flag::Intro);
        assert!(matches!(events[0].0, Event::OldUserPostGraceIntro { .. }));
    }

    #[test]
    fn new_user_basic_over_quota_first_time() {
        let mut i = base();
        i.user_type = Some(UserType::New);
        i.intro_shown = true;
        i.basic_state = TierState::ChargingUnconfirmed;
        let events = compute_events(&i);
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].1, Flag::BasicOver);
        assert!(matches!(
            events[0].0,
            Event::NewUserOverQuota {
                tier: PaymentTier::Basic,
                ..
            }
        ));
    }

    #[test]
    fn intro_event_serializes_with_camelcase_fields() {
        let mut i = base();
        i.user_type = Some(UserType::New);
        let events = compute_events(&i);
        assert_eq!(events.len(), 1);
        // Adjacently tagged: { "code": "...", "data": { ... camelCase fields } }.
        let v = serde_json::to_value(&events[0].0).unwrap();
        assert_eq!(v["code"], "MARKET_API_NEW_USER_INTRO");
        let data = v["data"].as_object().expect("data is object");
        assert!(data.contains_key("basicFreeQuota"));
        assert!(data.contains_key("premiumFreeQuota"));
        assert!(data.contains_key("docUrl"));
    }

    #[test]
    fn grace_event_serializes_grace_expires_at_as_rfc3339() {
        use chrono::TimeZone;
        let mut i = base();
        i.user_type = Some(UserType::Old);
        i.grace_expires_at = Utc
            .with_ymd_and_hms(2026, 6, 1, 0, 0, 0)
            .unwrap()
            .timestamp();
        let events = compute_events(&i);
        assert_eq!(events.len(), 1);
        let v = serde_json::to_value(&events[0].0).unwrap();
        assert_eq!(v["code"], "MARKET_API_OLD_USER_GRACE");
        let gea = v["data"]["graceExpiresAt"]
            .as_str()
            .expect("graceExpiresAt is string");
        assert!(
            gea.starts_with("2026-06-01T00:00:00"),
            "expected RFC3339 UTC, got {gea}"
        );
    }

    #[test]
    fn new_user_intro_and_both_tier_over_quota_at_once() {
        let mut i = base();
        i.user_type = Some(UserType::New);
        i.basic_state = TierState::ChargingUnconfirmed;
        i.premium_state = TierState::ChargingUnconfirmed;
        let events = compute_events(&i);
        assert_eq!(events.len(), 3);
        assert_eq!(events[0].1, Flag::Intro);
        assert_eq!(events[1].1, Flag::BasicOver);
        assert_eq!(events[2].1, Flag::PremiumOver);
    }

    #[test]
    fn old_post_grace_over_quota_uses_post_grace_variant() {
        let mut i = base();
        i.user_type = Some(UserType::Old);
        i.intro_shown = true;
        i.grace_shown = true;
        i.basic_state = TierState::ChargingUnconfirmed;
        let events = compute_events(&i);
        assert_eq!(events.len(), 1);
        assert!(matches!(
            events[0].0,
            Event::OldUserPostGraceOverQuota {
                tier: PaymentTier::Basic,
                ..
            }
        ));
    }

    #[test]
    fn dedupes_already_shown_events() {
        let mut i = base();
        i.user_type = Some(UserType::New);
        i.intro_shown = true;
        i.basic_state = TierState::ChargingConfirmed;
        let events = compute_events(&i);
        assert!(events.is_empty());
    }

    #[test]
    fn premium_over_quota_independent_of_basic() {
        let mut i = base();
        i.user_type = Some(UserType::New);
        i.intro_shown = true;
        i.basic_state = TierState::ChargingConfirmed;
        i.premium_state = TierState::ChargingUnconfirmed;
        let events = compute_events(&i);
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].1, Flag::PremiumOver);
        assert!(matches!(
            events[0].0,
            Event::NewUserOverQuota {
                tier: PaymentTier::Premium,
                ..
            }
        ));
    }

    #[test]
    fn over_quota_re_fires_after_state_resets_to_free() {
        // Lifecycle: Free → ChargingUnconfirmed (event fires) →
        // ChargingConfirmed (caller transitions after push) → Free
        // (header drops back) → ChargingUnconfirmed again (header
        // re-flips) → event MUST fire again.
        let mut i = base();
        i.user_type = Some(UserType::New);
        i.intro_shown = true;
        i.basic_state = TierState::ChargingUnconfirmed;
        // First transition.
        let events = compute_events(&i);
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].1, Flag::BasicOver);
        // Simulate: caller advanced Unconfirmed → Confirmed after firing.
        i.basic_state = TierState::ChargingConfirmed;
        // Steady charging-true: no repeat.
        let events = compute_events(&i);
        assert!(events.is_empty());
        // Server drops back to free → header resets state to Free.
        i.basic_state = TierState::Free;
        // Server re-flips to charging.
        i.basic_state = TierState::ChargingUnconfirmed;
        let events = compute_events(&i);
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].1, Flag::BasicOver);
    }

    #[test]
    fn over_quota_payload_embeds_resolved_payment_options() {
        let mut i = base();
        i.user_type = Some(UserType::New);
        i.intro_shown = true;
        i.basic_state = TierState::ChargingUnconfirmed;
        i.accepts = Some(sample_accepts());

        let events = compute_events(&i);
        assert_eq!(events.len(), 1);
        let Event::NewUserOverQuota { tier, payment } = &events[0].0 else {
            panic!("expected NewUserOverQuota variant");
        };
        assert_eq!(*tier, PaymentTier::Basic);
        assert_eq!(payment.len(), 2);
        // Basic tier → raw amount "100" (6 decimals) → display "0.0001".
        assert_eq!(payment[0]["amount"], "0.0001");
        assert_eq!(payment[0]["asset"], "0xUSDG");
        assert_eq!(payment[0]["name"], "USDG");
        assert_eq!(payment[0]["payTo"], "0xPAYTO");
        // chainId parsed from CAIP-2 `eip155:196`.
        assert_eq!(payment[0]["chainId"], 196);
        // Network resolves via chain cache to showName; falls back to
        // raw CAIP-2 when the cache is missing (test environments).
        let net = payment[0]["network"].as_str().unwrap();
        assert!(
            net == "X Layer" || net == "eip155:196",
            "unexpected network: {net}"
        );
        // Dropped fields are gone.
        assert!(payment[0].get("extra").is_none());
        assert!(payment[0].get("scheme").is_none());
        assert!(payment[0].get("maxTimeoutSeconds").is_none());
        // No preferred_asset → every entry is isDefault: false.
        assert_eq!(payment[0]["isDefault"], false);
        // sample_accepts has extra.name only (no symbol): symbol stays "".
        assert_eq!(payment[0]["symbol"], "");
        assert_eq!(payment[1]["asset"], "0xUSDT");
        assert_eq!(payment[1]["name"], "USDT");
        assert_eq!(payment[1]["symbol"], "");
        assert_eq!(payment[1]["isDefault"], false);
    }

    #[test]
    fn over_quota_payload_passes_through_extra_symbol() {
        // New server shape: extra carries both `name` (full human name)
        // and `symbol` (ticker). Both must surface on the payment entry
        // so the skill can render `<symbol> (<name>)` in the picker.
        let accepts = serde_json::json!([{
            "amount": {"basic": "100", "premium": "500"},
            "asset": "0xUSDG",
            "network": "eip155:196",
            "payTo": "0xPAYTO",
            "extra": {
                "name": "Global Dollar",
                "symbol": "USDG",
                "transferMethod": "eip3009",
                "version": "1"
            },
            "scheme": "exact",
            "maxTimeoutSeconds": 86400
        }]);
        let mut i = base();
        i.user_type = Some(UserType::New);
        i.intro_shown = true;
        i.basic_state = TierState::ChargingUnconfirmed;
        i.accepts = Some(accepts);

        let events = compute_events(&i);
        let Event::NewUserOverQuota { payment, .. } = &events[0].0 else {
            panic!("expected NewUserOverQuota variant");
        };
        assert_eq!(payment.len(), 1);
        assert_eq!(payment[0]["name"], "Global Dollar");
        assert_eq!(payment[0]["symbol"], "USDG");
    }

    #[test]
    fn over_quota_payload_marks_matching_preferred_as_default() {
        // With a saved default, the picker list stays full (2 entries)
        // but only the matching (asset, network) carries isDefault:true.
        let mut i = base();
        i.user_type = Some(UserType::New);
        i.intro_shown = true;
        i.basic_state = TierState::ChargingUnconfirmed;
        i.accepts = Some(sample_accepts());
        i.preferred_asset = Some(("0xUSDT".to_string(), "eip155:196".to_string()));

        let events = compute_events(&i);
        let Event::NewUserOverQuota { payment, .. } = &events[0].0 else {
            panic!("expected NewUserOverQuota variant");
        };
        assert_eq!(payment.len(), 2, "picker list is never narrowed");
        assert_eq!(payment[0]["asset"], "0xUSDG");
        assert_eq!(payment[0]["isDefault"], false);
        assert_eq!(payment[1]["asset"], "0xUSDT");
        assert_eq!(payment[1]["isDefault"], true);
    }

    #[test]
    fn over_quota_payload_preferred_without_match_marks_none_as_default() {
        // Saved default doesn't correspond to any accepts entry (e.g.
        // server dropped that asset) → every entry stays isDefault:false.
        let mut i = base();
        i.user_type = Some(UserType::New);
        i.intro_shown = true;
        i.basic_state = TierState::ChargingUnconfirmed;
        i.accepts = Some(sample_accepts());
        i.preferred_asset = Some(("0xUNKNOWN".to_string(), "eip155:196".to_string()));

        let events = compute_events(&i);
        let Event::NewUserOverQuota { payment, .. } = &events[0].0 else {
            panic!("expected NewUserOverQuota variant");
        };
        assert_eq!(payment.len(), 2);
        assert_eq!(payment[0]["isDefault"], false);
        assert_eq!(payment[1]["isDefault"], false);
    }

    #[test]
    fn over_quota_payload_handles_flat_amount_and_missing_tier() {
        let mut i = base();
        i.user_type = Some(UserType::New);
        i.intro_shown = true;
        i.premium_state = TierState::ChargingUnconfirmed;
        i.accepts = Some(serde_json::json!([
            // Flat amount — applies to both tiers.
            { "amount": "500", "asset": "0xFLAT", "network": "n", "payTo": "p" },
            // Tiered object missing the premium key — should be dropped.
            { "amount": { "basic": "100" }, "asset": "0xBASIC_ONLY" },
        ]));
        let events = compute_events(&i);
        assert_eq!(events.len(), 1);
        let Event::NewUserOverQuota { payment, .. } = &events[0].0 else {
            panic!("expected NewUserOverQuota variant");
        };
        assert_eq!(payment.len(), 1);
        assert_eq!(payment[0]["asset"], "0xFLAT");
        // "500" minimal → "0.0005" display (6 decimals).
        assert_eq!(payment[0]["amount"], "0.0005");
        // Non-CAIP-2 network → chainId is null.
        assert!(payment[0]["chainId"].is_null());
    }

    #[test]
    fn amount_minimal_to_display_renders_expected_strings() {
        assert_eq!(amount_minimal_to_display("500").unwrap(), "0.0005");
        assert_eq!(amount_minimal_to_display("100").unwrap(), "0.0001");
        assert_eq!(amount_minimal_to_display("1500000").unwrap(), "1.5");
        assert_eq!(amount_minimal_to_display("1000000").unwrap(), "1");
        assert_eq!(amount_minimal_to_display("0").unwrap(), "0");
        assert_eq!(amount_minimal_to_display("10").unwrap(), "0.00001");
        assert_eq!(
            amount_minimal_to_display("123456789").unwrap(),
            "123.456789"
        );
        assert!(amount_minimal_to_display("").is_none());
        assert!(amount_minimal_to_display("abc").is_none());
        assert!(amount_minimal_to_display("-100").is_none());
    }

    #[test]
    fn path_tier_filters_over_quota_to_matching_tier_only() {
        // Both tiers flipped to charging this response, but the current
        // request path is Basic — we should emit exactly one OVER_QUOTA
        // for Basic and leave Premium in ChargingUnconfirmed so its next
        // real use still prompts.
        let mut i = base();
        i.user_type = Some(UserType::New);
        i.intro_shown = true;
        i.basic_state = TierState::ChargingUnconfirmed;
        i.premium_state = TierState::ChargingUnconfirmed;
        i.path_tier = Some(PaymentTier::Basic);
        let events = compute_events(&i);
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].1, Flag::BasicOver);

        // Now the same response but the path is Premium.
        i.path_tier = Some(PaymentTier::Premium);
        let events = compute_events(&i);
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].1, Flag::PremiumOver);
    }

    #[test]
    fn path_tier_none_falls_back_to_emit_all_charging_tiers() {
        // Fallback for pre-/config requests where the endpoint map is
        // still empty — preserves the original "fire both" behavior.
        let mut i = base();
        i.user_type = Some(UserType::New);
        i.intro_shown = true;
        i.basic_state = TierState::ChargingUnconfirmed;
        i.premium_state = TierState::ChargingUnconfirmed;
        i.path_tier = None;
        let events = compute_events(&i);
        assert_eq!(events.len(), 2);
        assert_eq!(events[0].1, Flag::BasicOver);
        assert_eq!(events[1].1, Flag::PremiumOver);
    }

    #[test]
    fn over_quota_payload_omits_payment_when_accepts_absent() {
        let mut i = base();
        i.user_type = Some(UserType::New);
        i.intro_shown = true;
        i.basic_state = TierState::ChargingUnconfirmed;
        // i.accepts stays None
        let events = compute_events(&i);
        assert_eq!(events.len(), 1);
        let Event::NewUserOverQuota { tier, payment } = &events[0].0 else {
            panic!("expected NewUserOverQuota variant");
        };
        assert_eq!(*tier, PaymentTier::Basic);
        assert!(payment.is_empty());
        // Serialized form: `data.payment` is omitted when the vec is empty.
        let v = serde_json::to_value(&events[0].0).unwrap();
        assert!(v["data"].get("payment").is_none());
        assert_eq!(v["data"]["tier"], "basic");
    }

    #[test]
    fn push_and_drain_events_round_trip() {
        // Serialize against the `dispatch_notifications` tests in
        // `client.rs` that also push into `PENDING`. Reusing their
        // `TEST_ENV_MUTEX` keeps the set of mutexes small; see the
        // `PENDING` TEST CONTRACT doc comment.
        let _lock = crate::home::TEST_ENV_MUTEX.lock().unwrap();
        drain_events();
        push_event(Event::NewUserIntro {
            basic_free_quota: BASIC_FREE_QUOTA,
            premium_free_quota: PREMIUM_FREE_QUOTA,
            doc_url: DOC_URL.to_string(),
        });
        push_event(Event::OldUserGrace {
            grace_days: grace_days(),
            grace_expires_at: String::new(),
            basic_free_quota: BASIC_FREE_QUOTA,
            premium_free_quota: PREMIUM_FREE_QUOTA,
            doc_url: DOC_URL.to_string(),
        });
        let drained = drain_events();
        assert_eq!(drained.len(), 2);
        assert_eq!(drained[0]["code"], "MARKET_API_NEW_USER_INTRO");
        assert_eq!(drained[1]["code"], "MARKET_API_OLD_USER_GRACE");
        assert!(drain_events().is_empty());
    }
}
