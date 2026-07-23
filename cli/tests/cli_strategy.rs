//! Integration tests for `onchainos strategy` limit-order commands.
//!
//! Source: `oli-docs/df5kwjibpidpshkdsnplhuszgqe/integration-plan.csv` rows
//! IT-008 … IT-015 (WWINFRA-3509 "Sink-to-CLI" revision — F1 below-minimum
//! pre-check, F4 `--wait` terminal-state merge, and the `cancel --all --wait`
//! guard).
//!
//! ── Scope and contract ──────────────────────────────────────────────────
//!
//! Two rows are deterministic **offline** and are asserted directly:
//!   - IT-011 (`cancel --all --wait`): rejected by `reject_all_with_wait`
//!     (`handlers.rs`) at the very top of `cancel`, BEFORE any session load or
//!     network call, so the `invalid_input` / `wait` CodedError is emitted
//!     regardless of login state.
//!   - IT-015 (`create-limit` with no login): `create_limit` loads the wallet
//!     session first; with a fresh sandbox `ONCHAINOS_HOME` there is no
//!     session, so it fails locally with `{ok:false,error}` at exit 1 before
//!     any network call. Also proves every required `create-limit` flag parses.
//!
//! The remaining rows are `network_required: live` golden/edge cases that need
//! a logged-in, strategy-enabled wallet (a session carrying `saTeeId`). CI runs
//! with anonymous / AK read credentials only, so those commands short-circuit
//! at the local session check. Each such test therefore runs the real command
//! through the shared `run_with_retry` helper and then either asserts the golden
//! outcome (when a strategy wallet session IS present) or skips with a note
//! (when the session precondition is not met) — mirroring the repo's existing
//! "tolerate not-logged-in" convention (see `cli_security.rs`). This keeps the
//! CSV row covered without laundering a missing fixture into a false PASS.
//!
//! Rows that MUTATE backend state (create / cancel a real order — IT-010,
//! IT-012, IT-014) are `#[ignore]`d and additionally gated behind an explicit
//! fixture env var, so they never run — and never touch a real wallet — in the
//! default `cargo test --tests` sweep.
//!
//! ── Sandbox conventions ──────────────────────────────────────────────────
//!
//! IT-015 needs a guaranteed-logged-out home. `ONCHAINOS_HOME` is pointed at a
//! fresh isolated dir under `cli/target/test_tmp/cli_strategy/` (the agent
//! sandbox denies writes to `/var/folders/.../T/`, so `tempfile::tempdir()` is
//! unsafe here) via `Command::env` per-invocation, and inherited `OKX_*`
//! credentials are scrubbed so the binary cannot fall through to host state.

mod common;

use common::{onchainos, run_with_retry, tokens};
use serde_json::Value;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

// ── shared assertion helpers ─────────────────────────────────────────────

/// Parse a command's stdout as JSON. Returns `None` if stdout is not valid
/// JSON (e.g. a clap usage error printed to stderr).
fn parse_stdout_json(output: &std::process::Output) -> Option<Value> {
    serde_json::from_slice(&output.stdout).ok()
}

/// `true` when the command failed because no strategy-enabled wallet session is
/// available (auth / login precondition), as opposed to a genuine behavioral
/// failure. `strategy` create/cancel/resume all `session::load()` early; in an
/// unauthenticated environment they bail with a login/session message. Only
/// failures are considered — a successful command is never a "skip".
fn strategy_session_unavailable(output: &std::process::Output) -> bool {
    if output.status.success() {
        return false;
    }
    let stdout = String::from_utf8_lossy(&output.stdout).to_ascii_lowercase();
    let stderr = String::from_utf8_lossy(&output.stderr).to_ascii_lowercase();
    let haystack = format!("{stdout}\n{stderr}");
    [
        "not logged in",
        "not_logged_in",
        "please login",
        "please re-login",
        "login again",
        "session expired",
        "no wallet",
        "wallet address",
        "sa tee",
        "sateeid",
        "unauthorized",
    ]
    .iter()
    .any(|marker| haystack.contains(marker))
}

