//! Integration tests for the WBW-13651 "sink-to-CLI" parameter surface
//! (FR-1 `--since`, FR-3 `--max-results`, FR-6 `rank --all`).
//!
//! These are pure argument-validation checks: every case fails *before* any
//! upstream request is issued (mutual-exclusion / range / positive-duration
//! guards all return early), so the tests need no network and are
//! deterministic. Errors are emitted as a JSON envelope on stdout with exit 1
//! (`main.rs` → `output::error_coded`).
//!
//! This file is also the `competition` top-level area file, so it additionally
//! carries the `competition register` (OKX.AI trading-hackathon registration)
//! integration rows — see the dedicated section at the bottom.

mod common;

use common::{assert_ok_and_extract_data, onchainos, run_with_retry};
use serde_json::Value;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

/// Assert the command failed and its stdout/stderr error envelope contains
/// every `needle`.
fn assert_error_contains(output: &std::process::Output, needles: &[&str]) {
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        !output.status.success(),
        "expected failure, got success\nstdout: {stdout}\nstderr: {stderr}"
    );
    for needle in needles {
        assert!(
            stdout.contains(needle) || stderr.contains(needle),
            "expected output to contain {needle:?}\nstdout: {stdout}\nstderr: {stderr}"
        );
    }
}

// ── FR-6: `competition rank --all` ──────────────────────────────────────

#[test]
fn competition_rank_all_conflicts_with_sort_type() {
    // `--all` and `--sort-type` are mutually exclusive; the guard fires before
    // identity resolution / any network call.
    let output = onchainos()
        .args([
            "competition",
            "rank",
            "--activity-id",
            "12345",
            "--all",
            "--sort-type",
            "1",
        ])
        .output()
        .expect("failed to execute");
    assert_error_contains(
        &output,
        &["--all is mutually exclusive with --sort-type", "invalid_input"],
    );
}

// ── FR-1: `--since` positive-only + mutual exclusion ────────────────────

#[test]
fn social_news_latest_since_zero_is_rejected() {
    // `--since 0` (and `0m`/`0h`) would produce a zero-width window; the
    // positive-only parser rejects it as invalid_input before any request.
    let output = onchainos()
        .args(["social", "news-latest", "--since", "0"])
        .output()
        .expect("failed to execute");
    assert_error_contains(&output, &["duration must be positive", "invalid_input"]);
}

#[test]
fn social_news_latest_since_conflicts_with_begin() {
    let output = onchainos()
        .args([
            "social",
            "news-latest",
            "--since",
            "24h",
            "--begin",
            "1000",
        ])
        .output()
        .expect("failed to execute");
    assert_error_contains(
        &output,
        &["--since is mutually exclusive with --begin/--end", "invalid_input"],
    );
}

// ── FR-3: `--max-results` range ─────────────────────────────────────────

#[test]
fn token_search_max_results_out_of_range_is_rejected() {
    // `--max-results` must be 1..=500; the range check runs before the request.
    let output = onchainos()
        .args([
            "token",
            "search",
            "--query",
            "btc",
            "--max-results",
            "999",
        ])
        .output()
        .expect("failed to execute");
    assert_error_contains(
        &output,
        &["--max-results must be between 1 and 500", "invalid_input"],
    );
}

#[test]
fn token_search_max_results_non_integer_is_rejected() {
    let output = onchainos()
        .args([
            "token",
            "search",
            "--query",
            "btc",
            "--max-results",
            "abc",
        ])
        .output()
        .expect("failed to execute");
    assert_error_contains(&output, &["--max-results must be an integer", "invalid_input"]);
}

