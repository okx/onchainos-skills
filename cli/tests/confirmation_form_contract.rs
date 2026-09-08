const SKILL: &str = include_str!("../../skills/okx-ai/SKILL.md");
const ROUTER: &str = include_str!("../../skills/okx-ai/references/a2a/router.md");
const USER_ROUTER: &str = include_str!("../../skills/okx-ai/references/a2a/user/router.md");
const PROVIDER_ROUTER: &str = include_str!("../../skills/okx-ai/references/a2a/provider/router.md");
const EVALUATOR_ROUTER: &str =
    include_str!("../../skills/okx-ai/references/a2a/evaluator/router.md");
const IDENTITY_SEARCH: &str = include_str!("../../skills/okx-ai/references/identity/search.md");
const PREPARE: &str = include_str!("../../skills/okx-ai/references/a2a/user/create-prepare.md");
const CREATE: &str = include_str!("../../skills/okx-ai/references/a2a/user/create.md");
const GUIDE: &str = include_str!("../../skills/okx-ai/references/a2a/user/create-guide.md");
const SUBSCRIPTION_CREATE: &str =
    include_str!("../../skills/okx-ai/references/a2a/user/subscription-create.md");
const REFUND_PREPARE: &str =
    include_str!("../../skills/okx-ai/references/a2a/user/refund-prepare.md");
const REFUND_CONFIRM: &str =
    include_str!("../../skills/okx-ai/references/a2a/user/refund-confirm.md");
const REFUND_EXECUTE: &str =
    include_str!("../../skills/okx-ai/references/a2a/user/refund-execute.md");
const REFUND_CONTRACT: &str =
    include_str!("../../skills/okx-ai/references/shared/refund-contract.md");
const TASK_QUERY: &str = include_str!("../../skills/okx-ai/references/a2a/task-query.md");
const PROVIDER_ARBITRATION_QUERY: &str =
    include_str!("../../skills/okx-ai/references/a2a/provider/arbitration-query.md");
const PROVIDER_ARBITRATION_DECISION: &str =
    include_str!("../../skills/okx-ai/references/a2a/provider/arbitration-decision.md");
const COMPLETION: &str = include_str!("../../skills/okx-ai/references/a2a/completion.md");
const FEEDBACK: &str = include_str!("../../skills/okx-ai/references/a2a/feedback.md");
const NOTIFY: &str = include_str!("../../skills/okx-ai/references/a2a/notify.md");
const INTAKE: &str = include_str!("../../skills/okx-ai/references/a2a/user/intake.md");
const RECOVERY: &str = include_str!("../../skills/okx-ai/references/runtime/recovery.md");

#[test]
fn v2_uses_role_scoped_lazy_routing() {
    assert!(SKILL.contains("references/a2a/router.md"));
    assert!(SKILL.contains("Select exactly one row"));
    assert!(SKILL.contains("re-enter this Skill or the A2A parent router"));
    assert!(IDENTITY_SEARCH.contains("Do not load any A2A creation"));
    assert!(IDENTITY_SEARCH.contains("../a2a/user/create-prepare.md"));
    assert!(ROUTER.contains("select exactly one role router"));
    assert!(ROUTER.contains("Never preload all role routers"));
    assert!(ROUTER.contains("user/router.md"));
    assert!(ROUTER.contains("provider/router.md"));
    assert!(ROUTER.contains("evaluator/router.md"));
    assert!(USER_ROUTER.contains("Select exactly one final leaf"));
    assert!(PROVIDER_ROUTER.contains("Select exactly one final leaf"));
    assert!(EVALUATOR_ROUTER.contains("Select exactly one final leaf"));
    for retired in [
        "a2a/core.md",
        "task-action-routing.md",
        "task-output-templates.md",
    ] {
        assert!(!SKILL.contains(retired));
        assert!(!ROUTER.contains(retired));
    }
}

#[test]
fn prepare_is_read_only_and_create_uses_service_uuid() {
    assert!(PREPARE.contains("task-create-prepare --sid <selected-sid>"));
    assert!(PREPARE.contains("exactly once for the selected `sid` in one turn"));
    assert!(PREPARE.contains("never start a duplicate"));
    assert!(PREPARE.contains("is not authorization to create"));
    assert!(PREPARE.contains("payload.serviceId"));
    assert!(CREATE.contains("--service-id <payload.serviceId>"));
    assert!(CREATE.contains("`sid` is never the Service UUID"));
    assert!(CREATE.contains("explicit final confirmation"));
    assert!(CREATE.contains("communication-check"));
    assert!(CREATE.contains("--guide-consent-json"));
    assert!(GUIDE.contains("Consent"));
}

#[test]
fn unknown_system_events_stop_before_cli_dispatch() {
    let router = ROUTER.split_whitespace().collect::<Vec<_>>().join(" ");
    assert!(ROUTER.contains("Unknown system events are coverage failures"));
    assert!(router.contains("Stop before calling `next-action`"));
    assert!(ROUTER.contains("`common context`"));
}

#[test]
fn create_confirmation_combines_guide_task_and_payment() {
    let create = CREATE.split_whitespace().collect::<Vec<_>>().join(" ");
    let guide = GUIDE.split_whitespace().collect::<Vec<_>>().join(" ");
    let subscription_create = SUBSCRIPTION_CREATE
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");
    for forbidden in [
        "| Signal Execution |",
        "| Per-Signal Amount |",
        "| Trade Kit Environment |",
    ] {
        assert!(!CREATE.contains(forbidden));
    }
    assert!(CREATE.contains("List attachments below the table"));
    assert!(CREATE.contains("| Service Guide Consent | {guideConsent} |"));
    assert!(create.contains("displayed payment, and exact Guide Consent"));
    assert!(guide.contains("Do not render a standalone Guide confirmation"));
    assert!(guide.contains("single final confirmation card"));
    assert!(!GUIDE.contains("independent from the final task/payment confirmation"));
    assert!(!CREATE.contains("Guide Consent was confirmed separately"));
    assert!(subscription_create.contains("without asking for a separate confirmation"));
    assert!(subscription_create.contains("one explicit final confirmation"));
}

