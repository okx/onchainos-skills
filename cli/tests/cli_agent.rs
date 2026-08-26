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
    assert_error_contains, fresh_home, onchainos, parse_stdout_json, run_with_retry, scrubbed,
};
use serde_json::Value;
use std::fs;

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
        assert!(stdout.contains(expected), "help missing {expected:?}: {stdout}");
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
    let notify_command = data["notifyCommand"]
        .as_str()
        .expect("notifyCommand");
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
    assert!(notify_args
        .iter()
        .any(|arg| arg.as_str().is_some_and(|value| value.contains("funding notice"))));
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
    assert_eq!(result["pass"].as_bool(), Some(true), "expected pass:true, got {result}");
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
    assert_eq!(result["pass"].as_bool(), Some(true), "expected pass:true, got {result}");
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
    assert_eq!(result["pass"].as_bool(), Some(true), "expected pass:true, got {result}");
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
    assert_eq!(result["pass"].as_bool(), Some(true), "expected pass:true, got {result}");
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
    assert_eq!(result["pass"].as_bool(), Some(true), "a 0x address must not block, got {result}");
}

// ── IT-010: a profit guarantee advises instead of blocking ────────────────────
//   D9 is advisory: the hardcoded phrase list is only a partial backstop (the
//   skill layer flags guarantee wording in any language by meaning), so it
//   surfaces as `suggest` and pass stays true.
#[test]
fn validate_listing_a2a_profit_guarantee_suggests() {
    let result = validate_listing(
        "asp",
        r#"[{"serviceName":"DEX Arbitrage Signals","serviceDescription":"Guaranteed profit DEX arbitrage trading signals\nUser provides the target chain and budget\nDelivers structured signals","serviceType":"A2A","fee":"0.11"}]"#,
    );
    assert_eq!(result["pass"].as_bool(), Some(true), "expected pass:true, got {result}");
    assert!(
        findings(&result).iter().any(|f| f["severity"] == "suggest"),
        "expected an advisory (suggest) profit-guarantee finding, got {result}"
    );
    assert!(
        findings(&result).iter().all(|f| f["severity"] != "block"),
        "a profit guarantee must not block, got {result}"
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
    assert_eq!(result["pass"].as_bool(), Some(false), "expected pass:false for a test marker, got {result}");
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

// ── IT-013: an advisory finding never rescues a blocking one ──────────────────
//   Interaction case: a description carrying BOTH an advisory profit guarantee (D9)
//   and a blocking URL (D6) still fails — `pass` is driven only by block findings.
#[test]
fn validate_listing_a2a_suggest_and_block_together_blocks() {
    let result = validate_listing(
        "asp",
        r#"[{"serviceName":"DEX Arbitrage Signals","serviceDescription":"Guaranteed profit DEX arbitrage signals, see https://example.com\nUser provides the target chain and budget","serviceType":"A2A","fee":"0.11"}]"#,
    );
    assert_eq!(result["pass"].as_bool(), Some(false), "expected pass:false — a blocking URL overrides the advisory finding, got {result}");
    assert!(
        findings(&result).iter().any(|f| f["severity"] == "suggest"),
        "expected the advisory D9 to still surface alongside the block, got {result}"
    );
}

// ════════════════════════════════════════════════════════════════════════════
//  agent autotrade-consent-set --mode pause — local compatibility contract
// ══════════════════════════════════════════════════════════════════════════

#[test]
fn autotrade_pause_needs_only_job_id_and_keeps_existing_output() {
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
    let consent: serde_json::Value = serde_json::from_slice(
        &std::fs::read(&consent_path).expect("pause must persist manual policy"),
    )
    .expect("parse paused policy");
    assert_eq!(consent["mode"], "manual");

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

    let stored: serde_json::Value = serde_json::from_slice(
        &std::fs::read(consent_dir.join("job_environment.json")).unwrap(),
    )
    .unwrap();
    assert_eq!(stored["version"], 3);
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

    let stored: serde_json::Value = serde_json::from_slice(
        &std::fs::read(consent_dir.join("job_settings.json")).unwrap(),
    )
    .unwrap();
    assert_eq!(stored["version"], 3);
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
    let mut set = onchainos();
    scrubbed(&mut set, &dir);
    let set_output = set
        .args([
            "agent",
            "autotrade-consent-set",
            "--job-id",
            "job_unbounded_auto",
            "--agent-id",
            "8315",
            "--mode",
            "auto",
        ])
        .output()
        .expect("persist default auto policy");
    common::assert_ok_and_extract_data(&set_output);

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
fn autotrade_consent_request_suppresses_first_time_card_for_existing_policy() {
    let (_home, dir) = fresh_home("cli_agent_autotrade_consent_request_existing_policy");

    for (job_id, mode, extra_args) in [
        (
            "job_auto",
            "auto",
            vec!["--cap", "10", "--trade-amount", "1"],
        ),
        ("job_manual", "manual", Vec::new()),
    ] {
        let mut set = onchainos();
        scrubbed(&mut set, &dir);
        let mut set_args = vec![
            "agent",
            "autotrade-consent-set",
            "--job-id",
            job_id,
            "--agent-id",
            "8315",
            "--mode",
            mode,
        ];
        set_args.extend(extra_args);
        let set_output = set.args(set_args).output().expect("persist consent policy");
        common::assert_ok_and_extract_data(&set_output);

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
        assert_eq!(data["reason"], "consent_already_configured");
        assert_eq!(data["jobId"], job_id);
        assert_eq!(data["deliveryId"], "msg:delivery-1");
        assert_eq!(data["consentMode"], mode);
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
fn service_match_help_describes_pagination_headers_and_price_range() {
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
        "Search marketplace Services by capability, ASP, Service name, or price range.",
        "Results include searchAfter, hasMore, unmatchReason",
        "--agentic-id <AGENTIC_ID>",
        "--min-payment-token-amount <MIN_PAYMENT_TOKEN_AMOUNT>",
        "--max-payment-token-amount <MAX_PAYMENT_TOKEN_AMOUNT>",
        "--search-after <SEARCH_AFTER>",
        "Initial request without filters",
        "Continuation request",
    ] {
        assert!(help.contains(expected), "missing {expected:?} in help:\n{help}");
    }
    assert!(!help.contains("      --format "));
    assert!(!help.contains("backend raw data payload"));
}

#[test]
fn hidden_autotrade_watch_precheck_is_callable_and_rejects_an_unsafe_job_id_locally() {
    let (_home, dir) = fresh_home("cli_agent_autotrade_watch_precheck");
    let mut cmd = onchainos();
    scrubbed(&mut cmd, &dir);
    let output = cmd
        .args([
            "agent",
            "autotrade-watch-precheck",
            "--job-id",
            "../unsafe",
        ])
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
