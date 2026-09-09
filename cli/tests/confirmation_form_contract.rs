const SKILL: &str = include_str!("../../skills/okx-ai/SKILL.md");
const ROUTER: &str = include_str!("../../skills/okx-ai/references/a2a/router.md");
const USER_ROUTER: &str = include_str!("../../skills/okx-ai/references/a2a/user/router.md");
const PROVIDER_ROUTER: &str = include_str!("../../skills/okx-ai/references/a2a/provider/router.md");
const EVALUATOR_ROUTER: &str =
    include_str!("../../skills/okx-ai/references/a2a/evaluator/router.md");
const IDENTITY_SEARCH: &str = include_str!("../../skills/okx-ai/references/identity/search.md");
const PREPARE: &str = include_str!("../../skills/okx-ai/references/a2a/user/create-prepare.md");
const CREATE: &str = include_str!("../../skills/okx-ai/references/a2a/user/create.md");
const SUBSCRIPTION_CREATE: &str =
    include_str!("../../skills/okx-ai/references/a2a/user/subscription-create.md");
const SUBSCRIPTION_QUERY: &str =
    include_str!("../../skills/okx-ai/references/a2a/user/subscription.md");
const GUIDE: &str = include_str!("../../skills/okx-ai/references/a2a/user/create-guide.md");
const REFUND_PREPARE: &str =
    include_str!("../../skills/okx-ai/references/a2a/user/refund-prepare.md");
const REFUND_CONFIRM: &str =
    include_str!("../../skills/okx-ai/references/a2a/user/refund-confirm.md");
const REFUND_EXECUTE: &str =
    include_str!("../../skills/okx-ai/references/a2a/user/refund-execute.md");
const REFUND_CONTRACT: &str =
    include_str!("../../skills/okx-ai/references/shared/refund-contract.md");
const TASK_QUERY: &str = include_str!("../../skills/okx-ai/references/a2a/task-query.md");
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
fn create_confirmation_omits_follow_trade_configuration() {
    let create = CREATE.split_whitespace().collect::<Vec<_>>().join(" ");
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
    assert!(CREATE.contains("Render attachments below the field list"));
    assert!(CREATE.contains("- Job Name: {title}"));
    assert!(!CREATE.contains("| Job Name | Job Description |"));
    assert!(SUBSCRIPTION_CREATE.contains("- Subscription Name: {title}"));
    assert!(SUBSCRIPTION_CREATE.contains("not ASP configuration and not merely a local record"));
    assert!(SUBSCRIPTION_CREATE.contains("- Service Guide Consent: {guideConsent}"));
    assert!(!SUBSCRIPTION_CREATE.contains("- Service Parameters: {serviceParams}"));
    assert!(subscription_create.contains("internal follow-trade parameters"));
    assert!(!SUBSCRIPTION_CREATE.contains("| Field | Value |"));
    assert!(subscription_create.contains("without asking for a separate confirmation"));
    assert!(subscription_create.contains("one explicit final confirmation"));
    assert!(subscription_create.contains("Automatic copy-trading preference"));
    assert!(subscription_create.contains("Do not ask an additional platform-level mode question"));
    assert!(GUIDE.contains("silently persist that answer before asking the next"));
    assert!(GUIDE.contains("subscription-execution-config-set"));
    assert!(subscription_create.contains("one-time tasks never configure it"));
    assert!(!SUBSCRIPTION_CREATE.contains("Guide Consent remains a separate confirmation"));
    let subscription_detail = SUBSCRIPTION_QUERY.split_once("## Detail").unwrap().1;
    assert!(subscription_detail.contains("- Job ID: {jobId}"));
    assert!(subscription_detail.contains("- Status: {localizedStatusLabel}"));
    assert!(subscription_detail.contains("- Status Description: {localizedStatusDescription}"));
    assert!(SUBSCRIPTION_QUERY.contains("use `Refund completed`"));
    assert!(!subscription_detail.contains("| Job Name |"));
    assert!(create.contains("Keep Guide Consent in its separate confirmation"));
}

#[test]
fn one_time_creation_confirmation_is_vertical_and_confirm_only() {
    assert!(CREATE.contains("render exactly one field per bullet line"));
    assert!(CREATE.contains("To create this job, reply “Confirm”."));
    assert!(!CREATE.contains("To cancel, reply “Cancel”."));
    assert!(CREATE.contains("Do not add a `Cancel` action"));
}

#[test]
fn refund_reason_and_write_are_freshly_bound() {
    let confirmation = REFUND_CONFIRM
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_ascii_lowercase();
    let refund_execute = REFUND_EXECUTE
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");
    for expected in ["user-authored", "non-blank", "preserve", "verbatim"] {
        assert!(confirmation.contains(expected));
    }
    assert!(confirmation.contains("after both submission intent and the reason are present"));
    assert!(REFUND_PREPARE.contains("read-only Refund result"));
    assert!(REFUND_CONFIRM.contains("submit_refund_request"));
    assert!(REFUND_EXECUTE.contains("refund-execute JOB_ID_ARG"));
    assert!(REFUND_EXECUTE.contains("--refund-context-id"));
    assert!(REFUND_EXECUTE.contains("--confirm"));
    assert!(refund_execute
        .contains("may ask me to view the selected task's details for the refund result."));
    assert!(REFUND_EXECUTE.contains("Do not\nrender a CLI command"));
    assert!(!REFUND_EXECUTE.contains("onchainos agent status <jobId>"));
    assert!(REFUND_CONFIRM
        .contains("Bind each write to an explicit action selected from the latest preparation"));
}

#[test]
fn refund_finality_and_display_remain_exact() {
    for fact in ["Expired(8)", "Failed(9)", "job_asp_reject_expire"] {
        assert!(REFUND_CONTRACT.contains(fact));
    }
    assert!(REFUND_CONTRACT.contains("render `Refund completed`"));
    assert!(REFUND_CONTRACT.contains("A Tx Hash is optional audit metadata"));
    assert!(REFUND_CONFIRM.contains("## Output Templates"));
    assert!(REFUND_CONFIRM.contains("### Confirm Refund Request"));
    assert!(REFUND_CONFIRM.contains("include your refund reason"));
    assert!(REFUND_CONFIRM.contains("Render the complete [Confirm Refund Request]"));
    assert!(REFUND_CONFIRM.contains("preceding `B` or rejection enters"));
    assert!(TASK_QUERY.contains("## Output Templates"));
    assert!(TASK_QUERY.contains("### Refund Task List"));
    assert!(TASK_QUERY.contains("### Refund Request Details"));
    assert!(TASK_QUERY.contains("Preserve the full Job ID and the original refund reason"));
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
