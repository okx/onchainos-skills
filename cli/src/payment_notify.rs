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

use serde::{Deserialize, Serialize};

/// Process-global buffer of notification events emitted by `ApiClient`
/// during a request. Drained by the CLI output layer (`output::success`
/// / MCP `ok`) and attached to the response envelope. Populated by
/// `dispatch_notifications` rather than stderr so that Claude always
/// receives the notice in-band with the data, regardless of how the
/// CLI is invoked (subprocess, MCP stdio, etc.).
static PENDING: Mutex<Vec<serde_json::Value>> = Mutex::new(Vec::new());

/// Push a notification event onto the global buffer. Called from the
/// response handler once per unique (deduped) event.
pub fn push_event(event: serde_json::Value) {
    if let Ok(mut g) = PENDING.lock() {
        g.push(event);
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
    /// amount, payTo, ...) to `OVER_QUOTA` events so the skill can
    /// render exactly what the user is about to pay.
    pub accepts: Option<serde_json::Value>,
}

/// Fallback grace expiry used before the server's `/config` response
/// lands. 2026-05-31T00:00:00Z.
pub fn fallback_grace_expires_at() -> i64 {
    chrono::DateTime::parse_from_rfc3339("2026-05-31T00:00:00Z")
        .expect("hardcoded RFC3339 literal")
        .timestamp()
}

/// Decide which notifications should fire on this response. Returns one
/// `(json_line, flag)` pair per event, in intended print order. The
/// caller is responsible for flipping the flag and persisting.
///
/// There are exactly 5 event codes, mapping 1:1 to the 5 user-facing
/// states the skill renders — CLI only identifies which state the user
/// just entered; all copy lives in the skill.
///
/// Every event carries the same top-level shape:
/// `{ "code": "...", "data": { ... } }` — `data` is the event-specific
/// payload container (empty object for intro/grace, `{ tier, payment: [...] }`
/// for the two over-quota codes).
///
/// | User type | Window           | Quota         | Code                                         |
/// |-----------|------------------|---------------|----------------------------------------------|
/// | New       | —                | within        | `MARKET_API_NEW_USER_INTRO`                  |
/// | New       | —                | over (tier)   | `MARKET_API_NEW_USER_OVER_QUOTA`             |
/// | Old       | in grace         | —             | `MARKET_API_OLD_USER_GRACE`                  |
/// | Old       | post grace       | within        | `MARKET_API_OLD_USER_POST_GRACE_INTRO`       |
/// | Old       | post grace       | over (tier)   | `MARKET_API_OLD_USER_POST_GRACE_OVER_QUOTA`  |
///
/// Old users in grace never emit intro or over-quota events — grace wins.
pub fn compute_events(input: &NotifyInput, now: i64) -> Vec<(serde_json::Value, Flag)> {
    let mut events = Vec::new();
    let Some(user_type) = input.user_type else {
        return events;
    };
    let in_grace = matches!(user_type, UserType::Old) && now < input.grace_expires_at;

    if in_grace {
        if !input.grace_shown {
            events.push((
                make_event("MARKET_API_OLD_USER_GRACE", serde_json::json!({})),
                Flag::Grace,
            ));
        }
        return events;
    }

    if !input.intro_shown {
        let code = match user_type {
            UserType::New => "MARKET_API_NEW_USER_INTRO",
            UserType::Old => "MARKET_API_OLD_USER_POST_GRACE_INTRO",
        };
        events.push((make_event(code, serde_json::json!({})), Flag::Intro));
    }

    let over_code = match user_type {
        UserType::New => "MARKET_API_NEW_USER_OVER_QUOTA",
        UserType::Old => "MARKET_API_OLD_USER_POST_GRACE_OVER_QUOTA",
    };
    if input.basic_charging && !input.basic_over_shown {
        events.push((over_quota_event(over_code, "basic", &input.accepts), Flag::BasicOver));
    }
    if input.premium_charging && !input.premium_over_shown {
        events.push((
            over_quota_event(over_code, "premium", &input.accepts),
            Flag::PremiumOver,
        ));
    }

    events
}

fn make_event(code: &'static str, data: serde_json::Value) -> serde_json::Value {
    serde_json::json!({ "code": code, "data": data })
}

fn over_quota_event(
    code: &'static str,
    tier_key: &'static str,
    accepts: &Option<serde_json::Value>,
) -> serde_json::Value {
    let mut data = serde_json::json!({ "tier": tier_key });
    let payment = accepts
        .as_ref()
        .map(|a| payment_options_for_tier(a, tier_key))
        .unwrap_or_default();
    if !payment.is_empty() {
        data["payment"] = serde_json::Value::Array(payment);
    }
    make_event(code, data)
}

/// Project the `accepts` array into a tier-resolved form. Each entry
/// is passed through as-is with only `amount` rewritten from the
/// tiered `{basic, premium}` object down to the chosen tier's string
/// value. Any additional fields the server may add in future (e.g.
/// `description`, `expiresAt`) flow through untouched — no CLI change
/// required. Entries whose `amount` is a tiered object but doesn't
/// carry the requested tier are skipped.
fn payment_options_for_tier(accepts: &serde_json::Value, tier_key: &str) -> Vec<serde_json::Value> {
    let Some(arr) = accepts.as_array() else {
        return Vec::new();
    };
    arr.iter()
        .filter_map(|a| {
            let obj = a.as_object()?;
            let amount = match obj.get("amount") {
                Some(serde_json::Value::String(s)) => s.clone(),
                Some(serde_json::Value::Object(o)) => {
                    o.get(tier_key).and_then(|v| v.as_str())?.to_string()
                }
                _ => return None,
            };
            let mut out = obj.clone();
            out.insert("amount".into(), serde_json::Value::String(amount));
            Some(serde_json::Value::Object(out))
        })
        .collect()
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
        let expected = chrono::Utc
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

    fn code(v: &serde_json::Value) -> &str {
        v["code"].as_str().unwrap()
    }

    #[test]
    fn new_user_first_request_emits_intro() {
        let mut i = base();
        i.user_type = Some(UserType::New);
        let events = compute_events(&i, 0);
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].1, Flag::Intro);
        assert_eq!(code(&events[0].0), "MARKET_API_NEW_USER_INTRO");
    }

    #[test]
    fn old_user_in_grace_emits_grace_only() {
        let mut i = base();
        i.user_type = Some(UserType::Old);
        let events = compute_events(&i, 0);
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].1, Flag::Grace);
        assert_eq!(code(&events[0].0), "MARKET_API_OLD_USER_GRACE");
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
        assert_eq!(code(&events[0].0), "MARKET_API_OLD_USER_POST_GRACE_INTRO");
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
        assert_eq!(code(&events[0].0), "MARKET_API_NEW_USER_OVER_QUOTA");
        assert_eq!(events[0].0["data"]["tier"], "basic");
    }

    #[test]
    fn intro_event_wraps_empty_data_object() {
        let mut i = base();
        i.user_type = Some(UserType::New);
        let events = compute_events(&i, 0);
        assert_eq!(events.len(), 1);
        // Uniform shape: every event has `code` + `data` at the top level.
        assert!(events[0].0["data"].is_object());
        assert_eq!(events[0].0["data"].as_object().unwrap().len(), 0);
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
    fn old_post_grace_over_quota_uses_post_grace_code() {
        let mut i = base();
        i.user_type = Some(UserType::Old);
        i.intro_shown = true;
        i.grace_shown = true;
        i.basic_charging = true;
        let now = i.grace_expires_at + 1;
        let events = compute_events(&i, now);
        assert_eq!(events.len(), 1);
        assert_eq!(code(&events[0].0), "MARKET_API_OLD_USER_POST_GRACE_OVER_QUOTA");
        assert_eq!(events[0].0["data"]["tier"], "basic");
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
        assert_eq!(events[0].0["data"]["tier"], "premium");
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
        let payment = events[0].0["data"]["payment"]
            .as_array()
            .expect("payment array");
        assert_eq!(payment.len(), 2);
        // Basic tier → amount resolves to "100"
        assert_eq!(payment[0]["amount"], "100");
        assert_eq!(payment[0]["asset"], "0xUSDG");
        assert_eq!(payment[0]["network"], "eip155:196");
        assert_eq!(payment[0]["payTo"], "0xPAYTO");
        assert_eq!(payment[1]["asset"], "0xUSDT");
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
        let payment = events[0].0["data"]["payment"]
            .as_array()
            .expect("payment array");
        assert_eq!(payment.len(), 1);
        assert_eq!(payment[0]["asset"], "0xFLAT");
        assert_eq!(payment[0]["amount"], "500");
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
        // `data.payment` is omitted when nothing is known.
        assert!(events[0].0["data"].get("payment").is_none());
        // `data.tier` is still set.
        assert_eq!(events[0].0["data"]["tier"], "basic");
    }

    #[test]
    fn push_and_drain_events_round_trip() {
        drain_events();
        push_event(serde_json::json!({ "code": "A" }));
        push_event(serde_json::json!({ "code": "B" }));
        let drained = drain_events();
        assert_eq!(drained.len(), 2);
        assert_eq!(drained[0]["code"], "A");
        assert_eq!(drained[1]["code"], "B");
        assert!(drain_events().is_empty());
    }
}