#[test]
fn one_time_task_details_use_a_vertical_field_value_card() {
    assert!(TASK_QUERY.contains("### One-time Job Details"));
    assert!(TASK_QUERY.contains("| Field | Value |"));
    assert!(TASK_QUERY.contains("| Job ID | {jobId} |"));
    assert!(TASK_QUERY.contains("| Job Description | {description} |"));
    assert!(!TASK_QUERY.contains(
        "| Job Name | Job ID | Service Provider | Fee | Status | Job Description |"
    ));
}

#[test]
fn refund_reason_and_write_are_freshly_bound() {
    let confirmation = REFUND_CONFIRM
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_ascii_lowercase();
    for expected in ["user-authored", "non-blank", "preserve", "verbatim"] {
        assert!(confirmation.contains(expected));
    }
    assert!(confirmation.contains("final input"));
    assert!(REFUND_PREPARE.contains("read-only Refund V2 result"));
    assert!(REFUND_CONFIRM.contains("submit_refund_request"));
    assert!(REFUND_EXECUTE.contains("refund-execute JOB_ID_ARG"));
    assert!(REFUND_EXECUTE.contains("--refund-context-id"));
    assert!(REFUND_EXECUTE.contains("--confirm"));
    assert!(REFUND_CONFIRM.contains("bound to one latest preparation result"));
}

#[test]
fn refund_finality_and_display_remain_exact() {
    for fact in [
        "Expired(8)",
        "Failed(9)",
        "job_asp_reject_expire",
        "sub_failed_notify",
    ] {
        assert!(REFUND_CONTRACT.contains(fact));
    }
    assert!(REFUND_CONTRACT.contains("A Tx Hash is optional audit metadata"));
    assert!(REFUND_CONFIRM.contains("## Output Templates"));
    assert!(REFUND_CONFIRM.contains("### Confirm Refund Request"));
    assert!(REFUND_CONFIRM.contains(
        "If everything is correct, reply “Submit refund request.” and provide your reason."
    ));
    assert!(!REFUND_CONFIRM.contains("To make changes"));
    assert!(TASK_QUERY.contains("## Output Templates"));
    assert!(TASK_QUERY.contains("### Pending Refund Requests"));
    assert!(TASK_QUERY.contains("You have {pendingCount} pending refund requests:"));
    assert!(TASK_QUERY.contains("Reply with the number or Job ID to view details."));
    assert!(TASK_QUERY.contains("### Refund Request Details"));
    assert!(TASK_QUERY.contains("Preserve the full Job ID and the original refund reason"));
    assert!(TASK_QUERY.contains("This scene has no Recommend action"));
    assert!(PROVIDER_ARBITRATION_QUERY
        .contains("You have {pendingCount} refund requests from buyers awaiting your decision:"));
    assert!(PROVIDER_ARBITRATION_QUERY.contains(
        "A full refund will be issued automatically if no action is taken by the deadline. Reply with a number or Job ID to view the request."
    ));
    assert!(PROVIDER_ARBITRATION_DECISION.contains("### Buyer Refund Request"));
    assert!(PROVIDER_ARBITRATION_DECISION.contains("`message.rejectReason`"));
    assert!(PROVIDER_ARBITRATION_DECISION.contains("`detail.rejectReason`"));
    assert!(PROVIDER_ARBITRATION_DECISION.contains(
        "To refund the buyer, reply “Approve refund.” To dispute the request, reply “Request evaluation” and provide your reason."
    ));
    assert!(PROVIDER_ARBITRATION_DECISION.contains("--user-content-b64"));
    assert!(PROVIDER_ARBITRATION_DECISION.contains("--list-label-b64"));
}

#[test]
fn completion_orders_feedback_notification_and_cleanup() {
    let feedback = COMPLETION.find("feedback.md").unwrap();
    let notify = COMPLETION.find("notify.md").unwrap();
    let cleanup = COMPLETION.find("cleanup.md").unwrap();
    assert!(feedback < notify && notify < cleanup);
    assert!(FEEDBACK.contains("rating.required=true"));
    assert!(FEEDBACK.contains("`required=false`"));
    assert!(NOTIFY.contains("[onchainos:task-terminal]"));
    assert!(NOTIFY.contains("byte-for-byte"));
}

#[test]
fn deliverable_intake_spells_out_the_cli_contract() {
    let intake = INTAKE.split_whitespace().collect::<Vec<_>>().join(" ");
    assert!(intake.contains("never turn a system event into an A2A file"));
    assert!(intake.contains("Do not create a temporary file"));
    assert!(intake.contains("under the current `$TMPDIR`"));
    assert!(intake.contains(
        "--message '{\"event\":\"deliverable_received\",\"jobId\":\"<envelope.jobId>\"}'"
    ));
    assert!(intake.contains("--a2a-file \"<0600 raw envelope path under $TMPDIR>\""));
    assert!(intake.contains("Both `--message` and `--a2a-file` are required"));
    assert!(intake.contains("Do not substitute `deliver`"));
    assert!(RECOVERY.contains("--source-event cli_failed"));
}
