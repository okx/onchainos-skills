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
const DISPLAY_DECIMALS: usize = 6;

// ── Pricing constants (CLI-side, filled in later) ───────────────────────
// Embedded in intro / grace notifications so the skill can render the
// monthly free quota, doc URL, and grace-period length without reaching
// into `/config`. Placeholders until the product copy lands.
pub const BASIC_FREE_QUOTA: u64 = 1000000;
pub const PREMIUM_FREE_QUOTA: u64 = 100000;
pub const GRACE_DAYS: u32 = 30;
pub const DOC_URL: &str = "";

/// Process-global buffer of notification events emitted by `ApiClient`
/// during a request. Drained by the CLI output layer (`output::success`
/// / MCP `ok`) and attached to the response envelope. Populated by
/// `dispatch_notifications` rather than stderr so that Claude always
/// receives the notice in-band with the data, regardless of how the
/// CLI is invoked (subprocess, MCP stdio, etc.).
static PENDING: Mutex<Vec<serde_json::Value>> = Mutex::new(Vec::new());

/// Push a notification event onto the global buffer. Called from the
/// response handler once per unique (deduped) event.
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
    pub basic_charging: bool,
    pub premium_charging: bool,
    pub intro_shown: bool,
    pub grace_shown: bool,
    pub basic_over_shown: bool,
    pub premium_over_shown: bool,
    /// Latest `accepts` array known to the client (from `/config` or
    /// the most recent 402). Used to attach payment details (asset,
    /// amount, payTo, ...) to over-quota events so the skill can
    /// render exactly what the user is about to pay.
    pub accepts: Option<serde_json::Value>,
}

/// Format a Unix-seconds timestamp as an RFC3339 UTC string. Returns an
/// empty string if the timestamp is out of the representable range (no
/// panic — the notification is best-effort).
fn format_rfc3339_utc(unix_secs: i64) -> String {
    DateTime::<Utc>::from_timestamp(unix_secs, 0)
        .map(|dt| dt.to_rfc3339())
        .unwrap_or_default()
}

/// Fallback grace expiry used before the server's `/config` response
/// lands. 2026-05-31T00:00:00Z.
pub fn fallback_grace_expires_at() -> i64 {
    DateTime::parse_from_rfc3339("2026-05-31T00:00:00Z")
        .expect("hardcoded RFC3339 literal")
        .timestamp()
}

