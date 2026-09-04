const PUBLISH_ACTIONS: &str =
    include_str!("../../skills/okx-ai/references/task-user-actions-create.md");
const USER_PLAYBOOK: &str = include_str!("../../skills/okx-ai/references/task-user-playbook.md");

#[test]
fn skill_confirmation_templates_never_expose_execution_configuration() {
    for forbidden_row in [
        "| Signal Execution |",
        "| Per-Signal Amount |",
        "| Per-Signal Cap |",
        "| Trade Kit Environment |",
    ] {
        assert!(
            !PUBLISH_ACTIONS.contains(forbidden_row),
            "confirmation template must not contain {forbidden_row}"
        );
    }

    assert!(PUBLISH_ACTIONS.contains("Do not append or merge any other row"));
    assert!(PUBLISH_ACTIONS.contains("list them below the table; never add an Attachments row"));
    assert!(PUBLISH_ACTIONS.contains("Guide-defined Consent and Signal values"));
    assert!(PUBLISH_ACTIONS.contains("--guide-consent-json"));
    assert!(!PUBLISH_ACTIONS.contains("`--autotrade-*` arguments"));
    assert!(PUBLISH_ACTIONS.contains(
        "that\n\
returned form is the sole field authority"
    ));
    assert!(PUBLISH_ACTIONS.contains(
        "Appendix A\n\
is only a fallback render contract for a direct route"
    ));
    assert!(USER_PLAYBOOK.contains(
        "derives and locally validates a projection from the selected service Guide"
    ));
    assert!(USER_PLAYBOOK.contains("ASP supplies Guide text only"));
    assert!(!USER_PLAYBOOK.contains("Signal handling mode"));
    assert!(!USER_PLAYBOOK.contains("autoTradeConfigRequested"));
    assert!(USER_PLAYBOOK.contains(
        "its returned confirmation form is the sole field authority; never merge fields"
    ));
}

#[test]
fn skill_playbooks_delegate_optional_trade_kit_setup_to_agent_skills() {
    for playbook in [PUBLISH_ACTIONS, USER_PLAYBOOK] {
        assert!(playbook.contains("Install/connect"));
        assert!(playbook.contains("Later"));
    }

    assert!(PUBLISH_ACTIONS.contains("okx/agent-skills"));
    assert!(PUBLISH_ACTIONS.contains("okx-cex-auth"));
    assert!(PUBLISH_ACTIONS.contains("security scan"));
    assert!(PUBLISH_ACTIONS.contains("re-run it"));
    assert!(PUBLISH_ACTIONS.contains("only after install/upgrade and never to verify OAuth"));
    assert!(PUBLISH_ACTIONS.contains("never to verify OAuth"));
    assert!(PUBLISH_ACTIONS.contains("delegate"));
    assert!(PUBLISH_ACTIONS.contains("CLI/site/OAuth/API-key setup to that skill"));
}

#[test]
fn refund_requires_a_user_authored_reason_before_reject() {
    assert!(USER_PLAYBOOK.contains("A refund reason must be explicitly authored by the user"));
    assert!(USER_PLAYBOOK.contains("ask for the reason and end the turn without running `reject`"));
    assert!(USER_PLAYBOOK.contains(
        "Never use the refund request itself, a default, an example, or model-generated text as the reason"
    ));
}
