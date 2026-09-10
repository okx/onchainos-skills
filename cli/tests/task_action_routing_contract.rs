const ROUTER: &str = include_str!("../../skills/okx-ai/references/a2a/router.md");
const USER_ROUTER: &str = include_str!("../../skills/okx-ai/references/a2a/user/router.md");
const TASK_QUERY: &str = include_str!("../../skills/okx-ai/references/a2a/task-query.md");
const PROVIDER_ROUTER: &str = include_str!("../../skills/okx-ai/references/a2a/provider/router.md");
const ARBITRATION_DECISION: &str =
    include_str!("../../skills/okx-ai/references/a2a/provider/arbitration-decision.md");
const REFUND_CONFIRM: &str =
    include_str!("../../skills/okx-ai/references/a2a/user/refund-confirm.md");
const REFUND_RECONCILE: &str =
    include_str!("../../skills/okx-ai/references/a2a/refund-reconcile.md");
const DISPUTE: &str = include_str!("../../skills/okx-ai/references/a2a/provider/dispute.md");
const ARBITRATION_QUERY: &str =
    include_str!("../../skills/okx-ai/references/a2a/provider/arbitration-query.md");
const EVIDENCE_UPLOAD: &str =
    include_str!("../../skills/okx-ai/references/a2a/provider/evidence-upload.md");
const PROVIDER_SUBSCRIPTION: &str =
    include_str!("../../skills/okx-ai/references/a2a/provider/subscription.md");
const SUBSCRIPTION_RATING: &str =
    include_str!("../../skills/okx-ai/references/a2a/user/rating.md");
const DUPLICATE_SUBSCRIPTION: &str =
    include_str!("../../skills/okx-ai/references/a2a/user/duplicate-subscription.md");
const NOTIFY: &str = include_str!("../../skills/okx-ai/references/a2a/notify.md");
const USER_REVIEW: &str = include_str!("../../skills/okx-ai/references/a2a/user/review.md");
const USER_SUBSCRIPTION: &str =
    include_str!("../../skills/okx-ai/references/a2a/user/subscription.md");
const OKX_AI_SKILL: &str = include_str!("../../skills/okx-ai/SKILL.md");
const TASK_COMMON_SOURCE: &str = include_str!("../src/commands/agent_commerce/task/common/mod.rs");
const EVALUATOR_FLOW_SOURCE: &str =
    include_str!("../src/commands/agent_commerce/task/evaluator/flow.rs");
const EVALUATOR_INFO_SOURCE: &str =
    include_str!("../src/commands/agent_commerce/task/evaluator/info.rs");
const EVALUATOR_DISPUTE_STATUS_SOURCE: &str =
    include_str!("../src/commands/agent_commerce/task/evaluator/dispute_status.rs");
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
const ASP_CONTENT_SOURCE: &str = include_str!("../src/commands/agent_commerce/task/asp/content.rs");
const USER_CONTENT_SOURCE: &str =
    include_str!("../src/commands/agent_commerce/task/user/content.rs");
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
fn creation_monitoring_query_tips_have_explicit_user_routes() {
    let lifecycle_route = "Ask about a task's progress, status, lifecycle, timeline, current stage, current responsible party, or next step | [`../task-query.md`](../task-query.md)";
    assert!(USER_ROUTER.contains(lifecycle_route));
    let details_route = "Explicitly ask for task details, basic information, attributes, type, fee, provider, description, or delivery content; list or inspect tasks, saved deliverables, pending evaluations, or tasks the User rejected | [`../task-query.md`](../task-query.md)";
    assert!(USER_ROUTER.contains(details_route));
    let subscription_trade_route = "Direct reply to the Runtime Watch creation-start note using `Check subscription task status` or its localized rendering; or query local follow-trade results for a subscription Signal by `jobId` or `deliveryId` | [`subscription-trade-records.md`](subscription-trade-records.md)";
    assert!(USER_ROUTER.contains(subscription_trade_route));
    let generic_subscription_route = "List, inspect, or manage a subscription | [`subscription.md`](subscription.md) or [`subscription-manage.md`](subscription-manage.md)";
    assert!(USER_ROUTER.contains(generic_subscription_route));
    assert!(USER_ROUTER.contains(
        "All other subscription lifecycle/status wording uses the\ngeneric subscription query"
    ));
    let direct_subscription_tip = USER_ROUTER
        .find("Direct reply to the Runtime Watch creation-start note")
        .unwrap();
    let generic_subscription_query = USER_ROUTER
        .find("List, inspect, or manage a subscription")
        .unwrap();
    assert!(direct_subscription_tip < generic_subscription_query);
}

