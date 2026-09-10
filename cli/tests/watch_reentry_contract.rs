const WATCH: &str = include_str!("../../skills/okx-ai/references/runtime/watch.md");
const BACKLOG: &str = include_str!("../../skills/okx-ai/references/runtime/backlog.md");
const RELAY: &str = include_str!("../../skills/okx-ai/references/runtime/decision-relay.md");
const WAKE: &str = include_str!("../../skills/okx-ai/references/runtime/watch-wake.md");
const CREATE: &str = include_str!("../../skills/okx-ai/references/a2a/user/create.md");
const CREATE_IMPL: &str =
    include_str!("../src/commands/agent_commerce/task/user/create.rs");

#[test]
fn watch_reenters_after_nonterminal_results() {
    assert!(WATCH.contains("it never authorizes ending the turn after one watch call returns"));
    assert!(WATCH.contains("After processing all returned items, **always** call"));
    assert!(WATCH.contains("single long-poll call"));
    assert!(WATCH.contains("--job-id <X>"));
    assert!(CREATE.contains("broadcast_submitted/watch_task"));
}

#[test]
fn one_time_creation_shows_initial_lifecycle_before_scoped_watch() {
    assert!(CREATE_IMPL.contains("\"initialLifecycle\""));
    assert!(CREATE_IMPL.contains("initial_creation_display()"));
    assert!(CREATE.contains("hand the complete structured result\nunchanged"));
    assert!(CREATE.contains("Runtime Watch owns the\npost-result sequence"));

    let lifecycle = WATCH.find("1. **Initial progress.**").unwrap();
    let banner = WATCH.find("2. **Watch banner.**").unwrap();
    let note = WATCH
        .find("3. **Creation-start monitoring note.**")
        .unwrap();
    let first_watch = WATCH
        .find("4. **Scoped watch.** Only after steps 1–3")
        .unwrap();
    assert!(lifecycle < banner && banner < note && note < first_watch);

    for index in 0..5 {
        assert!(WATCH.contains(&format!("{{timeline[{index}].marker}}")));
    }
    assert!(WATCH.contains("`payload.initialLifecycle.taskType=one_time`"));
    assert!(WATCH.contains("Current status: {localized display.currentSummary}"));
    assert!(WATCH.contains("Handled by: {localized display.handledBy}"));
    assert!(WATCH.contains("Next: {localized display.next}"));
    assert!(WATCH.contains("Initial progress unavailable."));
    assert!(WATCH.contains("localized to the conversation language"));
    assert!(WATCH.contains("equal to both\n   `nextAction.params.jobId` and `payload.jobId`"));
}

#[test]
fn completed_asp_execution_can_show_one_cli_backed_deliverable_line() {
    let task_query = include_str!("../../skills/okx-ai/references/a2a/task-query.md");
    assert!(task_query.contains("`key=asp_execution` has marker `✓`"));
    assert!(task_query.contains("`display.deliverableAvailable=true`"));
    assert!(task_query.contains("Review readiness is CLI-owned by `display.reviewReady`"));
    assert!(task_query.contains(
        "onchainos agent task-deliverable-list --job-id <jobId> --role user"
    ));
    assert!(task_query.contains("│  Deliverable: [<absolutePath>](<absolutePath>)"));
    assert!(task_query.contains("add exactly one line under the ASP-execution detail"));
    assert!(task_query.contains("The Skill does not open the\nreturned file"));
}

#[test]
fn creation_watch_shows_one_localized_monitoring_note_without_reentry_repeats() {
    let note = "> Note: The job will continue running after it is created, but message monitoring may stop. You can:\n>\n> - Reply “Check the current task progress” to view the complete one-time task lifecycle.\n> - For subscriptions, reply “Check subscription task status” to view recent follow-trade results.";
    assert_eq!(WATCH.matches(note).count(), 1);
    assert!(WATCH.contains("`phase=creation`"));
    assert!(WATCH.contains("`reason=broadcast_submitted`"));
    assert!(WATCH.contains("`nextAction.id=watch_task`"));
    assert!(WATCH.contains("translate it into the conversation language"));
    assert!(WATCH.contains("including natural localized equivalents"));
    assert!(WATCH.contains("Do not show it for"));
    assert!(WATCH.contains("dispatch resume, wake re-entry, or any later watch call"));
    assert!(WATCH.contains("“Check the current task progress”"));
    assert!(WATCH.contains("“Check subscription task status”"));
    assert_eq!(WATCH.matches("- `phase=creation`;").count(), 1);
    assert_eq!(WATCH.matches("- `reason=broadcast_submitted`;").count(), 1);
}

#[test]
fn current_task_progress_is_a_complete_lifecycle_query() {
    let task_query = include_str!("../../skills/okx-ai/references/a2a/task-query.md");
    assert!(task_query.contains("`View task status`"));
    assert!(task_query.contains("equivalent progress/status wording"));
    assert!(task_query.contains("onchainos agent lifecycle <jobId>"));
    assert!(task_query.contains("current-wallet User identity resolution"));
    assert!(task_query.contains("single unambiguous Job\nID bound to the current conversation"));
    assert!(task_query.contains("run `active-tasks`"));
}

#[test]
fn lifecycle_follow_up_precedes_the_final_completion_node() {
    let task_query = include_str!("../../skills/okx-ai/references/a2a/task-query.md");
    let template = task_query
        .split_once("A2A single task · {jobId}")
        .unwrap()
        .1
        .split_once("Rendering rules:")
        .unwrap()
        .0;
    let follow_up = template.find("{each display.followUp item").unwrap();
    let completion = template.find("{timeline[4].marker}").unwrap();
    assert!(follow_up < completion);
    assert!(task_query.contains("The task\n   completion item remains the final displayed node for every outcome."));
    assert!(task_query.contains("When `display.handledBy` is exactly `ASP` or `Platform`"));
    assert!(task_query.contains(
        "Currently handled by {localized display.handledBy}; next: {localized display.next}. You can say “View task details” to review the details."
    ));
    assert!(task_query.contains("Preserve the meaning of `display.next`"));
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