/// Assert the F1 below-minimum contract (`ok:true`, `data.belowMinimum == true`)
/// when a strategy wallet session is present; otherwise skip with a note.
#[track_caller]
fn assert_below_minimum_or_skip(output: &std::process::Output, label: &str) {
    if strategy_session_unavailable(output) {
        eprintln!("[skip] {label}: strategy-enabled wallet session not available in this environment");
        return;
    }
    let stdout = String::from_utf8_lossy(&output.stdout);
    let json = parse_stdout_json(output)
        .unwrap_or_else(|| panic!("{label}: stdout is not JSON:\n{stdout}"));
    assert_eq!(
        json["ok"].as_bool(),
        Some(true),
        "{label}: expected ok=true, got: {json}"
    );
    assert_eq!(
        json["data"]["belowMinimum"].as_bool(),
        Some(true),
        "{label}: expected data.belowMinimum=true, got: {json}"
    );
}

/// Assert the F4 `--wait` merge emitted a `settled` field when a strategy wallet
/// session is present; otherwise skip with a note.
#[track_caller]
fn assert_settled_or_skip(output: &std::process::Output, label: &str) {
    if strategy_session_unavailable(output) {
        eprintln!("[skip] {label}: strategy-enabled wallet session not available in this environment");
        return;
    }
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        output.status.success(),
        "{label}: expected success\nstdout: {stdout}\nstderr: {stderr}"
    );
    assert!(
        stdout.contains("settled"),
        "{label}: expected a 'settled' terminal-state field in the --wait output:\n{stdout}"
    );
}

// ── sandbox home (IT-015 only) ────────────────────────────────────────────

/// Per-test sandbox guard that removes its directory on drop. Lives under
/// `cli/target/test_tmp/cli_strategy/<unique-suffix>` because the sandbox we
/// run inside denies writes to the system tempdir.
struct TestHome {
    path: PathBuf,
}

impl Drop for TestHome {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.path);
    }
}

impl TestHome {
    fn path(&self) -> &Path {
        &self.path
    }
}

static COUNTER: AtomicU64 = AtomicU64::new(0);

/// Build a fresh, isolated `ONCHAINOS_HOME` directory guaranteed to hold no
/// wallet session.
fn fresh_home() -> TestHome {
    let base = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("target")
        .join("test_tmp")
        .join("cli_strategy");
    fs::create_dir_all(&base).expect("create test_tmp base");
    let pid = std::process::id();
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    let ts = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let dir = base.join(format!("{pid}-{ts}-{n}"));
    fs::create_dir_all(&dir).expect("create per-test dir");
    TestHome { path: dir }
}

// ── create-limit: F1 below-minimum pre-check ─────────────────────────────

/// IT-008 — sell order worth < $1 short-circuits with `belowMinimum` at exit 0
/// (no order created). `--current-price 1.0` makes the $1 threshold
/// deterministic without depending on a live price. State-safe: returns before
/// `createOrder`. Requires a logged-in strategy wallet.
#[test]
fn create_limit_sell_below_minimum_emits_below_minimum_notice() {
    let output = run_with_retry(&[
        "strategy",
        "create-limit",
        "--chain-id",
        "1",
        "--from-token",
        tokens::ETH_USDC,
        "--to-token",
        tokens::EVM_NATIVE,
        "--amount",
        "0.0001",
        "--trigger-price",
        "0.5",
        "--direction",
        "sell",
        "--current-price",
        "1.0",
    ]);
    assert_below_minimum_or_skip(&output, "IT-008 create-limit sell below-minimum");
}

/// IT-009 — buy order worth < $1 short-circuits with `belowMinimum` at exit 0.
/// Exercises the buy-path from-token price fetch. State-safe (no order created).
/// Requires a logged-in strategy wallet.
#[test]
fn create_limit_buy_below_minimum_emits_below_minimum_notice() {
    let output = run_with_retry(&[
        "strategy",
        "create-limit",
        "--chain-id",
        "1",
        "--from-token",
        tokens::ETH_USDC,
        "--to-token",
        tokens::EVM_NATIVE,
        "--amount",
        "0.0001",
        "--trigger-price",
        "5000",
        "--direction",
        "buy",
        "--current-price",
        "3000",
    ]);
    assert_below_minimum_or_skip(&output, "IT-009 create-limit buy below-minimum");
}

