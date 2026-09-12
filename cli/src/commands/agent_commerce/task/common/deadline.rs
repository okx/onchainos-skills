//! deadline — shared decision/review-deadline reminder rendering.
//!
//! Home for the day-count + `chrono::Local` timestamp-formatting logic shared by
//! the User acceptance card (`job_submitted_escrow`, [`DeadlineKind::Review`]) and the
//! ASP arbitration card (`job_rejected_user_decision_prompt`, [`DeadlineKind::Decision`]).
//! Keeping the ceiling-days math and the formatting here (instead of copy-pasted into
//! each renderer) satisfies the no-duplication / cognitive-complexity constraint.

use chrono::{DateTime, Local, TimeZone, Utc};
use serde_json::Value;

pub(crate) const REVIEW_WINDOW_SECONDS: i64 = 3 * 86_400;

/// Normalize a positive Unix timestamp expressed in seconds or milliseconds.
pub(crate) fn normalize_timestamp_seconds(value: i64) -> Option<i64> {
    let seconds = if value.unsigned_abs() >= 100_000_000_000 {
        value.checked_div(1_000)?
    } else {
        value
    };
    (seconds > 0).then_some(seconds)
}

/// Parse a seconds/milliseconds Unix timestamp or an RFC3339 timestamp.
pub(crate) fn parse_timestamp_seconds(value: &str) -> Option<i64> {
    let value = value.trim();
    value
        .parse::<i64>()
        .ok()
        .and_then(normalize_timestamp_seconds)
        .or_else(|| {
            DateTime::parse_from_rfc3339(value)
                .ok()
                .and_then(|date_time| normalize_timestamp_seconds(date_time.timestamp()))
        })
}

/// Parse a timestamp from the scalar forms used by task APIs and events.
pub(crate) fn parse_timestamp_value(value: &Value) -> Option<i64> {
    value
        .as_i64()
        .and_then(normalize_timestamp_seconds)
        .or_else(|| {
            value
                .as_u64()
                .and_then(|value| i64::try_from(value).ok())
                .and_then(normalize_timestamp_seconds)
        })
        .or_else(|| value.as_str().and_then(parse_timestamp_seconds))
}

/// Return the first valid timestamp found under the supplied keys.
pub(crate) fn first_timestamp(detail: &Value, keys: &[&str]) -> Option<i64> {
    keys.iter()
        .find_map(|key| detail.get(*key).and_then(parse_timestamp_value))
}

/// Prefer the absolute review deadline returned by the service, then derive
/// the confirmed three-day window from the authoritative submission time.
pub(crate) fn review_deadline_from_detail(detail: &Value) -> Option<i64> {
    first_timestamp(
        detail,
        &["reviewDeadlineAt", "reviewWindowEndsAt", "expireTime"],
    )
    .or_else(|| {
        first_timestamp(detail, &["submittedAt", "submitTime"])
            .and_then(|submitted| submitted.checked_add(REVIEW_WINDOW_SECONDS))
    })
}

/// Which decision card the reminder is for; selects the auto-resolution wording.
#[derive(Clone, Copy)]
pub(crate) enum DeadlineKind {
    /// User acceptance card — lapse ⇒ auto-accept (payment released to ASP).
    Review,
    /// ASP arbitration card — lapse ⇒ auto-refund to the buyer.
    Decision,
}

/// Whole days remaining until `expire_time` (unix seconds), ceiling to whole
/// days with a minimum of 1 while any time remains; 0 only when already expired.
/// `now` is injected so unit tests are deterministic.
pub(crate) fn days_left(expire_time: i64, now: i64) -> i64 {
    let remaining = expire_time - now;
    if remaining <= 0 {
        return 0;
    }
    remaining / 86_400 + i64::from(remaining % 86_400 > 0)
}

/// Format a unix-seconds deadline as local `MM-DD HH:mm`. `None` when the
/// timestamp is not representable in local time (graceful no-line).
pub(crate) fn format_local_deadline(expire_time: i64) -> Option<String> {
    let expire_time = normalize_timestamp_seconds(expire_time)?;
    Local
        .timestamp_opt(expire_time, 0)
        .single()
        .map(|dt| dt.format("%m-%d %H:%M").to_string())
}

