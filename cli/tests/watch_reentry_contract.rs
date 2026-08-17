const WATCH_CORE: &str = include_str!("../../skills/okx-ai/references/watch-core.md");
const TASK_USER_PLAYBOOK: &str =
    include_str!("../../skills/okx-ai/references/task-user-playbook.md");

#[test]
fn watch_docs_do_not_end_after_a_nonterminal_result() {
    assert!(WATCH_CORE.contains("it never authorizes ending the turn after one watch call returns"));
    assert!(WATCH_CORE.contains("After processing all returned items, **always** call"));
    assert!(TASK_USER_PLAYBOOK.contains(
        "A returned notification, deliverable, or empty poll does **not** end the turn"
    ));
    assert!(!TASK_USER_PLAYBOOK.contains("execute watch, then **end this turn**"));
}
