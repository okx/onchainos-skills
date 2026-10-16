//! Integration tests — `onchainos agent subscription-push` fan-out to a
//! Service's subscription group conversations.
//!
//! ─── Contract under test (frozen) ─────────────────────────────────────────────
//!   Command: `onchainos agent subscription-push --agent-id <id> --service-id <id>
//!             --content <text>`.
//!
//!   Debug/no-backend mode is gated by `ONCHAINOS_TEST_MOCK_SUBSCRIPTION_PUSH=1`
//!   (active because the test binary is built in debug — `#[cfg(debug_assertions)]`).
//!   The group-list response `data` is supplied by env:
//!     - `ONCHAINOS_TEST_MOCK_GROUPS_JSON`  — inline JSON, or
//!     - `ONCHAINOS_TEST_MOCK_GROUPS_FILE`  — path to a JSON file.
//!   Both unset under mock mode → treated as `{"conversations":[]}`. A `data`
//!   value whose top-level `conversations` key is ABSENT (or not an array) is
//!   unparseable → fail-closed (non-zero exit, no success JSON, no send).
//!   `ONCHAINOS_TEST_MOCK_PUSH_FAIL=<conversationId[,convId...]>` injects a
//!   send failure for those conversations.
//!
//!   Success stdout is a single JSON object with keys `ok` (true),
//!   `conversationCount`, `sentCount`, `failedCount`, `skippedCount`, and
//!   `results[]` — each `{ conversationId, ok, messageId?|error? }`.
//!
//!   Fail-closed (empty required arg / query failure / unparseable groups) →
//!   NON-ZERO exit, NO success JSON on stdout, NO send.
//!
//!   Conversation is kept iff `conversationId` is non-empty AND `status` ∈
//!   {active, full} (case-insensitive); otherwise it is isolated into
//!   `skippedCount`; duplicates are deduped by `conversationId`.
//!
//! ─── Conventions ──────────────────────────────────────────────────────────────
//!   - Every row runs the built `onchainos` binary directly via the shared
//!     `onchainos()` helper (mock mode is offline — no HTTP, no wallet), never
//!     through `run_with_retry`.
//!   - Env vars are set per-invocation with `Command::env` (never
//!     `std::env::set_var`, which is process-global and races parallel tests).
//!   - Success-path stdout JSON is parsed with `parse_stdout_json`; fail-closed
//!     rows assert a non-zero exit and the ABSENCE of `"ok":true` in stdout.
//!   - The fixture is resolved relative to `CARGO_MANIFEST_DIR` and passed as an
//!     absolute path via `ONCHAINOS_TEST_MOCK_GROUPS_FILE`.

mod common;

use common::{fresh_home, onchainos, parse_stdout_json, scrubbed};
use serde_json::Value;

/// Absolute path to the shared groups fixture, resolved from the crate manifest
/// dir so the test is independent of the process working directory.
fn groups_fixture_path() -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join("subscription-push-groups.json")
}

/// Collect the `results[]` conversationIds from a success envelope.
fn result_conversation_ids(result: &Value) -> Vec<String> {
    result["results"]
        .as_array()
        .unwrap_or_else(|| panic!("`results` is not an array: {result}"))
        .iter()
        .map(|entry| {
            entry["conversationId"]
                .as_str()
                .unwrap_or_else(|| panic!("result entry missing conversationId: {result}"))
                .to_string()
        })
        .collect()
}

// ── push_01: an empty --agent-id fails closed before any send ─────────────────
//   Validation of a required identifier is fail-closed: non-zero exit and no
//   success JSON on stdout (so no `"ok":true`), and therefore no send.
#[test]
fn push_01_fail_closed_missing_agent_id() {
    let output = onchainos()
        .env("ONCHAINOS_TEST_MOCK_SUBSCRIPTION_PUSH", "1")
        .args([
            "agent",
            "subscription-push",
            "--agent-id",
            "",
            "--service-id",
            "svc-1",
            "--content",
            "hi",
        ])
        .output()
        .expect("run subscription-push with empty --agent-id");

    assert_ne!(
        output.status.code(),
        Some(0),
        "an empty --agent-id must fail closed\nstdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        !stdout.contains("\"ok\":true") && !stdout.contains("\"ok\": true"),
        "fail-closed validation must not emit a success envelope, got: {stdout}"
    );
}