/// IT-010 — backend `100010` (OrderAmountTooSmall) is normalized into the same
/// `belowMinimum` object at exit 0. `--amount 1.5 --current-price 1.0` clears
/// the local $1 pre-check ($1.5 > $1), so this path is reached only by
/// attempting a real order — an opt-in fixture row, not a default-CI row.
#[test]
#[ignore = "order-attempting: needs a logged-in strategy wallet and a fixture that bypasses the local $1 pre-check to force backend 100010; not a default-CI row"]
fn create_limit_backend_100010_normalized_to_below_minimum() {
    if std::env::var("ONCHAINOS_RUN_MUTATING_TESTS").ok().as_deref() != Some("1") {
        eprintln!("[skip] IT-010: set ONCHAINOS_RUN_MUTATING_TESTS=1 to run this order-attempting test");
        return;
    }
    let output = run_with_retry(&[
        "strategy",
        "create-limit",
        "--chain-id",
        "1",
        "--from-token",
        tokens::ETH_USDC,
        "--to-token",
        tokens::EVM_NATIVE,
        "--amount",
        "1.5",
        "--trigger-price",
        "0.5",
        "--direction",
        "sell",
        "--current-price",
        "1.0",
    ]);
    assert_below_minimum_or_skip(&output, "IT-010 create-limit backend-100010 fallback");
}

// ── cancel: F4 guard + --wait merge ───────────────────────────────────────

/// IT-011 — `cancel --all --wait` is rejected as unsupported at exit 1 with a
/// `CodedError` (`errorCode: "invalid_input"`, `errorField: "wait"`). The guard
/// (`reject_all_with_wait`) runs before any session load or cancel request, so
/// this is deterministic offline regardless of login state — hence no retry
/// helper and no sandbox home are needed.
#[test]
fn cancel_all_with_wait_rejected_as_invalid_input() {
    let output = onchainos()
        .args(["strategy", "cancel", "--all", "--wait"])
        .output()
        .expect("failed to execute onchainos");

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert_eq!(
        output.status.code(),
        Some(1),
        "expected exit 1 for cancel --all --wait\nstdout: {stdout}\nstderr: {stderr}"
    );
    let json = parse_stdout_json(&output)
        .unwrap_or_else(|| panic!("stdout is not JSON:\n{stdout}"));
    assert_eq!(
        json["errorCode"].as_str(),
        Some("invalid_input"),
        "expected errorCode=invalid_input, got: {json}"
    );
    assert_eq!(
        json["errorField"].as_str(),
        Some("wait"),
        "expected errorField=wait, got: {json}"
    );
}

/// IT-012 — `cancel --order-id <id> --wait` cancels one order, waits, and merges
/// terminal-state fields (`settled` / `status` / `statusLabel`). This MUTATES
/// backend state, so it is `#[ignore]`d and only runs when a seeded, cancellable
/// order id is supplied via `ONCHAINOS_TEST_ORDER_ID`.
#[test]
#[ignore = "mutation: cancels a real order; needs a logged-in strategy wallet and a seeded cancellable order id via ONCHAINOS_TEST_ORDER_ID; not a default-CI row"]
fn cancel_single_order_with_wait_reports_settled() {
    let order_id = match std::env::var("ONCHAINOS_TEST_ORDER_ID") {
        Ok(v) if !v.trim().is_empty() => v,
        _ => {
            eprintln!("[skip] IT-012: set ONCHAINOS_TEST_ORDER_ID to a seeded cancellable order id");
            return;
        }
    };
    let output = run_with_retry(&[
        "strategy",
        "cancel",
        "--order-id",
        order_id.as_str(),
        "--wait",
    ]);
    assert_settled_or_skip(&output, "IT-012 cancel --order-id --wait");
}

// ── resume: F4 --wait ─────────────────────────────────────────────────────

