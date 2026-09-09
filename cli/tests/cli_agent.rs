//! Integration tests — `onchainos agent` identity listing validation for the
//! A2A serviceDescription advisory-handling requirement (WWINFRA-3659 /
//! OLI-B-WWINFRA-3659).
//!
//! Source plan: `oli-docs/identity-a2a-service-description/integration-plan.csv`
//!   rows IT-001…IT-015. Spec: `oli-docs/identity-a2a-service-description/spec.md`.
//!
//! ─── Why a new `cli_agent.rs` ─────────────────────────────────────────────────
//! `agent` is a top-level subcommand with no existing integration file (identity
//! was previously covered only by the inline `#[path]` unit tests under
//! `src/commands/agent_commerce/identity/tests/`). Per the repo's one-file-per-
//! top-level-area convention (`project-context.md` `test-file-naming: cli_<area>.rs`,
//! `project-knowledge.md` §13) these `agent validate-listing` / `create` / `update`
//! rows land in a single new `cli_agent.rs`.
//!
//! ─── Conventions ──────────────────────────────────────────────────────────────
//!   - Every `validate-listing` row is `network_required: offline`: the command is
//!     a PURE-LOCAL validator (no HTTP, no wallet) — so it runs directly, NEVER via
//!     `run_with_retry`. Each still gets an isolated `ONCHAINOS_HOME` sandbox
//!     (audit logging writes there) staged under `cli/target/test_tmp/cli_agent/…`
//!     via the shared `fresh_home` + `scrubbed` helpers — NOT `tempfile::tempdir()`.
//!   - `validate-listing` prints the RAW `ValidationResult` (`{ pass, findings }`),
//!     NOT the `{ ok, data }` envelope, and always exits 0 (findings are data, not
//!     an error). So assertions parse stdout JSON directly with `parse_stdout_json`
//!     — `assert_ok_and_extract_data` does not apply here.
//!   - Offline assertions are DETERMINISTIC (the validator is a pure function of its
//!     input), so exact `pass` / `severity` / findings-count assertions are correct
//!     here — they are not network-varying results.
//!   - The two `create` / `update` rows are `network_required: live` AND require a
//!     logged-in test wallet: `create_impl`/`update_impl` run auth + signing-session
//!     load BEFORE `parse_services` (mutations.rs:117-140), so the empty-description
//!     `normalize_service` bail is only reachable with real creds. They are therefore
//!     `#[ignore]`d and, as live rows, go through the project's `run_with_retry`
//!     helper (rate-limit tolerance) rather than a bare invocation.
//!   - No environment-specific base URL or hostname is hardcoded anywhere.

mod common;

use common::{
    assert_error_contains, create_auto_consent_via_continuation, fresh_home, onchainos,
    parse_stdout_json, run_with_retry, scrubbed,
};
use serde_json::Value;
use std::fs;

#[test]
fn lifecycle_command_is_registered() {
    let output = onchainos()
        .args(["agent", "lifecycle", "job-1", "--help"])
        .output()
        .expect("run lifecycle help");
    assert_eq!(output.status.code(), Some(0));
    let help = String::from_utf8_lossy(&output.stdout);
    assert!(help.contains("<JOB_ID>"));
    assert!(help.contains("--agent-id <AGENT_ID>"));
}

#[test]
fn provider_subscription_decision_commands_are_registered() {
    for (command, required_args) in [
        ("accept-subscription", vec!["--agent-id <AGENT_ID>"]),
        (
            "decline-subscription",
            vec!["--agent-id <AGENT_ID>", "--reason <REASON>"],
        ),
    ] {
        let output = onchainos()
            .args(["agent", command, "job-1", "--help"])
            .output()
            .unwrap_or_else(|error| panic!("run {command} help: {error}"));
        assert_eq!(
            output.status.code(),
            Some(0),
            "{command} was not registered"
        );
        let help = String::from_utf8_lossy(&output.stdout);
        for required in required_args {
            assert!(
                help.contains(required),
                "{command} help missing {required:?}: {help}"
            );
        }
    }
}

#[test]
fn refund_commands_expose_the_prepare_confirm_contract() {
    let prepare = onchainos()
        .args(["agent", "refund-prepare", "job-1", "--help"])
        .output()
        .expect("run refund-prepare help");
    assert_eq!(prepare.status.code(), Some(0));
    let prepare_help = String::from_utf8_lossy(&prepare.stdout);
    assert!(prepare_help.contains("--reason <REASON>"));

    let execute = onchainos()
        .args(["agent", "refund-execute", "job-1", "--help"])
        .output()
        .expect("run refund-execute help");
    assert_eq!(execute.status.code(), Some(0));
    let execute_help = String::from_utf8_lossy(&execute.stdout);
    for expected in [
        "--operation <OPERATION>",
        "--refund-context-id <REFUND_CONTEXT_ID>",
        "--confirm",
        "close-zero",
        "direct-refund",
        "request-refund",
        "cancel-trial-conversion",
    ] {
        assert!(
            execute_help.contains(expected),
            "refund-execute help missing {expected:?}: {execute_help}"
        );
    }
    assert!(!execute_help.contains("finalize-expired-refund"));
}

#[test]
fn refund_execute_rejects_an_unregistered_operation_before_network_access() {
    let output = onchainos()
        .args([
            "agent",
            "refund-execute",
            "job-1",
            "--operation",
            "claim-auto-refund",
            "--refund-context-id",
            "refundctx_example",
            "--confirm",
        ])
        .output()
        .expect("parse invalid refund operation");
    assert_eq!(output.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&output.stderr).contains("invalid value"));
}

#[test]
fn legacy_claim_auto_refund_is_blocked_before_network_access() {
    let (_home, dir) = fresh_home("cli_agent_refund_legacy_claim");
    let mut cmd = onchainos();
    scrubbed(&mut cmd, &dir);
    let output = cmd
        .args(["agent", "claim-auto-refund", "job-1"])
        .output()
        .expect("run disabled claim-auto-refund command");

    assert_ne!(output.status.code(), Some(0));
    assert_error_contains(
        &output,
        &[
            "direct claim-auto-refund is disabled by Refund",
            "refund-prepare job-1",
            "Expired(8) is terminal",
        ],
    );
}

#[test]
fn legacy_refund_writes_are_blocked_before_network_access() {
    let cases: [(&str, Vec<&str>, [&str; 2]); 3] = [
        (
            "close",
            vec!["agent", "close", "job-1"],
            [
                "direct close is disabled for V2 tasks",
                "refund-prepare job-1",
            ],
        ),
        (
            "reject",
            vec!["agent", "reject", "job-1", "--reason", "not acceptable"],
            [
                "direct reject is disabled by Refund",
                "refund-prepare job-1",
            ],
        ),
        (
            "subscribe-reject",
            vec![
                "agent",
                "subscribe-reject",
                "sub-1",
                "--reason",
                "not acceptable",
            ],
            [
                "direct subscribe-reject is disabled by Refund",
                "refund-prepare sub-1",
            ],
        ),
    ];

    for (name, args, expected) in cases {
        let (_home, dir) = fresh_home(&format!("cli_agent_refund_legacy_{name}"));
        let mut cmd = onchainos();
        scrubbed(&mut cmd, &dir);
        let output = cmd
            .args(args)
            .output()
            .unwrap_or_else(|error| panic!("run disabled {name} command: {error}"));

        assert_ne!(
            output.status.code(),
            Some(0),
            "{name} unexpectedly succeeded"
        );
        assert_error_contains(&output, &expected);
    }
}

#[test]
fn my_tasks_help_documents_defaults_and_filters() {
    let output = onchainos()
        .args(["agent", "my-tasks", "--help"])
        .output()
        .expect("run my-tasks help");

    assert_eq!(output.status.code(), Some(0));
    let stdout = String::from_utf8_lossy(&output.stdout);
    for expected in [
        "--task-type <TASK_TYPE>",
        "[default: all]",
        "possible values: all, subscription, one-time",
        "--status-type <STATUS_TYPE>",
        "--page <PAGE>",
        "--page-size <PAGE_SIZE>",
        "[default: 10]",
    ] {
        assert!(
            stdout.contains(expected),
            "help missing {expected:?}: {stdout}"
        );
    }

    let parsed = onchainos()
        .args([
            "agent",
            "my-tasks",
            "--task-type",
            "one-time",
            "--status-type",
            "2",
            "--page",
            "2",
            "--page-size",
            "10",
            "--help",
        ])
        .output()
        .expect("parse representative my-tasks arguments");
    assert_eq!(parsed.status.code(), Some(0));
}

#[test]
fn my_tasks_rejects_invalid_ranges() {
    for args in [
        ["agent", "my-tasks", "--status-type", "3"],
        ["agent", "my-tasks", "--page", "0"],
        ["agent", "my-tasks", "--page-size", "0"],
        ["agent", "my-tasks", "--page-size", "101"],
    ] {
        let output = onchainos()
            .args(args)
            .output()
            .expect("run invalid my-tasks arguments");
        assert_ne!(output.status.code(), Some(0), "accepted {args:?}");
    }
}

