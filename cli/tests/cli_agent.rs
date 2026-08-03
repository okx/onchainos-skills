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

use common::{fresh_home, onchainos, parse_stdout_json, run_with_retry, scrubbed};
use serde_json::Value;

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
    assert_eq!(result["pass"].as_bool(), Some(true), "expected pass:true, got {result}");
    assert!(
        findings(&result).is_empty(),
        "a well-structured A2A listing should raise no findings, got {result}"
    );
}

// ── IT-002: a mis-structured (2-paragraph) A2A listing now passes with advice ──
//   The paragraph-count D1 is downgraded from block → suggest for A2A, so pass
//   flips to true while the suggestion still surfaces.
#[test]
fn validate_listing_a2a_two_paragraph_non_subscription_suggests() {
    let result = validate_listing(
        "asp",
        r#"[{"serviceName":"DEX Arbitrage Signals","serviceDescription":"Provides DEX arbitrage trading signals\nUser provides the target chain and budget","serviceType":"A2A","fee":"0.11"}]"#,
    );
    assert_eq!(result["pass"].as_bool(), Some(true), "expected pass:true, got {result}");
    assert!(
        findings(&result).iter().any(|f| f["severity"] == "suggest"),
        "expected at least one advisory (suggest) finding, got {result}"
    );
    assert!(
        findings(&result).iter().all(|f| f["severity"] != "block"),
        "no finding should be blocking for a merely mis-structured A2A listing, got {result}"
    );
}

// ── IT-003: a subscription A2A listing with the wrong paragraph count advises ──
//   subscription wants 2 paragraphs; a 3-paragraph body raises a lone `suggest`
//   D1 finding while pass stays true.
#[test]
fn validate_listing_a2a_subscription_wrong_paragraph_count_suggests() {
    let result = validate_listing(
        "asp",
        r#"[{"serviceName":"DEX Arbitrage Signals","serviceDescription":"Provides DEX arbitrage trading signals\nUser provides the target chain and budget\nDelivers structured signals","serviceType":"A2A","fee":"","subscription":[{"interval":"month","fee":"10"}]}]"#,
    );
    assert_eq!(
        findings(&result).first().map(|f| &f["severity"]),
        Some(&Value::String("suggest".into())),
        "expected findings[0].severity == \"suggest\", got {result}"
    );
    assert_eq!(result["pass"].as_bool(), Some(true), "expected pass:true, got {result}");
}

// ── IT-004: an overly long A2A paragraph no longer blocks — length is advisory ─
//   The D2/D3 display-width downgrade to suggest for A2A: the first paragraph
//   exceeds the per-paragraph width limit while the paragraph count is correct,
//   so no block finding is raised and pass stays true.
#[test]
fn validate_listing_a2a_overlong_paragraph_suggests() {
    let result = validate_listing(
        "asp",
        r#"[{"serviceName":"DEX Arbitrage Signals","serviceDescription":"Provides real-time DEX arbitrage trading signals across many chains with detailed routing analysis, slippage estimates, and expected net spread for every opportunity so subscribers can act quickly on each alert delivered throughout the trading day and night without missing any profitable cross-market movement that the monitoring engine continuously detects, ranks, and explains in plain language for confident and fast decisions\nUser provides the target chain and budget\nDelivers structured signals","serviceType":"A2A","fee":"0.11"}]"#,
    );
    assert_eq!(result["pass"].as_bool(), Some(true), "expected pass:true, got {result}");
    assert!(
        findings(&result).iter().all(|f| f["severity"] != "block"),
        "an over-length A2A paragraph must not block, got {result}"
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
    assert_eq!(result["pass"].as_bool(), Some(true), "expected pass:true, got {result}");
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
    assert_eq!(result["pass"].as_bool(), Some(false), "expected pass:false for a URL in the description, got {result}");
}

// ── IT-009: an A2A description containing a 0x address is still rejected ───────
//   Prohibited-content D7 (0x address) stays blocking for A2A.
#[test]
fn validate_listing_a2a_hex_address_blocks() {
    let result = validate_listing(
        "asp",
        r#"[{"serviceName":"DEX Arbitrage Signals","serviceDescription":"Provides signals for token 0x1234567890abcdef pairs\nUser provides the target chain and budget\nDelivers structured signals","serviceType":"A2A","fee":"0.11"}]"#,
    );
    assert_eq!(result["pass"].as_bool(), Some(false), "expected pass:false for a 0x address in the description, got {result}");
}

// ── IT-010: an A2A description promising guaranteed profit is still rejected ───
//   Prohibited-content D9 (profit/return guarantee) stays blocking for A2A.
#[test]
fn validate_listing_a2a_profit_guarantee_blocks() {
    let result = validate_listing(
        "asp",
        r#"[{"serviceName":"DEX Arbitrage Signals","serviceDescription":"Guaranteed profit DEX arbitrage trading signals\nUser provides the target chain and budget\nDelivers structured signals","serviceType":"A2A","fee":"0.11"}]"#,
    );
    assert_eq!(result["pass"].as_bool(), Some(false), "expected pass:false for a profit guarantee, got {result}");
}

// ── IT-011: an A2A description carrying a test/env marker is still rejected ────
//   Prohibited-content U1 (test marker) stays blocking for A2A.
#[test]
fn validate_listing_a2a_test_marker_blocks() {
    let result = validate_listing(
        "asp",
        r#"[{"serviceName":"DEX Arbitrage Signals","serviceDescription":"Provides DEX arbitrage trading signals (test)\nUser provides the target chain and budget\nDelivers structured signals","serviceType":"A2A","fee":"0.11"}]"#,
    );
    assert_eq!(result["pass"].as_bool(), Some(false), "expected pass:false for a test marker, got {result}");
}

// ── IT-012: an A2MCP description containing a web link is still rejected ───────
//   Regression guard: FE-22 prohibited content applies to every service type,
//   including A2MCP.
#[test]
fn validate_listing_a2mcp_url_blocks() {
    let result = validate_listing(
        "asp",
        r#"[{"serviceName":"Realtime Price Feed","serviceDescription":"Returns token price quotes, docs at https://example.com/api\ntokenAddress (string, required): token contract\nPOST","serviceType":"A2MCP","fee":"0.5","endpoint":"https://api.example.com/mcp"}]"#,
    );
    assert_eq!(result["pass"].as_bool(), Some(false), "expected pass:false for a URL in an A2MCP description, got {result}");
}

// ── IT-013: a mis-structured A2A listing that also carries a URL still fails ───
//   Interaction case: the advisory paragraph-count D1 (suggest) does NOT rescue a
//   listing that also carries a blocking D6 (URL); pass is driven only by block
//   findings.
#[test]
fn validate_listing_a2a_misstructured_and_url_blocks() {
    let result = validate_listing(
        "asp",
        r#"[{"serviceName":"DEX Arbitrage Signals","serviceDescription":"Provides DEX arbitrage trading signals, see https://example.com\nUser provides the target chain and budget","serviceType":"A2A","fee":"0.11"}]"#,
    );
    assert_eq!(result["pass"].as_bool(), Some(false), "expected pass:false — a blocking URL overrides the advisory structure finding, got {result}");
}

// ════════════════════════════════════════════════════════════════════════════
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
