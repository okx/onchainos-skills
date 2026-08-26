const PUBLISH_ACTIONS: &str =
    include_str!("../../skills/okx-ai/references/task-user-actions-publish.md");
const USER_PLAYBOOK: &str = include_str!("../../skills/okx-ai/references/task-user-playbook.md");

#[test]
fn skill_confirmation_templates_never_expose_execution_configuration() {
    for forbidden_row in [
        "| Signal Execution |",
        "| Per-Signal Amount |",
        "| Per-Signal Cap |",
    ] {
        assert!(
            !PUBLISH_ACTIONS.contains(forbidden_row),
            "confirmation template must not contain {forbidden_row}"
        );
    }

    assert!(PUBLISH_ACTIONS.contains(
        "Never add them\n\
to this or any other confirmation form"
    ));
    assert!(PUBLISH_ACTIONS.contains("pass them through the existing `--autotrade-*` arguments"));
    assert!(PUBLISH_ACTIONS.contains(
        "that\n\
returned form is the sole field authority"
    ));
    assert!(PUBLISH_ACTIONS.contains(
        "Appendix A is\n\
only a fallback render contract for a direct route"
    ));
    assert!(USER_PLAYBOOK.contains(
        "Never render execution mode, per-signal amount, per-signal cap, margin mode, or order policy as confirmation-form rows"
    ));
    assert!(USER_PLAYBOOK.contains(
        "persist mode/amount/cap/quote/environment/margin mode/order policy only from the user's reply"
    ));
    assert!(USER_PLAYBOOK.contains(
        "its returned confirmation form is the sole field authority; never merge fields"
    ));
}

#[test]
fn skill_playbooks_delegate_optional_trade_kit_setup_to_agent_skills() {
    for playbook in [PUBLISH_ACTIONS, USER_PLAYBOOK] {
        assert!(playbook.contains("On Install/connect"));
        assert!(playbook.contains("Later"));
        assert!(playbook.contains("okx/agent-skills"));
        assert!(playbook.contains("okx-cex-auth"));
        assert!(playbook.contains("security scan"));
    }

    assert!(PUBLISH_ACTIONS.contains("re-run it only after install/upgrade"));
    assert!(PUBLISH_ACTIONS.contains("never to verify OAuth"));
    assert!(PUBLISH_ACTIONS.contains("delegate CLI/site/OAuth/API-key setup to that skill"));
}