/// Format a seconds-or-milliseconds Unix timestamp to minute precision with
/// the local UTC offset. Display templates consume this value directly.
pub(crate) fn format_local_timestamp_with_offset(timestamp: i64) -> Option<String> {
    let seconds = normalize_timestamp_seconds(timestamp)?;
    let local = Local.timestamp_opt(seconds, 0).single()?;
    let offset = local.offset().local_minus_utc();
    let sign = if offset < 0 { '-' } else { '+' };
    let absolute = offset.unsigned_abs();
    Some(format!(
        "{} (UTC{sign}{:02}:{:02})",
        local.format("%Y-%m-%d %H:%M"),
        absolute / 3_600,
        (absolute % 3_600) / 60,
    ))
}

/// Format an authoritative unix timestamp to minute precision with an explicit
/// UTC offset. Millisecond-scale values are tolerated because some legacy event
/// envelopes used milliseconds while the current contract uses seconds.
pub(crate) fn format_utc_timestamp(timestamp: i64) -> Option<String> {
    let timestamp = normalize_timestamp_seconds(timestamp)?;
    chrono::DateTime::<Utc>::from_timestamp(timestamp, 0)
        .map(|dt| dt.format("%Y-%m-%d %H:%M (UTC+00:00)").to_string())
}

/// Build the `⏰` reminder line for a decision card. `None` when no line should
/// be shown (expire_time absent, `<= 0`, or not representable) — FR-5 no-op.
pub(crate) fn deadline_reminder_line(
    expire_time: Option<i64>,
    now: i64,
    kind: DeadlineKind,
) -> Option<String> {
    let expire = expire_time.and_then(normalize_timestamp_seconds)?;
    let when = format_local_deadline(expire)?;
    let line = match (expire <= now, kind) {
        (true, DeadlineKind::Review) => format!(
            "⏰ Review deadline has passed ({when}). The system may auto-accept at any time."
        ),
        (true, DeadlineKind::Decision) => format!(
            "⏰ Decision deadline has passed ({when}). The system may auto-refund to the buyer at any time."
        ),
        (false, DeadlineKind::Review) => format!(
            "⏰ Review deadline: {} day(s) (by {when}). If not reviewed in time, the system will auto-accept and release payment to the ASP — irreversible.",
            days_left(expire, now)
        ),
        (false, DeadlineKind::Decision) => format!(
            "⏰ Decision deadline: {} day(s) (by {when}). If not decided in time, the system will auto-refund to the buyer — irreversible.",
            days_left(expire, now)
        ),
    };
    Some(line)
}

#[cfg(test)]
mod tests {
    use super::*;

    const DAY: i64 = 86_400;
    const HOUR: i64 = 3_600;
    // Fixed reference "now" so tests never read the wall clock.
    const NOW: i64 = 1_000_000_000;

    // ── days_left ────────────────────────────────────────────────────────

    #[test]
    fn days_left_full_days() {
        assert_eq!(days_left(NOW + 3 * DAY, NOW), 3);
    }

    #[test]
    fn days_left_sub_day_ceils_to_one() {
        // now+2h ⇒ 1 (never 0 while time remains).
        assert_eq!(days_left(NOW + 2 * HOUR, NOW), 1);
        // now+6h ⇒ 1.
        assert_eq!(days_left(NOW + 6 * HOUR, NOW), 1);
    }

    #[test]
    fn days_left_partial_day_ceils_up() {
        // 3 days + 1 second still remaining ⇒ ceil to 4.
        assert_eq!(days_left(NOW + 3 * DAY + 1, NOW), 4);
    }

    #[test]
    fn days_left_expired_is_zero() {
        // now-1s ⇒ 0.
        assert_eq!(days_left(NOW - 1, NOW), 0);
        // exact now ⇒ 0.
        assert_eq!(days_left(NOW, NOW), 0);
    }

    // ── format_local_deadline ────────────────────────────────────────────

    #[test]
    fn format_local_deadline_representable() {
        // A representable epoch must render as "MM-DD HH:mm" (5+1+5 chars).
        let s = format_local_deadline(NOW).expect("epoch is representable");
        assert_eq!(s.len(), "MM-DD HH:mm".len());
        assert_eq!(s.as_bytes()[2], b'-');
        assert_eq!(s.as_bytes()[8], b':');
    }

