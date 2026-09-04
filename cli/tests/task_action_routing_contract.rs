const ACTION_ROUTING: &str = include_str!("../../skills/okx-ai/references/task-action-routing.md");
const ARBITRATION_REFERENCE: &str =
    include_str!("../../skills/okx-ai/references/task-arbitration.md");
const TASK_INTENT_ROUTING: &str =
    include_str!("../../skills/okx-ai/references/task-user-intent-routing.md");
const OKX_AI_SKILL: &str = include_str!("../../skills/okx-ai/SKILL.md");
const ARBITRATION_SOURCE: &str = include_str!("../src/commands/agent_commerce/task/arbitration.rs");

#[test]
fn arbitration_actions_have_one_domain_registry() {
    for action in [
        "agree_refund",
        "raise_arbitration",
        "sub_agree_refund",
        "raise_subscription_arbitration",
        "view_arbitration",
    ] {
        assert!(ARBITRATION_REFERENCE.contains(&format!("| `{action}` |")));
        assert!(!ACTION_ROUTING.contains(&format!("| `{action}` |")));
    }
    assert!(ARBITRATION_REFERENCE.contains("arbitration-list"));
    assert!(ARBITRATION_REFERENCE.contains("arbitration-detail"));
    assert!(
        ARBITRATION_REFERENCE.find("## Action routing").unwrap()
            < ARBITRATION_REFERENCE
                .find("## Open the rejection decision")
                .unwrap()
    );
    assert!(
        ARBITRATION_REFERENCE.find("## Output templates").unwrap()
            > ARBITRATION_REFERENCE
                .find("## Existing lifecycle handoff")
                .unwrap()
    );
    let arbitration_lower = ARBITRATION_REFERENCE.to_ascii_lowercase();
    for reverse_instruction in [
        "do not",
        "don't",
        "never",
        "must not",
        "does not need to",
        "need not",
    ] {
        assert!(
            !arbitration_lower.contains(reverse_instruction),
            "arbitration reference should use positive steps instead of `{reverse_instruction}`"
        );
    }
    let output_templates = ARBITRATION_REFERENCE
        .split_once("## Output templates")
        .unwrap()
        .1;
    assert!(!output_templates.to_ascii_lowercase().contains("match "));
    for intent in [
        "### Decide refund or arbitration",
        "### View arbitration cases",
        "### Confirm a case",
        "### View a case",
        "### Show refund result",
        "### Show arbitration started",
        "### Show blocked result",
    ] {
        assert!(output_templates.contains(intent));
    }
    assert!(!ARBITRATION_REFERENCE
        .contains("tasks --status disputed` and generic `status` are the public"));
    assert!(ARBITRATION_SOURCE.contains("pub fn build_decision_result"));
    assert!(ARBITRATION_SOURCE.contains("pub async fn handle_arbitration_list"));
    assert!(ARBITRATION_SOURCE.contains("pub async fn handle_arbitration_detail"));
    for obsolete in [
        "src/commands/agent_commerce/task/common/dispute.rs",
        "src/commands/agent_commerce/task/common/arbitration.rs",
        "src/commands/agent_commerce/task/common/arbitration_query.rs",
    ] {
        assert!(!std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join(obsolete)
            .exists());
    }
    for obsolete_reference in [
        "../skills/okx-ai/references/task-dispute.md",
        "../skills/okx-ai/references/task-arbitration-action-routing.md",
        "../skills/okx-ai/references/task-arbitration-output-templates.md",
    ] {
        assert!(!std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join(obsolete_reference)
            .exists());
    }
}

#[test]
fn task_and_arbitration_query_intents_use_distinct_commands() {
    assert!(TASK_INTENT_ROUTING
        .contains("onchainos agent tasks --agent-id <aspAgentId> --page 1 --limit 20"));
    assert!(TASK_INTENT_ROUTING.contains(
        "onchainos agent tasks --status rejected --agent-id <aspAgentId> --page 1 --limit 20"
    ));
    assert!(TASK_INTENT_ROUTING
        .contains("An ASP merchant can start arbitration for a task in `rejected` status"));
    assert!(
        OKX_AI_SKILL.contains("tasks that can be arbitrated (`哪些可以仲裁` / `可以仲裁的任务`)")
    );
    assert!(TASK_INTENT_ROUTING.contains("`which tasks can I arbitrate`"));
    assert!(ARBITRATION_REFERENCE.contains("## Query arbitration cases"));
    assert!(!ARBITRATION_REFERENCE.contains("task-user-intent-routing.md"));
    assert!(ARBITRATION_REFERENCE.contains("## Query an arbitration detail"));
    assert!(ARBITRATION_REFERENCE
        .contains("onchainos agent arbitration-list --agent-id <selectedAgentId>"));
    assert!(ARBITRATION_REFERENCE
        .contains("onchainos agent arbitration-detail <jobId> --agent-id <selectedAgentId>"));
}