// ── push_02: an empty target set succeeds with all-zero counts ────────────────
//   No conversations to fan out to → exit 0 with a well-formed zero envelope.
#[test]
fn push_02_empty_targets() {
    let output = onchainos()
        .env("ONCHAINOS_TEST_MOCK_SUBSCRIPTION_PUSH", "1")
        .env("ONCHAINOS_TEST_MOCK_GROUPS_JSON", r#"{"conversations":[]}"#)
        .args([
            "agent",
            "subscription-push",
            "--agent-id",
            "8315",
            "--service-id",
            "svc-1",
            "--content",
            "hi",
        ])
        .output()
        .expect("run subscription-push with an empty group list");

    assert_eq!(
        output.status.code(),
        Some(0),
        "an empty target set must exit 0\nstdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );
    let result = parse_stdout_json(&output);
    assert_eq!(
        result["ok"].as_bool(),
        Some(true),
        "expected ok:true, got {result}"
    );
    assert_eq!(
        result["conversationCount"].as_i64(),
        Some(0),
        "expected conversationCount:0, got {result}"
    );
    assert_eq!(
        result["sentCount"].as_i64(),
        Some(0),
        "expected sentCount:0, got {result}"
    );
    assert_eq!(
        result["failedCount"].as_i64(),
        Some(0),
        "expected failedCount:0, got {result}"
    );
}

// ── push_03: dedup + isolate invalid, then fan out to the survivors ───────────
//   The fixture carries 5 raw rows: {conv-active-1, conv-full-1} are kept
//   (active/full status, non-empty id); a duplicate conv-active-1, a row missing
//   conversationId, and an illegal `archived` status are all isolated. So unique
//   kept == 2 and skipped >= 3. Each kept conversation appears exactly once in
//   results and every send succeeds (no failure injection).
#[test]
fn push_03_dedup_isolate_parallel() {
    let fixture = groups_fixture_path();
    let output = onchainos()
        .env("ONCHAINOS_TEST_MOCK_SUBSCRIPTION_PUSH", "1")
        .env(
            "ONCHAINOS_TEST_MOCK_GROUPS_FILE",
            fixture.to_str().expect("utf8 fixture path"),
        )
        .args([
            "agent",
            "subscription-push",
            "--agent-id",
            "8315",
            "--service-id",
            "svc-1",
            "--content",
            "hello subscribers",
        ])
        .output()
        .expect("run subscription-push against the shared fixture");

    assert_eq!(
        output.status.code(),
        Some(0),
        "a valid fan-out must exit 0\nstdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );
    let result = parse_stdout_json(&output);
    assert_eq!(
        result["ok"].as_bool(),
        Some(true),
        "expected ok:true, got {result}"
    );
    assert_eq!(
        result["conversationCount"].as_i64(),
        Some(2),
        "unique kept conversations must be 2 (conv-active-1, conv-full-1), got {result}"
    );
    assert!(
        result["skippedCount"].as_i64().unwrap_or(0) >= 3,
        "duplicate + missing-id + illegal-status rows must be isolated (skipped >= 3), got {result}"
    );
    assert_eq!(
        result["sentCount"].as_i64(),
        Some(2),
        "both kept conversations must receive the push, got {result}"
    );

    let ids = result_conversation_ids(&result);
    assert_eq!(
        ids.len(),
        2,
        "results must carry exactly the two surviving conversations, got {result}"
    );
    for expected in ["conv-active-1", "conv-full-1"] {
        assert_eq!(
            ids.iter().filter(|id| id.as_str() == expected).count(),
            1,
            "{expected:?} must appear exactly once in results (deduped), got {result}"
        );
    }
}

// ── push_04: one conversation's send fails; the rest still deliver ────────────
//   Failure injection on conv-full-1 → sentCount 1, failedCount 1, and the
//   conv-full-1 result carries ok:false plus an `error`. The command reports the
//   per-conversation failure and must not fall back to any "deliver" wording.
#[test]
fn push_04_partial_failure() {
    let fixture = groups_fixture_path();
    let output = onchainos()
        .env("ONCHAINOS_TEST_MOCK_SUBSCRIPTION_PUSH", "1")
        .env(
            "ONCHAINOS_TEST_MOCK_GROUPS_FILE",
            fixture.to_str().expect("utf8 fixture path"),
        )
        .env("ONCHAINOS_TEST_MOCK_PUSH_FAIL", "conv-full-1")
        .args([
            "agent",
            "subscription-push",
            "--agent-id",
            "8315",
            "--service-id",
            "svc-1",
            "--content",
            "partial delivery test",
        ])
        .output()
        .expect("run subscription-push with an injected send failure");

    assert_eq!(
        output.status.code(),
        Some(0),
        "a partial failure is still a completed fan-out (exit 0)\nstdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );
    let result = parse_stdout_json(&output);
    assert_eq!(
        result["ok"].as_bool(),
        Some(true),
        "expected ok:true, got {result}"
    );
    assert_eq!(
        result["sentCount"].as_i64(),
        Some(1),
        "exactly one conversation must succeed, got {result}"
    );
    assert_eq!(
        result["failedCount"].as_i64(),
        Some(1),
        "exactly one conversation must fail, got {result}"
    );

    let entries = result["results"]
        .as_array()
        .unwrap_or_else(|| panic!("`results` is not an array: {result}"));
    let failed = entries
        .iter()
        .find(|entry| entry["conversationId"] == "conv-full-1")
        .unwrap_or_else(|| panic!("expected a conv-full-1 result entry, got {result}"));
    assert_eq!(
        failed["ok"].as_bool(),
        Some(false),
        "conv-full-1 must report ok:false, got {result}"
    );
    assert!(
        failed["error"].is_string(),
        "a failed conversation must carry an `error` string, got {result}"
    );

    // The subscription-push path must not silently fall back to the one-time
    // `deliver` wording.
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        !stdout.contains("deliver"),
        "subscription-push must not emit a `deliver` fallback, got: {stdout}"
    );
}

// ── push_05: an unparseable group list fails closed ───────────────────────────
//   The `data` value lacks a top-level `conversations` array → unparseable →
//   non-zero exit with no success envelope and no send.
#[test]
fn push_05_unparseable_fail_closed() {
    let output = onchainos()
        .env("ONCHAINOS_TEST_MOCK_SUBSCRIPTION_PUSH", "1")
        .env("ONCHAINOS_TEST_MOCK_GROUPS_JSON", r#"{"unexpected":true}"#)
        .args([
            "agent",
            "subscription-push",
            "--agent-id",
            "8315",
            "--service-id",
            "svc-1",
            "--content",
            "hi",
        ])
        .output()
        .expect("run subscription-push with an unparseable group list");

    assert_ne!(
        output.status.code(),
        Some(0),
        "an unparseable group list must fail closed\nstdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        !stdout.contains("\"ok\":true") && !stdout.contains("\"ok\": true"),
        "an unparseable group list must not emit a success envelope, got: {stdout}"
    );
}

// ── push_06: agentId is normalized (trimmed) before it reaches the A2A call ────
//   A leading/trailing-whitespace --agent-id must still pass validation (non-empty after
//   trim) and the EXACT trimmed value must be what reaches `--my-agent-id` on the A2A side —
//   observable here because the mock embeds the agent_id it received into `messageId`.
#[test]
fn push_06_agent_id_is_trimmed_before_reaching_a2a() {
    let output = onchainos()
        .env("ONCHAINOS_TEST_MOCK_SUBSCRIPTION_PUSH", "1")
        .env(
            "ONCHAINOS_TEST_MOCK_GROUPS_JSON",
            r#"{"conversations":[{"conversationId":"conv-1","status":"active"}]}"#,
        )
        .args([
            "agent",
            "subscription-push",
            "--agent-id",
            "  agent-1  ",
            "--service-id",
            "svc-1",
            "--content",
            "hi",
        ])
        .output()
        .expect("run subscription-push with a whitespace-padded agent-id");

    assert_eq!(
        output.status.code(),
        Some(0),
        "a whitespace-padded --agent-id must still pass validation\nstdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );
    let result = parse_stdout_json(&output);
    let message_id = result["results"][0]["messageId"]
        .as_str()
        .unwrap_or_else(|| panic!("expected a messageId, got {result}"));
    assert!(
        message_id.contains("agent=agent-1:"),
        "the A2A call must receive the TRIMMED agent-id (agent-1), got messageId: {message_id}"
    );
    assert!(
        !message_id.contains("agent=  agent-1  "),
        "the A2A call must not receive the untrimmed agent-id, got messageId: {message_id}"
    );
}

// ── push_07: content is validated for emptiness after trim, but sent verbatim ─
//   Only `content.trim().is_empty()` gates validation; the exact caller-provided body —
//   including legitimate surrounding whitespace/newlines — must reach the A2A `--message`
//   flag untouched. The mock embeds the received content's byte length into `messageId` so
//   this is observable without a real `okx-a2a` process.
#[test]
fn push_07_content_is_not_trimmed_when_sent_to_a2a() {
    let padded_content = "  hello subscribers  \n";
    let output = onchainos()
        .env("ONCHAINOS_TEST_MOCK_SUBSCRIPTION_PUSH", "1")
        .env(
            "ONCHAINOS_TEST_MOCK_GROUPS_JSON",
            r#"{"conversations":[{"conversationId":"conv-1","status":"active"}]}"#,
        )
        .args([
            "agent",
            "subscription-push",
            "--agent-id",
            "agent-1",
            "--service-id",
            "svc-1",
            "--content",
            padded_content,
        ])
        .output()
        .expect("run subscription-push with a whitespace-padded content body");

    assert_eq!(
        output.status.code(),
        Some(0),
        "a whitespace-padded --content must still pass validation\nstdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );
    let result = parse_stdout_json(&output);
    let message_id = result["results"][0]["messageId"]
        .as_str()
        .unwrap_or_else(|| panic!("expected a messageId, got {result}"));
    assert!(
        message_id.contains(&format!("contentLen={}", padded_content.len())),
        "the A2A call must receive content at its ORIGINAL (untrimmed) length {}, got messageId: {message_id}",
        padded_content.len()
    );
    assert!(
        !message_id.contains(&format!("contentLen={}", padded_content.trim().len())),
        "content must not have been trimmed before reaching A2A, got messageId: {message_id}"
    );
}

// ── push_08: audit tags use the trimmed agentId/serviceId, not the raw input ──
//   The `ASP/subscription_push` audit entry's `args` must carry `agentId=`/`serviceId=` at
//   their trimmed values — never the raw whitespace-padded input the CLI was invoked with.
#[test]
fn push_08_audit_tags_use_trimmed_identifiers() {
    let (_tmp, home) = fresh_home("cli_subscription_push_audit");
    let output = scrubbed(&mut onchainos(), &home)
        .env("ONCHAINOS_TEST_MOCK_SUBSCRIPTION_PUSH", "1")
        .env(
            "ONCHAINOS_TEST_MOCK_GROUPS_JSON",
            r#"{"conversations":[{"conversationId":"conv-1","status":"active"}]}"#,
        )
        .args([
            "agent",
            "subscription-push",
            "--agent-id",
            "  agent-1  ",
            "--service-id",
            "  svc-1  ",
            "--content",
            "hi",
        ])
        .output()
        .expect("run subscription-push under an isolated ONCHAINOS_HOME");

    assert_eq!(
        output.status.code(),
        Some(0),
        "a whitespace-padded agent-id/service-id must still succeed\nstdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );

    let audit_path = home.join("audit.jsonl");
    let audit = std::fs::read_to_string(&audit_path)
        .unwrap_or_else(|e| panic!("read {}: {e}", audit_path.display()));
    let entry: Value = audit
        .lines()
        .filter_map(|line| serde_json::from_str::<Value>(line).ok())
        .find(|entry| entry["command"] == "ASP/subscription_push")
        .unwrap_or_else(|| panic!("no ASP/subscription_push audit entry in: {audit}"));
    let args = entry["args"]
        .as_array()
        .unwrap_or_else(|| panic!("audit entry has no args array: {entry}"));
    let args: Vec<&str> = args.iter().filter_map(Value::as_str).collect();

    assert!(
        args.contains(&"agentId=agent-1"),
        "audit args must carry the trimmed agentId, got: {args:?}"
    );
    assert!(
        args.contains(&"serviceId=svc-1"),
        "audit args must carry the trimmed serviceId, got: {args:?}"
    );
    assert!(
        !args
            .iter()
            .any(|a| a.starts_with("agentId=  ") || a.starts_with("serviceId=  ")),
        "audit args must not carry the raw whitespace-padded identifiers, got: {args:?}"
    );
}
