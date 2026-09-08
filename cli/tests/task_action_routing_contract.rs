const ROUTER: &str = include_str!("../../skills/okx-ai/references/a2a/router.md");
const USER_ROUTER: &str = include_str!("../../skills/okx-ai/references/a2a/user/router.md");
const TASK_QUERY: &str = include_str!("../../skills/okx-ai/references/a2a/task-query.md");
const PROVIDER_ROUTER: &str = include_str!("../../skills/okx-ai/references/a2a/provider/router.md");
const ARBITRATION_DECISION: &str =
    include_str!("../../skills/okx-ai/references/a2a/provider/arbitration-decision.md");
const REFUND_CONFIRM: &str =
    include_str!("../../skills/okx-ai/references/a2a/user/refund-confirm.md");
const DISPUTE: &str = include_str!("../../skills/okx-ai/references/a2a/provider/dispute.md");
const ARBITRATION_QUERY: &str =
    include_str!("../../skills/okx-ai/references/a2a/provider/arbitration-query.md");
const EVIDENCE_UPLOAD: &str =
    include_str!("../../skills/okx-ai/references/a2a/provider/evidence-upload.md");
const NOTIFY: &str = include_str!("../../skills/okx-ai/references/a2a/notify.md");
const USER_REVIEW: &str = include_str!("../../skills/okx-ai/references/a2a/user/review.md");
const OKX_AI_SKILL: &str = include_str!("../../skills/okx-ai/SKILL.md");
const TASK_COMMON_SOURCE: &str = include_str!("../src/commands/agent_commerce/task/common/mod.rs");
const EVALUATOR_FLOW_SOURCE: &str =
    include_str!("../src/commands/agent_commerce/task/evaluator/flow.rs");
const EVALUATOR_INFO_SOURCE: &str =
    include_str!("../src/commands/agent_commerce/task/evaluator/info.rs");
const DISPUTE_LIFECYCLE_SOURCE: &str =
    include_str!("../src/commands/agent_commerce/task/user/flow_lifecycle/dispute.rs");
const USER_MANAGE_SOURCE: &str =
    include_str!("../src/commands/agent_commerce/task/user/flow_lifecycle/manage.rs");
const PENDING_V2_SOURCE: &str =
    include_str!("../src/commands/agent_commerce/task/common/pending_v2.rs");
const ASP_DISPUTE_RAISE_SOURCE: &str =
    include_str!("../src/commands/agent_commerce/task/asp/dispute_raise.rs");
const ASP_SUBSCRIPTION_SOURCE: &str =
    include_str!("../src/commands/agent_commerce/task/asp/subscription.rs");
const ASP_FLOW_SOURCE: &str = include_str!("../src/commands/agent_commerce/task/asp/flow.rs");
const REFUND_LIST_SOURCE: &str = include_str!("../src/commands/agent_commerce/task/refund_list.rs");

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
    assert!(ARBITRATION_DECISION.contains("For an event-created card"));
    assert!(ARBITRATION_DECISION.contains("card opened directly"));
    assert!(ARBITRATION_DECISION.contains("refund-detail"));
    for action in [
        "agree_refund",
        "raise_arbitration",
        "sub_agree_refund",
        "raise_subscription_arbitration",
    ] {
        assert!(DISPUTE.contains(&format!("| `{action}` |")));
    }
    assert!(DISPUTE.contains("onchainos agent dispute raise <params.jobId>"));
    assert!(DISPUTE.contains("onchainos agent subscribe-dispute <params.jobId>"));
    assert!(DISPUTE.contains("approveAndCreateDispute"));
    assert!(!DISPUTE.contains("dispute confirm"));
    assert!(ARBITRATION_QUERY.contains("refund-list --role provider --scope requested"));
    assert!(ARBITRATION_QUERY.contains("refund-detail <jobId> --role provider"));
    assert!(ARBITRATION_QUERY.contains("arbitration-list"));
    assert!(ARBITRATION_QUERY.contains("arbitration-detail"));
}

