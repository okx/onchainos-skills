//! Integration tests for `onchainos wallet login` — sysLocale CLI reporting (WOO-96).
//!
//! Source plan: `oli-docs/ti1ewy8zgiudeyknqi1lr8thgig/integration-plan.csv` rows IT-001…IT-019.
//! Spec: `oli-docs/ti1ewy8zgiudeyknqi1lr8thgig/spec.md` (Appendix C names this file as the target).
//!
//! ─── Wiring status (READ BEFORE UN-IGNORING) ──────────────────────────────────
//!
//! The CSV was authored against the *full* feature. The reviewed source code (Stage 6.1 / A-14)
//! shipped ONLY the pure `cli/src/commands/agentic_wallet/locale.rs` module. The wiring into
//! `auth.rs` / `wallet_api.rs` (Stage 5.1 tasks T2/T4) is **TBC-BLOCKED** per `spec.md §11.2 TBC[3]`
//! (hard backend-first release order, §10.3) and is NOT in the binary yet:
//!   - `cmd_login` still uses the legacy `validate_locale` whitelist (`auth.rs:668`).
//!   - AK default locale is still `"en-US"` (hyphen) at `auth.rs:765`, not `en_US`.
//!   - `normalize_locale` / `detect_system_locale` are never called outside `locale.rs` unit tests.
//!   - `auth_init` / `ak_auth_verify` do not send the `sysLocale` body field.
//!
//! Therefore every row that asserts the NEW behavior is `#[ignore]`d with a precise reason. Those
//! bodies are complete and faithful — un-ignore them once T2/T4 land. Two further facts to know:
//!   1. The `normalize_locale` / `detect_system_locale` debug lines are gated behind
//!      `cfg!(feature = "debug-log")`. Even after wiring lands they only appear under
//!      `cargo test --tests --features debug-log`; the default integration-test command omits it.
//!   2. The `could not be normalized, omitting` / `using default en_US` warnings (spec §3.2) are
//!      user-facing (un-gated) but still depend on the T2/T4 wiring that replaces `validate_locale`.
//!
//! Active (runnable against today's binary): IT-016 (force bypass), IT-017, IT-018, IT-019.
//!
//! ─── Conventions ──────────────────────────────────────────────────────────────
//!   - Each test gets an isolated `ONCHAINOS_HOME` under `cli/target/test_tmp/cli_wallet/` (the
//!     bash sandbox denies writes to the system tempdir, so `tempfile::tempdir()` is NOT used).
//!     The home is set per-invocation via `Command::env` — never `std::env::set_var` (process-global,
//!     races across parallel tests in the same binary).
//!   - `live` rows go through `run_login_with_retry` (3×, retries on rate-limit / 5xx / timeout / DNS),
//!     the env-isolating analog of `common::run_with_retry` (which cannot stage a per-test home/env).
//!     `offline` rows invoke the binary directly with no retry.
//!   - AK credentials are read from the process env (CI secrets) and the test skips when they are
//!     unset — they are never hardcoded.
//!   - No base URL is hardcoded anywhere; `OKX_BASE_URL` is scrubbed so tests use the binary default.

mod common;

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Output;
use std::sync::atomic::{AtomicU64, Ordering};

use common::onchainos;
use serde_json::{json, Value};

// ─── Sandbox home ─────────────────────────────────────────────────────────────

/// RAII guard for an `ONCHAINOS_HOME` staged under `cli/target/test_tmp/`.
/// Removes the directory on drop.
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
/// `cli/target/test_tmp/cli_wallet/<pid>-<nanos>-<counter>`.
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

/// Stage a `wallets.json` so `derive_last_login_mode` reports `ak` (when
/// `is_ak == true`) or `email` (when `false`). Mirrors `WalletsJson`
/// (`#[serde(rename_all = "camelCase")]`) via a raw map to stay decoupled
/// from the binary's internal struct. Used by the mode-switch rows.
fn stage_wallets(home: &Path, email: &str, is_ak: bool) {
    let body = json!({
        "email": email,
        "isNew": false,
        "projectId": "p",
        "selectedAccountId": "acc-1",
        "accountsMap": {},
        "accounts": [{
            "projectId": "p",
            "accountId": "acc-1",
            "accountName": "Default",
            "isDefault": true,
        }],
        "isAk": is_ak,
    });
    let path = home.join("wallets.json");
    let f = fs::File::create(&path).expect("create wallets.json");
    serde_json::to_writer_pretty(f, &body).expect("write wallets.json");
}

