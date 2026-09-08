const WATCH: &str = include_str!("../../skills/okx-ai/references/runtime/watch.md");
const BACKLOG: &str = include_str!("../../skills/okx-ai/references/runtime/backlog.md");
const RELAY: &str = include_str!("../../skills/okx-ai/references/runtime/decision-relay.md");
const WAKE: &str = include_str!("../../skills/okx-ai/references/runtime/watch-wake.md");
const CREATE: &str = include_str!("../../skills/okx-ai/references/a2a/user/create.md");

#[test]
fn watch_reenters_after_nonterminal_results() {
    assert!(WATCH.contains("it never authorizes ending the turn after one watch call returns"));
    assert!(WATCH.contains("After processing all returned items, **always** call"));
    assert!(WATCH.contains("single long-poll call"));
    assert!(WATCH.contains("--job-id <X>"));
    assert!(CREATE.contains("broadcast_submitted/watch_task"));
}

#[test]
fn creation_watch_shows_one_localized_monitoring_note_without_reentry_repeats() {
    let note = "> Note: Message monitoring may stop after the job is created, but the job will continue running. Reply “Resume message monitoring” to receive updates.";
    assert_eq!(WATCH.matches(note).count(), 1);
    assert!(WATCH.contains("`phase=creation`"));
    assert!(WATCH.contains("`reason=broadcast_submitted`"));
    assert!(WATCH.contains("`nextAction.id=watch_task`"));
    assert!(WATCH.contains("translate it into the user's initial locale"));
    assert!(WATCH.contains("Do not show it for"));
    assert!(WATCH.contains("dispatch resume, wake re-entry, or any later watch call"));
}

#[test]
fn decision_reply_claims_before_relay() {
    let claim = RELAY.find("Otherwise claim first").unwrap();
    let execute = RELAY.find("On `handled`").unwrap();
    assert!(claim < execute);
    assert!(RELAY.contains("execute only the item's `llmContent` commands verbatim"));
    assert!(RELAY.contains("List-origin items never start watch"));
}

#[test]
fn backlog_is_one_shot_and_filters_only_retired_cards() {
    assert!(BACKLOG.contains("okx-a2a user outdated-list --json"));
    assert!(BACKLOG.contains("autotrade_consent"));
    assert!(BACKLOG.contains("autotrade_config_required"));
    assert!(BACKLOG.contains("never starts watch"));
    assert!(BACKLOG.contains("JobID <prefix>"));
}

#[test]
fn wake_keeps_exact_watch_scope() {
    assert!(WAKE.contains("okx-a2a user watch --json --job-id <X>"));
    assert!(WAKE.contains("chronology guard"));
    assert!(WAKE.contains("COUNT=1"));
    assert!(WAKE.contains("backlog.md"));
}

#[test]
fn scoped_terminal_detection_requires_leading_marker() {
    assert!(WATCH.contains("first non-whitespace characters"));
    assert!(WATCH.contains("marker appearing later inside a title"));
    assert!(WATCH.contains("refund-reconcile.md"));
    assert!(WATCH.contains("Only a leading terminal marker"));
}