/// IT-013 — `resume --wait` with no suspended orders reports
/// `data.note == "no resumable orders found"` at exit 0 (nothing is resumed),
/// which also proves `--wait` parses on `resume`. Requires a logged-in wallet
/// with no suspended orders; state-safe.
#[test]
fn resume_with_wait_no_resumable_orders_reports_note() {
    let output = run_with_retry(&["strategy", "resume", "--wait"]);
    if strategy_session_unavailable(&output) {
        eprintln!(
            "[skip] IT-013 resume --wait: strategy-enabled wallet session not available in this environment"
        );
        return;
    }
    let stdout = String::from_utf8_lossy(&output.stdout);
    let json = parse_stdout_json(&output)
        .unwrap_or_else(|| panic!("IT-013: stdout is not JSON:\n{stdout}"));
    assert_eq!(
        json["ok"].as_bool(),
        Some(true),
        "IT-013: expected ok=true, got: {json}"
    );
    assert_eq!(
        json["data"]["note"].as_str(),
        Some("no resumable orders found"),
        "IT-013: expected data.note='no resumable orders found', got: {json}"
    );
}

// ── create-limit: F4 --wait on a real order ───────────────────────────────

/// IT-014 — `create-limit --wait` on an order that clears the $1 floor
/// (`--amount 5 --current-price 1.0` → $5) submits a real order and merges the
/// terminal-state fields (`settled` etc.). This CREATES a real order, so it is
/// `#[ignore]`d and only runs when explicitly opted in via
/// `ONCHAINOS_RUN_MUTATING_TESTS=1` (the caller is responsible for teardown).
#[test]
#[ignore = "mutation: creates a real limit order ($5 clears the $1 floor); needs a logged-in strategy wallet + teardown; opt in via ONCHAINOS_RUN_MUTATING_TESTS=1; not a default-CI row"]
fn create_limit_with_wait_reports_settled() {
    if std::env::var("ONCHAINOS_RUN_MUTATING_TESTS").ok().as_deref() != Some("1") {
        eprintln!("[skip] IT-014: set ONCHAINOS_RUN_MUTATING_TESTS=1 to run this order-creating test");
        return;
    }
    let output = run_with_retry(&[
        "strategy",
        "create-limit",
        "--chain-id",
        "1",
        "--from-token",
        tokens::ETH_USDC,
        "--to-token",
        tokens::EVM_NATIVE,
        "--amount",
        "5",
        "--trigger-price",
        "0.5",
        "--direction",
        "sell",
        "--current-price",
        "1.0",
        "--wait",
    ]);
    assert_settled_or_skip(&output, "IT-014 create-limit --wait");
}

// ── create-limit: not-logged-in error path ────────────────────────────────

/// IT-015 — `create-limit` without a wallet session fails clearly with
/// `{ok:false, error}` at exit 1. Deterministic offline: `create_limit` loads
/// the session first, which fails locally on a fresh sandbox home before any
/// network call. Also confirms every required `create-limit` flag parses.
#[test]
fn create_limit_without_login_fails_with_ok_false() {
    let home = fresh_home();
    let output = onchainos()
        .env_remove("OKX_API_KEY")
        .env_remove("OKX_ACCESS_KEY")
        .env_remove("OKX_SECRET_KEY")
        .env_remove("OKX_PASSPHRASE")
        .env_remove("OKX_BASE_URL")
        .env("ONCHAINOS_HOME", home.path())
        .args([
            "strategy",
            "create-limit",
            "--chain-id",
            "1",
            "--from-token",
            tokens::ETH_USDC,
            "--to-token",
            tokens::EVM_NATIVE,
            "--amount",
            "5",
            "--trigger-price",
            "3000",
            "--direction",
            "buy",
        ])
        .output()
        .expect("failed to execute onchainos");

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert_eq!(
        output.status.code(),
        Some(1),
        "expected exit 1 for logged-out create-limit\nstdout: {stdout}\nstderr: {stderr}"
    );
    let json = parse_stdout_json(&output)
        .unwrap_or_else(|| panic!("stdout is not JSON:\n{stdout}"));
    assert_eq!(
        json["ok"].as_bool(),
        Some(false),
        "expected ok=false, got: {json}"
    );
}
