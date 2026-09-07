const ROUTER: &str = include_str!("../../skills/okx-ai-v2/references/a2a/router.md");
const USER_ROUTER: &str = include_str!("../../skills/okx-ai-v2/references/a2a/user/router.md");
const PROVIDER_ROUTER: &str =
    include_str!("../../skills/okx-ai-v2/references/a2a/provider/router.md");
const ARBITRATION_DECISION: &str =
    include_str!("../../skills/okx-ai-v2/references/a2a/provider/arbitration-decision.md");
const DISPUTE: &str = include_str!("../../skills/okx-ai-v2/references/a2a/provider/dispute.md");
const ARBITRATION_QUERY: &str =
    include_str!("../../skills/okx-ai-v2/references/a2a/provider/arbitration-query.md");
const EVIDENCE_UPLOAD: &str =
    include_str!("../../skills/okx-ai-v2/references/a2a/provider/evidence-upload.md");
const NOTIFY: &str = include_str!("../../skills/okx-ai-v2/references/a2a/notify.md");
const OKX_AI_SKILL: &str = include_str!("../../skills/okx-ai-v2/SKILL.md");
const TASK_COMMON_SOURCE: &str = include_str!("../src/commands/agent_commerce/task/common/mod.rs");
const EVALUATOR_FLOW_SOURCE: &str =
    include_str!("../src/commands/agent_commerce/task/evaluator/flow.rs");
const EVALUATOR_INFO_SOURCE: &str =
    include_str!("../src/commands/agent_commerce/task/evaluator/info.rs");
const DISPUTE_LIFECYCLE_SOURCE: &str =
    include_str!("../src/commands/agent_commerce/task/user/flow_lifecycle/dispute.rs");
const PENDING_V2_SOURCE: &str =
    include_str!("../src/commands/agent_commerce/task/common/pending_v2.rs");
const ASP_DISPUTE_RAISE_SOURCE: &str =
    include_str!("../src/commands/agent_commerce/task/asp/dispute_raise.rs");
const ASP_SUBSCRIPTION_SOURCE: &str =
    include_str!("../src/commands/agent_commerce/task/asp/subscription.rs");
const ASP_FLOW_SOURCE: &str = include_str!("../src/commands/agent_commerce/task/asp/flow.rs");

#[test]
fn action_ids_are_partitioned_by_domain_and_role() {
    for action in ["login", "register_user_agent", "watch_task", "stop"] {
        assert!(
            ROUTER.contains(&format!("`{action}`")),
            "missing action {action}"
        );
    }
    for action in ["open_create_playbook", "submit_refund_request"] {
        assert!(
            USER_ROUTER.contains(&format!("`{action}`")),
            "missing user action {action}"
        );
    }
    for action in ["agree_refund", "raise_arbitration", "view_arbitration"] {
        assert!(
            PROVIDER_ROUTER.contains(&format!("`{action}`")),
            "missing provider action {action}"
        );
    }
    assert!(ROUTER.contains("Preserve `agentId`"));
    assert!(ROUTER.contains("Never substitute retired"));
}

#[test]
fn arbitration_decision_execution_and_query_are_separate() {
    assert!(ARBITRATION_DECISION.contains("pending-decisions-v2 request-prompt"));
    assert!(ARBITRATION_DECISION.contains("explicit instruction to arbitrate"));
    for action in [
        "agree_refund",
        "raise_arbitration",
        "sub_agree_refund",
        "raise_subscription_arbitration",
    ] {
        assert!(DISPUTE.contains(&format!("| `{action}` |")));
    }
    assert!(DISPUTE.contains("## Start arbitration directly"));
    let direct_flow = DISPUTE
        .split_once("## Start arbitration directly")
        .unwrap()
        .1
        .split_once("## Reason handoff")
        .unwrap()
        .0;
    assert!(direct_flow.contains("onchainos agent dispute raise <jobId>"));
    assert!(direct_flow.contains("onchainos agent subscribe-dispute <jobId>"));
    assert!(!direct_flow.contains("pending-decisions-v2 request-prompt"));
    assert!(ARBITRATION_QUERY.contains("tasks --status rejected"));
    assert!(ARBITRATION_QUERY.contains("arbitration-list"));
    assert!(ARBITRATION_QUERY.contains("arbitration-detail"));
}

