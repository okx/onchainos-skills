const WATCH_CORE: &str = include_str!("../../skills/okx-ai-v2/references/runtime/watch.md");
const WATCH_OUTDATED_LIST: &str =
    include_str!("../../skills/okx-ai-v2/references/runtime/watch-outdated-list.md");
const TASK_USER_PLAYBOOK: &str =
    include_str!("../../skills/okx-ai-v2/references/a2a/user/playbook.md");
const TASK_CREATE_ACTIONS: &str =
    include_str!("../../skills/okx-ai-v2/references/a2a/user/create.md");
const PENDING_V2_SOURCE: &str =
    include_str!("../src/commands/agent_commerce/task/common/pending_v2.rs");

#[test]
fn watch_docs_do_not_end_after_a_nonterminal_result() {
    assert!(WATCH_CORE.contains("it never authorizes ending the turn after one watch call returns"));
    assert!(WATCH_CORE.contains("After processing all returned items, **always** call"));
    assert!(TASK_USER_PLAYBOOK
        .contains("A returned notification, deliverable, or empty poll does **not** end the turn"));
    assert!(!TASK_USER_PLAYBOOK.contains("execute watch, then **end this turn**"));
}

#[test]
fn task_creation_keeps_the_existing_scoped_watch_handoff() {
    assert!(TASK_CREATE_ACTIONS.contains("`reason=broadcast_submitted`"));
    assert!(TASK_CREATE_ACTIONS.contains("`nextAction.id=watch_task`"));
    assert!(WATCH_CORE.contains("okx-a2a user watch --json --job-id <X>"));
    assert!(WATCH_CORE.contains("single long-poll call"));
}

#[test]
fn direct_review_execution_starts_only_after_the_decision_reply_is_claimed() {
    let claim = WATCH_CORE
        .find("Otherwise claim first")
        .expect("watch contract must claim a non-defer reply");
    let execute = WATCH_CORE
        .find("On `handled`")
        .expect("watch contract must execute llmContent after claim");
    assert!(claim < execute);

    let review_claim = PENDING_V2_SOURCE
        .find("for a non-defer reply claim the decision")
        .expect("buyer review contract must claim the decision first");
    let review_action = PENDING_V2_SOURCE
        .find("Interpret the choice and complete the selected review action here")
        .expect("buyer review contract must define the direct A/B branch");
    assert!(review_claim < review_action);
    assert!(PENDING_V2_SOURCE.contains("current conversation owns choice parsing"));
    assert!(PENDING_V2_SOURCE.contains("keep it verbatim"));
}

#[test]
fn retired_autotrade_mode_cards_are_claimed_without_rendering() {
    for document in [WATCH_CORE, WATCH_OUTDATED_LIST] {
        assert!(document.contains("--source-event \"autotrade_consent\""));
        assert!(document.contains("--source-event \"autotrade_config_required\""));
        assert!(document.contains("okx-a2a user check --todo-ids <item.id> --json"));
        assert!(
            document.contains("do not render")
                || document.contains("remove it from the display set")
        );
        assert!(document.contains("do not execute") || document.contains("never execute"));
    }
}

#[test]
fn scoped_terminal_detection_requires_a_canonical_leading_marker() {
    assert!(WATCH_CORE.contains("first non-whitespace characters"));
    assert!(WATCH_CORE.contains("marker appearing later inside a title"));
    assert!(WATCH_CORE.contains("never a substring inside business data"));
    assert!(WATCH_CORE
        .contains("`[Job Expired]` / `[ASP Acceptance Expired]` / `[Auto-Refund Processing]`"));
    assert!(WATCH_CORE.contains("user/refund.md"));
    assert!(WATCH_CORE.contains("Only a leading terminal marker"));
    assert!(WATCH_CORE.contains("no Buyer claim/finalize action exists"));
    let legacy_terminal_sentence = WATCH_CORE
        .lines()
        .find(|line| line.contains("Legacy notifications may instead begin with"))
        .expect("watch-core must document legacy terminal headings");
    assert!(!legacy_terminal_sentence.contains("[Job Expired]"));
}
