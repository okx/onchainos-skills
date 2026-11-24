//! Integration tests for the phased `wallet login --phase init|open|poll|full`
//! surface introduced when social login replaced the email/OTP + AK flows.
//!
//! ── Scope and contract ──────────────────────────────────────────────────
//!
//! Default tests are **offline, deterministic** tests of the new clap surface and its
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
//! A second, **golden** group pins the discoverability guarantee added to the
//! `init` success packet: `data.nextSteps.completeLogin` is the exact `--phase
//! poll` command with the packet's own `authSessionId` interpolated. `init`
//! opens the login page and returns `data.nextSteps.displayLoginUrl`; the Agent
//! renders that URL before it invokes the poll command. These init cases are
//! opt-in live tests because session creation now requires the backend.
//!
//! ── Sandbox conventions ──────────────────────────────────────────────────
//!
//! - `ONCHAINOS_HOME` points at a fresh isolated dir under
//!   `cli/target/test_tmp/cli_wallet_login_phase/` (the agent sandbox denies
//!   writes to the system tempdir, so `tempfile::tempdir()` is unsafe here).
//! - Authentication state is isolated through `ONCHAINOS_HOME`.
//! - The `poll` test targets a synthetic all-zero UUID that can never have a
//!   stored per-id key, so it deterministically hits the "no login in
//!   progress" guard regardless of any ambient keyring state.

mod common;

use common::{assert_ok_and_extract_data, onchainos};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

const AUTH_SOURCE: &str = include_str!("../src/commands/agentic_wallet/auth/mod.rs");
const WALLET_SKILL: &str =
    include_str!("../../skills/okx-agentic-wallet/references/wallet.md");
const WALLET_CLI_REFERENCE: &str =
    include_str!("../../skills/okx-agentic-wallet/references/wallet-cli-reference.md");

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
    cmd.env("ONCHAINOS_HOME", home)
        .env("ONCHAINOS_FORCE_FILE_KEYRING", "1")
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
#[ignore = "Creates a real backend login session; run explicitly against an approved test backend. Offline transport coverage lives in wallet_api unit tests."]
fn login_no_phase_defaults_to_init() {
    let (_tmp, home) = fresh_home();

    // No `--phase` → default `init`: mints the login session, attempts to open
    // the browser, and returns the login URL without polling. This test disables
    // browser opening through the sandbox helper.
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
    let data = assert_ok_and_extract_data(&output);
    assert_eq!(data["opened"], false, "browser is disabled in this test sandbox");
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

// ── discoverability: `init` next-step commands ────────────────────
//
// `init` creates a backend session, so these golden cases are opt-in live tests. `assert_ok_and_extract_data`
// (shared `common` helper) parses stdout, asserts `ok:true`, and returns `data`.

/// `init` shows the exact command to finish signing in.
///
/// `data.nextSteps.completeLogin` MUST be the `--phase poll` command whose
/// embedded `--session-id` equals `data.authSessionId` — the discoverability
/// guarantee (spec §6.2/§8.1). Asserting full-string equality (not just the
/// `contains:` substring the plan lists) also proves the session id is
/// interpolated, not a placeholder.
#[test]
#[ignore = "Creates a real backend login session; run explicitly against an approved test backend. Offline transport coverage lives in wallet_api unit tests."]
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

/// `init` returns the URL the Agent displays before polling.
///
/// Bare `wallet login` defaults to `--phase init`; the packet emits
/// `data.nextSteps.displayLoginUrl`, which equals `data.loginUrl`. With browser
/// opening disabled, `openLoginUrl` also carries the same manual fallback.
#[test]
#[ignore = "Creates a real backend login session; run explicitly against an approved test backend. Offline transport coverage lives in wallet_api unit tests."]
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
    assert_eq!(data["nextSteps"]["displayLoginUrl"], data["loginUrl"]);
    assert_eq!(
        data["nextSteps"]["requiredOrder"],
        serde_json::json!(["displayLoginUrl", "completeLogin"])
    );
    assert_eq!(data["opened"], false);
}

#[test]
fn login_init_opens_browser_before_emitting_result() {
    let init_body = AUTH_SOURCE
        .split_once("pub(super) async fn cmd_login_init()")
        .expect("cmd_login_init source")
        .1
        .split_once("fn next_steps_for_login")
        .expect("next_steps_for_login source")
        .0;
    let open_index = init_body.find("try_open_browser").expect("browser open call");
    let output_index = init_body.find("output::success").expect("init output");
    assert!(open_index < output_index);
}