// ─────────────────────────────────────────────────────────────────────────
// OKX.AI trading-hackathon registration — `competition register`
// (REQ-1785229146140-578ab6; integration-plan.csv rows IT-001..IT-010)
//
// Coverage split:
//   • Offline/deterministic (IT-004/005/006/009/010): run now, no network.
//     - IT-004 needs a *fresh* ONCHAINOS_HOME so address auto-resolution has no
//       wallet session and bails "not logged in".
//     - IT-005/006 bail in the pre-request validation (`require_uid_for_cefi`,
//       `validate_address_for_chain`) before any login/network work.
//     - IT-009/010 are clap parse errors (exit 2) before application logic.
//   • Live/flaky (IT-001/002/007): go through `run_with_retry`; require a
//     logged-in session (ambient ONCHAINOS_HOME=../.onchainos) plus real agent
//     ids that we must NOT hardcode. Each reads its id(s) from env and SKIPs
//     when unset — the sanctioned pattern for environment-dependent rows.
//   • Mock (IT-003/008): env-driven backend, NOT wrapped in `run_with_retry`.
//     - IT-003 needs a mock backend (OKX_BASE_URL) returning code 0 + a session,
//       both operator-supplied; skipped when the mock env is absent.
//     - IT-008 points OKX_BASE_URL at a closed localhost port (a test fixture,
//       not an environment URL, so it is a literal) to force a connection error.
//
// Env knobs for the non-offline rows (skip-when-unset; documented in notes.md):
//   HACKATHON_QUALIFYING_AGENT_ID     — IT-001, IT-002, IT-008
//   HACKATHON_CEFI_UID                — IT-002
//   HACKATHON_MOCK_BASE_URL           — IT-003 (mock backend returning code 0)
//   HACKATHON_NON_QUALIFYING_AGENT_ID — IT-007
// ─────────────────────────────────────────────────────────────────────────

/// Per-test sandbox guard: removes its `ONCHAINOS_HOME` dir on drop. Staged
/// under `cli/target/test_tmp/cli_competition_sink/` — NOT `tempfile::tempdir()`
/// (the agent bash sandbox denies writes to the system tempdir).
struct RegTestHome {
    path: PathBuf,
}

impl Drop for RegTestHome {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.path);
    }
}

static REG_HOME_COUNTER: AtomicU64 = AtomicU64::new(0);

/// Build a fresh, isolated `ONCHAINOS_HOME` directory (unique per process +
/// nanos + counter, so parallel tests never collide).
fn fresh_reg_home() -> (RegTestHome, PathBuf) {
    let base = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("target")
        .join("test_tmp")
        .join("cli_competition_sink");
    fs::create_dir_all(&base).expect("create test_tmp base");
    let pid = std::process::id();
    let n = REG_HOME_COUNTER.fetch_add(1, Ordering::Relaxed);
    let ts = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let dir = base.join(format!("{pid}-{ts}-{n}"));
    fs::create_dir_all(&dir).expect("create per-test dir");
    let path = dir.clone();
    (RegTestHome { path: dir }, path)
}

/// Assert the command failed and its stdout carries the JSON error envelope
/// (`ok == false`), returning the parsed value. Used by the rows whose contract
/// is the envelope shape rather than a specific message.
fn assert_json_error_envelope(output: &std::process::Output) -> Value {
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        !output.status.success(),
        "expected failure, got success\nstdout: {stdout}\nstderr: {stderr}"
    );
    let json: Value = serde_json::from_str(&stdout)
        .unwrap_or_else(|e| panic!("expected a JSON envelope on stdout: {e}\nraw: {stdout}"));
    assert_eq!(
        json["ok"],
        Value::Bool(false),
        "expected ok=false envelope: {json}"
    );
    json
}

// ── Offline / deterministic rows ─────────────────────────────────────────

/// IT-004 — no login + `--address` omitted: address auto-resolution finds no
/// wallet session and bails "not logged in" (exit 1). Deterministic in a fresh
/// ONCHAINOS_HOME sandbox; never touches the network.
#[test]
fn competition_register_web3_not_logged_in_errors() {
    let (_home_guard, home) = fresh_reg_home();
    let output = onchainos()
        .env("ONCHAINOS_HOME", &home)
        .env_remove("OKX_BASE_URL")
        .env_remove("OKX_API_KEY")
        .env_remove("OKX_ACCESS_KEY")
        .env_remove("OKX_SECRET_KEY")
        .env_remove("OKX_PASSPHRASE")
        .args([
            "competition",
            "register",
            "--agent-id",
            "agent-any",
            "--account-type",
            "web3",
        ])
        .output()
        .expect("failed to execute");
    assert_error_contains(&output, &["not logged in"]);
    assert_eq!(output.status.code(), Some(1), "expected exit 1");
}

