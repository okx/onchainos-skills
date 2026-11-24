mod common;

use common::onchainos;

const REQUEST: &str = include_str!("../../skills/okx-ai/references/runtime/decision-request.md");
const RELAY: &str = include_str!("../../skills/okx-ai/references/runtime/decision-relay.md");
const BACKLOG: &str = include_str!("../../skills/okx-ai/references/runtime/backlog.md");
const PROVIDER_ARBITRATION_DECISION: &str =
    include_str!("../../skills/okx-ai/references/a2a/provider/arbitration-decision.md");

fn help(args: &[&str]) -> String {
    let output = onchainos().args(args).output().expect("run help");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).expect("UTF-8 help")
}

#[test]
fn durable_decision_docs_use_existing_cli_surface() {
    let root = help(&["agent", "pending-decisions-v2", "--help"]);
    for command in [
        "request",
        "request-prompt",
        "resolve-prompt",
        "pick",
        "list",
    ] {
        assert!(root.contains(command));
    }
    assert!(REQUEST.contains("pending-decisions-v2 request"));
    assert!(REQUEST.contains("request-prompt"));
    assert!(BACKLOG.contains("pending-decisions-v2 list --format markdown"));
}

#[test]
fn request_and_relay_flags_match_cli_help() {
    let request_help = help(&["agent", "pending-decisions-v2", "request", "--help"]);
    for flag in [
        "--job-id",
        "--role",
        "--agent-id",
        "--user-content",
        "--list-label",
    ] {
        assert!(request_help.contains(flag));
        assert!(REQUEST.contains(flag));
    }
    assert!(RELAY.contains("okx-a2a user check --todo-ids <id> --json"));
    assert!(RELAY.contains("claim first"));
}

#[test]
fn encoded_prompt_flags_match_cli_help_and_refund_decision_docs() {
    let prompt_help = help(&["agent", "pending-decisions-v2", "request-prompt", "--help"]);
    for flag in [
        "--user-content-b64",
        "--list-label-b64",
        "--refund-display-b64",
    ] {
        assert!(prompt_help.contains(flag));
        assert!(PROVIDER_ARBITRATION_DECISION.contains(flag));
    }
}