/// Run `agent validate-listing` offline in an isolated `ONCHAINOS_HOME` sandbox
/// and return the parsed raw `{ pass, findings }` JSON. Asserts exit 0 (the CSV
/// `exit_code` for every validate-listing row): validate-listing surfaces findings
/// as data and never fails the process, even when `pass == false`.
fn validate_listing(role: &str, service_json: &str) -> Value {
    let (_home, dir) = fresh_home("cli_agent");
    let mut cmd = onchainos();
    scrubbed(&mut cmd, &dir);
    let output = cmd
        .args([
            "agent",
            "validate-listing",
            "--role",
            role,
            "--service",
            service_json,
        ])
        .output()
        .expect("failed to execute `onchainos agent validate-listing`");

    assert_eq!(
        output.status.code(),
        Some(0),
        "validate-listing must exit 0 (findings are data, not a process error)\nstdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );
    parse_stdout_json(&output)
}

/// Extract the `findings` array, panicking with the full envelope if the shape is
/// wrong — a clearer failure than an index into a missing field.
fn findings(result: &Value) -> &Vec<Value> {
    result["findings"]
        .as_array()
        .unwrap_or_else(|| panic!("`findings` is not an array: {result}"))
}

fn read_consent_metadata(path: &std::path::Path) -> Value {
    let raw = std::fs::read_to_string(path).expect("read consent markdown");
    let json = raw
        .strip_prefix("<!-- onchainos-autotrade:consent\n")
        .and_then(|value| value.split_once("\n-->"))
        .map(|(metadata, _)| metadata)
        .expect("parse consent metadata envelope");
    serde_json::from_str(json).expect("parse consent metadata JSON")
}

fn funding_notice_image_dir() -> std::path::PathBuf {
    let image_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("target")
        .join("test_tmp")
        .join("funding notice");
    fs::create_dir_all(&image_dir).expect("create funding-notice image dir");
    image_dir
}

#[test]
fn funding_notice_outputs_canonical_json_and_png() {
    let image_dir = funding_notice_image_dir();
    let output = onchainos()
        .env_remove("CODEX_THREAD_ID")
        .args([
            "agent",
            "funding-notice",
            "--chain",
            "XLayer",
            "--currency",
            "USDT",
            "--shortfall",
            "0.01",
            "--deposit-address",
            "0x1234567890abcdef1234567890abcdef12345678",
            "--format",
            "json",
            "--image-dir",
            image_dir.to_str().expect("utf8 image dir"),
        ])
        .output()
        .expect("run funding-notice");

    let data = common::assert_ok_and_extract_data(&output);
    assert_eq!(data["mustLocalize"], true);
    assert_eq!(data["mustNotifyWithImagePath"], true);
    assert_eq!(data["mustRunNotifyCommand"], true);
    assert_eq!(data["mustRepeatInFinalResponse"], true);
    assert_eq!(data["mustRenderMarkdownImageBelowFirstOption"], true);
    assert_eq!(data["displayMode"], "image-notify");
    assert!(data["terminalQr"].is_null());
    assert_eq!(data["endTurn"], true);
    assert_eq!(data["chain"], "XLayer");
    assert_eq!(data["depositChain"], "XLayer");
    assert_eq!(data["currency"], "USDT");
    assert_eq!(data["shortfall"], "0.01");
    assert_eq!(
        data["depositAddress"],
        "0x1234567890abcdef1234567890abcdef12345678"
    );

    let content = data["contentCanonical"].as_str().expect("contentCanonical");
    for expected in [
        "Insufficient USDT balance on XLayer",
        "1. Scan and deposit",
        "2. Swap",
        "3. Bridge",
        "4. Withdraw from OKX",
        "Gas is paid by the platform",
        "After topping up, tell me \"I topped up\".",
    ] {
        assert!(
            content.contains(expected),
            "contentCanonical missing {expected:?}: {content}"
        );
    }
    let notify_command = data["notifyCommand"].as_str().expect("notifyCommand");
    assert!(notify_command.contains("$ONCHAINOS_FUNDING_NOTICE_CONTENT"));
    assert!(notify_command.contains("--image-path"));
    assert!(notify_command.contains("'"));
    let notify_args = data["notifyCommandArgs"]
        .as_array()
        .expect("notifyCommandArgs");
    assert_eq!(notify_args[0], "onchainos");
    assert_eq!(notify_args[1], "agent");
    assert_eq!(notify_args[2], "user-notify");
    assert!(notify_args.iter().any(|arg| arg == "--image-path"));
    assert!(notify_args.iter().any(|arg| arg
        .as_str()
        .is_some_and(|value| value.contains("funding notice"))));
    let policy = data["displayPolicy"].as_str().expect("displayPolicy");
    assert!(policy.contains("Non-TTY"));
    assert!(policy.contains("run notifyCommandArgs"));
    assert!(policy.contains("put markdownImage under option 1"));

    let image_path = data["imagePath"].as_str().expect("imagePath");
    let markdown_image = data["markdownImage"].as_str().expect("markdownImage");
    assert!(markdown_image.starts_with("![QR Code]("));
    assert!(markdown_image.contains("onchainos-funding-qr-"));
    let bytes = fs::read(image_path).expect("read generated QR PNG");
    assert!(bytes.starts_with(b"\x89PNG\r\n\x1a\n"));
    let _ = fs::remove_file(image_path);
}

#[test]
fn funding_notice_unknown_chain_does_not_claim_gas_subsidy() {
    let image_dir = funding_notice_image_dir();
    let output = onchainos()
        .args([
            "agent",
            "funding-notice",
            "--chain",
            "Base",
            "--currency",
            "USDC",
            "--shortfall",
            "2.5",
            "--deposit-address",
            "0x1234567890abcdef1234567890abcdef12345678",
            "--format",
            "json",
            "--image-dir",
            image_dir.to_str().expect("utf8 image dir"),
        ])
        .output()
        .expect("run funding-notice");

    let data = common::assert_ok_and_extract_data(&output);
    let content = data["contentCanonical"].as_str().expect("contentCanonical");
    assert!(content.contains("Insufficient USDC balance on Base"));
    assert!(content.contains("2.5 USDC"));
    assert!(content.contains("Ensure the wallet meets the network gas requirements."));
    assert!(!content.contains("Gas is paid by the platform"));

    let image_path = data["imagePath"].as_str().expect("imagePath");
    let _ = fs::remove_file(image_path);
}

#[test]
fn funding_notice_accepts_payment_402_reason() {
    let output = onchainos()
        .env_remove("CODEX_THREAD_ID")
        .args([
            "agent",
            "funding-notice",
            "--chain",
            "XLayer",
            "--currency",
            "USDT",
            "--shortfall",
            "0.01",
            "--deposit-address",
            "0x1234567890abcdef1234567890abcdef12345678",
            "--reason",
            "payment-402",
            "--format",
            "json",
        ])
        .output()
        .expect("run funding-notice");

    let data = common::assert_ok_and_extract_data(&output);
    assert_eq!(data["reason"], "payment-402");
    let image_path = data["imagePath"].as_str().expect("imagePath");
    assert!(fs::read(image_path)
        .expect("read generated QR PNG")
        .starts_with(b"\x89PNG\r\n\x1a\n"));
    let _ = fs::remove_file(image_path);
}

// ════════════════════════════════════════════════════════════════════════════
//  agent validate-listing — A2A advisory (suggest) cases
// ════════════════════════════════════════════════════════════════════════════

// ── IT-001: a well-structured A2A listing passes with no findings ─────────────
//   Golden happy path: valid 3-paragraph non-subscription A2A description →
//   pass:true, zero findings, exit 0.
#[test]
fn validate_listing_a2a_well_structured_passes() {
    let result = validate_listing(
        "asp",
        r#"[{"serviceName":"DEX Arbitrage Signals","serviceDescription":"Provides DEX arbitrage trading signals\nUser provides the target chain and budget\nDelivers structured signals, copy-trading supported","serviceType":"A2A","fee":"0.11"}]"#,
    );
    assert_eq!(
        result["pass"].as_bool(),
        Some(true),
        "expected pass:true, got {result}"
    );
    assert!(
        findings(&result).is_empty(),
        "a well-structured A2A listing should raise no findings, got {result}"
    );
}

// ── IT-002: a 2-paragraph A2A listing passes cleanly ──────────────────────────
//   The paragraph-count rule is gone entirely, so a 2-paragraph non-subscription
//   description is simply valid — pass:true with no findings at all (not even a
//   suggestion).
#[test]
fn validate_listing_a2a_two_paragraph_non_subscription_passes() {
    let result = validate_listing(
        "asp",
        r#"[{"serviceName":"DEX Arbitrage Signals","serviceDescription":"Provides DEX arbitrage trading signals\nUser provides the target chain and budget","serviceType":"A2A","fee":"0.11"}]"#,
    );
    assert_eq!(
        result["pass"].as_bool(),
        Some(true),
        "expected pass:true, got {result}"
    );
    assert!(
        findings(&result).is_empty(),
        "paragraph count is not validated — expected zero findings, got {result}"
    );
}

// ── IT-003: billing model does not change the description rules ────────────────
//   The same 3-paragraph body that is valid per-call is equally valid on a
//   subscription service: no billing-model branch remains in the validator.
#[test]
fn validate_listing_a2a_subscription_paragraph_count_not_checked() {
    let result = validate_listing(
        "asp",
        r#"[{"serviceName":"DEX Arbitrage Signals","serviceDescription":"Provides DEX arbitrage trading signals\nUser provides the target chain and budget\nDelivers structured signals","serviceGuide":"Choose a market and submit your budget.","serviceType":"A2A","fee":"","subscription":[{"interval":"month","fee":"10"}]}]"#,
    );
    assert_eq!(
        result["pass"].as_bool(),
        Some(true),
        "expected pass:true, got {result}"
    );
    assert!(
        findings(&result).is_empty(),
        "a subscription service must be validated identically to a per-call one, got {result}"
    );
}

// ── IT-004: an over-length A2A description advises but never blocks ───────────
//   D2 (total display width over 2000 = 1000 CJK) is advisory: exactly one
//   `suggest` finding, pass stays true. There is no per-paragraph limit.
#[test]
fn validate_listing_a2a_overlong_description_suggests() {
    // 2 400 half-width chars on one line → display width 2400 > 2000.
    let long = "A".repeat(2400);
    let service = format!(
        r#"[{{"serviceName":"DEX Arbitrage Signals","serviceDescription":"{long}","serviceType":"A2A","fee":"0.11"}}]"#
    );
    let result = validate_listing("asp", &service);
    assert_eq!(
        result["pass"].as_bool(),
        Some(true),
        "expected pass:true, got {result}"
    );
    assert!(
        findings(&result).iter().any(|f| f["severity"] == "suggest"),
        "expected an advisory (suggest) length finding, got {result}"
    );
    assert!(
        findings(&result).iter().all(|f| f["severity"] != "block"),
        "an over-length A2A description must not block, got {result}"
    );
}

// ════════════════════════════════════════════════════════════════════════════
//  agent validate-listing — A2MCP regression guards (unchanged by this change)
// ════════════════════════════════════════════════════════════════════════════

// ── IT-005: a valid A2MCP listing keeps passing exactly as before ─────────────
//   Regression guard: A2MCP is explicitly out of scope; a valid request-style
//   description exits 0 with pass:true.
#[test]
fn validate_listing_a2mcp_valid_passes() {
    let result = validate_listing(
        "asp",
        r#"[{"serviceName":"Realtime Price Feed","serviceDescription":"Returns realtime token price quotes\ntokenAddress (string, required): token contract; chainIndex (string, required): chain id\nPOST","serviceType":"A2MCP","fee":"0.5","endpoint":"https://api.example.com/mcp"}]"#,
    );
    assert_eq!(
        result["pass"].as_bool(),
        Some(true),
        "expected pass:true, got {result}"
    );
}

// ── IT-006: an A2MCP single-paragraph description raises zero findings ─────────
//   A2MCP early-returns before the structural checks, so a single-paragraph
//   layout is not checked → no D1, empty findings.
#[test]
fn validate_listing_a2mcp_single_paragraph_no_findings() {
    let result = validate_listing(
        "asp",
        r#"[{"serviceName":"Realtime Price Feed","serviceDescription":"Summarizes input text into a short abstract","serviceType":"A2MCP","fee":"0.5","endpoint":"https://api.example.com/mcp"}]"#,
    );
    assert!(
        findings(&result).is_empty(),
        "A2MCP structure is not checked — expected zero findings, got {result}"
    );
}

// ════════════════════════════════════════════════════════════════════════════
//  agent validate-listing — still-blocking cases (block preserved)
// ════════════════════════════════════════════════════════════════════════════

// ── IT-007: an empty A2A service description is still rejected ─────────────────
//   The empty-description D1 branch stays severity `block` (missing-required-
//   field), consistent with the create/update normalize_service bail.
#[test]
fn validate_listing_a2a_empty_description_blocks() {
    let result = validate_listing(
        "asp",
        r#"[{"serviceName":"DEX Arbitrage Signals","serviceDescription":"","serviceType":"A2A","fee":"0.11"}]"#,
    );
    assert_eq!(
        findings(&result).first().map(|f| &f["severity"]),
        Some(&Value::String("block".into())),
        "expected findings[0].severity == \"block\" for an empty A2A description, got {result}"
    );
}

// ── IT-008: an A2A description containing a web link is still rejected ─────────
//   Prohibited-content D6 (URL) stays blocking for A2A on anti-abuse grounds.
#[test]
fn validate_listing_a2a_url_blocks() {
    let result = validate_listing(
        "asp",
        r#"[{"serviceName":"DEX Arbitrage Signals","serviceDescription":"Provides DEX arbitrage trading signals, see https://example.com\nUser provides the target chain and budget\nDelivers structured signals","serviceType":"A2A","fee":"0.11"}]"#,
    );
    assert_eq!(
        result["pass"].as_bool(),
        Some(false),
        "expected pass:false for a URL in the description, got {result}"
    );
}

// ── IT-009: a 0x address in an A2A description does NOT block ─────────────────
//   The former D7 hex-address rule was removed (a contract address is legitimate
//   content in a service description); this pins that it no longer blocks. Mirrors
//   the `hex_in_service_description_passes` unit test.
#[test]
fn validate_listing_a2a_hex_address_passes() {
    let result = validate_listing(
        "asp",
        r#"[{"serviceName":"DEX Arbitrage Signals","serviceDescription":"Provides signals for token 0x1234567890abcdef pairs\nUser provides the target chain and budget\nDelivers structured signals","serviceType":"A2A","fee":"0.11"}]"#,
    );
    assert_eq!(
        result["pass"].as_bool(),
        Some(true),
        "a 0x address must not block, got {result}"
    );
}

// ── IT-010: profit wording is no longer a description finding ─────────────────
#[test]
fn validate_listing_a2a_profit_guarantee_passes_without_finding() {
    let result = validate_listing(
        "asp",
        r#"[{"serviceName":"DEX Arbitrage Signals","serviceDescription":"Guaranteed profit DEX arbitrage trading signals\nUser provides the target chain and budget\nDelivers structured signals","serviceType":"A2A","fee":"0.11"}]"#,
    );
    assert_eq!(
        result["pass"].as_bool(),
        Some(true),
        "expected pass:true, got {result}"
    );
    assert!(
        findings(&result).is_empty(),
        "expected no finding, got {result}"
    );
}

// ── IT-011: an A2A description carrying a test/env marker is still rejected ────
//   Prohibited-content U1 (test marker) stays blocking for A2A.
#[test]
fn validate_listing_a2a_test_marker_blocks() {
    let result = validate_listing(
        "asp",
        r#"[{"serviceName":"DEX Arbitrage Signals","serviceDescription":"Provides DEX arbitrage trading signals (test)\nUser provides the target chain and budget\nDelivers structured signals","serviceType":"A2A","fee":"0.11"}]"#,
    );
    assert_eq!(
        result["pass"].as_bool(),
        Some(false),
        "expected pass:false for a test marker, got {result}"
    );
}

// ── IT-012: an A2MCP description may carry a URL; other prohibited content can't ─
//   The URL ban (D6) is A2A-only: an A2MCP request description MUST include a
//   working `curl` example using the real endpoint, so a URL there is legal and
//   must NOT fail the listing. The test marker (U1) still blocks A2MCP.
#[test]
fn validate_listing_a2mcp_url_allowed_test_marker_still_blocks() {
    let with_curl = validate_listing(
        "asp",
        r#"[{"serviceName":"Realtime Price Feed","serviceDescription":"Returns token price quotes\ntokenAddress (string, required): token contract\nPOST\ncurl -X POST https://api.example.com/mcp -d '{\"tokenAddress\":\"0x1234\"}'","serviceType":"A2MCP","fee":"0.5","endpoint":"https://api.example.com/mcp"}]"#,
    );
    assert_eq!(
        with_curl["pass"].as_bool(),
        Some(true),
        "an A2MCP request example carrying the endpoint URL must not block, got {with_curl}"
    );

    let with_marker = validate_listing(
        "asp",
        r#"[{"serviceName":"Realtime Price Feed","serviceDescription":"Returns token price quotes (test)\ntokenAddress (string, required): token contract\nPOST","serviceType":"A2MCP","fee":"0.5","endpoint":"https://api.example.com/mcp"}]"#,
    );
    assert_eq!(
        with_marker["pass"].as_bool(),
        Some(false),
        "a test marker must still block an A2MCP listing, got {with_marker}"
    );
}

// ── IT-013: removed profit rule does not affect a blocking URL ─────────────────
#[test]
fn validate_listing_a2a_profit_text_and_url_blocks_for_url_only() {
    let result = validate_listing(
        "asp",
        r#"[{"serviceName":"DEX Arbitrage Signals","serviceDescription":"Guaranteed profit DEX arbitrage signals, see https://example.com\nUser provides the target chain and budget","serviceType":"A2A","fee":"0.11"}]"#,
    );
    assert_eq!(
        result["pass"].as_bool(),
        Some(false),
        "expected pass:false for the blocking URL, got {result}"
    );
    assert_eq!(
        findings(&result).len(),
        1,
        "expected only the URL finding, got {result}"
    );
    assert_eq!(findings(&result)[0]["severity"], "block");
}

// ════════════════════════════════════════════════════════════════════════════
//  agent autotrade-consent-set --mode pause — local compatibility contract
// ══════════════════════════════════════════════════════════════════════════

#[test]
fn autotrade_pause_needs_only_job_id_and_clears_execution_policy() {
    let (_home, dir) = fresh_home("cli_agent_autotrade_pause");
    let job_id = "job_pause_zh";

    for store in ["consent", "grants", "pending"] {
        let store_dir = dir.join("autotrade").join(store);
        std::fs::create_dir_all(&store_dir).expect("create autotrade store");
        std::fs::write(store_dir.join(format!("{job_id}.json")), b"seed")
            .expect("seed autotrade state");
    }
    let mut cmd = onchainos();
    scrubbed(&mut cmd, &dir);
    let output = cmd
        .args([
            "agent",
            "autotrade-consent-set",
            "--job-id",
            job_id,
            "--mode",
            "pause",
        ])
        .output()
        .expect("run autotrade pause");
    let data = common::assert_ok_and_extract_data(&output);

    assert_eq!(
        data,
        serde_json::json!({"consentMode":"pause","cleared":true,"jobId":job_id})
    );

    let consent_path = dir
        .join("autotrade")
        .join("consent")
        .join(format!("{job_id}.json"));
    assert!(
        !consent_path.exists(),
        "pause must leave the subscription without an execution policy"
    );

    for store in ["grants", "pending"] {
        assert!(
            !dir.join("autotrade")
                .join(store)
                .join(format!("{job_id}.json"))
                .exists(),
            "pause must clear the {store} record"
        );
    }
}

#[test]
fn autotrade_pause_keeps_legacy_agent_id_compatible() {
    let (_home, dir) = fresh_home("cli_agent_autotrade_pause_legacy");
    let mut cmd = onchainos();
    scrubbed(&mut cmd, &dir);
    let output = cmd
        .args([
            "agent",
            "autotrade-consent-set",
            "--job-id",
            "job_pause_legacy",
            "--agent-id",
            "5254",
            "--mode",
            "pause",
        ])
        .output()
        .expect("run legacy autotrade pause");
    let data = common::assert_ok_and_extract_data(&output);
    assert_eq!(data["consentMode"], "pause");
    assert_eq!(data["cleared"], true);
}

#[test]
fn autotrade_non_pause_modes_still_require_agent_id() {
    let (_home, dir) = fresh_home("cli_agent_autotrade_non_pause");
    let mut cmd = onchainos();
    scrubbed(&mut cmd, &dir);
    let output = cmd
        .args([
            "agent",
            "autotrade-consent-set",
            "--job-id",
            "job_manual",
            "--mode",
            "manual",
        ])
        .output()
        .expect("run autotrade manual without agent id");

    assert_error_contains(&output, &["--agent-id is required unless --mode pause"]);
}

#[test]
fn autotrade_environment_set_upgrades_only_the_existing_policy() {
    let (_home, dir) = fresh_home("cli_agent_autotrade_environment_set");
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs();
    let consent_dir = dir.join("autotrade/consent");
    std::fs::create_dir_all(&consent_dir).unwrap();
    std::fs::write(
        consent_dir.join("job_environment.json"),
        serde_json::to_vec_pretty(&serde_json::json!({
            "version": 1,
            "jobId": "job_environment",
            "mode": "auto",
            "capU": "20",
            "tradeAmountU": "10",
            "quoteToken": "usdc",
            "createdAt": now,
            "expiresAt": now + 3600
        }))
        .unwrap(),
    )
    .unwrap();

    let mut cmd = onchainos();
    scrubbed(&mut cmd, &dir);
    let output = cmd
        .args([
            "agent",
            "autotrade-consent-set",
            "--job-id",
            "job_environment",
            "--agent-id",
            "8315",
            "--mode",
            "environment-set",
            "--environment",
            "demo",
        ])
        .output()
        .expect("persist Trade Kit environment");
    let result = common::assert_ok_and_extract_data(&output);
    assert_eq!(result["tradeEnvironment"], "demo");

    let stored = read_consent_metadata(&consent_dir.join("job_environment.md"));
    assert_eq!(stored["version"], 6);
    assert_eq!(stored["mode"], "auto");
    assert_eq!(stored["capU"], "20");
    assert_eq!(stored["tradeAmountU"], "10");
    assert_eq!(stored["quoteToken"], "usdc");
    assert_eq!(stored["tradeEnvironment"], "demo");
    assert_eq!(stored["createdAt"], now);
    assert_eq!(stored["expiresAt"], now + 3600);
}

#[test]
fn autotrade_settings_update_persists_all_trade_kit_choices_without_rewriting_policy() {
    let (_home, dir) = fresh_home("cli_agent_autotrade_settings_update");
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs();
    let consent_dir = dir.join("autotrade/consent");
    std::fs::create_dir_all(&consent_dir).unwrap();
    std::fs::write(
        consent_dir.join("job_settings.json"),
        serde_json::to_vec_pretty(&serde_json::json!({
            "version": 2,
            "jobId": "job_settings",
            "mode": "auto",
            "capU": "20",
            "tradeAmountU": "10",
            "quoteToken": "usdc",
            "createdAt": now,
            "expiresAt": now + 3600
        }))
        .unwrap(),
    )
    .unwrap();

    let mut cmd = onchainos();
    scrubbed(&mut cmd, &dir);
    let output = cmd
        .args([
            "agent",
            "autotrade-consent-set",
            "--job-id",
            "job_settings",
            "--agent-id",
            "8315",
            "--mode",
            "settings-update",
            "--environment",
            "demo",
            "--margin-mode",
            "isolated",
            "--order-policy",
            "signal_price_limit",
        ])
        .output()
        .expect("persist complete Trade Kit settings");
    let result = common::assert_ok_and_extract_data(&output);
    assert_eq!(result["tradeEnvironment"], "demo");
    assert_eq!(result["marginMode"], "isolated");
    assert_eq!(result["orderPolicy"], "signal_price_limit");

    let stored = read_consent_metadata(&consent_dir.join("job_settings.md"));
    assert_eq!(stored["version"], 6);
    assert_eq!(stored["mode"], "auto");
    assert_eq!(stored["capU"], "20");
    assert_eq!(stored["tradeAmountU"], "10");
    assert_eq!(stored["quoteToken"], "usdc");
    assert_eq!(stored["tradeEnvironment"], "demo");
    assert_eq!(stored["marginMode"], "isolated");
    assert_eq!(stored["orderPolicy"], "signal_price_limit");
    assert_eq!(stored["createdAt"], now);
    assert_eq!(stored["expiresAt"], now + 3600);
}

#[test]
fn autotrade_auto_accepts_missing_cap_and_authorizes_any_positive_amount() {
    let (_home, dir) = fresh_home("cli_agent_autotrade_unbounded_auto");
    create_auto_consent_via_continuation(&dir, "job_unbounded_auto", "8315", None, None);

    let mut check = onchainos();
    scrubbed(&mut check, &dir);
    let check_output = check
        .args([
            "agent",
            "autotrade-grant-check",
            "--job-id",
            "job_unbounded_auto",
            "--venue",
            "dex",
            "--action",
            "buy",
            "--amount",
            "999999",
            "--format",
            "json",
        ])
        .output()
        .expect("check unbounded auto grant");
    assert!(check_output.status.success());
    let result: serde_json::Value =
        serde_json::from_slice(&check_output.stdout).expect("parse grant-check result");
    assert_eq!(result, serde_json::json!({"ok": true}));
}

#[test]
fn autotrade_consent_request_suppresses_mode_card_for_auto_policy() {
    let (_home, dir) = fresh_home("cli_agent_autotrade_consent_request_existing_policy");

    for job_id in ["job_auto"] {
        create_auto_consent_via_continuation(&dir, job_id, "8315", Some("1"), Some("10"));
        let context_dir = dir.join("autotrade/delivery-context").join(job_id);
        std::fs::create_dir_all(&context_dir).unwrap();
        std::fs::write(
            context_dir.join("msg:delivery-1.json"),
            serde_json::to_vec_pretty(&serde_json::json!({
                "version": 2,
                "jobId": job_id,
                "agentId": "8315",
                "providerAgentId": "8779",
                "deliveryId": "msg:delivery-1",
                "savedPath": "/tmp/legacy-signal.txt",
                "deliverableType": "spot",
                "receivedAtMs": 1,
                "executionPath": "legacy_wrapper"
            }))
            .unwrap(),
        )
        .unwrap();

        let mut request = onchainos();
        scrubbed(&mut request, &dir);
        let output = request
            .args([
                "agent",
                "autotrade-consent-request",
                "--job-id",
                job_id,
                "--agent-id",
                "8315",
                "--delivery-id",
                "msg:delivery-1",
                "--signal-type",
                "spot",
            ])
            .output()
            .expect("request first-time consent with an existing policy");
        let data = common::assert_ok_and_extract_data(&output);

        assert_eq!(data["decision"], false);
        assert_eq!(data["decisionPushed"], false);
        assert_eq!(data["status"], "skipped");
        assert_eq!(data["reason"], "guide_execution_unavailable");
        assert_eq!(data["jobId"], job_id);
        assert_eq!(data["deliveryId"], "msg:delivery-1");
        assert_eq!(data["terminal"], true);
    }
}

#[test]
fn user_notify_image_path_must_exist() {
    let (_home, dir) = fresh_home("cli_agent_user_notify_image");
    let mut cmd = onchainos();
    scrubbed(&mut cmd, &dir);
    let output = cmd
        .args([
            "agent",
            "user-notify",
            "--content",
            "notice",
            "--image-path",
            "/tmp/onchainos-missing-qr.png",
        ])
        .output()
        .expect("run user-notify with missing image");

    assert_error_contains(&output, &["--image-path file not found"]);
}

#[test]
fn user_notify_rejects_local_image_links_in_content() {
    let (_home, dir) = fresh_home("cli_agent_user_notify_local_image_link");
    let mut cmd = onchainos();
    scrubbed(&mut cmd, &dir);
    let output = cmd
        .args([
            "agent",
            "user-notify",
            "--content",
            "![QR Code](file:///tmp/deposit_usdt.png)",
        ])
        .output()
        .expect("run user-notify with local image link");

    assert_error_contains(&output, &["use --image-path <file>"]);
}

#[test]
fn service_match_help_describes_pagination_flow_and_price_range() {
    let (_home, dir) = fresh_home("cli_agent_service_match_help");
    let mut cmd = onchainos();
    scrubbed(&mut cmd, &dir);
    let output = cmd
        .args(["agent", "service-match", "--help"])
        .output()
        .expect("run service-match help");

    assert_eq!(output.status.code(), Some(0));
    let help = String::from_utf8_lossy(&output.stdout);
    for expected in [
        "Search marketplace Services by capability, ASP, Service ID, Service name, or price range.",
        "Results include searchAfter, hasMore, unmatchReason, action, tip",
        "--sid <SERVICE_ID>",
        "--min-payment-token-amount <MIN_PAYMENT_TOKEN_AMOUNT>",
        "--max-payment-token-amount <MAX_PAYMENT_TOKEN_AMOUNT>",
        "--search-after <SEARCH_AFTER>",
        "Initial request without filters",
        "Continuation request",
    ] {
        assert!(
            help.contains(expected),
            "missing {expected:?} in help:\n{help}"
        );
    }
    assert!(!help.contains("      --format "));
    assert!(!help.contains("--agentic-id"));
    assert!(!help.contains("backend raw data payload"));
}

#[test]
fn hidden_autotrade_watch_precheck_is_callable_and_rejects_an_unsafe_job_id_locally() {
    let (_home, dir) = fresh_home("cli_agent_autotrade_watch_precheck");
    let mut cmd = onchainos();
    scrubbed(&mut cmd, &dir);
    let output = cmd
        .args(["agent", "autotrade-watch-precheck", "--job-id", "../unsafe"])
        .output()
        .expect("run autotrade-watch-precheck");

    assert_error_contains(&output, &["invalid job id"]);
}

// ════════════════════════════════════════════════════════════════════════
//  agent create / update — §2.2 normalize_service seam (live, wallet-gated)
// ════════════════════════════════════════════════════════════════════════════
//
//  Both rows are `network_required: live` and require a logged-in test wallet:
//  auth (`ensure_tokens_refreshed`) + signing-session load run BEFORE
//  `parse_services` (mutations.rs:117-140), so the empty-`serviceDescription`
//  bail from `normalize_service` (utils.rs) is only reachable with real creds.
//  They are `#[ignore]`d so CI (no wallet) does not fail on the earlier auth
//  error; run them explicitly with `cargo test -- --ignored` against a wallet.
//  As live rows they go through `run_with_retry` for rate-limit tolerance.

// ── IT-014: `agent create` with an empty service description is rejected ───────
//   §2.2 seam: normalize_service bails with the missing-required-field message on
//   stdout ($.error) with exit 1.
#[test]
#[ignore = "live: requires a logged-in test wallet — auth/signing runs before service validation, so the serviceDescription bail is only reachable with creds"]
fn agent_create_empty_service_description_missing_required_field() {
    let output = run_with_retry(&[
        "agent",
        "create",
        "--role",
        "asp",
        "--name",
        "Arb Signals Bot",
        "--description",
        "DEX arbitrage trading signal provider",
        "--service",
        r#"[{"serviceName":"DEX Arbitrage Signals","serviceDescription":"","serviceType":"A2A","fee":"0.11"}]"#,
    ]);
    assert_eq!(
        output.status.code(),
        Some(1),
        "expected exit 1\nstdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("missing required field in --service: serviceDescription"),
        "expected the missing-required-field error on stdout, got: {stdout}"
    );
}

// ── IT-015: `agent update` to an empty service description is rejected ─────────
//   Same §2.2 consistency contract as create via normalize_service; message on
//   stdout ($.error) with exit 1.
#[test]
#[ignore = "live: requires a logged-in test wallet — auth/signing runs before service validation, so the serviceDescription bail is only reachable with creds"]
fn agent_update_empty_service_description_missing_required_field() {
    let output = run_with_retry(&[
        "agent",
        "update",
        "--agent-id",
        "12345",
        "--service",
        r#"[{"operation":"update","id":"7","serviceName":"DEX Arbitrage Signals","serviceDescription":"","serviceType":"A2A","fee":"0.11"}]"#,
    ]);
    assert_eq!(
        output.status.code(),
        Some(1),
        "expected exit 1\nstdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("missing required field in --service: serviceDescription"),
        "expected the missing-required-field error on stdout, got: {stdout}"
    );
}

// ════════════════════════════════════════════════════════════════════════════
//  agent validate-listing / create / update — A2A price validation
//  (WWINFRA-3829 · FE-PRICE-01 empty price · FE-PRICE-02 subscription price > 0)
//
//  Source plan: `oli-docs/qni0daj9tofmssxzdepllevkgqf/integration-plan.csv` rows
//  IT-001…IT-011. Spec: `oli-docs/qni0daj9tofmssxzdepllevkgqf/spec.md`.
//
//  NOTE: this file's IT-001…IT-015 above belong to a DIFFERENT, already-merged
//  requirement (WWINFRA-3659, serviceDescription advisory). The rows below are a
//  SEPARATE plan (WWINFRA-3829, price validation) that also numbers from IT-001;
//  the two ID spaces are unrelated. These reuse the `validate_listing` + `findings`
//  helpers defined above.
//
//  Conventions (mirror the validate-listing block above):
//    - Every `validate-listing` row is `network_required: offline` — a pure-local
//      validator (no HTTP, no wallet), so it runs directly, NEVER via
//      `run_with_retry`. `validate_listing` already stages an isolated
//      `ONCHAINOS_HOME` sandbox under `cli/target/test_tmp/cli_agent/…` via
//      `fresh_home` + `scrubbed` (not `tempfile::tempdir()`).
//    - Offline pass/findings assertions are DETERMINISTIC (the validator is a pure
//      function of its input), so exact `pass` / finding-code assertions are correct.
//    - The two `create` / `update` rows are `network_required: live` AND wallet-gated:
//      `create_impl`/`update_impl` run auth + signing-session load BEFORE
//      `normalize_service`, so the FE-PRICE-02 bail is only reachable with real creds.
//      They are `#[ignore]`d and, as live rows, go through the project's
//      `run_with_retry` helper (rate-limit tolerance) rather than a bare invocation.
//    - No environment-specific base URL or hostname is hardcoded anywhere.
// ════════════════════════════════════════════════════════════════════════════

/// True when `findings` carries at least one finding whose `code` equals `code`.
/// Reads the shared `findings` helper above, so a wrong envelope shape panics with
/// the full JSON rather than silently reporting "not present".
//  agent validate-listing — FE-EP-01 A2MCP endpoint duplicate-prevention
//
//  Covers integration-plan.csv rows IT-001..IT-015 for the A-side (ASP) A2MCP
//  Endpoint duplicate check (rule EP1, spec §1.4/§2.x; branch
//  feat/a2mcp-endpoint-dedup). Every row is `network_required: offline` and
//  `exit_code 0` — findings are surfaced as `{ pass, findings }` data, never a
//  process error — so each is driven directly via `validate_listing_with` and
//  never through `run_with_retry`. These sit alongside the earlier
//  description-rule `validate_listing_*` tests; the CSV test_id is noted per fn.
// ════════════════════════════════════════════════════════════════════════════

/// True when `result.findings` contains at least one finding carrying `code`.
/// Run `agent validate-listing` with the exact flags specified by a test row.
/// This is needed by endpoint-dedup cases that omit `--service` or override
/// the default role.
fn validate_listing_with(extra_args: &[&str]) -> Value {
    let (_home, dir) = fresh_home("cli_agent");
    let mut cmd = onchainos();
    scrubbed(&mut cmd, &dir);
    let mut args: Vec<&str> = vec!["agent", "validate-listing"];
    args.extend_from_slice(extra_args);
    let output = cmd
        .args(&args)
        .output()
        .expect("failed to execute `onchainos agent validate-listing`");

    assert_eq!(
        output.status.code(),
        Some(0),
        "validate-listing must exit 0 (findings are data, not a process error)\nstdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );
    parse_stdout_json(&output)
}
fn has_finding_code(result: &Value, code: &str) -> bool {
    findings(result).iter().any(|f| f["code"] == code)
}

// ── IT-001: an A2A monthly subscription priced at 10 USDT passes ──────────────
//   Golden path — a priced subscription tier does not fire FE-PRICE-01/02 (§6.2, AC#5).
#[test]
fn validate_listing_a2a_subscription_priced_passes() {
    let result = validate_listing(
        "asp",
        r#"[{"serviceName":"DEX Arbitrage Signals","serviceDescription":"Provides DEX arbitrage trading signals\nUser provides the target chain and budget\nDelivers structured signals","serviceGuide":"Choose a market and submit your budget.","serviceType":"A2A","fee":"","subscription":[{"interval":"month","fee":"10"}]}]"#,
    );
    assert_eq!(
        result["pass"].as_bool(),
        Some(true),
        "an A2A subscription priced at 10 USDT must pass, got {result}"
    );
}

// ── IT-002: a blank A2A subscription price is flagged PRICE_EMPTY (FE-PRICE-01) ─
//   The empty tier raises PRICE_EMPTY (spec §6.1/§6.4, AC#2).
#[test]
fn validate_listing_a2a_subscription_empty_price_flags_price_empty() {
    let result = validate_listing(
        "asp",
        r#"[{"serviceName":"DEX Arbitrage Signals","serviceDescription":"Provides DEX arbitrage trading signals\nUser provides the target chain and budget\nDelivers structured signals","serviceGuide":"Choose a market and submit your budget.","serviceType":"A2A","fee":"","subscription":[{"interval":"month","fee":""}]}]"#,
    );
    assert!(
        has_finding_code(&result, "PRICE_EMPTY"),
        "a blank A2A subscription price must raise a PRICE_EMPTY finding, got {result}"
    );
}

// ── IT-003: a spaces-only A2A subscription price is treated as empty ───────────
//   FE-PRICE-01 whitespace-trim (spec §6.1): the fee trims to "" and raises PRICE_EMPTY.
#[test]
fn validate_listing_a2a_subscription_whitespace_price_flags_price_empty() {
    let result = validate_listing(
        "asp",
        r#"[{"serviceName":"DEX Arbitrage Signals","serviceDescription":"Provides DEX arbitrage trading signals\nUser provides the target chain and budget\nDelivers structured signals","serviceGuide":"Choose a market and submit your budget.","serviceType":"A2A","fee":"","subscription":[{"interval":"month","fee":"  "}]}]"#,
    );
    assert!(
        has_finding_code(&result, "PRICE_EMPTY"),
        "a spaces-only A2A subscription price must trim to empty and raise PRICE_EMPTY, got {result}"
    );
}

// ── IT-004: an A2A subscription price of 0 is blocked (FE-PRICE-02) ────────────
//   The greater-than-zero rule → SUBSCRIPTION_PRICE_ZERO finding (spec §6.2, AC#3).
#[test]
fn validate_listing_a2a_subscription_zero_price_flags_price_zero() {
    let result = validate_listing(
        "asp",
        r#"[{"serviceName":"DEX Arbitrage Signals","serviceDescription":"Provides DEX arbitrage trading signals\nUser provides the target chain and budget\nDelivers structured signals","serviceGuide":"Choose a market and submit your budget.","serviceType":"A2A","fee":"","subscription":[{"interval":"month","fee":"0"}]}]"#,
    );
    assert!(
        has_finding_code(&result, "SUBSCRIPTION_PRICE_ZERO"),
        "an A2A subscription price of 0 must raise SUBSCRIPTION_PRICE_ZERO, got {result}"
    );
}

// ── IT-005: an A2A subscription price of 0.00 is recognised as zero ────────────
//   FE-PRICE-02 zero-form (spec §6.2/§15.2): is_zero_value matches 0.00.
#[test]
fn validate_listing_a2a_subscription_zero_decimal_price_flags_price_zero() {
    let result = validate_listing(
        "asp",
        r#"[{"serviceName":"DEX Arbitrage Signals","serviceDescription":"Provides DEX arbitrage trading signals\nUser provides the target chain and budget\nDelivers structured signals","serviceGuide":"Choose a market and submit your budget.","serviceType":"A2A","fee":"","subscription":[{"interval":"month","fee":"0.00"}]}]"#,
    );
    assert!(
        has_finding_code(&result, "SUBSCRIPTION_PRICE_ZERO"),
        "an A2A subscription price of 0.00 must raise SUBSCRIPTION_PRICE_ZERO, got {result}"
    );
}

// ── IT-006: a six-decimal zero (0.000000) subscription price is blocked as a
//   format violation, not SUBSCRIPTION_PRICE_ZERO ──────────────────────────────
//   A2A subscription fees now cap at 2 decimals (skills-v2), so a 6-decimal
//   value fails the format check (P5) before the zero check ever runs — the
//   original max-precision-zero boundary (spec §6.2/§15.2) is superseded by
//   this tighter cap; the 2-decimal zero boundary is covered by
//   `validate_listing_a2a_subscription_zero_decimal_price_flags_price_zero`.
#[test]
fn validate_listing_a2a_subscription_six_decimal_price_flags_format_not_zero() {
    let result = validate_listing(
        "asp",
        r#"[{"serviceName":"DEX Arbitrage Signals","serviceDescription":"Provides DEX arbitrage trading signals\nUser provides the target chain and budget\nDelivers structured signals","serviceGuide":"Choose a market and submit your budget.","serviceType":"A2A","fee":"","subscription":[{"interval":"month","fee":"0.000000"}]}]"#,
    );
    assert!(
        has_finding_code(&result, "P5"),
        "an A2A subscription price of 0.000000 exceeds the 2-decimal cap and must raise P5, got {result}"
    );
    assert!(
        !has_finding_code(&result, "SUBSCRIPTION_PRICE_ZERO"),
        "the format check must short-circuit before the zero check, got {result}"
    );
}

// ── IT-007: an A2A subscription priced at 0.01 USDT is accepted ────────────────
//   FE-PRICE-02 lower boundary — the smallest positive price passes (spec §6.2, AC#5).
#[test]
fn validate_listing_a2a_subscription_min_positive_price_passes() {
    let result = validate_listing(
        "asp",
        r#"[{"serviceName":"DEX Arbitrage Signals","serviceDescription":"Provides DEX arbitrage trading signals\nUser provides the target chain and budget\nDelivers structured signals","serviceGuide":"Choose a market and submit your budget.","serviceType":"A2A","fee":"","subscription":[{"interval":"month","fee":"0.01"}]}]"#,
    );
    assert_eq!(
        result["pass"].as_bool(),
        Some(true),
        "an A2A subscription priced at 0.01 USDT must pass, got {result}"
    );
}

// ── IT-008: a one-off (non-subscription) A2A service priced at 0 is allowed ────
//   The greater-than-zero rule is subscription-only → no SUBSCRIPTION_PRICE_ZERO
//   for a single-purchase fee of 0 (spec §6.2 scope, AC#4).
#[test]
fn validate_listing_a2a_one_off_zero_fee_not_flagged_price_zero() {
    let result = validate_listing(
        "asp",
        r#"[{"serviceName":"DEX Arbitrage Signals","serviceDescription":"Provides DEX arbitrage trading signals\nUser provides the target chain and budget\nDelivers structured signals","serviceType":"A2A","fee":"0"}]"#,
    );
    assert!(
        !has_finding_code(&result, "SUBSCRIPTION_PRICE_ZERO"),
        "a one-off A2A fee of 0 must not raise SUBSCRIPTION_PRICE_ZERO, got {result}"
    );
}

// ── IT-009: a free (0-priced) A2MCP service is unaffected by the new rules ─────
//   A2MCP is out of scope, so neither PRICE_EMPTY nor SUBSCRIPTION_PRICE_ZERO
//   fires (spec §13, AC#13).
#[test]
fn validate_listing_a2mcp_zero_fee_unaffected_by_price_rules() {
    let result = validate_listing(
        "asp",
        r#"[{"serviceName":"Realtime Price Feed","serviceDescription":"Returns realtime token price quotes\ntokenAddress (string, required): token contract; chainIndex (string, required): chain id\nPOST","serviceType":"A2MCP","fee":"0","endpoint":"https://api.example.com/mcp"}]"#,
    );
    assert!(
        !has_finding_code(&result, "SUBSCRIPTION_PRICE_ZERO"),
        "a free A2MCP service must not raise SUBSCRIPTION_PRICE_ZERO, got {result}"
    );
    assert!(
        !has_finding_code(&result, "PRICE_EMPTY"),
        "a free A2MCP service must not raise PRICE_EMPTY, got {result}"
    );
}

// ════════════════════════════════════════════════════════════════════════════
//  agent create / update — FE-PRICE-02 strict path (live, wallet-gated)
// ════════════════════════════════════════════════════════════════════════════
//  Both rows are `network_required: live` and require a logged-in test wallet:
//  auth + signing-session load run BEFORE `normalize_service` in
//  `create_impl`/`update_impl`, so the FE-PRICE-02 subscription-price bail is only
//  reachable with real creds. They are `#[ignore]`d so CI (no wallet) does not fail
//  on the earlier auth error; run them explicitly with `cargo test -- --ignored`
//  against a wallet. As live rows they go through `run_with_retry` for rate-limit
//  tolerance (the create/update-agent endpoints are rate-limited).

// ── IT-010: `agent create` with a 0 A2A subscription price is rejected ─────────
//   FE-PRICE-02 on the strict create path → {ok:false} exit 1 (spec §3, AC#3).
#[test]
#[ignore = "live: requires a logged-in test wallet — auth/signing runs before normalize_service, so the FE-PRICE-02 subscription-price bail is only reachable with creds"]
fn agent_create_a2a_subscription_zero_price_rejected() {
    let output = run_with_retry(&[
        "agent",
        "create",
        "--role",
        "asp",
        "--name",
        "Arb Signals Bot",
        "--description",
        "DEX arbitrage trading signal provider",
        "--service",
        r#"[{"serviceName":"DEX Arbitrage Signals","serviceDescription":"Provides DEX arbitrage trading signals\nUser provides the target chain and budget\nDelivers structured signals","serviceType":"A2A","fee":"","subscription":[{"interval":"month","fee":"0"}]}]"#,
    ]);
    assert_eq!(
        output.status.code(),
        Some(1),
        "expected exit 1\nstdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );
    let parsed = parse_stdout_json(&output);
    assert_eq!(
        parsed["ok"].as_bool(),
        Some(false),
        "expected {{ok:false}} on the strict create path, got {parsed}"
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("must be greater than 0"),
        "expected the FE-PRICE-02 subscription-price message, got: {stdout}"
    );
}

// ── IT-011: `agent update` to a 0 A2A subscription price is rejected ───────────
//   FE-PRICE-02 on the strict update path → {ok:false} exit 1 (spec §3, AC#7).
#[test]
#[ignore = "live: requires a logged-in test wallet — auth/signing runs before normalize_service, so the FE-PRICE-02 subscription-price bail is only reachable with creds"]
fn agent_update_a2a_subscription_zero_price_rejected() {
    let output = run_with_retry(&[
        "agent",
        "update",
        "--agent-id",
        "12345",
        "--service",
        r#"[{"operation":"update","id":"7","serviceName":"DEX Arbitrage Signals","serviceDescription":"Provides DEX arbitrage trading signals\nUser provides the target chain and budget\nDelivers structured signals","serviceType":"A2A","fee":"","subscription":[{"interval":"month","fee":"0"}]}]"#,
    ]);
    assert_eq!(
        output.status.code(),
        Some(1),
        "expected exit 1\nstdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );
    let parsed = parse_stdout_json(&output);
    assert_eq!(
        parsed["ok"].as_bool(),
        Some(false),
        "expected {{ok:false}} on the strict update path, got {parsed}"
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("must be greater than 0"),
        "expected the FE-PRICE-02 subscription-price message, got: {stdout}"
    );
}

// ── IT-001 · golden: two A2MCP services sharing one Endpoint → second rejected ─
//   Core EP1 finding on service[1].endpoint, pass:false (spec §2.2). Mirrors unit
//   test a2mcp_duplicate_endpoints_fail_ep1.
#[test]
fn validate_listing_a2mcp_duplicate_endpoints_flag_ep1() {
    let result = validate_listing_with(&[
        "--service",
        r#"[{"serviceName":"Chain Data Service","serviceDescription":"Provides market data.","fee":"10","serviceType":"A2MCP","endpoint":"https://api.example.com/mcp"},{"serviceName":"Market Data Service","serviceDescription":"Provides market data.","fee":"10","serviceType":"A2MCP","endpoint":"https://api.example.com/mcp"}]"#,
    ]);
    assert_eq!(
        findings(&result).first().map(|f| &f["code"]),
        Some(&Value::String("EP1".into())),
        "expected findings[0].code == \"EP1\" for a duplicate endpoint, got {result}"
    );
    assert_eq!(
        result["pass"].as_bool(),
        Some(false),
        "a duplicate A2MCP endpoint must fail the listing, got {result}"
    );
}

// ── IT-002 · golden: two A2MCP services with DIFFERENT Endpoints both accepted ─
//   Happy path, no EP1 finding (spec §2.1). Mirrors a2mcp_different_endpoints_pass.
#[test]
fn validate_listing_a2mcp_different_endpoints_pass() {
    let result = validate_listing_with(&[
        "--service",
        r#"[{"serviceName":"Chain Data Service","serviceDescription":"Provides market data.","fee":"10","serviceType":"A2MCP","endpoint":"https://api.example.com/mcp"},{"serviceName":"Market Data Service","serviceDescription":"Provides market data.","fee":"10","serviceType":"A2MCP","endpoint":"https://data.example.com/mcp"}]"#,
    ]);
    assert_eq!(
        result["pass"].as_bool(),
        Some(true),
        "distinct A2MCP endpoints must pass, got {result}"
    );
    assert!(
        !has_finding_code(&result, "EP1"),
        "no duplicate-endpoint finding expected for distinct endpoints, got {result}"
    );
}

// ── IT-003 · edge: Endpoints differing only in letter-case are still duplicates ─
//   Case-insensitive compare via eq_ignore_ascii_case (spec §1.4). Mirrors
//   a2mcp_duplicate_endpoints_case_insensitive.
#[test]
fn validate_listing_a2mcp_duplicate_endpoints_case_insensitive() {
    let result = validate_listing_with(&[
        "--service",
        r#"[{"serviceName":"Chain Data Service","serviceDescription":"Provides market data.","fee":"10","serviceType":"A2MCP","endpoint":"https://api.example.com/mcp"},{"serviceName":"Market Data Service","serviceDescription":"Provides market data.","fee":"10","serviceType":"A2MCP","endpoint":"https://API.EXAMPLE.COM/mcp"}]"#,
    ]);
    assert_eq!(
        findings(&result).first().map(|f| &f["code"]),
        Some(&Value::String("EP1".into())),
        "a case-only endpoint difference must still flag EP1, got {result}"
    );
    assert_eq!(
        result["pass"].as_bool(),
        Some(false),
        "a case-insensitive duplicate endpoint must fail the listing, got {result}"
    );
}

// ── IT-004 · edge: editing a service but keeping its own Endpoint is not flagged ─
//   Self-exclusion keyed by service id (spec §1.5). Mirrors a2mcp_self_exclusion_same_id.
#[test]
fn validate_listing_a2mcp_self_exclusion_same_id_passes() {
    let result = validate_listing_with(&[
        "--service",
        r#"[{"id":"svc-123","serviceName":"Chain Data Service","serviceDescription":"Provides market data.","fee":"10","serviceType":"A2MCP","endpoint":"https://api.example.com/mcp","operation":"update"},{"id":"svc-123","serviceName":"Market Data Service","serviceDescription":"Provides market data.","fee":"10","serviceType":"A2MCP","endpoint":"https://api.example.com/mcp","operation":"update"}]"#,
    ]);
    assert_eq!(
        result["pass"].as_bool(),
        Some(true),
        "two entries with the same id are the same service being edited, not a clash — expected pass, got {result}"
    );
    assert!(
        !has_finding_code(&result, "EP1"),
        "self-exclusion by id must suppress EP1, got {result}"
    );
}

// ── IT-005 · edge: among three services, only the one reusing an earlier Endpoint fails ─
//   All-problems-at-once, no short-circuit; the sole EP1 finding points at
//   service[2].endpoint (spec §2.4). Mirrors a2mcp_three_services_two_collide.
#[test]
fn validate_listing_a2mcp_three_services_second_collision_flagged() {
    let result = validate_listing_with(&[
        "--service",
        r#"[{"serviceName":"Chain Data Service","serviceDescription":"Provides market data.","fee":"10","serviceType":"A2MCP","endpoint":"https://api.example.com/mcp"},{"serviceName":"Market Data Service","serviceDescription":"Provides market data.","fee":"10","serviceType":"A2MCP","endpoint":"https://data.example.com/mcp"},{"serviceName":"Weather Data Service","serviceDescription":"Provides market data.","fee":"10","serviceType":"A2MCP","endpoint":"https://api.example.com/mcp"}]"#,
    ]);
    assert_eq!(
        findings(&result).first().map(|f| &f["field"]),
        Some(&Value::String("service[2].endpoint".into())),
        "expected findings[0].field == \"service[2].endpoint\", got {result}"
    );
    assert_eq!(
        findings(&result).first().map(|f| &f["code"]),
        Some(&Value::String("EP1".into())),
        "the third service reusing the first endpoint must flag EP1, got {result}"
    );
    assert_eq!(
        result["pass"].as_bool(),
        Some(false),
        "a colliding third endpoint must fail the listing, got {result}"
    );
}

// ── IT-006 · edge: a service being deleted frees its Endpoint for another ──────
//   operation:delete is skipped by the dedup pass (spec Appendix A.1). Mirrors
//   a2mcp_delete_operation_skipped.
#[test]
fn validate_listing_a2mcp_delete_operation_frees_endpoint() {
    let result = validate_listing_with(&[
        "--service",
        r#"[{"serviceName":"Chain Data Service","serviceDescription":"Provides market data.","fee":"10","serviceType":"A2MCP","endpoint":"https://api.example.com/mcp"},{"id":"svc-del-1","serviceName":"Market Data Service","serviceDescription":"Provides market data.","fee":"10","serviceType":"A2MCP","endpoint":"https://api.example.com/mcp","operation":"delete"}]"#,
    ]);
    assert_eq!(
        result["pass"].as_bool(),
        Some(true),
        "a delete-op service never claims its endpoint, so the survivor must pass, got {result}"
    );
    assert!(
        !has_finding_code(&result, "EP1"),
        "a deleted service must not trigger an EP1 collision, got {result}"
    );
}

// ── IT-007 · edge: an A2A service (no Endpoint) never clashes with an A2MCP one ─
//   Only A2MCP services with endpoints participate (spec §1.4). Mirrors
//   mixed_a2a_a2mcp_no_cross_type_collision.
#[test]
fn validate_listing_mixed_a2a_a2mcp_no_cross_type_collision() {
    let result = validate_listing_with(&[
        "--service",
        r#"[{"serviceName":"Ledger Info Service","serviceDescription":"Provides market data.","fee":"5","serviceType":"A2A"},{"serviceName":"Market Data Service","serviceDescription":"Provides market data.","fee":"10","serviceType":"A2MCP","endpoint":"https://api.example.com/mcp"}]"#,
    ]);
    assert_eq!(
        result["pass"].as_bool(),
        Some(true),
        "an A2A service has no endpoint to collide with an A2MCP one — expected pass, got {result}"
    );
    assert!(
        !has_finding_code(&result, "EP1"),
        "no cross-type endpoint collision expected, got {result}"
    );
}

// ── IT-008 · golden: the rejection message names the conflicting service ───────
//   EP1 copy must contain "is already used by" AND both service names so the
//   skill layer can recognise it (spec §2.3). The extra `--name IgnoredAgentName`
//   proves the Agent name does not leak into the message. Mirrors
//   ep1_message_contains_both_service_names.
#[test]
fn validate_listing_ep1_message_names_conflicting_service() {
    let result = validate_listing_with(&[
        "--service",
        r#"[{"serviceName":"Chain Data Service","serviceDescription":"Provides market data.","fee":"10","serviceType":"A2MCP","endpoint":"https://api.example.com/mcp"},{"serviceName":"Market Data Service","serviceDescription":"Provides market data.","fee":"10","serviceType":"A2MCP","endpoint":"https://api.example.com/mcp"}]"#,
        "--name",
        "IgnoredAgentName",
    ]);
    assert_eq!(
        result["pass"].as_bool(),
        Some(false),
        "a duplicate endpoint must fail the listing, got {result}"
    );
    let ep1 = findings(&result)
        .iter()
        .find(|f| f["code"] == "EP1")
        .unwrap_or_else(|| panic!("expected an EP1 finding, got {result}"));
    let message = ep1["message"]
        .as_str()
        .unwrap_or_else(|| panic!("EP1 message is not a string: {result}"));
    for needle in [
        "is already used by",
        "Market Data Service",
        "Chain Data Service",
    ] {
        assert!(
            message.contains(needle),
            "EP1 message must contain {needle:?}, got: {message}"
        );
    }
}

// ── IT-009 · error: a duplicate service NAME and duplicate Endpoint both surface ─
//   Rejection accumulates S2 and EP1 in one pass, pass:false (spec §2.4). Mirrors
//   ep1_coexists_with_other_findings.
#[test]
fn validate_listing_ep1_coexists_with_duplicate_name() {
    let result = validate_listing_with(&[
        "--service",
        r#"[{"serviceName":"Repeat Data Service","serviceDescription":"Provides market data.","fee":"10","serviceType":"A2MCP","endpoint":"https://api.example.com/mcp"},{"serviceName":"Repeat Data Service","serviceDescription":"Provides market data.","fee":"10","serviceType":"A2MCP","endpoint":"https://api.example.com/mcp"}]"#,
    ]);
    assert!(
        findings(&result).len() >= 2,
        "expected at least two findings (duplicate name + duplicate endpoint), got {result}"
    );
    assert!(
        has_finding_code(&result, "S2"),
        "expected a duplicate-service-name finding (S2), got {result}"
    );
    assert!(
        has_finding_code(&result, "EP1"),
        "expected a duplicate-endpoint finding (EP1), got {result}"
    );
    assert_eq!(
        result["pass"].as_bool(),
        Some(false),
        "coexisting S2 + EP1 must fail the listing, got {result}"
    );
}

// ── IT-010 · error: invalid service JSON returns a format error, not a crash ───
//   Parse failure yields a PARSE finding (fe::FE13), still exit 0 (spec §3.2).
#[test]
fn validate_listing_invalid_service_json_returns_parse() {
    let result = validate_listing_with(&["--service", "not-json"]);
    assert_eq!(
        findings(&result).first().map(|f| &f["code"]),
        Some(&Value::String("PARSE".into())),
        "invalid service JSON must yield findings[0].code == \"PARSE\", got {result}"
    );
    assert_eq!(
        result["pass"].as_bool(),
        Some(false),
        "unparseable --service must fail the listing, got {result}"
    );
}

// ── IT-011 · edge: for a non-ASP role, service Endpoints are not checked at all ─
//   --service is ignored for user/evaluator; dedup runs only in the asp branch
//   (spec §1.1/§1.3, regression guard).
#[test]
fn validate_listing_user_role_ignores_service_endpoints() {
    let result = validate_listing_with(&[
        "--role",
        "user",
        "--service",
        r#"[{"serviceName":"Chain Data Service","serviceDescription":"Provides market data.","fee":"10","serviceType":"A2MCP","endpoint":"https://api.example.com/mcp"},{"serviceName":"Market Data Service","serviceDescription":"Provides market data.","fee":"10","serviceType":"A2MCP","endpoint":"https://api.example.com/mcp"}]"#,
    ]);
    assert_eq!(
        result["pass"].as_bool(),
        Some(true),
        "a user-role listing ignores --service entirely — expected pass, got {result}"
    );
    assert!(
        findings(&result).is_empty(),
        "no service checks (including EP1) run for a non-ASP role, got {result}"
    );
}

// ── IT-012 · edge: an empty service list is accepted with no errors ────────────
//   Boundary: zero services, no findings (spec §2.1).
#[test]
fn validate_listing_empty_service_list_passes() {
    let result = validate_listing_with(&["--service", "[]"]);
    assert_eq!(
        result["pass"].as_bool(),
        Some(true),
        "an empty service list must pass, got {result}"
    );
    assert!(
        findings(&result).is_empty(),
        "an empty service list raises no findings, got {result}"
    );
}

// ── IT-013 · edge: a single A2MCP service has no Endpoint to collide with ───────
//   Boundary: one service, dedup is a no-op (spec §2.1).
#[test]
fn validate_listing_single_a2mcp_service_passes() {
    let result = validate_listing_with(&[
        "--service",
        r#"[{"serviceName":"Chain Data Service","serviceDescription":"Provides market data.","fee":"10","serviceType":"A2MCP","endpoint":"https://api.example.com/mcp"}]"#,
    ]);
    assert_eq!(
        result["pass"].as_bool(),
        Some(true),
        "a single A2MCP service must pass, got {result}"
    );
    assert!(
        !has_finding_code(&result, "EP1"),
        "a single service cannot collide, so no EP1, got {result}"
    );
}

// ── IT-014 · edge: running the check with no services supplied passes cleanly ──
//   Flag default: --service absent (role defaults to asp), no findings (spec §1.1).
#[test]
fn validate_listing_no_service_flag_passes() {
    let result = validate_listing_with(&[]);
    assert_eq!(
        result["pass"].as_bool(),
        Some(true),
        "no --service supplied must pass, got {result}"
    );
    assert!(
        findings(&result).is_empty(),
        "an absent --service raises no findings, got {result}"
    );
}

// ── IT-015 · edge: Endpoints differing only by surrounding spaces are duplicates ─
//   parse_services_lenient trims before comparison (spec §1.4 whitespace
//   normalization).
#[test]
fn validate_listing_a2mcp_endpoints_differ_only_by_whitespace_flag_ep1() {
    let result = validate_listing_with(&[
        "--service",
        r#"[{"serviceName":"Chain Data Service","serviceDescription":"Provides market data.","fee":"10","serviceType":"A2MCP","endpoint":"https://data.example.com/mcp"},{"serviceName":"Market Data Service","serviceDescription":"Provides market data.","fee":"10","serviceType":"A2MCP","endpoint":" https://data.example.com/mcp "}]"#,
    ]);
    assert_eq!(
        findings(&result).first().map(|f| &f["code"]),
        Some(&Value::String("EP1".into())),
        "endpoints differing only by surrounding whitespace must flag EP1, got {result}"
    );
    assert_eq!(
        result["pass"].as_bool(),
        Some(false),
        "a whitespace-only endpoint difference must fail the listing, got {result}"
    );
}