#[test]
fn arbitration_decision_execution_and_query_are_separate() {
    assert!(ARBITRATION_DECISION.contains("Bind an event-created card"));
    assert!(ARBITRATION_DECISION.contains("card opened from a selected pending request"));
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
    let refund_confirmation = REFUND_CONFIRM
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");
    assert!(REFUND_CONFIRM.contains("reply “Submit refund request” and include your refund reason"));
    assert!(REFUND_CONFIRM.contains("both the submission intent and a refund reason"));
    assert!(REFUND_CONFIRM.contains("clear submission intent without a reason"));
    assert!(refund_confirmation.contains("The preceding `B` or rejection enters this confirmation"));
    assert!(refund_confirmation.contains("preserve it as a draft reason"));

    assert!(ARBITRATION_DECISION.contains("request platform evaluation"));
    assert!(ARBITRATION_DECISION.contains("include your evaluation reason"));
    assert!(ARBITRATION_DECISION.contains("`Request evaluation` with a non-blank reason"));
    assert!(ARBITRATION_DECISION.contains("`Request evaluation` with a missing reason"));
    assert!(ARBITRATION_DECISION.contains("Apply this response matrix to either binding"));
    assert!(ARBITRATION_DECISION.contains("complete Template 6.4"));
    assert!(ARBITRATION_DECISION.contains("Treat this Template 6.4 view as the ASP confirmation"));
    assert!(ARBITRATION_DECISION.contains("### Seller Refund Rejection"));
    assert!(ARBITRATION_DECISION.contains("卖方拒绝退款"));
    assert!(ARBITRATION_DECISION.contains("请补充申请评审的理由"));

    assert!(PENDING_V2_SOURCE.contains("both the submission intent and a refund reason"));
    assert!(PENDING_V2_SOURCE.contains("B never counts as submission intent"));
    assert!(PENDING_V2_SOURCE.contains("both the decision intent and any evaluation reason"));
    assert!(PENDING_V2_SOURCE.contains("Request evaluation: <verbatim reason>"));
    assert!(PENDING_V2_SOURCE.contains("Seller Refund Rejection field-list card"));
    assert!(PENDING_V2_SOURCE.contains("does not authorize Evaluation"));
    assert!(ASP_FLOW_SOURCE.contains("pending-decisions-v2 request-prompt"));
    assert!(ASP_FLOW_SOURCE.contains("BEGIN TEMPLATE 6.4 SOURCE"));
    assert!(ASP_FLOW_SOURCE.contains("--refund-display-b64"));
    assert!(ASP_DISPUTE_RAISE_SOURCE
        .contains("Ask me to view this task's details for the evaluation result."));
    assert!(!ASP_DISPUTE_RAISE_SOURCE.contains("Check: onchainos agent arbitration-detail"));
    assert!(ASP_SUBSCRIPTION_SOURCE
        .contains("Ask me to view this task's details for the evaluation result."));
    assert!(!ASP_SUBSCRIPTION_SOURCE.contains("Check: onchainos agent arbitration-detail"));
}