/// Strip OKX_* / ONCHAINOS_HOME inherited from the host so each test sees a
/// pristine environment, then pin `ONCHAINOS_HOME` to the sandbox. Callers
/// re-set only what a given row needs (AK creds, LANG, …) afterwards.
fn scrubbed<'a>(cmd: &'a mut assert_cmd::Command, home: &Path) -> &'a mut assert_cmd::Command {
    cmd.env_remove("OKX_API_KEY")
        .env_remove("OKX_ACCESS_KEY")
        .env_remove("OKX_SECRET_KEY")
        .env_remove("OKX_PASSPHRASE")
        .env_remove("OKX_BASE_URL")
        .env("ONCHAINOS_HOME", home)
}

/// Parse `stdout` as JSON, panicking with raw stdout/stderr on failure.
fn parse_stdout_json(output: &Output) -> Value {
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    serde_json::from_str(&stdout).unwrap_or_else(|e| {
        panic!("invalid JSON in stdout: {e}\nstdout: {stdout}\nstderr: {stderr}")
    })
}

/// True when combined stdout+stderr carries a transient-failure marker worth a retry.
fn is_transient(haystack: &str) -> bool {
    const MARKERS: &[&str] = &[
        "Rate limited",
        "rate limit",
        "timed out",
        "timeout",
        "502",
        "503",
        "504",
        "Bad Gateway",
        "Service Unavailable",
        "Gateway Timeout",
        "dns error",
        "failed to lookup",
        "could not resolve",
    ];
    MARKERS.iter().any(|m| haystack.contains(m))
}

/// Env-isolating retry wrapper for `live` rows — the analog of
/// `common::run_with_retry` that can also stage a per-test home / env.
/// Up to 3 attempts; retries only on transient markers (rate-limit / 5xx /
/// timeout / DNS). A clean non-transient failure (e.g. backend rejection of a
/// placeholder email) returns immediately so the assertion sees the real result.
fn run_login_with_retry(home: &Path, envs: &[(&str, &str)], args: &[&str]) -> Output {
    let run_once = || {
        let mut cmd = onchainos();
        scrubbed(&mut cmd, home);
        for (k, v) in envs {
            cmd.env(k, v);
        }
        cmd.args(args);
        cmd.output().expect("failed to execute onchainos")
    };

    for attempt in 0..3u64 {
        if attempt > 0 {
            std::thread::sleep(std::time::Duration::from_secs(attempt));
        }
        let output = run_once();
        if output.status.success() {
            return output;
        }
        let combined = format!(
            "{}{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr),
        );
        if !is_transient(&combined) {
            return output;
        }
    }
    run_once()
}

/// Read AK credentials from the process env (CI secrets). Returns `None` when
/// any of the three is unset, so credential-dependent tests skip cleanly
/// rather than hardcoding secrets.
fn ak_creds_from_env() -> Option<(String, String, String)> {
    let key = std::env::var("OKX_API_KEY").ok()?;
    let secret = std::env::var("OKX_SECRET_KEY").ok()?;
    let pass = std::env::var("OKX_PASSPHRASE").ok()?;
    if key.is_empty() || secret.is_empty() || pass.is_empty() {
        return None;
    }
    Some((key, secret, pass))
}

// ════════════════════════════════════════════════════════════════════════════
//  IT-001 … IT-011 — `--locale` normalization + sysLocale detection (debug-log)
//  All #[ignore]d: assert NEW behavior not yet wired into the binary.
// ════════════════════════════════════════════════════════════════════════════

// ── IT-001: --locale zh_CN → debug normalize result=zh_CN (live) ──────────────
#[test]
#[ignore = "T2/T4 wiring TBC-blocked (spec §11.2 TBC[3]); needs --features debug-log. See file header."]
fn wallet_login_locale_zh_cn_normalizes_to_zh_cn() {
    let home = fresh_home();
    let output = run_login_with_retry(
        home.path(),
        &[],
        &["wallet", "login", "test+it-zh@example.com", "--locale", "zh_CN"],
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("result=zh_CN"),
        "expected debug normalize_locale `result=zh_CN`; stderr was: {stderr}"
    );
}

// ── IT-002: --locale en_US → debug normalize result=en_US (live) ──────────────
#[test]
#[ignore = "T2/T4 wiring TBC-blocked (spec §11.2 TBC[3]); needs --features debug-log. See file header."]
fn wallet_login_locale_en_us_normalizes_to_en_us() {
    let home = fresh_home();
    let output = run_login_with_retry(
        home.path(),
        &[],
        &["wallet", "login", "test+it-en@example.com", "--locale", "en_US"],
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("result=en_US"),
        "expected debug normalize_locale `result=en_US`; stderr was: {stderr}"
    );
}

