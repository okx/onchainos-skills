const PUBLISH_ACTIONS: &str =
    include_str!("../../skills/okx-ai/references/task-user-actions-create.md");
const USER_PLAYBOOK: &str = include_str!("../../skills/okx-ai/references/task-user-playbook.md");
const USER_REFUND: &str = include_str!("../../skills/okx-ai/references/task-user-refund.md");

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
fn refund_v2_requires_a_user_authored_reason_and_explicit_confirmation() {
    assert!(USER_REFUND.contains("It must be non-blank"));
    assert!(USER_REFUND.contains("authored by the User"));
    assert!(USER_REFUND.contains("Never supply,"));
    assert!(USER_REFUND.contains("paraphrase, translate, or improve it"));
    assert!(USER_REFUND.contains("refund-execute"));
    assert!(USER_REFUND.contains("--confirm"));
    assert!(USER_PLAYBOOK.contains("task-user-refund.md"));
}

#[test]
fn deliverable_review_b_reason_is_the_scoped_direct_rejection_confirmation() {
    assert!(USER_REFUND.contains(
        "B` together with a non-blank\n\
User-authored reason is the User's final confirmation"
    ));
    assert!(USER_REFUND.contains("Handle this reply in the current user conversation"));
    assert!(USER_REFUND.contains("refund_request_confirmation_required"));
    assert!(USER_REFUND.contains("returned `nextAction.id=submit_refund_request`"));
    assert!(USER_REFUND.contains("owns reason extraction, fresh preparation, execution, and"));
    assert!(USER_REFUND.contains("give one concise localized\nconfirmation"));
    assert!(USER_REFUND.contains("Describe it\nas submitted rather than settled"));
    assert!(USER_REFUND.contains(
        "onchainos agent status <jobId> --agent-id <buyerAgentId>"
    ));
    assert!(USER_REFUND.contains("a reason\nreceived outside that active card remains input only"));
}