#[test]
fn single_record_views_use_field_lists_while_multi_record_queries_keep_tables() {
    let one_time_detail = TASK_QUERY
        .split_once("### One-time Job Details")
        .unwrap()
        .1
        .split_once("## Buyer refund tasks")
        .unwrap()
        .0;
    assert!(one_time_detail.contains("- Job Name: {title}"));
    assert!(!one_time_detail.contains("| Job Name |"));

    let refund_detail = TASK_QUERY
        .split_once("### Refund Request Details")
        .unwrap()
        .1;
    assert!(refund_detail.contains("- Service Name: {serviceName}"));
    assert!(refund_detail.contains("- Refund Result: {localizedStatusLabel}"));
    assert!(refund_detail.contains("- Result Description: {localizedStatusDescription}"));
    assert!(refund_detail.contains("- Evaluation Result: {localizedEvaluationResultDescription}"));
    assert!(refund_detail.contains("- Evaluation Reason: {localizedEvaluationReason}"));
    assert!(refund_detail.contains("Never treat the original `Reason for Refund` as an evaluation reason."));
    assert!(!refund_detail.contains("| Service Name |"));

    assert!(REFUND_CONFIRM.contains("- Job ID: {jobId}"));
    assert!(REFUND_CONFIRM.contains("- Refund Amount: {refundAmount}"));
    assert!(!REFUND_CONFIRM.contains("| Service Name |"));

    assert!(ARBITRATION_DECISION.contains("- Buyer’s Reason: {buyerReason}"));
    assert!(ARBITRATION_DECISION.contains("- Response Deadline: {responseDeadline}"));
    assert!(ARBITRATION_DECISION.contains("- Refund Status: {localizedStatusLabel}"));
    assert!(ARBITRATION_DECISION.contains("- Status Description: {localizedStatusDescription}"));
    assert!(!ARBITRATION_DECISION.contains("| Service Name |"));

    let evaluation_detail = ARBITRATION_QUERY
        .split_once("### Evaluation Details")
        .unwrap()
        .1;
    assert!(evaluation_detail.contains("- Evaluation Status: {localizedStatusLabel}"));
    assert!(evaluation_detail.contains("- Status Description: {localizedStatusDescription}"));
    assert!(evaluation_detail.contains("- Evaluation Result: {localizedVerdictDescription}"));
    assert!(!evaluation_detail.contains("- Evaluation Stage:"));
    assert!(!evaluation_detail.contains("- Task Status:"));
    assert!(!evaluation_detail.contains("| Service Name |"));

    assert!(TASK_QUERY
        .contains("| # | Service Name | Job ID | Task Type | Refund Amount | Response Deadline |"));
    assert!(ARBITRATION_QUERY.contains(
        "| # | Service Name | Job ID | Task Type | Requested Refund | Response Deadline |"
    ));
    assert!(ARBITRATION_QUERY
        .contains("| # | Service Name | Job ID | Status | Evaluation Started | Key Time |"));
}

#[test]
fn task_evaluation_and_refund_statuses_have_localized_business_meaning() {
    let task_query = TASK_QUERY.split_whitespace().collect::<Vec<_>>().join(" ");
    for value in [
        "证据准备中",
        "评审中",
        "已裁决",
        "尚未产生裁决",
        "用户胜诉，退款成功",
        "裁决结果暂无法识别",
        "评审状态暂不可用",
        "当前返回信息不足，暂无法确定评审状态",
        "证据自动收集中，请等候。",
    ] {
        assert!(
            ARBITRATION_QUERY.contains(value),
            "missing mapping: {value}"
        );
    }
    assert!(task_query.contains("raw `statusName` remains a protocol compatibility key"));
    assert!(TASK_QUERY.contains("`Refund completed` -> `退款成功`"));
    assert!(TASK_QUERY.contains("`Refund not issued` -> `未退款`"));
    assert!(REFUND_RECONCILE.contains("displayed business result is the localized"));
    assert!(ASP_FLOW_SOURCE.contains("Evaluation status: Evidence preparation"));
    assert!(DISPUTE_LIFECYCLE_SOURCE.contains("Evaluation status: Evidence preparation"));
    assert!(ASP_CONTENT_SOURCE.contains("Evaluation Status: Decided"));
    assert!(USER_CONTENT_SOURCE.contains("Evaluation status: Decided"));
    for raw_outcome in ["Outcome: ASPWins", "Outcome: ClientWins"] {
        assert!(!ASP_CONTENT_SOURCE.contains(raw_outcome));
        assert!(!USER_CONTENT_SOURCE.contains(raw_outcome));
    }
}