// ── IT-003: --locale ja_JP passes through → debug result=ja_JP (live) ─────────
#[test]
#[ignore = "T2/T4 wiring TBC-blocked (spec §11.2 TBC[3]); needs --features debug-log. See file header."]
fn wallet_login_locale_ja_jp_passes_through() {
    let home = fresh_home();
    let output = run_login_with_retry(
        home.path(),
        &[],
        &["wallet", "login", "test+it-ja@example.com", "--locale", "ja_JP"],
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("result=ja_JP"),
        "expected debug normalize_locale `result=ja_JP` (pass-through); stderr was: {stderr}"
    );
}

// ── IT-004: LANG=zh_CN.UTF-8 → sysLocale debug normalized=zh_CN (live) ─────────
#[test]
#[ignore = "T2/T4 wiring TBC-blocked (spec §11.2 TBC[3]); needs --features debug-log. See file header."]
fn wallet_login_system_locale_zh_detected() {
    let home = fresh_home();
    let output = run_login_with_retry(
        home.path(),
        &[("LANG", "zh_CN.UTF-8"), ("LC_ALL", "zh_CN.UTF-8")],
        &["wallet", "login", "test+it-sys-zh@example.com"],
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("normalized=zh_CN"),
        "expected debug detect_system_locale `normalized=zh_CN`; stderr was: {stderr}"
    );
}

// ── IT-005: LANG=ja_JP.UTF-8 → sysLocale debug normalized=ja_JP (live) ─────────
#[test]
#[ignore = "T2/T4 wiring TBC-blocked (spec §11.2 TBC[3]); needs --features debug-log. See file header."]
fn wallet_login_system_locale_ja_detected() {
    let home = fresh_home();
    let output = run_login_with_retry(
        home.path(),
        &[("LANG", "ja_JP.UTF-8"), ("LC_ALL", "ja_JP.UTF-8")],
        &["wallet", "login", "test+it-sys-ja@example.com"],
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("normalized=ja_JP"),
        "expected debug detect_system_locale `normalized=ja_JP`; stderr was: {stderr}"
    );
}

// ── IT-006: AK login default locale en_US (underscore) → debug locale=en_US ───
//   network_required: live; tbc-blocked (AK ship gated on backend §10.3 TBC[3]).
#[test]
#[ignore = "AK default still `en-US` (auth.rs:765); T2/T4 TBC-blocked (spec §11.2 TBC[3]); needs --features debug-log + AK creds. See file header."]
fn wallet_login_ak_default_locale_en_us_underscore() {
    let Some((key, secret, pass)) = ak_creds_from_env() else {
        eprintln!("SKIP IT-006: OKX_API_KEY/OKX_SECRET_KEY/OKX_PASSPHRASE not set");
        return;
    };
    let home = fresh_home();
    let output = run_login_with_retry(
        home.path(),
        &[
            ("OKX_API_KEY", key.as_str()),
            ("OKX_SECRET_KEY", secret.as_str()),
            ("OKX_PASSPHRASE", pass.as_str()),
        ],
        &["wallet", "login"],
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("locale=en_US"),
        "expected AK debug params `locale=en_US` (underscore default); stderr was: {stderr}"
    );
}

// ── IT-007: LANG=yue_HK (Cantonese) → sysLocale normalized=zh_CN (live) ────────
#[test]
#[ignore = "T2/T4 wiring TBC-blocked (spec §11.2 TBC[3]); needs --features debug-log. See file header."]
fn wallet_login_system_locale_cantonese_maps_to_zh_cn() {
    let home = fresh_home();
    let output = run_login_with_retry(
        home.path(),
        &[("LANG", "yue_HK"), ("LC_ALL", "yue_HK")],
        &["wallet", "login", "test+it-sys-yue@example.com"],
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("normalized=zh_CN"),
        "expected debug detect_system_locale `normalized=zh_CN` for Cantonese; stderr was: {stderr}"
    );
}

// ── IT-008: LANG=en_GB.UTF-8 → sysLocale normalized=en_US (live) ───────────────
#[test]
#[ignore = "T2/T4 wiring TBC-blocked (spec §11.2 TBC[3]); needs --features debug-log. See file header."]
fn wallet_login_system_locale_en_gb_maps_to_en_us() {
    let home = fresh_home();
    let output = run_login_with_retry(
        home.path(),
        &[("LANG", "en_GB.UTF-8"), ("LC_ALL", "en_GB.UTF-8")],
        &["wallet", "login", "test+it-sys-engb@example.com"],
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("normalized=en_US"),
        "expected debug detect_system_locale `normalized=en_US` for en_GB; stderr was: {stderr}"
    );
}