#[test]
fn arbitration_reason_handoff_survives_leaf_split() {
    assert!(DISPUTE.contains("## Reason handoff"));
    assert!(DISPUTE.contains("[ARBITRATION_REASON_CONTEXT]"));
    assert!(DISPUTE.contains("--reason-b64 <reasonB64>"));
    assert!(DISPUTE.contains("resumeEvent=sub_asp_dispute"));
    assert!(DISPUTE.contains("arbitration_reason_context_missing"));
    assert!(EVIDENCE_UPLOAD.contains("[ARBITRATION_REASON_CONTEXT]"));
    assert!(EVIDENCE_UPLOAD.contains("taskType=subscription"));
    assert!(EVIDENCE_UPLOAD.contains("arbitration_reason_context_missing"));

    assert!(ASP_DISPUTE_RAISE_SOURCE.contains("common::okx_a2a::session_send"));
    assert!(ASP_DISPUTE_RAISE_SOURCE.contains("failed to hand off the arbitration reason"));
    assert!(
        ASP_DISPUTE_RAISE_SOURCE
            .find("common::okx_a2a::session_send")
            .unwrap()
            < ASP_DISPUTE_RAISE_SOURCE
                .find("signing::sign_uop_and_broadcast")
                .unwrap()
    );
    assert!(!ASP_DISPUTE_RAISE_SOURCE.contains("atomic_write"));
    assert!(!ASP_DISPUTE_RAISE_SOURCE.contains("task_state_dir"));

    assert!(ASP_FLOW_SOURCE.contains("[ARBITRATION_REASON_CONTEXT]"));
    assert!(ASP_FLOW_SOURCE.contains("--reason-b64"));
    assert!(ASP_FLOW_SOURCE.contains("arbitration_reason_context_missing"));
    assert!(!ASP_FLOW_SOURCE.contains("Use `--reason \"\"`"));

    let subscription_dispute = ASP_SUBSCRIPTION_SOURCE
        .split_once("pub async fn handle_dispute(")
        .unwrap()
        .1;
    assert!(subscription_dispute.contains("build_subscription_reason_handoff"));
    assert!(subscription_dispute.contains("common::okx_a2a::session_send"));
    assert!(subscription_dispute.contains("failed to hand off the arbitration reason"));
    assert!(
        subscription_dispute
            .find("common::okx_a2a::session_send")
            .unwrap()
            < subscription_dispute
                .find("signing::sign_uop_and_broadcast")
                .unwrap()
    );

    let subscription_event = ASP_FLOW_SOURCE
        .split_once("Event::SubAspDispute =>")
        .unwrap()
        .1;
    assert!(subscription_event.contains("[ARBITRATION_REASON_CONTEXT]"));
    assert!(subscription_event.contains("taskType` is `subscription`"));
    assert!(subscription_event.contains("arbitration_reason_context_missing"));
}

#[test]
fn task_and_arbitration_query_intents_use_distinct_leaves() {
    assert!(OKX_AI_SKILL.contains("references/a2a/router.md"));
    assert!(!USER_ROUTER.contains("../provider/"));
    assert!(PROVIDER_ROUTER.contains("List ASP tasks or saved deliverables"));
    assert!(PROVIDER_ROUTER.contains("Arbitration candidates, cases, or detail"));
    assert!(PROVIDER_ROUTER.contains("arbitration-query.md"));
    assert!(ARBITRATION_QUERY.contains(
        "onchainos agent tasks --status rejected --agent-id <aspAgentId> --page 1 --limit 20"
    ));
    assert!(ARBITRATION_QUERY
        .contains("onchainos agent my-subscriptions --role provider --status rejected"));
    assert!(ARBITRATION_QUERY.contains("onchainos agent arbitration-list --agent-id <aspAgentId>"));
    assert!(ARBITRATION_QUERY
        .contains("onchainos agent arbitration-detail <jobId> --agent-id <aspAgentId>"));
}

#[test]
fn cli_guidance_targets_role_scoped_v2_tree() {
    let legacy_prefix = ["skills/okx-ai", "/references/"].concat();
    for source in [
        PENDING_V2_SOURCE,
        EVALUATOR_FLOW_SOURCE,
        EVALUATOR_INFO_SOURCE,
        DISPUTE_LIFECYCLE_SOURCE,
        TASK_COMMON_SOURCE,
    ] {
        assert!(
            !source.contains(&legacy_prefix),
            "CLI guidance references the legacy skill tree"
        );
    }
    assert!(PENDING_V2_SOURCE.contains("skills/okx-ai-v2/SKILL.md"));
    assert!(TASK_COMMON_SOURCE.contains("references/a2a/router.md"));
    assert!(!TASK_COMMON_SOURCE.contains("references/a2a/user/session.md"));
    assert!(EVALUATOR_FLOW_SOURCE.contains("references/a2a/evaluator/rubric.md"));
    assert!(!EVALUATOR_FLOW_SOURCE.contains("references/a2a/evaluator/dispute.md"));
    assert!(DISPUTE_LIFECYCLE_SOURCE.contains("references/runtime/recovery.md"));
}

#[test]
fn notification_and_refund_actions_are_registered() {
    assert!(PROVIDER_ROUTER.contains("| `notify_user` |"));
    assert!(PROVIDER_ROUTER.contains("[`../notify.md`](../notify.md)"));
    assert!(NOTIFY.contains("For `notify_user`, notify once and end."));
    assert!(NOTIFY.contains("payload.notification.content"));
    assert!(NOTIFY.contains("onchainos agent user-notify"));

    for action in [
        "resolve_refund_target",
        "prepare_refund",
        "provide_refund_reason",
        "cancel_trial_conversion",
        "close_zero_price",
        "execute_direct_refund",
        "submit_refund_request",
        "view_refund_status",
    ] {
        assert!(
            USER_ROUTER.contains(&format!("`{action}`")),
            "missing Refund V2 action {action}"
        );
    }
    assert!(ROUTER.contains("payload.schemaVersion=2"));
    assert!(ROUTER.contains("refundContextId"));
    assert!(ROUTER.contains("Never substitute retired"));
}