#[test]
fn user_facing_statuses_use_cli_labels_across_task_subscription_and_rating_flows() {
    assert!(OKX_AI_SKILL.contains("CLI-provided `statusLabel` and `statusDescription`"));
    assert!(OKX_AI_SKILL.contains("Never render raw state fields"));

    assert!(TASK_QUERY.contains("`Awaiting ASP acceptance` as `ASP 待接单`"));
    assert!(TASK_QUERY.contains("`Refund completed` as `退款成功`"));

    assert!(PROVIDER_SUBSCRIPTION.contains("{localizedStatusLabel}"));
    assert!(PROVIDER_SUBSCRIPTION.contains("Do not display raw `status`"));
    assert!(!PROVIDER_SUBSCRIPTION.contains("render CLI `statusName` verbatim"));

    assert!(SUBSCRIPTION_RATING.contains("<localizedStatusLabel>"));
    assert!(SUBSCRIPTION_RATING.contains("do not display raw `status`"));
    assert!(!SUBSCRIPTION_RATING.contains("render `statusName` verbatim"));

    assert!(DUPLICATE_SUBSCRIPTION.contains("payload.statusLabel"));
    assert!(DUPLICATE_SUBSCRIPTION.contains("payload.statusDescription"));
    assert!(DUPLICATE_SUBSCRIPTION.contains("do not display a raw numeric `payload.status`"));
    assert!(!DUPLICATE_SUBSCRIPTION.contains("Map `payload.status` for display"));

    assert!(EVALUATOR_DISPUTE_STATUS_SOURCE.contains("Task status: {}"));
    assert!(EVALUATOR_DISPUTE_STATUS_SOURCE.contains("Evaluation round status: {}"));
    assert!(!EVALUATOR_DISPUTE_STATUS_SOURCE.contains("taskStatus   : {} ({})"));
    assert!(!EVALUATOR_DISPUTE_STATUS_SOURCE.contains("dispute_round_status: {} ({})"));
}

#[test]
fn pending_refund_list_uses_only_current_rows_and_action_guidance() {
    assert!(PROVIDER_ROUTER.contains("first query the rejected-task set"));
    assert!(ARBITRATION_QUERY.contains("the current rejected-task set"));
    assert!(ARBITRATION_QUERY.contains("refund-list --role provider --scope requested"));
    assert!(ARBITRATION_QUERY.contains(
        "Render only this table followed by its action guidance. Do not add a count"
    ));
    assert!(ARBITRATION_QUERY.contains(
        "from a separate agent or query."
    ));
    assert!(ARBITRATION_QUERY.contains(
        "Translate every user-facing table header and the action guidance into the"
    ));
    assert!(ARBITRATION_QUERY.contains(
        "Reply with the number or Job ID to view details, then select \"Approve Refund\" or \"Request Review\"."
    ));
    assert!(!ARBITRATION_QUERY.contains("You have {pendingCount} refund requests"));
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
    assert!(TASK_QUERY.contains("call `okx-a2a user list` or\n`outdated-list` before rendering"));
    assert!(TASK_QUERY.contains("start or resume a watch"));
    assert!(USER_SUBSCRIPTION.contains("## Status-query handoff"));
    assert!(USER_SUBSCRIPTION.contains(
        "Do not run `subscription-list`, `subscribe-detail`, or a one-time timeline"
    ));

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
    assert!(USER_ROUTER
        .contains("| `provide_refund_reason` | [`refund-confirm.md`](refund-confirm.md) |"));
    assert!(ROUTER.contains("payload.schemaVersion=2"));
    assert!(ROUTER.contains("refundContextId"));
    assert!(ROUTER.contains("Never substitute retired"));
}
