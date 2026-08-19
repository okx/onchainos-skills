//! Integration tests for the phased `wallet login --phase init|open|poll|full`
//! surface introduced when social login replaced the email/OTP + AK flows.
//!
//! ── Scope and contract ──────────────────────────────────────────────────
//!
//! These are **offline, deterministic** tests of the new clap surface and its
//! pre-network guards — no backend round-trip, no live login. They pin the
//! handler-dispatch contract that the unit tests (which only cover helpers
//! like `build_login_url` / `classify_poll`) cannot reach:
//!
//!   - `--phase open` requires `--url`            (wallet.rs dispatch guard)
//!   - `--phase open` rejects non-http(s) URLs    (auth::is_browsable_url guard)
//!   - `--phase poll` with an unknown session id  (auth::cmd_login_poll guard)
//!
//! All three error *before* any network or browser call, so they are stable
//! across platforms and CI. Errors are emitted by `output::error` as a JSON
//! envelope `{ "ok": false, "error": "<msg>" }` on **stdout** with exit code 1
//! (main.rs:247-248), so each test asserts the exit is non-success AND the
//! envelope carries the expected message — never a bare `.failure()`.
//!
//! A second, **golden** group pins the C4 discoverability guarantee added to the
//! `init` success packet: `data.nextSteps.completeLogin` is the exact `--phase
//! poll` command with the packet's own `authSessionId` interpolated, and — when
//! the browser could not be opened (`ONCHAINOS_NO_BROWSER=1` → `opened:false`) —
//! `data.nextSteps.openLoginUrl` echoes `data.loginUrl`. `init` mints the session
//! locally, so these succeed offline with no backend round-trip.
//!
//! ── Sandbox conventions ──────────────────────────────────────────────────
//!
//! - `ONCHAINOS_HOME` points at a fresh isolated dir under
//!   `cli/target/test_tmp/cli_wallet_login_phase/` (the agent sandbox denies
//!   writes to the system tempdir, so `tempfile::tempdir()` is unsafe here).
//! - All `OKX_*` / `OKX_BASE_URL` env vars are scrubbed so the binary cannot
//!   fall through to host credentials.
//! - The `poll` test targets a synthetic all-zero UUID that can never have a
//!   stored per-id key, so it deterministically hits the "no login in
//!   progress" guard regardless of any ambient keyring state.

mod common;

use common::{assert_ok_and_extract_data, onchainos};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

// ── Sandbox helpers ──────────────────────────────────────────────────

/// Per-test sandbox guard that removes its directory on drop.
pub struct TestHome {
    path: PathBuf,
}

impl Drop for TestHome {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.path);
    }
}

static COUNTER: AtomicU64 = AtomicU64::new(0);

/// Build a fresh, isolated `ONCHAINOS_HOME` directory.
fn fresh_home() -> (TestHome, PathBuf) {
    let base = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("target")
        .join("test_tmp")
        .join("cli_wallet_login_phase");
    fs::create_dir_all(&base).expect("create test_tmp base");
    let pid = std::process::id();
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    let ts = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let dir = base.join(format!("{pid}-{ts}-{n}"));
    fs::create_dir_all(&dir).expect("create per-test dir");
    let path = dir.clone();
    (TestHome { path: dir }, path)
}

/// Strip host `OKX_*` env vars so each test sees a pristine environment, set
/// `ONCHAINOS_HOME`, and suppress the browser open (`init` opens best-effort).
fn scrubbed<'a>(cmd: &'a mut assert_cmd::Command, home: &Path) -> &'a mut assert_cmd::Command {
    cmd.env_remove("OKX_API_KEY")
        .env_remove("OKX_ACCESS_KEY")
        .env_remove("OKX_SECRET_KEY")
        .env_remove("OKX_PASSPHRASE")
        .env_remove("OKX_BASE_URL")
        .env("ONCHAINOS_HOME", home)
        .env("ONCHAINOS_NO_BROWSER", "1")
}

/// Assert the command failed (non-zero exit) and the stdout error envelope
/// contains `needle`.
#[track_caller]
fn assert_error_contains(output: &std::process::Output, needle: &str, label: &str) {
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        !output.status.success(),
        "[{label}] expected non-zero exit\nstdout: {stdout}\nstderr: {stderr}",
    );
    assert!(
        stdout.contains(needle),
        "[{label}] error envelope missing {needle:?}\nstdout: {stdout}\nstderr: {stderr}\nexit: {:?}",
        output.status.code(),
    );
}