/// IT-005 — `--account-type cefi` without `--uid` trips the pre-request
/// validation bail (exit 1). `--address` is explicit so resolution + login are
/// skipped, keeping the row offline/deterministic.
#[test]
fn competition_register_cefi_missing_uid_errors() {
    let output = onchainos()
        .args([
            "competition",
            "register",
            "--agent-id",
            "agent-any",
            "--account-type",
            "cefi",
            "--address",
            "0x1111111111111111111111111111111111111111",
        ])
        .output()
        .expect("failed to execute");
    assert_error_contains(&output, &["uid"]);
    assert_eq!(output.status.code(), Some(1), "expected exit 1");
}

/// IT-006 — an explicit malformed `--address` fails `validate_address_for_chain`
/// (chainIndex 196 = EVM) before any network call (exit 1). offline.
#[test]
fn competition_register_web3_invalid_address_errors() {
    let output = onchainos()
        .args([
            "competition",
            "register",
            "--agent-id",
            "agent-any",
            "--account-type",
            "web3",
            "--address",
            "0xZZZ",
        ])
        .output()
        .expect("failed to execute");
    assert_error_contains(&output, &["address"]);
    assert_eq!(output.status.code(), Some(1), "expected exit 1");
}

/// IT-009 — required `--agent-id` missing: clap prints a usage error to stderr
/// and exits 2 before any application logic. offline.
#[test]
fn competition_register_missing_agent_id_is_usage_error() {
    let output = onchainos()
        .args(["competition", "register", "--account-type", "web3"])
        .output()
        .expect("failed to execute");
    assert_error_contains(&output, &["--agent-id"]);
    assert_eq!(output.status.code(), Some(2), "expected clap usage exit 2");
}

/// IT-010 — `--account-type` outside {web3, cefi} is rejected by clap's
/// PossibleValuesParser on stderr with exit 2. offline.
#[test]
fn competition_register_invalid_account_type_is_usage_error() {
    let output = onchainos()
        .args([
            "competition",
            "register",
            "--agent-id",
            "agent-any",
            "--account-type",
            "binance",
        ])
        .output()
        .expect("failed to execute");
    assert_error_contains(&output, &["possible values"]);
    assert_eq!(output.status.code(), Some(2), "expected clap usage exit 2");
}

// ── Live / flaky rows (env-gated; run through run_with_retry) ─────────────

/// IT-001 — golden Web3 happy path: a logged-in user registers a qualifying
/// Trading ASP; `--activity-id` defaults to "5" and `--address` auto-resolves
/// to the wallet's X Layer address. Requires a logged-in session + a qualifying
/// agent id (env-supplied); skipped when the env is absent.
#[test]
fn competition_register_web3_golden_happy_path() {
    let Some(agent_id) = std::env::var("HACKATHON_QUALIFYING_AGENT_ID")
        .ok()
        .filter(|s| !s.is_empty())
    else {
        eprintln!(
            "SKIP IT-001: set HACKATHON_QUALIFYING_AGENT_ID (+ a logged-in ONCHAINOS_HOME) to run"
        );
        return;
    };
    let output = run_with_retry(&[
        "competition",
        "register",
        "--agent-id",
        agent_id.as_str(),
        "--account-type",
        "web3",
    ]);
    let data = assert_ok_and_extract_data(&output);
    assert_eq!(
        data["activityId"], "5",
        "expected the default hackathon activityId: {data}"
    );
}

/// IT-002 — golden CeFi happy path: `--account-type cefi` with `--uid`; the
/// wallet X Layer address still auto-resolves and the confirmation carries
/// accountType "cefi". Requires a session, a qualifying agent id, and a CeFi
/// uid (env-supplied); skipped when either env is absent.
#[test]
fn competition_register_cefi_golden_happy_path() {
    let (Some(agent_id), Some(uid)) = (
        std::env::var("HACKATHON_QUALIFYING_AGENT_ID")
            .ok()
            .filter(|s| !s.is_empty()),
        std::env::var("HACKATHON_CEFI_UID")
            .ok()
            .filter(|s| !s.is_empty()),
    ) else {
        eprintln!(
            "SKIP IT-002: set HACKATHON_QUALIFYING_AGENT_ID and HACKATHON_CEFI_UID (+ a logged-in ONCHAINOS_HOME) to run"
        );
        return;
    };
    let output = run_with_retry(&[
        "competition",
        "register",
        "--agent-id",
        agent_id.as_str(),
        "--account-type",
        "cefi",
        "--uid",
        uid.as_str(),
    ]);
    let data = assert_ok_and_extract_data(&output);
    assert_eq!(
        data["accountType"], "cefi",
        "expected cefi accountType in the confirmation: {data}"
    );
}