    #[test]
    fn format_local_deadline_out_of_range_is_none() {
        assert!(format_local_deadline(i64::MAX).is_none());
    }

    #[test]
    fn format_utc_timestamp_is_explicit_and_tolerates_milliseconds() {
        assert_eq!(
            format_utc_timestamp(1_700_000_000),
            Some("2023-11-14 22:13 (UTC+00:00)".to_string())
        );
        assert_eq!(
            format_utc_timestamp(1_700_000_000_000),
            Some("2023-11-14 22:13 (UTC+00:00)".to_string())
        );
        assert!(format_utc_timestamp(i64::MAX).is_none());
    }

    #[test]
    fn shared_timestamp_parser_accepts_seconds_milliseconds_and_rfc3339() {
        assert_eq!(parse_timestamp_seconds("1700000000"), Some(1_700_000_000));
        assert_eq!(
            parse_timestamp_seconds("1700000000000"),
            Some(1_700_000_000)
        );
        assert_eq!(
            parse_timestamp_seconds("2023-11-14T22:13:20Z"),
            Some(1_700_000_000)
        );
    }

    #[test]
    fn shared_review_deadline_prefers_exact_then_submitted_plus_three_days() {
        assert_eq!(
            review_deadline_from_detail(&serde_json::json!({
                "reviewDeadlineAt": 1_700_000_123,
                "submittedAt": 1_700_000_000
            })),
            Some(1_700_000_123)
        );
        assert_eq!(
            review_deadline_from_detail(&serde_json::json!({
                "submittedAt": 1_700_000_000_000_i64
            })),
            Some(1_700_000_000 + REVIEW_WINDOW_SECONDS)
        );
        assert_eq!(review_deadline_from_detail(&serde_json::json!({})), None);
    }

    // ── deadline_reminder_line ───────────────────────────────────────────

    #[test]
    fn reminder_review_active_full_string() {
        let line = deadline_reminder_line(Some(NOW + 3 * DAY), NOW, DeadlineKind::Review)
            .expect("active review line");
        let when = format_local_deadline(NOW + 3 * DAY).unwrap();
        assert_eq!(
            line,
            format!(
                "⏰ Review deadline: 3 day(s) (by {when}). If not reviewed in time, the system will auto-accept and release payment to the ASP — irreversible."
            )
        );
    }

    #[test]
    fn reminder_review_expired_full_string() {
        let line = deadline_reminder_line(Some(NOW - 1), NOW, DeadlineKind::Review)
            .expect("expired review line");
        let when = format_local_deadline(NOW - 1).unwrap();
        assert_eq!(
            line,
            format!(
                "⏰ Review deadline has passed ({when}). The system may auto-accept at any time."
            )
        );
    }

    #[test]
    fn reminder_decision_active_full_string() {
        let line = deadline_reminder_line(Some(NOW + DAY), NOW, DeadlineKind::Decision)
            .expect("active decision line");
        let when = format_local_deadline(NOW + DAY).unwrap();
        assert_eq!(
            line,
            format!(
                "⏰ Decision deadline: 1 day(s) (by {when}). If not decided in time, the system will auto-refund to the buyer — irreversible."
            )
        );
    }

    #[test]
    fn reminder_decision_expired_full_string() {
        let line = deadline_reminder_line(Some(NOW - 1), NOW, DeadlineKind::Decision)
            .expect("expired decision line");
        let when = format_local_deadline(NOW - 1).unwrap();
        assert_eq!(
            line,
            format!(
                "⏰ Decision deadline has passed ({when}). The system may auto-refund to the buyer at any time."
            )
        );
    }

    #[test]
    fn reminder_none_when_absent_or_nonpositive() {
        assert!(deadline_reminder_line(None, NOW, DeadlineKind::Review).is_none());
        assert!(deadline_reminder_line(Some(0), NOW, DeadlineKind::Review).is_none());
        assert!(deadline_reminder_line(Some(-5), NOW, DeadlineKind::Decision).is_none());
    }

    #[test]
    fn reminder_none_when_not_representable() {
        assert!(deadline_reminder_line(Some(i64::MAX), NOW, DeadlineKind::Review).is_none());
    }
}