// ── `--phase open` requires `--url` ──────────────────────────────────

#[test]
fn login_phase_open_without_url_errors() {
    let (_tmp, home) = fresh_home();

    let output = scrubbed(&mut onchainos(), &home)
        .args(["wallet", "login", "--phase", "open"])
        .output()
        .expect("run onchainos");

    assert_error_contains(&output, "is required for", "login --phase open (no --url)");
}

// ── `--phase open` rejects non-http(s) URLs ──────────────────────────

#[test]
fn login_phase_open_rejects_non_http_url() {
    let (_tmp, home) = fresh_home();

    let output = scrubbed(&mut onchainos(), &home)
        .args(["wallet", "login", "--phase", "open", "--url", "file:///etc/passwd"])
        .output()
        .expect("run onchainos");

    assert_error_contains(
        &output,
        "must be an http(s) URL",
        "login --phase open (file:// url)",
    );
}

// ── `--phase poll` with no pending session ───────────────────────────

#[test]
fn login_phase_poll_unknown_session_errors() {
    let (_tmp, home) = fresh_home();

    // All-zero UUID — never minted by `init`, so no per-id key can exist.
    let output = scrubbed(&mut onchainos(), &home)
        .args([
            "wallet",
            "login",
            "--phase",
            "poll",
            "--session-id",
            "00000000-0000-0000-0000-000000000000",
        ])
        .output()
        .expect("run onchainos");

    assert_error_contains(
        &output,
        "no login in progress",
        "login --phase poll (unknown session)",
    );
}

// ── bare `login` (no --phase) defaults to `init` ─────────────────────

#[test]
fn login_no_phase_defaults_to_init() {
    let (_tmp, home) = fresh_home();

    // No `--phase` → default `init`: mints and returns the login URL + opened
    // flag. No network, no polling; browser suppressed via ONCHAINOS_NO_BROWSER.
    let output = scrubbed(&mut onchainos(), &home)
        .args(["wallet", "login"])
        .output()
        .expect("run onchainos");

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        output.status.success(),
        "bare `wallet login` should succeed as init\nstdout: {stdout}\nstderr: {}",
        String::from_utf8_lossy(&output.stderr),
    );
    assert!(
        stdout.contains("loginUrl") && stdout.contains("authSessionId") && stdout.contains("opened"),
        "init output must carry loginUrl + authSessionId + opened\nstdout: {stdout}",
    );
}

// ── the removed `full` phase is rejected ─────────────────────────────

#[test]
fn login_phase_full_is_rejected() {
    let (_tmp, home) = fresh_home();

    // `full` was removed; clap must reject it as an invalid `--phase` value.
    let output = scrubbed(&mut onchainos(), &home)
        .args(["wallet", "login", "--phase", "full"])
        .output()
        .expect("run onchainos");

    assert!(
        !output.status.success(),
        "`--phase full` should be rejected (variant removed)",
    );
    let combined = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );
    assert!(
        combined.contains("invalid value") || combined.contains("full"),
        "expected clap invalid-value error for `--phase full`\noutput: {combined}",
    );
}

// ── C4 discoverability: `init` next-step commands ────────────────────
//
// `init` mints the session locally (uuid + keypair) and never touches the
// backend, so these golden cases run offline. `assert_ok_and_extract_data`
// (shared `common` helper) parses stdout, asserts `ok:true`, and returns `data`.

/// IT-001 — `init` shows the exact command to finish signing in.
///
/// `data.nextSteps.completeLogin` MUST be the `--phase poll` command whose
/// embedded `--session-id` equals `data.authSessionId` — the C4 discoverability
/// guarantee (spec §6.2/§8.1). Asserting full-string equality (not just the
/// `contains:` substring the plan lists) also proves the session id is
/// interpolated, not a placeholder.
#[test]
fn login_init_next_steps_complete_login_matches_session_id() {
    let (_tmp, home) = fresh_home();

    let output = scrubbed(&mut onchainos(), &home)
        .args(["wallet", "login", "--phase", "init"])
        .output()
        .expect("run onchainos");

    let data = assert_ok_and_extract_data(&output);

    let session_id = data["authSessionId"]
        .as_str()
        .expect("init data.authSessionId must be a string");
    let complete_login = data["nextSteps"]["completeLogin"]
        .as_str()
        .expect("init data.nextSteps.completeLogin must be a string");

    assert_eq!(
        complete_login,
        format!("onchainos wallet login --phase poll --session-id {session_id}"),
        "completeLogin must be the poll command with authSessionId interpolated\ndata: {data}",
    );
}

