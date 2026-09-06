const ROUTER: &str = include_str!("../../skills/okx-ai-v2/references/a2a/router.md");
const USER_ROUTER: &str = include_str!("../../skills/okx-ai-v2/references/a2a/user/router.md");
const PROVIDER_ROUTER: &str = include_str!("../../skills/okx-ai-v2/references/a2a/provider/router.md");
const ARBITRATION_DECISION: &str = include_str!("../../skills/okx-ai-v2/references/a2a/provider/arbitration-decision.md");
const DISPUTE: &str = include_str!("../../skills/okx-ai-v2/references/a2a/provider/dispute.md");
const ARBITRATION_QUERY: &str = include_str!("../../skills/okx-ai-v2/references/a2a/provider/arbitration-query.md");
const TASK_COMMON_SOURCE: &str = include_str!("../src/commands/agent_commerce/task/common/mod.rs");
const AGENT_COMMERCE_SOURCE: &str = include_str!("../src/commands/agent_commerce/mod.rs");
const EVALUATOR_FLOW_SOURCE: &str = include_str!("../src/commands/agent_commerce/task/evaluator/flow.rs");

#[test]
fn action_ids_are_partitioned_by_domain_and_role() {
    for action in ["login", "register_user_agent", "watch_task", "stop"] {
        assert!(ROUTER.contains(&format!("`{action}`")), "missing action {action}");
    }
    for action in ["open_create_playbook", "submit_refund_request"] {
        assert!(USER_ROUTER.contains(&format!("`{action}`")), "missing user action {action}");
    }
    for action in ["agree_refund", "raise_arbitration", "view_arbitration"] {
        assert!(PROVIDER_ROUTER.contains(&format!("`{action}`")), "missing provider action {action}");
    }
    assert!(ROUTER.contains("Preserve `agentId`"));
    assert!(ROUTER.contains("Never substitute retired"));
}

#[test]
fn arbitration_decision_execution_and_query_are_separate() {
    assert!(ARBITRATION_DECISION.contains("pending-decisions-v2 request-prompt"));
    for action in ["agree_refund", "raise_arbitration", "sub_agree_refund", "raise_subscription_arbitration"] {
        assert!(DISPUTE.contains(&format!("| `{action}` |")));
    }
    assert!(ARBITRATION_QUERY.contains("tasks --status rejected"));
    assert!(ARBITRATION_QUERY.contains("arbitration-list"));
    assert!(ARBITRATION_QUERY.contains("arbitration-detail"));
}

#[test]
fn cli_guidance_targets_role_scoped_v2_tree() {
    assert!(TASK_COMMON_SOURCE.contains("references/a2a/router.md"));
    assert!(!TASK_COMMON_SOURCE.contains("references/a2a/user/session.md"));
    assert!(EVALUATOR_FLOW_SOURCE.contains("references/a2a/evaluator/rubric.md"));
    assert!(!EVALUATOR_FLOW_SOURCE.contains("references/a2a/evaluator/dispute.md"));
}

#[test]
fn stale_system_events_cannot_replay_terminal_side_effects() {
    assert!(ROUTER.contains("Call `next-action` exactly once"));
    assert!(ROUTER.contains("never authorizes a duplicate call"));
    assert!(AGENT_COMMERCE_SOURCE.contains("ignore this stale notification and end the turn immediately"));
    assert!(AGENT_COMMERCE_SOURCE.contains("do NOT call `next-action` again"));
    assert!(AGENT_COMMERCE_SOURCE.contains("do NOT replay terminal notification"));
    assert!(!AGENT_COMMERCE_SOURCE.contains("re-run next-action with the `event` field"));
    let stale_gate = AGENT_COMMERCE_SOURCE
        .find("Stop stale system events before any recovery or persistence")
        .unwrap();
    let delivery_recovery = AGENT_COMMERCE_SOURCE
        .find("For job_submitted: prefer an unprocessed spool delivery")
        .unwrap();
    assert!(stale_gate < delivery_recovery);
}