/// Decide which notifications should fire on this response. Returns one
/// `(Event, Flag)` pair per event, in intended print order. The caller
/// is responsible for flipping the flag and persisting.
///
/// There are exactly 5 event variants, mapping 1:1 to the 5 user-facing
/// states the skill renders — CLI only identifies which state the user
/// just entered; all copy lives in the skill.
///
/// | User type | Window     | Quota       | Variant                            |
/// |-----------|------------|-------------|------------------------------------|
/// | New       | —          | within      | `Event::NewUserIntro`              |
/// | New       | —          | over (tier) | `Event::NewUserOverQuota`          |
/// | Old       | in grace   | —           | `Event::OldUserGrace`              |
/// | Old       | post grace | within      | `Event::OldUserPostGraceIntro`     |
/// | Old       | post grace | over (tier) | `Event::OldUserPostGraceOverQuota` |
///
/// Old users in grace never emit intro or over-quota events — grace wins.
pub fn compute_events(input: &NotifyInput, now: i64) -> Vec<(Event, Flag)> {
    let mut events = Vec::new();
    let Some(user_type) = input.user_type else {
        return events;
    };
    let in_grace = matches!(user_type, UserType::Old) && now < input.grace_expires_at;

    if in_grace {
        if !input.grace_shown {
            events.push((
                Event::OldUserGrace {
                    grace_days: GRACE_DAYS,
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
            UserType::New => Event::NewUserIntro {
                basic_free_quota: BASIC_FREE_QUOTA,
                premium_free_quota: PREMIUM_FREE_QUOTA,
                doc_url: DOC_URL.to_string(),
            },
            UserType::Old => Event::OldUserPostGraceIntro {
                grace_days: GRACE_DAYS,
                basic_free_quota: BASIC_FREE_QUOTA,
                premium_free_quota: PREMIUM_FREE_QUOTA,
                doc_url: DOC_URL.to_string(),
            },
        };
        events.push((event, Flag::Intro));
    }

    let over_quota = |tier: PaymentTier| -> Event {
        let payment = input
            .accepts
            .as_ref()
            .map(|a| payment_options_for_tier(a, tier))
            .unwrap_or_default();
        match user_type {
            UserType::New => Event::NewUserOverQuota { tier, payment },
            UserType::Old => Event::OldUserPostGraceOverQuota { tier, payment },
        }
    };

    if input.basic_charging && !input.basic_over_shown {
        events.push((over_quota(PaymentTier::Basic), Flag::BasicOver));
    }
    if input.premium_charging && !input.premium_over_shown {
        events.push((over_quota(PaymentTier::Premium), Flag::PremiumOver));
    }

    events
}

/// Project the `accepts` array into the display-ready form used by the
/// skill. Each surviving entry is reduced to a fixed five-field shape:
/// `{amount, asset, name, network, payTo}`. Entries whose `amount` is a
/// tiered object but doesn't carry the requested tier are dropped.
fn payment_options_for_tier(
    accepts: &serde_json::Value,
    tier: PaymentTier,
) -> Vec<serde_json::Value> {
    let tier_key = tier.as_key();
    let Some(arr) = accepts.as_array() else {
        return Vec::new();
    };
    arr.iter()
        .filter_map(|entry| transform_payment_entry(entry, tier_key))
        .collect()
}

/// Per-entry projection. Picks the tier-resolved amount, renders it as
/// a display decimal with `DISPLAY_DECIMALS` fractional digits, hoists
/// `extra.name` to `name`, resolves `network` from CAIP-2 to its chain
/// `showName` (falls back to the raw CAIP-2 on cache miss), and drops
/// everything else (`scheme`, `maxTimeoutSeconds`, `extra.*`).
fn transform_payment_entry(
    entry: &serde_json::Value,
    tier_key: &str,
) -> Option<serde_json::Value> {
    let obj = entry.as_object()?;
    let raw_amount = match obj.get("amount")? {
        serde_json::Value::String(s) => s.clone(),
        serde_json::Value::Object(o) => o.get(tier_key).and_then(|v| v.as_str())?.to_string(),
        _ => return None,
    };
    let amount = amount_minimal_to_display(&raw_amount)?;
    let name = obj
        .get("extra")
        .and_then(|e| e.get("name"))
        .and_then(|v| v.as_str())
        .unwrap_or_default()
        .to_string();
    let network = obj
        .get("network")
        .and_then(|v| v.as_str())
        .map(display_network)
        .unwrap_or_default();
    let asset = obj.get("asset").cloned().unwrap_or(serde_json::Value::Null);
    let pay_to = obj.get("payTo").cloned().unwrap_or(serde_json::Value::Null);
    Some(serde_json::json!({
        "amount": amount,
        "asset": asset,
        "name": name,
        "network": network,
        "payTo": pay_to,
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

    fn base() -> NotifyInput {
        NotifyInput {
            user_type: None,
            grace_expires_at: fallback_grace_expires_at(),
            basic_charging: false,
            premium_charging: false,
            intro_shown: false,
            grace_shown: false,
            basic_over_shown: false,
            premium_over_shown: false,
            accepts: None,
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
    fn fallback_grace_is_2026_05_31_utc() {
        use chrono::TimeZone;
        let expected = Utc
            .with_ymd_and_hms(2026, 5, 31, 0, 0, 0)
            .unwrap()
            .timestamp();
        assert_eq!(fallback_grace_expires_at(), expected);
    }

    #[test]
    fn no_events_when_user_type_unknown() {
        assert!(compute_events(&base(), 0).is_empty());
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
        let events = compute_events(&i, 0);
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].1, Flag::Intro);
        assert!(matches!(events[0].0, Event::NewUserIntro { .. }));
    }

    #[test]
    fn old_user_in_grace_emits_grace_only() {
        let mut i = base();
        i.user_type = Some(UserType::Old);
        let events = compute_events(&i, 0);
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].1, Flag::Grace);
        assert!(matches!(events[0].0, Event::OldUserGrace { .. }));
    }

    #[test]
    fn old_user_in_grace_suppresses_over_quota_even_if_charging_flag_set() {
        let mut i = base();
        i.user_type = Some(UserType::Old);
        i.grace_shown = true;
        i.basic_charging = true;
        let events = compute_events(&i, 0);
        assert!(events.is_empty());
    }

    #[test]
    fn old_user_post_grace_emits_post_grace_intro() {
        let mut i = base();
        i.user_type = Some(UserType::Old);
        i.grace_shown = true;
        let now = i.grace_expires_at + 1;
        let events = compute_events(&i, now);
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].1, Flag::Intro);
        assert!(matches!(events[0].0, Event::OldUserPostGraceIntro { .. }));
    }

    #[test]
    fn new_user_basic_over_quota_first_time() {
        let mut i = base();
        i.user_type = Some(UserType::New);
        i.intro_shown = true;
        i.basic_charging = true;
        let events = compute_events(&i, 0);
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].1, Flag::BasicOver);
        assert!(matches!(
            events[0].0,
            Event::NewUserOverQuota { tier: PaymentTier::Basic, .. }
        ));
    }

    #[test]
    fn intro_event_serializes_with_camelcase_fields() {
        let mut i = base();
        i.user_type = Some(UserType::New);
        let events = compute_events(&i, 0);
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
        let events = compute_events(&i, 0);
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
        i.basic_charging = true;
        i.premium_charging = true;
        let events = compute_events(&i, 0);
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
        i.basic_charging = true;
        let now = i.grace_expires_at + 1;
        let events = compute_events(&i, now);
        assert_eq!(events.len(), 1);
        assert!(matches!(
            events[0].0,
            Event::OldUserPostGraceOverQuota { tier: PaymentTier::Basic, .. }
        ));
    }

    #[test]
    fn dedupes_already_shown_events() {
        let mut i = base();
        i.user_type = Some(UserType::New);
        i.intro_shown = true;
        i.basic_over_shown = true;
        i.basic_charging = true;
        let events = compute_events(&i, 0);
        assert!(events.is_empty());
    }

    #[test]
    fn premium_over_quota_independent_of_basic() {
        let mut i = base();
        i.user_type = Some(UserType::New);
        i.intro_shown = true;
        i.basic_over_shown = true;
        i.premium_charging = true;
        let events = compute_events(&i, 0);
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].1, Flag::PremiumOver);
        assert!(matches!(
            events[0].0,
            Event::NewUserOverQuota { tier: PaymentTier::Premium, .. }
        ));
    }

    #[test]
    fn over_quota_re_fires_after_shown_is_reset() {
        // Lifecycle: charging flips on → event fires → user re-runs
        // (shown=true, ack implicit) → server drops back to free (caller
        // resets shown=false) → charging flips on again → event MUST fire
        // again to force another confirming step.
        let mut i = base();
        i.user_type = Some(UserType::New);
        i.intro_shown = true;
        i.basic_charging = true;
        // First transition.
        let events = compute_events(&i, 0);
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].1, Flag::BasicOver);
        // Simulate: caller flipped basic_over_shown to true after firing.
        i.basic_over_shown = true;
        // Steady charging-true: no repeat.
        let events = compute_events(&i, 0);
        assert!(events.is_empty());
        // Server drops back to free → caller resets shown.
        i.basic_charging = false;
        i.basic_over_shown = false;
        // Server re-flips to charging.
        i.basic_charging = true;
        let events = compute_events(&i, 0);
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].1, Flag::BasicOver);
    }

    #[test]
    fn over_quota_payload_embeds_resolved_payment_options() {
        let mut i = base();
        i.user_type = Some(UserType::New);
        i.intro_shown = true;
        i.basic_charging = true;
        i.accepts = Some(sample_accepts());

        let events = compute_events(&i, 0);
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
        assert_eq!(payment[1]["asset"], "0xUSDT");
        assert_eq!(payment[1]["name"], "USDT");
    }

    #[test]
    fn over_quota_payload_handles_flat_amount_and_missing_tier() {
        let mut i = base();
        i.user_type = Some(UserType::New);
        i.intro_shown = true;
        i.premium_charging = true;
        i.accepts = Some(serde_json::json!([
            // Flat amount — applies to both tiers.
            { "amount": "500", "asset": "0xFLAT", "network": "n", "payTo": "p" },
            // Tiered object missing the premium key — should be dropped.
            { "amount": { "basic": "100" }, "asset": "0xBASIC_ONLY" },
        ]));
        let events = compute_events(&i, 0);
        assert_eq!(events.len(), 1);
        let Event::NewUserOverQuota { payment, .. } = &events[0].0 else {
            panic!("expected NewUserOverQuota variant");
        };
        assert_eq!(payment.len(), 1);
        assert_eq!(payment[0]["asset"], "0xFLAT");
        // "500" minimal → "0.0005" display (6 decimals).
        assert_eq!(payment[0]["amount"], "0.0005");
    }

    #[test]
    fn amount_minimal_to_display_renders_expected_strings() {
        assert_eq!(amount_minimal_to_display("500").unwrap(), "0.0005");
        assert_eq!(amount_minimal_to_display("100").unwrap(), "0.0001");
        assert_eq!(amount_minimal_to_display("1500000").unwrap(), "1.5");
        assert_eq!(amount_minimal_to_display("1000000").unwrap(), "1");
        assert_eq!(amount_minimal_to_display("0").unwrap(), "0");
        assert_eq!(amount_minimal_to_display("10").unwrap(), "0.00001");
        assert_eq!(amount_minimal_to_display("123456789").unwrap(), "123.456789");
        assert!(amount_minimal_to_display("").is_none());
        assert!(amount_minimal_to_display("abc").is_none());
        assert!(amount_minimal_to_display("-100").is_none());
    }

    #[test]
    fn over_quota_payload_omits_payment_when_accepts_absent() {
        let mut i = base();
        i.user_type = Some(UserType::New);
        i.intro_shown = true;
        i.basic_charging = true;
        // i.accepts stays None
        let events = compute_events(&i, 0);
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
        drain_events();
        push_event(Event::NewUserIntro {
            basic_free_quota: BASIC_FREE_QUOTA,
            premium_free_quota: PREMIUM_FREE_QUOTA,
            doc_url: DOC_URL.to_string(),
        });
        push_event(Event::OldUserGrace {
            grace_days: GRACE_DAYS,
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
