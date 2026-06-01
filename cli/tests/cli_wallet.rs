//! Integration tests for `onchainos wallet login` — locale validation surface.
//!
//! Covers the `--locale` whitelist enforcement added in WWINFRA-3325:
//! - Valid locales (`en_US`, `zh_CN`) pass through unchanged.
//! - Omitted `--locale` preserves existing behavior (no `locale` field on the wire).
//! - Invalid locales (`en-US` hyphenated, `EN-US` uppercase, empty string) fall back
//!   to `en_US` with a stderr warning of the exact form
//!   `locale '<original>' not in supported list (en_US, zh_CN), falling back to en_US`.
//! - Empty email argument fails fast with `email is required` and exit 1 — no network.
//!
//! The placeholder email may be rejected by the backend, so we assert only:
//!   1. The CLI did not crash (exit code reached `main.rs` cleanly).
//!   2. Stdout is parseable JSON.
//!   3. For fallback rows: stderr contains the exact warning substring.
//!
//! We deliberately do NOT assert `ok: true` on live rows because backend
//! rejection of a placeholder email is not a CLI bug.

mod common;

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Output;
use std::sync::atomic::{AtomicU64, Ordering};

use common::onchainos;

/// RAII guard for an `ONCHAINOS_HOME` staged under `cli/target/test_tmp/`.
/// Removes the directory on drop. We can't use `tempfile::tempdir()` because
/// the agent's bash sandbox denies writes to the system tempdir.
struct TestHome {
    path: PathBuf,
}

impl TestHome {
    fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for TestHome {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.path);
    }
}

static COUNTER: AtomicU64 = AtomicU64::new(0);

/// Build a fresh, isolated `ONCHAINOS_HOME` directory under
/// `cli/target/test_tmp/cli_wallet/`.
fn fresh_home() -> TestHome {
    let base = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("target")
        .join("test_tmp")
        .join("cli_wallet");
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

/// Build the standard live-row CLI invocation. `extra` is the per-row tail
/// (email + optional `--locale <v>`). Each invocation gets its own
/// `ONCHAINOS_HOME` so wallet credentials never leak between tests.
fn run_wallet_login(extra: &[&str]) -> (Output, TestHome) {
    let home = fresh_home();
    let mut args: Vec<&str> = vec!["wallet", "login"];
    args.extend_from_slice(extra);

    // Mirror `common::run_with_retry` semantics but with env isolation: retry up
    // to 3 times only on rate-limit stdout. Backend rejection of the placeholder
    // email exits non-zero with no rate-limit marker, so the first attempt
    // returns immediately in that case.
    let mut last: Option<Output> = None;
    for attempt in 0..3 {
        if attempt > 0 {
            std::thread::sleep(std::time::Duration::from_secs(attempt));
        }
        let output = onchainos()
            .env("ONCHAINOS_HOME", home.path())
            .args(&args)
            .output()
            .expect("failed to execute onchainos");

        if output.status.success() {
            return (output, home);
        }
        let stdout = String::from_utf8_lossy(&output.stdout);
        if !stdout.contains("Rate limited") {
            return (output, home);
        }
        last = Some(output);
    }
    (last.expect("no attempt recorded"), home)
}

/// Assert that the live login attempt produced valid JSON on stdout.
/// Does NOT require `ok: true` — placeholder emails may be rejected by the
/// backend, which is not a CLI bug. We only require that the CLI itself
/// returned a structured response (i.e. didn't panic / crash).
fn assert_stdout_is_json(output: &Output) {
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        !stdout.trim().is_empty(),
        "expected non-empty stdout JSON from CLI (was it a panic?)\nstdout: {stdout}\nstderr: {stderr}",
    );
    serde_json::from_str::<serde_json::Value>(&stdout).unwrap_or_else(|e| {
        panic!("stdout is not valid JSON: {e}\nraw stdout: {stdout}\nstderr: {stderr}")
    });
}

