//! Integration coverage for `onchainos agent list-jobs-by-verified-sender`.
//!
//! The offline transport is deliberately per-command: the handler accepts a
//! raw backend response through `ONCHAINOS_TEST_MOCK_LIST_JOBS_BY_VERIFIED_SENDER_JSON`
//! or an injected error through `ONCHAINOS_TEST_MOCK_LIST_JOBS_BY_VERIFIED_SENDER_ERROR`.
//! No test mutates process-global environment state or needs credentials.

mod common;

use common::{fresh_home, onchainos, parse_stdout_json, scrubbed};
use predicates::prelude::*;

const MOCK_JSON: &str = "ONCHAINOS_TEST_MOCK_LIST_JOBS_BY_VERIFIED_SENDER_JSON";
const MOCK_ERROR: &str = "ONCHAINOS_TEST_MOCK_LIST_JOBS_BY_VERIFIED_SENDER_ERROR";

fn command_with_home(stem: &str) -> (common::TestHome, assert_cmd::Command) {
    let (guard, home) = fresh_home(stem);
    let mut command = onchainos();
    scrubbed(&mut command, &home);
    (guard, command)
}

fn list_args<'a>(
    group_id: &'a str,
    sender_inbox_id: &'a str,
    agentic_id: &'a str,
) -> [&'a str; 8] {
    [
        "agent",
        "list-jobs-by-verified-sender",
        "--group-id",
        group_id,
        "--sender-inbox-id",
        sender_inbox_id,
        "--agentic-id",
        agentic_id,
    ]
}

#[test]
fn list_jobs_help_advertises_the_exact_command_and_required_flags() {
    onchainos()
        .args(["agent", "list-jobs-by-verified-sender", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("list-jobs-by-verified-sender"))
        .stdout(predicate::str::contains("--group-id"))
        .stdout(predicate::str::contains("--sender-inbox-id"))
        .stdout(predicate::str::contains("--agentic-id"));
}

#[test]
fn list_jobs_requires_all_three_long_flags() {
    onchainos()
        .args([
            "agent",
            "list-jobs-by-verified-sender",
            "--group-id",
            "group-1",
        ])
        .assert()
        .code(2)
        .stderr(predicate::str::contains("--sender-inbox-id"))
        .stderr(predicate::str::contains("--agentic-id"));
}

#[test]
fn list_jobs_rejects_whitespace_identifiers_before_transport() {
    for (label, group_id, sender_inbox_id, agentic_id) in [
        ("group", "   ", "inbox-1", "agent-1"),
        ("sender inbox", "group-1", "\t", "agent-1"),
        ("agentic", "group-1", "inbox-1", "\n"),
    ] {
        let (_guard, mut command) = command_with_home(&format!("list-jobs-whitespace-{label}"));
        let output = command
            .env(MOCK_JSON, r#"{"jobs":[]}"#)
            .args(list_args(group_id, sender_inbox_id, agentic_id))
            .output()
            .expect("execute whitespace validation case");

        assert!(!output.status.success(), "{label} whitespace must fail closed");
        let stdout = String::from_utf8_lossy(&output.stdout);
        assert!(
            stdout.contains("\"ok\":false") || stdout.contains("\"ok\": false"),
            "{label} validation must retain the standard error envelope; stdout: {stdout}"
        );
    }
}

#[test]
fn list_jobs_complete_argv_returns_compact_success_envelope() {
    let (_guard, mut command) = command_with_home("list-jobs-success");
    let output = command
        .env(
            MOCK_JSON,
            r#"{"jobs":[{"jobId":"job-1","userAgentId":"user-1","aspAgentId":"asp-1"}]}"#,
        )
        .args(list_args(" group +&?中文 ", " inbox +&?中文 ", " agent-1 "))
        .output()
        .expect("execute successful list jobs command");

    assert!(
        output.status.success(),
        "complete argv must reach the handler\nstdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        !stdout.trim_end().contains('\n'),
        "success JSON must be a single compact line: {stdout}"
    );
    let envelope = parse_stdout_json(&output);
    assert_eq!(envelope["ok"].as_bool(), Some(true));
    assert_eq!(envelope["data"]["jobs"][0]["jobId"].as_str(), Some("job-1"));
    assert_eq!(envelope["data"]["jobs"][0]["userAgentId"].as_str(), Some("user-1"));
    assert_eq!(envelope["data"]["jobs"][0]["aspAgentId"].as_str(), Some("asp-1"));
}

#[test]
fn list_jobs_allows_an_empty_jobs_array() {
    let (_guard, mut command) = command_with_home("list-jobs-empty");
    let output = command
        .env(MOCK_JSON, r#"{"jobs":[]}"#)
        .args(list_args("group-1", "inbox-1", "agent-1"))
        .output()
        .expect("execute empty list jobs command");

    assert!(output.status.success());
    let envelope = parse_stdout_json(&output);
    assert_eq!(envelope["ok"].as_bool(), Some(true));
    assert_eq!(envelope["data"]["jobs"].as_array().map(Vec::len), Some(0));
}

#[test]
fn list_jobs_preserves_injected_business_auth_and_network_failures() {
    for error in ["business code 130100073", "HTTP 401 unauthorized", "network unavailable"] {
        let (_guard, mut command) = command_with_home("list-jobs-error");
        let output = command
            .env(MOCK_ERROR, error)
            .args(list_args("group-1", "inbox-1", "agent-1"))
            .output()
            .expect("execute injected list jobs error");

        assert!(!output.status.success(), "{error} must remain a failure");
        let combined = format!(
            "{}{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(combined.contains(error), "error must remain classifiable: {combined}");
        assert!(combined.contains("\"ok\":false") || combined.contains("\"ok\": false"));
    }
}

#[test]
fn list_jobs_rejects_schema_errors_fail_closed() {
    for body in [
        r#"{}"#,
        r#"{"jobs":{}}"#,
        r#"{"jobs":[{"jobId":"job-1","userAgentId":"user-1"}]}"#,
        r#"{"jobs":[{"jobId":"job-1","userAgentId":"user-1","aspAgentId":" "}]}"#,
        "not-json",
    ] {
        let (_guard, mut command) = command_with_home("list-jobs-schema");
        let output = command
            .env(MOCK_JSON, body)
            .args(list_args("group-1", "inbox-1", "agent-1"))
            .output()
            .expect("execute malformed list jobs response");

        assert!(!output.status.success(), "schema body must fail closed: {body}");
        let stdout = String::from_utf8_lossy(&output.stdout);
        assert!(stdout.contains("\"ok\":false") || stdout.contains("\"ok\": false"));
    }
}
