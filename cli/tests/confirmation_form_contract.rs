const PUBLISH_ACTIONS: &str = include_str!("../../skills/okx-ai-v2/references/a2a/user/create.md");
const USER_PLAYBOOK: &str = include_str!("../../skills/okx-ai-v2/references/a2a/user/playbook.md");
const USER_REFUND: &str = include_str!("../../skills/okx-ai-v2/references/a2a/user/refund.md");
const REFUND_ACTION_ROUTING: &str =
    include_str!("../../skills/okx-ai-v2/references/shared/task-action-routing.md");
const REFUND_OUTPUT_TEMPLATES: &str =
    include_str!("../../skills/okx-ai-v2/references/shared/task-output-templates.md");
const CANONICAL_SKILL: &str = include_str!("../../skills/okx-ai-v2/SKILL.md");
const USER_INTENT_ROUTER: &str =
    include_str!("../../skills/okx-ai-v2/references/a2a/user/router.md");
const A2A_CORE_ROUTER: &str = include_str!("../../skills/okx-ai-v2/references/a2a/core.md");
const IDENTITY_SEARCH: &str = include_str!("../../skills/okx-ai-v2/references/identity/search.md");
const RUNTIME_ROUTER: &str = include_str!("../../skills/okx-ai-v2/references/runtime/router.md");
const CHAT_COMM_INIT: &str =
    include_str!("../../skills/okx-ai-v2/references/shared/chat-comm-init.md");
const COMPLETION_ACTIONS: &str =
    include_str!("../../skills/okx-ai-v2/references/a2a/completion.md");
const V2_USER_ROUTER: &str = USER_INTENT_ROUTER;
const V2_SKILL: &str = CANONICAL_SKILL;

#[test]
fn v2_task_entry_keeps_two_level_lazy_routing_contract() {
    assert!(CANONICAL_SKILL.contains("references/runtime/router.md"));
    assert!(!CANONICAL_SKILL.contains("references/runtime/README.md"));
    assert!(CANONICAL_SKILL.contains("Select exactly one row"));
    assert!(CANONICAL_SKILL.contains("never scan Skill directories"));
    assert!(!CANONICAL_SKILL.contains("§Activation"));

    assert!(USER_INTENT_ROUTER.contains("Read only [`../../identity/search.md`]"));
    assert!(USER_INTENT_ROUTER.contains("nextAction.id=open_create_playbook"));
    assert!(IDENTITY_SEARCH.contains("Only now read `output-templates.md`"));
    assert!(PUBLISH_ACTIONS.contains("read only\n[`../../runtime/watch.md`]"));
    assert!(!PUBLISH_ACTIONS.contains("task-output-templates.md"));

    assert!(A2A_CORE_ROUTER.split_whitespace().count() <= 500);
    assert!(!A2A_CORE_ROUTER.contains("task-output-templates.md"));
    assert!(A2A_CORE_ROUTER.contains("legacy\nSkill-read trigger strings"));
    for leaf in [
        "watch.md",
        "backlog.md",
        "../shared/chat-comm-init.md",
        "attachment.md",
        "recovery.md",
        "cli-reference.md",
    ] {
        assert!(RUNTIME_ROUTER.contains(leaf));
    }
    assert!(CHAT_COMM_INIT.contains("okx-a2a doctor --fix --json"));
    assert!(PUBLISH_ACTIONS.contains("../../shared/chat-comm-init.md"));
}

#[test]
fn completion_preserves_terminal_marker_and_skips_same_owner_rating() {
    let contract = COMPLETION_ACTIONS
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");
    assert!(COMPLETION_ACTIONS.contains("[onchainos:task-terminal]"));
    assert!(contract.contains("preserve that exact prefix byte-for-byte"));
    assert!(COMPLETION_ACTIONS.contains("payload.rating.required=false"));
    assert!(contract.contains("Do not call `feedback-submit`"));
}
const V2_USER_REFUND: &str = include_str!("../../skills/okx-ai-v2/references/a2a/user/refund.md");
const V2_REFUND_DISPLAY: &str =
    include_str!("../../skills/okx-ai-v2/references/a2a/user/refund-display.md");
const V2_USER_RATING: &str = include_str!("../../skills/okx-ai-v2/references/a2a/user/rating.md");