/// IT-002 — if the browser doesn't open, `init` shows which URL to open.
///
/// Bare `wallet login` defaults to `--phase init`; with the browser suppressed
/// (`opened:false`) the packet emits `data.nextSteps.openLoginUrl`, which MUST be
/// non-empty and equal `data.loginUrl` (spec §4.3/§6.2/§17.3-3). Also exercises
/// the `--phase` default boundary. The `opened:true` branch (openLoginUrl
/// omitted) is unreachable headless and is covered by the `next_steps_for_login`
/// unit test (spec §18.2).
#[test]
fn login_bare_default_init_next_steps_open_login_url() {
    let (_tmp, home) = fresh_home();

    let output = scrubbed(&mut onchainos(), &home)
        .args(["wallet", "login"])
        .output()
        .expect("run onchainos");

    let data = assert_ok_and_extract_data(&output);

    let login_url = data["loginUrl"]
        .as_str()
        .expect("init data.loginUrl must be a string");
    let open_login_url = data["nextSteps"]["openLoginUrl"]
        .as_str()
        .expect("data.nextSteps.openLoginUrl must be present when browser is suppressed");

    assert!(
        !open_login_url.is_empty(),
        "openLoginUrl must be non-empty\ndata: {data}",
    );
    assert_eq!(
        open_login_url, login_url,
        "openLoginUrl must echo loginUrl so the user can open it manually\ndata: {data}",
    );
}

/// IT-006 — `init` shows the finish command even against a different backend.
///
/// The base URL used to build `data.loginUrl` is override-driven (`OKX_BASE_URL`
/// → `option_env!` → `DEFAULT_BASE_URL`), so the override host is fed in as the
/// test input and re-used to build the expected substring — no environment-drift
/// literal. `data.loginUrl` MUST use the overridden host and
/// `data.nextSteps.completeLogin` MUST still be present and correct: the
/// discoverability guarantee is base-URL-independent (spec §7.4/§10.1; C4).
#[test]
fn login_init_next_steps_base_url_independent() {
    // Fed to the CLI as `OKX_BASE_URL` AND used to build the expected substring,
    // so this asserts against the injected value (offline), not a live host.
    const OVERRIDE_BASE_URL: &str = "https://web3.okx.com";

    let (_tmp, home) = fresh_home();

    // `scrubbed` clears any inherited `OKX_BASE_URL`; re-set the override after.
    let output = scrubbed(&mut onchainos(), &home)
        .env("OKX_BASE_URL", OVERRIDE_BASE_URL)
        .args(["wallet", "login", "--phase", "init"])
        .output()
        .expect("run onchainos");

    let data = assert_ok_and_extract_data(&output);

    let login_url = data["loginUrl"]
        .as_str()
        .expect("init data.loginUrl must be a string");
    assert!(
        login_url.contains(&format!("{OVERRIDE_BASE_URL}/account/sociallogin")),
        "loginUrl must use the overridden base host\ndata: {data}",
    );

    let session_id = data["authSessionId"]
        .as_str()
        .expect("init data.authSessionId must be a string");
    let complete_login = data["nextSteps"]["completeLogin"]
        .as_str()
        .expect("init data.nextSteps.completeLogin must be a string");
    assert_eq!(
        complete_login,
        format!("onchainos wallet login --phase poll --session-id {session_id}"),
        "completeLogin must stay correct regardless of base URL\ndata: {data}",
    );
}

// ── explicit login/status path can request the combined snapshot ─────

#[test]
fn wallet_status_accepts_include_subscriptions_flag() {
    let (_tmp, home) = fresh_home();

    // A clean home is logged out, so this remains fully offline while pinning
    // the new one-command user-facing status surface.
    let output = scrubbed(&mut onchainos(), &home)
        .args(["wallet", "status", "--include-subscriptions"])
        .output()
        .expect("run onchainos wallet status --include-subscriptions");

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        output.status.success(),
        "combined status flag must be accepted\nstdout: {stdout}\nstderr: {}",
        String::from_utf8_lossy(&output.stderr),
    );
    assert!(stdout.contains("\"loggedIn\":false"));
    assert!(!stdout.contains("postLoginSubscriptions"));
}