// ── IT-009: LANG=C (POSIX placeholder) → sysLocale normalized=None (live) ──────
#[test]
#[ignore = "T2/T4 wiring TBC-blocked (spec §11.2 TBC[3]); needs --features debug-log. See file header."]
fn wallet_login_system_locale_posix_c_omitted() {
    let home = fresh_home();
    let output = run_login_with_retry(
        home.path(),
        &[("LANG", "C"), ("LC_ALL", "C")],
        &["wallet", "login", "test+it-sys-c@example.com"],
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("normalized=None"),
        "expected debug detect_system_locale `normalized=None` for POSIX C; stderr was: {stderr}"
    );
}

// ── IT-010: --locale zh-Hant → debug result=zh_CN (live) ──────────────────────
#[test]
#[ignore = "T2/T4 wiring TBC-blocked (spec §11.2 TBC[3]); needs --features debug-log. See file header."]
fn wallet_login_locale_zh_hant_normalizes_to_zh_cn() {
    let home = fresh_home();
    let output = run_login_with_retry(
        home.path(),
        &[],
        &["wallet", "login", "test+it-zhhant@example.com", "--locale", "zh-Hant"],
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("result=zh_CN"),
        "expected debug normalize_locale `result=zh_CN` for zh-Hant; stderr was: {stderr}"
    );
}

// ── IT-011: --locale en-GB → debug result=en_US (live) ────────────────────────
#[test]
#[ignore = "T2/T4 wiring TBC-blocked (spec §11.2 TBC[3]); needs --features debug-log. See file header."]
fn wallet_login_locale_en_gb_normalizes_to_en_us() {
    let home = fresh_home();
    let output = run_login_with_retry(
        home.path(),
        &[],
        &["wallet", "login", "test+it-engb-flag@example.com", "--locale", "en-GB"],
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("result=en_US"),
        "expected debug normalize_locale `result=en_US` for en-GB; stderr was: {stderr}"
    );
}

// ════════════════════════════════════════════════════════════════════════════
//  IT-012 … IT-015 — normalize→None warnings (un-gated, but wiring-dependent)
// ════════════════════════════════════════════════════════════════════════════

// ── IT-012: --locale "" (blank) → stderr "could not be normalized, omitting" ──
#[test]
#[ignore = "New normalize_locale warning not wired into cmd_login (T2/T4 TBC-blocked, spec §11.2 TBC[3]); binary still emits the legacy whitelist warning. See file header."]
fn wallet_login_locale_blank_warns_omitting() {
    let home = fresh_home();
    let output = run_login_with_retry(
        home.path(),
        &[],
        &["wallet", "login", "test+it-blank@example.com", "--locale", ""],
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("could not be normalized, omitting"),
        "expected stderr `could not be normalized, omitting` for blank locale; stderr was: {stderr}"
    );
}

// ── IT-013: --locale C (POSIX placeholder) → stderr "...omitting" ─────────────
#[test]
#[ignore = "New normalize_locale warning not wired into cmd_login (T2/T4 TBC-blocked, spec §11.2 TBC[3]); binary still emits the legacy whitelist warning. See file header."]
fn wallet_login_locale_posix_c_warns_omitting() {
    let home = fresh_home();
    let output = run_login_with_retry(
        home.path(),
        &[],
        &["wallet", "login", "test+it-posix@example.com", "--locale", "C"],
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("could not be normalized, omitting"),
        "expected stderr `could not be normalized, omitting` for POSIX C; stderr was: {stderr}"
    );
}

// ── IT-014: --locale "zh_CN; DROP" (injection) → stderr "...omitting" ─────────
#[test]
#[ignore = "New normalize_locale warning not wired into cmd_login (T2/T4 TBC-blocked, spec §11.2 TBC[3]); binary still emits the legacy whitelist warning. See file header."]
fn wallet_login_locale_injection_warns_omitting() {
    let home = fresh_home();
    let output = run_login_with_retry(
        home.path(),
        &[],
        &["wallet", "login", "test+it-inj@example.com", "--locale", "zh_CN; DROP"],
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("could not be normalized, omitting"),
        "expected stderr `could not be normalized, omitting` for illegal-char locale; stderr was: {stderr}"
    );
}