/// Exact stderr line emitted by `cmd_login` on locale fallback, parameterized
/// by the original (pre-validation) locale string the user passed.
fn fallback_warning(original: &str) -> String {
    format!("locale '{original}' not in supported list (en_US, zh_CN), falling back to en_US")
}

// ─── IT-001: valid en_US locale ────────────────────────────────────────────

#[test]
fn wallet_login_valid_en_us_locale() {
    let (output, _home) = run_wallet_login(&[
        "test+integration-en@example.com",
        "--locale",
        "en_US",
    ]);
    assert_stdout_is_json(&output);

    // No fallback warning should appear for a valid locale.
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        !stderr.contains("falling back to en_US"),
        "valid en_US locale must not trigger fallback warning; stderr was: {stderr}"
    );
}

// ─── IT-002: valid zh_CN locale ────────────────────────────────────────────

#[test]
fn wallet_login_valid_zh_cn_locale() {
    let (output, _home) = run_wallet_login(&[
        "test+integration-zh@example.com",
        "--locale",
        "zh_CN",
    ]);
    assert_stdout_is_json(&output);

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        !stderr.contains("falling back to en_US"),
        "valid zh_CN locale must not trigger fallback warning; stderr was: {stderr}"
    );
}

// ─── IT-003: --locale omitted (preserves existing behavior) ───────────────

#[test]
fn wallet_login_omitted_locale_uses_default() {
    let (output, _home) = run_wallet_login(&["test+integration-default@example.com"]);
    assert_stdout_is_json(&output);

    // With no --locale flag, validate_locale is never called, so no fallback
    // warning should appear.
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        !stderr.contains("falling back to en_US"),
        "omitting --locale must not trigger fallback warning; stderr was: {stderr}"
    );
}

// ─── IT-005: hyphenated en-US → fallback + warning ─────────────────────────

#[test]
fn wallet_login_hyphenated_en_us_falls_back_with_warning() {
    let (output, _home) = run_wallet_login(&[
        "test+integration-hyphenated@example.com",
        "--locale",
        "en-US",
    ]);
    assert_stdout_is_json(&output);

    let stderr = String::from_utf8_lossy(&output.stderr);
    let expected = fallback_warning("en-US");
    assert!(
        stderr.contains(&expected),
        "expected stderr to contain fallback warning `{expected}`; stderr was: {stderr}"
    );
}

// ─── IT-006: uppercase EN_US → fallback + warning (case-sensitive) ────────

#[test]
fn wallet_login_uppercase_en_us_falls_back_with_warning() {
    let (output, _home) = run_wallet_login(&[
        "test+integration-case@example.com",
        "--locale",
        "EN_US",
    ]);
    assert_stdout_is_json(&output);

    let stderr = String::from_utf8_lossy(&output.stderr);
    let expected = fallback_warning("EN_US");
    assert!(
        stderr.contains(&expected),
        "expected stderr to contain fallback warning `{expected}`; stderr was: {stderr}"
    );
}

// ─── IT-007: empty-string --locale '' → fallback + warning ────────────────

#[test]
fn wallet_login_empty_string_locale_falls_back_with_warning() {
    let (output, _home) = run_wallet_login(&[
        "test+integration-empty-locale@example.com",
        "--locale",
        "",
    ]);
    assert_stdout_is_json(&output);

    let stderr = String::from_utf8_lossy(&output.stderr);
    let expected = fallback_warning("");
    assert!(
        stderr.contains(&expected),
        "expected stderr to contain fallback warning `{expected}`; stderr was: {stderr}"
    );
}

// ─── IT-008: empty email → exit 1, no network ─────────────────────────────

#[test]
fn wallet_login_empty_email_fails() {
    let home = fresh_home();
    onchainos()
        .env("ONCHAINOS_HOME", home.path())
        .args(["wallet", "login", ""])
        .assert()
        .failure()
        .stdout(predicates::str::contains("email is required"));
}
