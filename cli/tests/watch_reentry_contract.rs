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
    let note = "> Note: The job will continue running after it is created, but message monitoring may stop. You can:\n>\n> - Reply “Check the current task progress” for a one-time status check.\n> - For subscriptions, reply “Check subscription task status” to view recent follow-trade results.";
    assert_eq!(WATCH.matches(note).count(), 1);
    assert!(WATCH.contains("`phase=creation`"));
    assert!(WATCH.contains("`reason=broadcast_submitted`"));
    assert!(WATCH.contains("`nextAction.id=watch_task`"));
    assert!(WATCH.contains("translate it into the user's initial locale"));
    assert!(WATCH.contains("Do not show it for"));
    assert!(WATCH.contains("dispatch resume, wake re-entry, or any later watch call"));
    assert!(WATCH.contains("render `Check the current task progress` as\n`查询当前任务进展`"));
    assert!(WATCH.contains("`Check subscription task status` as `查询订阅任务状态`"));
}

#[test]
fn current_task_progress_is_a_one_time_status_query() {
    let task_query = include_str!("../../skills/okx-ai/references/a2a/task-query.md");
    assert!(task_query.contains("`查询当前任务进展`"));
    assert!(task_query.contains("one-time fresh status query"));
    assert!(task_query.contains("single unambiguous Job ID bound to the current conversation"));
    assert!(task_query.contains("run `active-tasks`"));
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