// ── IT-015: AK path, --locale "bad!" → stderr "using default en_US" ───────────
//   network_required: live; tbc-blocked (AK ship gated on backend §10.3 TBC[3]).
#[test]
#[ignore = "New AK normalize_locale fallback warning not wired (T2/T4 TBC-blocked, spec §11.2 TBC[3]); needs AK creds. See file header."]
fn wallet_login_ak_locale_bad_uses_default_en_us() {
    let Some((key, secret, pass)) = ak_creds_from_env() else {
        eprintln!("SKIP IT-015: OKX_API_KEY/OKX_SECRET_KEY/OKX_PASSPHRASE not set");
        return;
    };
    let home = fresh_home();
    let output = run_login_with_retry(
        home.path(),
        &[
            ("OKX_API_KEY", key.as_str()),
            ("OKX_SECRET_KEY", secret.as_str()),
            ("OKX_PASSPHRASE", pass.as_str()),
        ],
        &["wallet", "login", "--locale", "bad!"],
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("using default en_US"),
        "expected AK stderr `using default en_US` for invalid locale; stderr was: {stderr}"
    );
}

// ════════════════════════════════════════════════════════════════════════════
//  IT-016 … IT-019 — runnable against today's binary
// ════════════════════════════════════════════════════════════════════════════

// ── IT-016: --force bypasses the mode-diff guard (live) ───────────────────────
//   Precondition: persisted is_ak=true. With --force the mode-switch prompt is
//   bypassed, so the confirming envelope must NOT fire. The CSV's
//   `json:$.confirming == false` means "the prompt did not fire": no CLI path
//   emits a literal `confirming:false` key, so we assert `confirming != true`
//   AND exit != 2 (the exit-2 confirming code). A backend rejection of the
//   placeholder email on the proceeding live call is tolerated.
#[test]
fn wallet_login_force_bypasses_mode_diff() {
    let home = fresh_home();
    stage_wallets(home.path(), "", true);

    let output = run_login_with_retry(
        home.path(),
        &[],
        &["wallet", "login", "test+it-force@example.com", "--force"],
    );

    assert_ne!(
        output.status.code(),
        Some(2),
        "--force must bypass the mode-diff confirming gate (exit 2); got exit {:?}\nstdout: {}\nstderr: {}",
        output.status.code(),
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );

    // stdout is the JSON envelope on both success and bail; confirming must not be true.
    let stdout = String::from_utf8_lossy(&output.stdout);
    if !stdout.trim().is_empty() {
        let json = parse_stdout_json(&output);
        assert_ne!(
            json.get("confirming"),
            Some(&Value::Bool(true)),
            "--force must not produce a confirming prompt; got: {json}"
        );
    }
}

// ── IT-017: empty email argument → "email is required", exit 1 (offline) ──────
#[test]
fn wallet_login_empty_email_errors_exit_1() {
    let home = fresh_home();
    let output = scrubbed(&mut onchainos(), home.path())
        .args(["wallet", "login", ""])
        .output()
        .expect("run onchainos");

    assert_eq!(output.status.code(), Some(1), "expected exit 1");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("email is required"),
        "expected stdout to contain `email is required`; stdout was: {stdout}"
    );
}

// ── IT-018: AK login with no keys configured → "please set OKX_API_KEY", exit 1 (offline) ─
#[test]
fn wallet_login_ak_no_keys_errors_exit_1() {
    let home = fresh_home();
    // `scrubbed` clears OKX_API_KEY/OKX_SECRET_KEY/OKX_PASSPHRASE so the AK path
    // falls through to the "please set ..." bail regardless of host CI secrets.
    let output = scrubbed(&mut onchainos(), home.path())
        .args(["wallet", "login"])
        .output()
        .expect("run onchainos");

    assert_eq!(output.status.code(), Some(1), "expected exit 1");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("please set OKX_API_KEY"),
        "expected stdout to name the missing AK env vars; stdout was: {stdout}"
    );
}

// ── IT-019: email login after a prior AK session → confirming, exit 2 (offline) ─
#[test]
fn wallet_login_mode_switch_emits_confirming() {
    let home = fresh_home();
    // Prior session is_ak=true → derive_last_login_mode == "ak". Logging in by
    // email is a mode switch; the guard fires BEFORE any network call.
    stage_wallets(home.path(), "", true);

    let output = scrubbed(&mut onchainos(), home.path())
        .args(["wallet", "login", "test+it-modediff@example.com"])
        .output()
        .expect("run onchainos");

    assert_eq!(output.status.code(), Some(2), "expected exit 2 (confirming)");
    let json = parse_stdout_json(&output);
    assert_eq!(
        json["confirming"],
        Value::Bool(true),
        "mode switch must emit confirming:true; got: {json}"
    );
}