/// IT-007 — a non-qualifying agent (not a trading ASP / no subscription service
/// / no 3-day trial) is rejected by the backend with a generic non-zero code +
/// descriptive msg, surfaced verbatim as `{ok:false,error:<msg>}`. Env-supplied
/// agent id; skipped when absent.
#[test]
fn competition_register_non_qualifying_agent_is_rejected() {
    let Some(agent_id) = std::env::var("HACKATHON_NON_QUALIFYING_AGENT_ID")
        .ok()
        .filter(|s| !s.is_empty())
    else {
        eprintln!(
            "SKIP IT-007: set HACKATHON_NON_QUALIFYING_AGENT_ID (+ a logged-in ONCHAINOS_HOME) to run"
        );
        return;
    };
    let output = run_with_retry(&[
        "competition",
        "register",
        "--agent-id",
        agent_id.as_str(),
        "--account-type",
        "web3",
    ]);
    let json = assert_json_error_envelope(&output);
    let err = json["error"].as_str().unwrap_or("");
    assert!(
        !err.is_empty(),
        "expected a non-empty backend error message: {json}"
    );
}

// ── Mock rows (env/fixture-driven backend; NOT wrapped in run_with_retry) ──

/// IT-003 — edge: `--activity-id 6` override + explicit `--address` flow into
/// the confirmation. Runs against a mock backend (OKX_BASE_URL override
/// returning code 0) plus a logged-in session, both operator-supplied via env;
/// skipped when the mock env is absent (base URL is never hardcoded).
#[test]
fn competition_register_web3_activity_override_against_mock() {
    let Some(mock_base_url) = std::env::var("HACKATHON_MOCK_BASE_URL")
        .ok()
        .filter(|s| !s.is_empty())
    else {
        eprintln!(
            "SKIP IT-003: set HACKATHON_MOCK_BASE_URL (mock backend returning code 0) + a logged-in ONCHAINOS_HOME to run"
        );
        return;
    };
    let output = onchainos()
        .env("OKX_BASE_URL", &mock_base_url)
        .args([
            "competition",
            "register",
            "--agent-id",
            "agent-any",
            "--account-type",
            "web3",
            "--activity-id",
            "6",
            "--address",
            "0x1111111111111111111111111111111111111111",
        ])
        .output()
        .expect("failed to execute");
    let data = assert_ok_and_extract_data(&output);
    assert_eq!(
        data["activityId"], "6",
        "expected the overridden activityId: {data}"
    );
}

/// IT-008 — network error: OKX_BASE_URL points at a closed local port so the
/// POST fails; the connection error surfaces as `{ok:false,...}` exit 1.
/// Requires a logged-in session (so the request reaches the network layer),
/// gated on the qualifying-agent env. The closed localhost port is a fixture,
/// not an environment URL, so it is a literal. No retry — the failure is the
/// asserted behavior, not a flake.
#[test]
fn competition_register_network_error_surfaces_ok_false() {
    let Some(agent_id) = std::env::var("HACKATHON_QUALIFYING_AGENT_ID")
        .ok()
        .filter(|s| !s.is_empty())
    else {
        eprintln!(
            "SKIP IT-008: set HACKATHON_QUALIFYING_AGENT_ID (+ a logged-in ONCHAINOS_HOME) to run"
        );
        return;
    };
    let output = onchainos()
        .env("OKX_BASE_URL", "http://127.0.0.1:59999")
        .args([
            "competition",
            "register",
            "--agent-id",
            agent_id.as_str(),
            "--account-type",
            "web3",
        ])
        .output()
        .expect("failed to execute");
    let _ = assert_json_error_envelope(&output);
}