#[test]
fn login_skill_requires_display_between_init_and_poll() {
    let skill = WALLET_SKILL.split_whitespace().collect::<Vec<_>>().join(" ");
    let cli_reference = WALLET_CLI_REFERENCE
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");
    assert!(skill.contains("`init` → display link → `poll`"));
    assert!(skill.contains("send this block as a visible Agent commentary message"));
    assert!(skill.contains("complete that message before invoking `poll`"));
    assert!(cli_reference.contains("opens the login page"));
    assert!(cli_reference.contains("polls the login result every 2 seconds"));
    assert!(
        cli_reference.contains("`nextSteps.requiredOrder=[\"displayLoginUrl\",\"completeLogin\"]`")
    );
    assert!(cli_reference.contains("sequence owned by [wallet.md](wallet.md)"));
}

#[test]
fn login_poll_interval_is_two_seconds() {
    assert!(AUTH_SOURCE.contains("const SOCIAL_LOGIN_POLL_INTERVAL_SECS: u64 = 2;"));
    assert!(AUTH_SOURCE.contains("Duration::from_secs(SOCIAL_LOGIN_POLL_INTERVAL_SECS)"));
}

/// `init` uses the canonical login host and shows the finish command.
#[test]
#[ignore = "Creates a real backend login session; run explicitly against an approved test backend. Offline transport coverage lives in wallet_api unit tests."]
fn login_init_uses_compiled_base_url_and_shows_next_steps() {
    let (_tmp, home) = fresh_home();

    let output = scrubbed(&mut onchainos(), &home)
        // Legacy runtime endpoint injection must not affect the CLI.
        .env("OKX_BASE_URL", "https://ignored.invalid")
        .args(["wallet", "login", "--phase", "init"])
        .output()
        .expect("run onchainos");

    let data = assert_ok_and_extract_data(&output);

    let login_url = data["loginUrl"]
        .as_str()
        .expect("init data.loginUrl must be a string");
    let expected_login_url = format!(
        "{}/account/sociallogin",
        env!("ONCHAINOS_COMPILED_BASE_URL").trim_end_matches('/')
    );
    assert!(
        login_url.contains(&expected_login_url),
        "loginUrl must use the compiled base host\ndata: {data}",
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

#[test]
fn removed_base_url_flag_is_rejected() {
    let (_tmp, home) = fresh_home();
    let output = scrubbed(&mut onchainos(), &home)
        .args(["--base-url", "https://ignored.invalid", "wallet", "status"])
        .output()
        .expect("run onchainos");

    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("unexpected argument '--base-url'"));
}

#[test]
#[ignore = "Creates a real backend login session; run explicitly against an approved test backend. Offline transport coverage lives in wallet_api unit tests."]
fn hidden_dev_flag_uses_beta_base_url() {
    let (_tmp, home) = fresh_home();
    let output = scrubbed(&mut onchainos(), &home)
        .args(["--dev", "wallet", "login", "--phase", "init"])
        .output()
        .expect("run onchainos");

    let data = assert_ok_and_extract_data(&output);
    let login_url = data["loginUrl"]
        .as_str()
        .expect("init data.loginUrl must be a string");
    assert!(
        login_url.starts_with("https://beta.okex.org/account/sociallogin?"),
        "--dev must select the beta login host\ndata: {data}",
    );
}

#[test]
fn dev_flag_is_hidden_from_help() {
    let output = onchainos()
        .arg("--help")
        .output()
        .expect("run onchainos --help");

    assert!(output.status.success());
    assert!(!String::from_utf8_lossy(&output.stdout).contains("--dev"));
}

// ── legacy status flag compatibility ───────────────────────────────────────

#[test]
fn wallet_status_accepts_include_subscriptions_flag() {
    let (_tmp, home) = fresh_home();

    // Keep accepting the old flag so existing callers do not break, but treat
    // it as an offline no-op that cannot add the post-login snapshot.
    let output = scrubbed(&mut onchainos(), &home)
        .args(["wallet", "status", "--include-subscriptions"])
        .output()
        .expect("run onchainos wallet status --include-subscriptions");

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        output.status.success(),
        "legacy status flag must remain accepted\nstdout: {stdout}\nstderr: {}",
        String::from_utf8_lossy(&output.stderr),
    );
    assert!(stdout.contains("\"loggedIn\":false"));
    assert!(!stdout.contains("postLoginSubscriptions"));
}

#[test]
fn wallet_status_hides_legacy_include_subscriptions_flag_from_help() {
    let (_tmp, home) = fresh_home();

    let output = scrubbed(&mut onchainos(), &home)
        .args(["wallet", "status", "--help"])
        .output()
        .expect("run onchainos wallet status --help");

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        output.status.success(),
        "wallet status --help must succeed\nstdout: {stdout}\nstderr: {}",
        String::from_utf8_lossy(&output.stderr),
    );
    assert!(
        !stdout.contains("--include-subscriptions"),
        "legacy compatibility flag must not be advertised\nstdout: {stdout}",
    );
}