#[test]
fn skill_confirmation_templates_never_expose_execution_configuration() {
    let publish_contract = PUBLISH_ACTIONS
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");
    let playbook_contract = USER_PLAYBOOK
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");
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

    assert!(publish_contract.contains("Do not append or merge any other row"));
    assert!(publish_contract.contains("list them below the table; never add an Attachments row"));
    assert!(publish_contract.contains("Guide-defined Consent and Signal values"));
    assert!(PUBLISH_ACTIONS.contains("--guide-consent-json"));
    assert!(!PUBLISH_ACTIONS.contains("`--autotrade-*` arguments"));
    assert!(publish_contract.contains("that returned form is the sole field authority"));
    assert!(publish_contract.contains(
        "Appendix A is only a fallback render contract for a direct route"
    ));
    assert!(playbook_contract
        .contains("derives and locally validates a projection from the selected service Guide"));
    assert!(USER_PLAYBOOK.contains("ASP supplies Guide text only"));
    assert!(!USER_PLAYBOOK.contains("Signal handling mode"));
    assert!(!USER_PLAYBOOK.contains("autoTradeConfigRequested"));
    assert!(playbook_contract.contains(
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
    assert!(USER_PLAYBOOK.contains("refund.md"));
}

#[test]
fn deliverable_review_b_reason_is_the_scoped_direct_rejection_confirmation() {
    let refund_contract = USER_REFUND.split_whitespace().collect::<Vec<_>>().join(" ");
    assert!(USER_REFUND.contains(
        "B` together with a non-blank\n\
User-authored reason is the User's final confirmation"
    ));
    assert!(USER_REFUND.contains("Handle this reply in the current user conversation"));
    assert!(USER_REFUND.contains("refund_request_confirmation_required"));
    assert!(USER_REFUND.contains("returned `nextAction.id=submit_refund_request`"));
    assert!(USER_REFUND.contains("owns reason extraction, fresh preparation, execution, and"));
    assert!(USER_REFUND.contains("give one concise localized\nconfirmation"));
    assert!(refund_contract.contains("Describe it as submitted rather than settled"));
    assert!(USER_REFUND.contains(
        "onchainos agent status <jobId> --agent-id <buyerAgentId>"
    ));
    assert!(USER_REFUND.contains("a reason\nreceived outside that active card remains input only"));
    assert!(REFUND_ACTION_ROUTING.contains("active deliverable-review"));
    assert!(REFUND_OUTPUT_TEMPLATES.contains("active deliverable-review"));
}

#[test]
fn v2_okx_ai_skill_routes_refunds_to_one_contract() {
    assert!(CANONICAL_SKILL.contains("references/a2a/user/router.md"));
    assert!(USER_INTENT_ROUTER.contains("[`refund.md`](refund.md)"));
    assert!(USER_INTENT_ROUTER.contains("paid-deliverable rejection"));
    assert!(!USER_INTENT_ROUTER.contains("refunds are not yet migrated"));
    assert!(V2_USER_REFUND.contains("[Refund Presentation](refund-display.md)"));
    for canonical_reference in [
        "references/a2a/core.md",
        "references/shared/task-action-routing.md",
    ] {
        assert!(CANONICAL_SKILL.contains(canonical_reference));
    }
    assert!(!CANONICAL_SKILL.contains("references/shared/task-output-templates.md"));
    assert!(!CANONICAL_SKILL.contains("../okx-ai/"));
}

#[test]
fn refund_documents_keep_finality_in_the_canonical_reference() {
    const FINALITY_LINK: &str = "../a2a/user/refund.md#progress-arbitration-and-finality";

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

#[test]
fn v2_buyer_rating_is_bound_to_one_active_subscription() {
    assert!(V2_USER_ROUTER
        .contains("| Rate or review an active subscription | [`rating.md`](rating.md) |"));
    assert!(V2_SKILL.contains("Buyer rating or review of an Active subscription"));
    assert!(!V2_USER_ROUTER.contains("## Rate an active subscription"));
    assert!(!USER_PLAYBOOK.contains("| Rate |"));

    let list = V2_USER_RATING
        .find("onchainos agent my-tasks --task-type subscription --status-type 1 --page 1")
        .expect("missing Active subscription selection");
    let duplicate_check = V2_USER_RATING
        .find("onchainos agent task-feedback")
        .expect("missing duplicate-rating guard");
    let submit = V2_USER_RATING
        .find("onchainos agent feedback-submit")
        .expect("missing rating submission");
    assert!(list < duplicate_check && duplicate_check < submit);

    let contract = V2_USER_RATING
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");

    for binding in [
        "--agent-id PROVIDER_AGENT_ID_ARG",
        "--creator-id BUYER_AGENT_ID_ARG",
        "--score SCORE_ARG",
        "--task-id JOB_ID_ARG",
        "--description REVIEW_ARG",
    ] {
        assert!(
            V2_USER_RATING.contains(binding),
            "missing rating binding {binding}"
        );
    }

    for invariant in [
        "selected row's `jobId`, `buyerAgentId`, and `providerAgentId`",
        "following unreviewed orders. Please select the order you want to review.",
        "Preserve pagination, continuing only while `hasNext=true`",
        "Populate every table cell from its returned row",
        "render `statusName` verbatim",
        "A non-empty `data[]` means the Buyer already rated",
        "Never draft, infer, translate, or rewrite the description",
        "do not request another confirmation",
        "Only `ok=true` with a non-empty `data.txHash` proves success",
        "do not claim that the rating succeeded",
    ] {
        assert!(
            contract.contains(invariant),
            "missing rating invariant {invariant}"
        );
    }

    let mut previous = 0;
    for field in ["- Task ID:", "- Score:", "- Review:", "- Transaction hash:"] {
        let position = V2_USER_RATING
            .find(field)
            .unwrap_or_else(|| panic!("missing rating result field {field}"));
        assert!(
            position >= previous,
            "rating result field order changed at {field}"
        );
        previous = position;
    }
}
