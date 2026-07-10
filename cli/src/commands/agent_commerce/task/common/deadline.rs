//! deadline — shared decision/review-deadline reminder rendering.
//!
//! Home for the day-count + `chrono::Local` timestamp-formatting logic shared by
//! the User acceptance card (`job_submitted_escrow`, [`DeadlineKind::Review`]) and the
//! ASP arbitration card (`job_rejected_user_decision_prompt`, [`DeadlineKind::Decision`]).
//! Keeping the ceiling-days math and the formatting here (instead of copy-pasted into
//! each renderer) satisfies the no-duplication / cognitive-complexity constraint.

use chrono::{Local, TimeZone};

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
    Local
        .timestamp_opt(expire_time, 0)
        .single()
        .map(|dt| dt.format("%m-%d %H:%M").to_string())
}

/// Build the `⏰` reminder line for a decision card. `None` when no line should
/// be shown (expire_time absent, `<= 0`, or not representable) — FR-5 no-op.
pub(crate) fn deadline_reminder_line(
    expire_time: Option<i64>,
    now: i64,
    kind: DeadlineKind,
) -> Option<String> {
    let expire = expire_time.filter(|&t| t > 0)?;
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