#[test]
fn refund_and_evaluation_prompts_collect_inline_or_missing_reasons() {
    assert!(REFUND_CONFIRM.contains("reply “Submit refund request” and include your refund reason"));
    assert!(REFUND_CONFIRM.contains("both the submission intent and a refund reason"));
    assert!(REFUND_CONFIRM.contains("clear submission intent without a reason"));

    assert!(ARBITRATION_DECISION.contains("request platform evaluation"));
    assert!(ARBITRATION_DECISION.contains("include your evaluation reason"));
    assert!(ARBITRATION_DECISION.contains("both the intent and an evaluation reason"));
    assert!(ARBITRATION_DECISION.contains("intent without a reason"));

    assert!(PENDING_V2_SOURCE.contains("both the submission intent and a refund reason"));
    assert!(PENDING_V2_SOURCE.contains("both the decision intent and any evaluation reason"));
    assert!(PENDING_V2_SOURCE.contains("Request evaluation: <verbatim reason>"));
}

#[test]
fn arbitration_reason_handoff_survives_leaf_split() {
    assert!(DISPUTE.contains("## Reason handoff"));
    assert!(DISPUTE.contains("[ARBITRATION_REASON_CONTEXT]"));
    assert!(DISPUTE.contains("\"reasonB64\":\"<URL-safe base64>\""));
    assert!(DISPUTE.contains("\"resumeEvent\":\"job_disputed\""));
    assert!(DISPUTE.contains("\"resumeEvent\":\"sub_asp_dispute\""));
    assert!(EVIDENCE_UPLOAD.contains("[ARBITRATION_REASON_CONTEXT]"));
    assert!(EVIDENCE_UPLOAD.contains("task type"));
    assert!(EVIDENCE_UPLOAD.contains("resume event"));
    assert!(EVIDENCE_UPLOAD.contains("arbitration_reason_context_missing"));

    assert!(ASP_DISPUTE_RAISE_SOURCE.contains("common::okx_a2a::session_send"));
    assert!(ASP_DISPUTE_RAISE_SOURCE.contains("failed to hand off the evaluation reason"));
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
    assert!(ASP_FLOW_SOURCE.contains("taskType` is `one_time`"));
    assert!(ASP_FLOW_SOURCE.contains("taskType` is `subscription`"));
    assert!(ASP_FLOW_SOURCE.contains("resumeEvent` is `job_disputed`"));
    assert!(ASP_FLOW_SOURCE.contains("resumeEvent` is `sub_asp_dispute`"));
    assert!(ASP_FLOW_SOURCE.contains("arbitration_reason_context_missing"));
    assert!(!ASP_FLOW_SOURCE.contains("Use `--reason \"\"`"));

    let subscription_dispute = ASP_SUBSCRIPTION_SOURCE
        .split_once("pub async fn handle_dispute(")
        .unwrap()
        .1;
    assert!(subscription_dispute.contains("build_subscription_reason_handoff"));
    assert!(subscription_dispute.contains("common::okx_a2a::session_send"));
    assert!(subscription_dispute.contains("failed to hand off the evaluation reason"));
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
fn task_and_evaluation_query_intents_use_distinct_leaves() {
    assert!(OKX_AI_SKILL.contains("references/a2a/router.md"));
    assert!(!USER_ROUTER.contains("../provider/"));
    assert!(USER_ROUTER.contains("pending evaluations, or tasks the User rejected"));
    assert!(TASK_QUERY.contains(
        "onchainos agent refund-list --role buyer --scope available --agent-id <userAgentId>"
    ));
    assert!(TASK_QUERY.contains(
        "onchainos agent refund-list --role buyer --scope requested --agent-id <userAgentId>"
    ));
    assert!(TASK_QUERY.contains("onchainos agent refund-detail <jobId> --role buyer"));
    assert!(PROVIDER_ROUTER.contains("List ASP tasks or saved deliverables"));
    assert!(PROVIDER_ROUTER.contains("Pending, available, required, or in-progress evaluations"));
    assert!(PROVIDER_ROUTER.contains("arbitration-query.md"));
    assert!(ARBITRATION_QUERY.contains(
        "onchainos agent refund-list --role provider --scope requested --agent-id <aspAgentId>"
    ));
    assert!(ARBITRATION_QUERY.contains("onchainos agent refund-detail <jobId> --role provider"));
    assert!(ARBITRATION_QUERY.contains("onchainos agent arbitration-list --agent-id <aspAgentId>"));
    assert!(ARBITRATION_QUERY
        .contains("onchainos agent arbitration-detail <jobId> --agent-id <aspAgentId>"));
    assert!(REFUND_LIST_SOURCE.contains("RefundListScope::Available.one_time_status()"));
    assert!(REFUND_LIST_SOURCE.contains("RefundListScope::Requested.subscription_status()"));
}

#[test]
fn submitted_one_time_status_recovers_and_displays_the_review_card_directly() {
    assert!(TASK_QUERY.contains("Task type: one_time"));
    assert!(TASK_QUERY.contains("Task status: submitted"));
    assert!(!TASK_QUERY.contains("onchainos agent next-action"));
    assert!(
        TASK_QUERY.contains("onchainos agent task-deliverable-list --job-id <jobId> --role user")
    );
    assert!(TASK_QUERY.contains("onchainos agent pending-decisions-v2 request"));
    assert!(TASK_QUERY.contains("buyer-review:<jobId>:job_submitted"));
    assert!(TASK_QUERY.contains("immediately append the exact same localized"));
    assert!(TASK_QUERY.contains("`okx-a2a user list`, `outdated-list`, or `watch`"));

    assert!(USER_REVIEW.contains("okx-a2a user list --job-id <jobId> --all-providers --json"));
    assert!(USER_REVIEW.contains("idempotencyKey` exactly"));
    assert!(USER_REVIEW.contains("okx-a2a user check --todo-ids <id> --json"));
}

#[test]
fn cli_guidance_targets_role_scoped_skill_tree() {
    let retired_prefix = ["skills/okx-ai-v", "2/references/"].concat();
    for source in [
        PENDING_V2_SOURCE,
        EVALUATOR_FLOW_SOURCE,
        EVALUATOR_INFO_SOURCE,
        DISPUTE_LIFECYCLE_SOURCE,
        TASK_COMMON_SOURCE,
    ] {
        assert!(
            !source.contains(&retired_prefix),
            "CLI guidance references the retired skill tree"
        );
    }
    assert!(PENDING_V2_SOURCE.contains("skills/okx-ai/SKILL.md"));
    assert!(TASK_COMMON_SOURCE.contains("references/a2a/user/router.md"));
    assert!(TASK_COMMON_SOURCE.contains("references/a2a/provider/router.md"));
    assert!(TASK_COMMON_SOURCE.contains("references/a2a/evaluator/router.md"));
    assert!(!TASK_COMMON_SOURCE.contains("=> \"references/a2a/router.md\""));
    assert!(!TASK_COMMON_SOURCE.contains("references/a2a/user/session.md"));
    assert!(USER_MANAGE_SOURCE.contains(
        "read `skills/okx-ai/references/a2a/user/subscription-manage.md` §Signal-receipt watch entry directly"
    ));
    assert!(USER_MANAGE_SOURCE.contains(
        "Route `nextAction.id=watch_task` directly to `skills/okx-ai/references/runtime/watch.md`"
    ));
    assert!(EVALUATOR_FLOW_SOURCE.contains("references/a2a/evaluator/rubric.md"));
    assert!(EVALUATOR_FLOW_SOURCE
        .contains("Step 3 — Read `skills/okx-ai/references/a2a/evaluator/rubric.md` directly"));
    assert!(!EVALUATOR_FLOW_SOURCE.contains("Step 3 — Enter through `skills/okx-ai/SKILL.md`"));
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
            "missing Refund action {action}"
        );
    }
    assert!(ROUTER.contains("payload.schemaVersion=2"));
    assert!(ROUTER.contains("refundContextId"));
    assert!(ROUTER.contains("Never substitute retired"));
}
