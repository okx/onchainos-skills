const PUBLISH_ACTIONS: &str =
    include_str!("../../skills/okx-ai/references/task-user-actions-create.md");
const USER_PLAYBOOK: &str = include_str!("../../skills/okx-ai/references/task-user-playbook.md");
const USER_REFUND: &str = include_str!("../../skills/okx-ai/references/task-user-refund.md");
const REFUND_ACTION_ROUTING: &str =
    include_str!("../../skills/okx-ai/references/task-action-routing.md");
const REFUND_OUTPUT_TEMPLATES: &str =
    include_str!("../../skills/okx-ai/references/task-output-templates.md");
const CANONICAL_SKILL: &str = include_str!("../../skills/okx-ai/SKILL.md");
const USER_INTENT_ROUTER: &str =
    include_str!("../../skills/okx-ai/references/task-user-intent-routing.md");
const V2_USER_ROUTER: &str = include_str!("../../skills/okx-ai-v2/references/a2a/user/router.md");
const V2_SKILL: &str = include_str!("../../skills/okx-ai-v2/SKILL.md");
const V2_USER_REFUND: &str = include_str!("../../skills/okx-ai-v2/references/a2a/user/refund.md");
const V2_REFUND_DISPLAY: &str =
    include_str!("../../skills/okx-ai-v2/references/a2a/user/refund-display.md");

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
    assert!(USER_PLAYBOOK
        .contains("derives and locally validates a projection from the selected service Guide"));
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
    let contract = V2_USER_REFUND
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");
    assert!(contract.contains("authored by the User"));
    assert!(contract.contains("non-blank"));
    assert!(contract.contains("preserved verbatim"));
    assert!(contract.contains("After explicit confirmation"));
    assert!(contract.contains("refund-execute"));
    assert!(contract.contains("--confirm"));
    assert!(USER_PLAYBOOK.contains("task-user-refund.md"));
}

#[test]
fn v2_buyer_router_owns_its_refund_contract() {
    assert!(CANONICAL_SKILL.contains("references/task-user-refund.md"));
    assert!(CANONICAL_SKILL.contains("only for CLI syntax or schema lookup"));
    assert!(USER_INTENT_ROUTER.contains("[`task-user-refund.md`](task-user-refund.md)"));
    assert!(V2_USER_ROUTER.contains("| Buyer refunds and paid-deliverable rejection"));
    assert!(V2_USER_ROUTER.contains("| [Buyer refunds](refund.md) |"));
    assert!(!V2_USER_ROUTER.contains("../../../../okx-ai/references/task-user-refund.md"));
    assert!(V2_USER_ROUTER.contains("paid-deliverable rejection"));
    assert!(V2_USER_REFUND.contains("[Refund Presentation](refund-display.md)"));
    for canonical_reference in [
        "../okx-ai/references/task-core.md",
        "../okx-ai/references/task-output-templates.md",
        "../okx-ai/references/task-action-routing.md",
    ] {
        assert!(V2_SKILL.contains(canonical_reference));
    }
}

#[test]
fn refund_documents_keep_finality_in_the_canonical_reference() {
    const FINALITY_LINK: &str = "task-user-refund.md#progress-arbitration-and-finality";

    assert!(USER_REFUND.contains("<a id=\"progress-arbitration-and-finality\"></a>"));
    assert!(V2_USER_REFUND.contains("<a id=\"progress-arbitration-and-finality\"></a>"));
    assert!(REFUND_ACTION_ROUTING.contains(FINALITY_LINK));
    assert!(REFUND_OUTPUT_TEMPLATES.contains(FINALITY_LINK));

    for implementation_term in [
        "Expired(8)",
        "Failed(9)",
        "job_asp_reject_expire",
        "sub_failed_notify",
        "uopData.executeResult",
    ] {
        assert!(!REFUND_ACTION_ROUTING.contains(implementation_term));
        assert!(!REFUND_OUTPUT_TEMPLATES.contains(implementation_term));
    }

    assert!(REFUND_ACTION_ROUTING.contains("payload.schemaVersion=2"));
    for binding in ["params.jobId", "params.operation", "params.refundContextId"] {
        assert!(REFUND_ACTION_ROUTING.contains(binding));
    }
}

#[test]
fn refund_render_contract_preserves_field_and_state_semantics() {
    let refund_section = V2_REFUND_DISPLAY;

    let mut previous = 0;
    for (field, source) in [
        ("`task_name`", "`payload.job.jobName`"),
        ("`job_id`", "`payload.job.jobId`"),
        ("`task_type`", "`payload.job.jobType`"),
        ("`service_provider`", "`payload.job.providerAgentId`"),
        ("`current_status`", "`payload.job.statusName`"),
        ("`payment_amount`", "`payload.payment.originalAmount`"),
    ] {
        let position = refund_section
            .find(field)
            .unwrap_or_else(|| panic!("missing refund display field {field}"));
        assert!(
            position >= previous,
            "refund display field order changed at {field}"
        );
        assert!(
            refund_section
                .lines()
                .any(|line| line.contains(field) && line.contains(source)),
            "refund display field {field} lost authoritative source {source}"
        );
        previous = position;
    }

    let reason_position = refund_section
        .find("`refund_reason`")
        .expect("missing verbatim refund reason row");
    let rules_position = refund_section
        .find("`refund_rules`")
        .expect("missing refund rules block");
    assert!(reason_position > previous);
    assert!(rules_position > reason_position);
    assert!(refund_section.contains("`refund_reason`"));
    assert!(refund_section.contains("`payload.request.userReason`"));
    assert!(refund_section.contains("localized `refund_task_details` heading"));
    assert!(refund_section.contains("Use only fields 1–6"));
    assert!(refund_section.contains("Do not add Service, deadline, receipt"));

    let mut previous_rule = 0;
    for semantic_rule in [
        "`provider_response`",
        "`full_original_payment`",
        "`onchain_confirmation`",
        "`progress_visibility`",
    ] {
        let position = refund_section
            .find(semantic_rule)
            .unwrap_or_else(|| panic!("missing refund semantic rule {semantic_rule}"));
        assert!(
            position >= previous_rule,
            "refund semantic rule order changed at {semantic_rule}"
        );
        previous_rule = position;
    }

    for settlement_state in [
        "Pending:",
        "Confirmed:",
        "No refundable payment:",
        "Incomplete:",
    ] {
        assert!(refund_section.contains(settlement_state));
    }
}
