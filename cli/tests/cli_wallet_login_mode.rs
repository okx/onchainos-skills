//! Integration tests for the `wallet login` / `wallet status` / `wallet logout`
//! login-mode contract.
//!
//! Source: `oli-docs/ux87w4zqyigckwkm63qleunmgfg/integration-plan.csv` (IT-001 … IT-020).
//!
//! Design pivot (2026-05-14): there is NO persisted `lastLoginMode` field on
//! disk. The mode is derived from `wallets.json` (`email` + `is_ak`) via
//! `super::common::derive_last_login_mode`:
//!   - `is_ak == true`            → "ak"
//!   - `is_ak == false && email`  → "email"
//!   - both empty / wallets.json absent → null
//!
//! Each test creates its own isolated `TempDir` for `ONCHAINOS_HOME`, stages
//! the fixture wallets.json (or omits it), runs the binary, and asserts on the
//! exit code + stdout JSON + audit.jsonl.
//!
//! Tests that require a stub HTTP backend (CSV `network_required=mock`) are
//! left as `#[ignore]` for Stage 7 until the mock harness exists.
//!
//! ─── Accepted gaps (spec §5.3 / §12.2) ────────────────────────────────────
//!
//! These behaviors are intentional and must NOT be "fixed" by future maintainers.
//! See `oli-docs/kweedzkuaonfrwxhjp3lsewqgcg/spec.md` for full context.
//!
//!   - **AG-1** (spec §5.3): `user_choice=no` is never written to the audit log.
//!     The user pressing "no" at the mode-diff prompt is just "didn't re-run the
//!     command with `--force`" — the CLI exits 2 and has no way to observe the
//!     follow-up decision. Tests assert *presence* of `user_choice=yes` on the
//!     `--force` retry path; they explicitly do NOT assert *absence* of
//!     `user_choice=no`, because CLI cannot emit it.
//!
//!   - **AG-2** (spec §11.1 / §12.2): the login-diff warning copy is English-only.
//!     `--locale` does not affect this message. Tests assert the literal English
//!     substring `"not the account you used last time"` (skill discriminator)
//!     plus the scene-specific phrasing (`"Login method: <X> → <Y>"` for
//!     mode-switch cases (the Email side carries the masked email of that
//!     account in parens; the API Key side does not show the key), plus
//!     `"Account: <masked-old> → <masked-new>"` for same-mode email
//!     account-switch cases, and the fixed sentence
//!     `"The API Key in your env has changed"` for same-mode AK account-
//!     switch cases (no key, raw or masked, ever appears in the message
//!     since the user already knows the env value). User-facing mode
//!     labels are `Email` and `API Key`; internal audit args keep the
//!     raw lowercase `email` / `ak` tokens.
//!
//!   - **AG-3** (spec §12.2): first-time login (empty `ONCHAINOS_HOME`,
//!     `last_login_mode = None`) silently proceeds with no mode-diff warning.
//!     IT-014 documents this case; no `prompt_shown` event is emitted.
//!
//!   - **AG-4** (RETIRED — was: AK→AK fires the old `bail!()` AK-switch
//!     guard at exit 1): the standalone AK-switch guard was folded into the
//!     unified login-diff pre-check. AK→AK with a different `api_key` now
//!     goes through the same exit-2 CliConfirming envelope as Email1→Email2,
//!     with PII-masked api_keys in the scene line and a `prompt_shown`
//!     audit event (switch_kind=account). IT-020 was rewritten accordingly.
//!
//! Showcase Stage 9.5 reconciliation (spec §11.2) — informational, no
//! assertion: Set A (6 rows) mechanically drops `--format json` from the `cli`
//! cell; Set B (13 rows) leaves `cli` cells unchanged and only refreshes the
//! `result` cell after re-running. No `--format` flag exists or will be added
//! (spec §1.4); the CLI always emits JSON via `cli/src/output.rs`.

mod common;

use common::onchainos;
use serde_json::{json, Value};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

// ── Helpers ──────────────────────────────────────────────────────────

/// Per-test sandbox guard that removes its directory on drop. Lives under
/// `cli/target/test_tmp/<unique-suffix>` because the sandbox we run inside
/// denies writes to the system `/var/folders/.../T/` tempdir.
pub struct TestHome {
    path: PathBuf,
}

impl Drop for TestHome {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.path);
    }
}

static COUNTER: AtomicU64 = AtomicU64::new(0);

/// Build a fresh, isolated `ONCHAINOS_HOME` directory under `cli/target/test_tmp/`.
fn fresh_home() -> (TestHome, PathBuf) {
    let base = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("target")
        .join("test_tmp")
        .join("cli_wallet_login_mode");
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

/// Stage a wallets.json file inside `home` matching the fixture description.
/// Mirrors `WalletsJson` shape (`#[serde(rename_all = "camelCase")]`) using a
/// raw JSON map so the integration test stays decoupled from the binary's
/// internal struct definition.
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

/// Stage a session.json file inside `home` with the given `apiKey` field.
/// Used by IT-020 to set up the pre-existing AK switch precondition.
fn stage_session(home: &Path, api_key: &str) {
    let body = json!({
        "teeId": "",
        "sessionCert": "",
        "encryptedSessionSk": "",
        "sessionKeyExpireAt": "",
        "apiKey": api_key,
    });
    let path = home.join("session.json");
    let f = fs::File::create(&path).expect("create session.json");
    serde_json::to_writer_pretty(f, &body).expect("write session.json");
}

/// Read `$home/audit.jsonl` and return one parsed JSON value per non-empty
/// line. The first line is a `{"type":"device", ...}` header which we keep
/// verbatim — callers should filter by `command` field as needed.
fn audit_lines(home: &Path) -> Vec<Value> {
    let path = home.join("audit.jsonl");
    if !path.exists() {
        return Vec::new();
    }
    let raw = fs::read_to_string(&path).expect("read audit.jsonl");
    raw.lines()
        .filter(|l| !l.trim().is_empty())
        .map(|l| serde_json::from_str(l).expect("parse audit.jsonl line"))
        .collect()
}

/// Parse `stdout` as JSON. Panics with the raw stdout/stderr on parse failure.
fn parse_stdout_json(output: &std::process::Output) -> Value {
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    serde_json::from_str(&stdout).unwrap_or_else(|e| {
        panic!("invalid JSON in stdout: {e}\nstdout: {stdout}\nstderr: {stderr}")
    })
}

/// Strip any OKX_* / ONCHAINOS_HOME env vars inherited from the host so each
/// test sees a pristine environment. The caller re-sets only what's needed.
fn scrubbed<'a>(cmd: &'a mut assert_cmd::Command, home: &Path) -> &'a mut assert_cmd::Command {
    cmd.env_remove("OKX_API_KEY")
        .env_remove("OKX_ACCESS_KEY")
        .env_remove("OKX_SECRET_KEY")
        .env_remove("OKX_PASSPHRASE")
        .env_remove("OKX_BASE_URL")
        .env("ONCHAINOS_HOME", home)
}

// ── IT-001: wallet status, no wallets.json ───────────────────────────

#[test]
fn it_001_status_no_wallets_reports_null_mode() {
    let (_tmp, home) = fresh_home();

    let output = scrubbed(&mut onchainos(), &home)
        .args(["wallet", "status"])
        .output()
        .expect("run onchainos");

    assert_eq!(output.status.code(), Some(0), "expected exit 0");
    let json = parse_stdout_json(&output);
    assert_eq!(json["ok"], Value::Bool(true));
    assert_eq!(json["data"]["lastLoginMode"], Value::Null);
    assert_eq!(json["data"]["loggedIn"], Value::Bool(false));
}

// ── IT-002: wallet status, email mode ────────────────────────────────

#[test]
fn it_002_status_email_mode_reports_email() {
    let (_tmp, home) = fresh_home();
    stage_wallets(&home, "fixture@example.com", false);

    let output = scrubbed(&mut onchainos(), &home)
        .args(["wallet", "status"])
        .output()
        .expect("run onchainos");

    assert_eq!(output.status.code(), Some(0));
    let json = parse_stdout_json(&output);
    assert_eq!(json["data"]["lastLoginMode"], Value::String("email".into()));
}

// ── IT-003: wallet status, ak mode ───────────────────────────────────

#[test]
fn it_003_status_ak_mode_reports_ak() {
    let (_tmp, home) = fresh_home();
    // is_ak=true; email may be empty or set — derive_last_login_mode short-circuits on is_ak.
    stage_wallets(&home, "", true);

    let output = scrubbed(&mut onchainos(), &home)
        .args(["wallet", "status"])
        .output()
        .expect("run onchainos");

    assert_eq!(output.status.code(), Some(0));
    let json = parse_stdout_json(&output);
    assert_eq!(json["data"]["lastLoginMode"], Value::String("ak".into()));
}

// ── IT-004: SKIPPED — invalid-mode branch unreachable under design pivot ─

#[test]
#[ignore = "spec §10.1: no persisted last_login_mode field — the 'invalid value → null' branch is unreachable under the design pivot (2026-05-14). Mode is derived from (email + is_ak), neither of which has an invalid third state."]
fn it_004_status_invalid_mode_falls_back_to_null() {
    // Intentionally empty — see #[ignore] reason above.
}

// ── IT-005: wallet status, logged-out (no wallets.json) ──────────────

#[test]
fn it_005_status_logged_out_reports_null_and_false() {
    let (_tmp, home) = fresh_home();
    // No wallets.json staged — equivalent to logged-out / pre-feature.

    let output = scrubbed(&mut onchainos(), &home)
        .args(["wallet", "status"])
        .output()
        .expect("run onchainos");

    assert_eq!(output.status.code(), Some(0));
    let json = parse_stdout_json(&output);
    assert_eq!(json["data"]["loggedIn"], Value::Bool(false));
    assert_eq!(json["data"]["lastLoginMode"], Value::Null);
}

// ── IT-006: wallet login with email, prior session was ak → exit 2 confirming ─

#[test]
fn it_006_login_email_after_ak_emits_confirming() {
    let (_tmp, home) = fresh_home();
    // Prior session: is_ak=true, no email — derive → "ak".
    stage_wallets(&home, "", true);

    let output = scrubbed(&mut onchainos(), &home)
        .args(["wallet", "login", "user@example.com"])
        .output()
        .expect("run onchainos");

    assert_eq!(output.status.code(), Some(2), "expected exit 2");
    let json = parse_stdout_json(&output);
    assert_eq!(json["confirming"], Value::Bool(true));
}

// ── IT-007: same scenario, message contains the discriminator substring ─

#[test]
fn it_007_login_email_after_ak_message_contains_discriminator() {
    let (_tmp, home) = fresh_home();
    stage_wallets(&home, "", true);

    let output = scrubbed(&mut onchainos(), &home)
        .args(["wallet", "login", "user@example.com"])
        .output()
        .expect("run onchainos");

    assert_eq!(output.status.code(), Some(2));
    let json = parse_stdout_json(&output);
    let msg = json["message"].as_str().expect("message field is string");
    assert!(
        msg.contains("not the account you used last time"),
        "message must contain 'not the account you used last time' verbatim (skill discriminator); got: {msg}"
    );
}

// ── IT-008: same scenario, next instruction is exact ─────────────────

#[test]
fn it_008_login_email_after_ak_next_is_exact() {
    let (_tmp, home) = fresh_home();
    stage_wallets(&home, "", true);

    let output = scrubbed(&mut onchainos(), &home)
        .args(["wallet", "login", "user@example.com"])
        .output()
        .expect("run onchainos");

    assert_eq!(output.status.code(), Some(2));
    let json = parse_stdout_json(&output);
    assert_eq!(
        json["next"],
        Value::String(
            "If the user confirms, re-run the same command with --force flag appended to proceed."
                .into()
        )
    );
}

// ── IT-009: same scenario, mode tokens present, PII absent ───────────

#[test]
fn it_009_login_email_after_ak_message_has_backticked_modes_no_pii() {
    let (_tmp, home) = fresh_home();
    // Use an email + AK on the persisted side to give the PII assertion teeth:
    // even though is_ak=true wins for derivation, the stored email should NOT
    // surface in the user-facing message.
    stage_wallets(&home, "fixture@example.com", true);

    let output = scrubbed(&mut onchainos(), &home)
        .args(["wallet", "login", "user@example.com"])
        .output()
        .expect("run onchainos");

    assert_eq!(output.status.code(), Some(2));
    let json = parse_stdout_json(&output);
    let msg = json["message"].as_str().expect("message field is string");
    // Mode-switch scene line uses display tokens "Email" / "API Key". On
    // AK→Email switch, the Email side is NEW, so the masked NEW email
    // appears in parens. The persisted (fixture@example.com) is NOT shown
    // even in masked form for this direction.
    assert!(
        msg.contains("Login method: API Key → Email (u***r@example.com)"),
        "message must contain 'Login method: API Key → Email (u***r@example.com)'; got: {msg}"
    );
    assert!(
        !msg.contains("fixture@example.com"),
        "message must NOT leak the persisted email (PII); got: {msg}"
    );
    assert!(
        !msg.contains("OKX_API_KEY"),
        "message must NOT leak the API Key env var name; got: {msg}"
    );
}

// ── IT-010: wallet login (no email arg) with AK env → ak; prior was email ─

#[test]
fn it_010_login_ak_env_after_email_emits_confirming() {
    let (_tmp, home) = fresh_home();
    // Prior session: email-mode (non-empty email, is_ak=false).
    stage_wallets(&home, "fixture@example.com", false);

    let output = scrubbed(&mut onchainos(), &home)
        .env("OKX_API_KEY", "dummyKey")
        .env("OKX_SECRET_KEY", "dummySecret")
        .env("OKX_PASSPHRASE", "dummyPass")
        .args(["wallet", "login"])
        .output()
        .expect("run onchainos");

    assert_eq!(output.status.code(), Some(2), "expected exit 2");
    let json = parse_stdout_json(&output);
    assert_eq!(json["confirming"], Value::Bool(true));
}

// ── IT-011: same scenario, message renders correct last/current mode tokens ─

#[test]
fn it_011_login_ak_env_after_email_message_renders_modes() {
    let (_tmp, home) = fresh_home();
    stage_wallets(&home, "fixture@example.com", false);

    let output = scrubbed(&mut onchainos(), &home)
        .env("OKX_API_KEY", "dummyKey")
        .env("OKX_SECRET_KEY", "dummySecret")
        .env("OKX_PASSPHRASE", "dummyPass")
        .args(["wallet", "login"])
        .output()
        .expect("run onchainos");

    assert_eq!(output.status.code(), Some(2));
    let json = parse_stdout_json(&output);
    let msg = json["message"].as_str().expect("message field is string");
    // Direction: persisted email → planned ak. On Email→AK switch the
    // Email side is OLD, so the masked PERSISTED email appears in parens
    // (fixture@example.com → f***e@example.com).
    assert!(
        msg.contains("Login method: Email (f***e@example.com) → API Key"),
        "message must contain 'Login method: Email (f***e@example.com) → API Key'; got: {msg}"
    );
    // PII §8.1: raw persisted email must not appear in the message even
    // though its masked form does.
    assert!(
        !msg.contains("fixture@example.com"),
        "raw persisted email leaked; got: {msg}"
    );
}

// ── IT-012, IT-013, IT-014: SKIPPED — require mock backend (Stage 7) ──

#[test]
#[ignore = "Stage 7: pending OKX mock backend harness (IT-012, network_required=mock)"]
fn it_012_login_force_bypasses_mode_diff_against_mock_backend() {
    // AG-1 (spec §5.3): when wired with a mock backend, this test will assert
    // presence of `user_choice=yes` in the audit log on the `--force` retry
    // path. It must explicitly NOT assert absence of `user_choice=no` — CLI
    // cannot emit it (see top-of-file AG-1 note).
    // Intentionally empty — see #[ignore] reason above.
}

#[test]
#[ignore = "Stage 7: pending OKX mock backend harness (IT-013, network_required=mock)"]
fn it_013_login_same_mode_email_proceeds_against_mock_backend() {
    // AG-4 / §3.3: mode-diff fires BEFORE the AK-switch guard. When modes
    // match (here: email→email), mode-diff is a no-op and login proceeds
    // normally. The AK-switch guard (auth.rs:530-544) only applies to AK→AK
    // with a different api_key; it does not fire on this same-mode path.
    // Intentionally empty — see #[ignore] reason above.
}

#[test]
#[ignore = "Stage 7: pending OKX mock backend harness (IT-014, network_required=mock)"]
fn it_014_login_first_time_no_diff_against_mock_backend() {
    // Intentionally empty — see #[ignore] reason above.
}

// ── IT-015: SKIPPED — invalid-mode branch unreachable under design pivot ─

#[test]
#[ignore = "spec §10.1: no persisted last_login_mode field — the 'invalid value → no prompt' branch is unreachable under the design pivot (2026-05-14)."]
fn it_015_login_with_invalid_persisted_mode_falls_through() {
    // Intentionally empty — see #[ignore] reason above.
}

// ── IT-016: audit emits prompt_shown only (no user_choice) on exit 2 ──

#[test]
fn it_016_login_mode_diff_writes_prompt_shown_only_when_not_forced() {
    let (_tmp, home) = fresh_home();
    stage_wallets(&home, "", true);

    let output = scrubbed(&mut onchainos(), &home)
        .args(["wallet", "login", "user@example.com"])
        .output()
        .expect("run onchainos");

    assert_eq!(output.status.code(), Some(2), "expected exit 2");

    let entries = audit_lines(&home);
    let prompt_events: Vec<&Value> = entries
        .iter()
        .filter(|e| {
            e.get("command")
                .and_then(|c| c.as_str())
                .map(|c| c.starts_with("login_mode_prompt_"))
                .unwrap_or(false)
        })
        .collect();

    assert_eq!(
        prompt_events.len(),
        1,
        "expected exactly one login_mode_prompt_* event; got: {prompt_events:?}"
    );
    let evt = prompt_events[0];
    assert_eq!(
        evt["command"],
        Value::String("login_mode_prompt_shown".into())
    );
    let args = evt["args"].as_array().expect("args is array");
    let args_strs: Vec<&str> = args.iter().filter_map(|v| v.as_str()).collect();
    assert!(
        args_strs.contains(&"current_mode=email"),
        "audit args must contain 'current_mode=email'; got: {args_strs:?}"
    );
    assert!(
        args_strs.contains(&"last_login_mode=ak"),
        "audit args must contain 'last_login_mode=ak'; got: {args_strs:?}"
    );

    // No user_choice line in this flow.
    assert!(
        !entries.iter().any(|e| e
            .get("command")
            .and_then(|c| c.as_str())
            .map(|c| c == "login_mode_prompt_user_choice")
            .unwrap_or(false)),
        "audit must NOT contain login_mode_prompt_user_choice in the not-forced flow"
    );
}

// ── IT-017: SKIPPED — requires mock backend ──────────────────────────

#[test]
#[ignore = "Stage 7: pending OKX mock backend harness (IT-017, network_required=mock)"]
fn it_017_login_force_writes_prompt_shown_and_user_choice() {
    // AG-1 (spec §5.3): once wired with a mock backend, assert PRESENCE of
    // `user_choice=yes` on the `--force` retry path. Do NOT add a symmetric
    // assertion for absence of `user_choice=no` — CLI never emits "no"
    // because exiting 2 and not retrying is indistinguishable from "user
    // walked away" (see top-of-file AG-1 note).
    // Intentionally empty — `--force` triggers auth_init which would hit live API.
}

// ── IT-018: wallet logout wipes wallets.json ─────────────────────────

#[test]
fn it_018_logout_removes_wallets_json() {
    let (_tmp, home) = fresh_home();
    stage_wallets(&home, "fixture@example.com", false);
    assert!(
        home.join("wallets.json").exists(),
        "precondition: wallets.json staged"
    );

    let output = scrubbed(&mut onchainos(), &home)
        .args(["wallet", "logout"])
        .output()
        .expect("run onchainos");

    assert_eq!(output.status.code(), Some(0), "expected exit 0");
    let json = parse_stdout_json(&output);
    assert_eq!(json["ok"], Value::Bool(true));

    assert!(
        !home.join("wallets.json").exists(),
        "wallets.json must be removed after logout"
    );
}

// ── IT-019: wallet status after logout → null mode + loggedIn=false ──

#[test]
fn it_019_status_after_logout_reports_null_and_false() {
    let (_tmp, home) = fresh_home();
    stage_wallets(&home, "fixture@example.com", false);

    // Step 1: logout.
    let logout = scrubbed(&mut onchainos(), &home)
        .args(["wallet", "logout"])
        .output()
        .expect("run onchainos logout");
    assert_eq!(logout.status.code(), Some(0));

    // Step 2: status — must observe the wipe.
    let status = scrubbed(&mut onchainos(), &home)
        .args(["wallet", "status"])
        .output()
        .expect("run onchainos status");
    assert_eq!(status.status.code(), Some(0));
    let json = parse_stdout_json(&status);
    assert_eq!(json["data"]["lastLoginMode"], Value::Null);
    assert_eq!(json["data"]["loggedIn"], Value::Bool(false));
}

// ── IT-015 (new CSV): wallet login with no email arg AND no AK env → exit 1, error mentions OKX_API_KEY ─
//
// Source: `oli-docs/kweedzkuaonfrwxhjp3lsewqgcg/integration-plan.csv` row IT-015
// (no lark_record_id). Generic-error coverage for spec §3.1 row 2.
//
// Note on numbering: the legacy tests in this file follow the original
// (ux87w4...) 20-row CSV. The current CSV (kweedzk...) renumbered to 15 rows;
// its row IT-015 is this "no email and no AK env" branch, which has no
// counterpart in the legacy numbering. Kept under `it_015_new_*` to avoid
// renaming the legacy `it_015_*` slot (`#[ignore]`d, design-pivot-unreachable).

#[test]
fn it_015_new_login_no_email_no_ak_env_errors_exit_1() {
    let (_tmp, home) = fresh_home();
    // No wallets.json, no AK env vars, no email arg → cmd_login falls
    // through to the `_ => bail!()` arm at auth.rs:550 emitting the
    // "please set OKX_API_KEY..." error.

    let output = scrubbed(&mut onchainos(), &home)
        .args(["wallet", "login"])
        .output()
        .expect("run onchainos");

    assert_eq!(output.status.code(), Some(1), "expected exit 1");
    let json = parse_stdout_json(&output);
    assert_eq!(json["ok"], Value::Bool(false));
    let err = json["error"].as_str().expect("error field is string");
    assert!(
        err.contains("OKX_API_KEY"),
        "error must mention OKX_API_KEY env var per spec §3.1 row 2; got: {err}"
    );
}

// ── IT-020: AK→AK with different API key → unified login-diff confirming (exit 2) ─

#[test]
fn it_020_ak_to_different_ak_key_emits_confirming() {
    // After the unification of the AK-switch guard into the login-diff
    // pre-check (this PR), AK→AK with a different api_key goes through the
    // same exit-2 + CliConfirming envelope as Email1→Email2 and mode
    // switches. The previously separate `auth.rs:530-544` `bail!()` guard
    // (exit 1, raw api_keys in the message) has been deleted. Replaces the
    // earlier IT-020 assertion which expected exit 1 from that guard.
    //
    // Spec changes encoded by this test:
    //   - exit code: 1 → 2
    //   - JSON shape: `{ok: false, error: ...}` → `{confirming: true, message, next}`
    //   - message: raw api_keys → masked api_keys + skill discriminator
    //   - audit: silent → `login_mode_prompt_shown` with switch_kind=account
    let (_tmp, home) = fresh_home();
    stage_wallets(&home, "", true);
    let old_key = "OLDKEY1234-5678-90ab-cdef-1234567890ab";
    let new_key = "NEWKEY5678-9012-34cd-ef01-5678901234ef";
    stage_session(&home, old_key);

    let output = scrubbed(&mut onchainos(), &home)
        .env("OKX_API_KEY", new_key)
        .env("OKX_SECRET_KEY", "anySecret")
        .env("OKX_PASSPHRASE", "anyPass")
        .args(["wallet", "login"])
        .output()
        .expect("run onchainos");

    assert_eq!(
        output.status.code(),
        Some(2),
        "AK→AK with different api_key must fire the unified login-diff gate (exit 2)"
    );
    let json = parse_stdout_json(&output);
    assert_eq!(json["confirming"], Value::Bool(true));

    let msg = json["message"].as_str().expect("message field is string");
    assert!(
        msg.contains("not the account you used last time"),
        "message must contain skill discriminator; got: {msg}"
    );
    // Same-mode AK switch uses a fixed PII-clean sentence — no api_key
    // (raw or masked) appears in the user-facing message.
    assert!(
        msg.contains("The API Key in your env has changed"),
        "message must contain the AK-account-switch fixed line; got: {msg}"
    );
    // PII §8.1 regression: raw api_keys must NEVER appear.
    assert!(
        !msg.contains(old_key),
        "message leaked old raw api_key; got: {msg}"
    );
    assert!(
        !msg.contains(new_key),
        "message leaked new raw api_key; got: {msg}"
    );

    // next field is the verbatim skill instruction.
    assert_eq!(
        json["next"],
        Value::String(
            "If the user confirms, re-run the same command with --force flag appended to proceed."
                .into()
        )
    );

    // Audit: exactly one login_mode_prompt_* event (the prompt_shown), with
    // switch_kind=account, current_mode=ak, last_login_mode=ak.
    let lines = audit_lines(&home);
    let prompt_events: Vec<&Value> = lines
        .iter()
        .filter(|e| {
            e.get("command")
                .and_then(|c| c.as_str())
                .map(|c| c.starts_with("login_mode_prompt_"))
                .unwrap_or(false)
        })
        .collect();
    assert_eq!(
        prompt_events.len(),
        1,
        "expected exactly one login_mode_prompt_* event; got: {prompt_events:?}"
    );
    assert_eq!(
        prompt_events[0]["command"],
        Value::String("login_mode_prompt_shown".into())
    );
    let args = prompt_events[0]["args"]
        .as_array()
        .expect("args is array");
    let arg_strs: Vec<&str> = args.iter().filter_map(|v| v.as_str()).collect();
    assert!(arg_strs.contains(&"current_mode=ak"));
    assert!(arg_strs.contains(&"last_login_mode=ak"));
    assert!(arg_strs.contains(&"switch_kind=account"));
    // PII guard on the audit trail too: no raw api_key anywhere in args.
    for s in &arg_strs {
        assert!(
            !s.contains(old_key) && !s.contains(new_key),
            "audit args leaked raw api_key: {s}"
        );
    }
}

// ── IT-021: wallet login email1 → email2 (same mode, different email) ─

#[test]
fn it_021_login_email_to_different_email_emits_confirming() {
    // Same-mode different-account scenario: persisted email is
    // "alice@example.com" (is_ak=false), incoming login is for
    // "bob@example.com". Without this guard, an OTP would be sent to bob's
    // inbox and alice's local binding would be silently overwritten on
    // verify. The guard intercepts before the auth API call: exit 2 with
    // confirming envelope, message uses the same skill discriminator as the
    // mode-switch case, scene line names the masked old and new emails. No
    // network call is reachable on the exit-2 path so this stays offline.
    let (_tmp, home) = fresh_home();
    stage_wallets(&home, "alice@example.com", false);

    let output = scrubbed(&mut onchainos(), &home)
        .args(["wallet", "login", "bob@example.com"])
        .output()
        .expect("run onchainos");

    assert_eq!(output.status.code(), Some(2));
    let json = parse_stdout_json(&output);
    assert_eq!(json["confirming"], Value::Bool(true));

    let msg = json["message"].as_str().expect("message field is string");
    assert!(
        msg.contains("not the account you used last time"),
        "message must contain skill discriminator; got: {msg}"
    );
    // Scene line shape: "Account: <masked-old> → <masked-new>".
    assert!(
        msg.contains("Account: a***e@example.com → b***b@example.com"),
        "message must contain the account-switch scene line with masked emails; got: {msg}"
    );
    // PII §8.1 regression: raw email local parts must NEVER appear.
    assert!(
        !msg.contains("alice@example.com"),
        "message leaked old raw email; got: {msg}"
    );
    assert!(
        !msg.contains("bob@example.com"),
        "message leaked new raw email; got: {msg}"
    );
    // Masked forms (first char + last char + domain) should appear.
    assert!(
        msg.contains("a***e@example.com"),
        "old masked email missing; got: {msg}"
    );
    assert!(
        msg.contains("b***b@example.com"),
        "new masked email missing; got: {msg}"
    );

    // next field is the verbatim skill instruction.
    assert_eq!(
        json["next"],
        Value::String(
            "If the user confirms, re-run the same command with --force flag appended to proceed."
                .into()
        )
    );

    // Audit: exactly one login_mode_prompt_* event (the prompt_shown), with
    // switch_kind=account, current_mode=email, last_login_mode=email; NO
    // user_choice event (the user hasn't decided yet — this is the pre-
    // decision warning). Other audit events from the CLI invocation (e.g.
    // command-completion entries) are not the concern of this test.
    let lines = audit_lines(&home);
    let prompt_events: Vec<&Value> = lines
        .iter()
        .filter(|e| {
            e.get("command")
                .and_then(|c| c.as_str())
                .map(|c| c.starts_with("login_mode_prompt_"))
                .unwrap_or(false)
        })
        .collect();
    assert_eq!(
        prompt_events.len(),
        1,
        "expected exactly one login_mode_prompt_* event; got: {prompt_events:?}"
    );
    assert_eq!(
        prompt_events[0]["command"],
        Value::String("login_mode_prompt_shown".into())
    );
    let args = prompt_events[0]["args"]
        .as_array()
        .expect("args is array");
    let arg_strs: Vec<&str> = args.iter().filter_map(|v| v.as_str()).collect();
    assert!(arg_strs.contains(&"current_mode=email"));
    assert!(arg_strs.contains(&"last_login_mode=email"));
    assert!(arg_strs.contains(&"switch_kind=account"));
    // PII guard on the audit trail too: no raw email anywhere in args.
    for s in &arg_strs {
        assert!(
            !s.contains("alice@example.com") && !s.contains("bob@example.com"),
            "audit args leaked raw email: {s}"
        );
    }
}

// ── IT-022: wallet login email → same email (case-insensitive, trimmed) ─

#[test]
fn it_022_login_same_email_case_insensitive_no_prompt() {
    // Same persisted email, login arg only differs by case + surrounding
    // whitespace → mode-diff guard MUST treat as same account, no exit 2.
    // The auth_init network call follows; we don't care what it returns —
    // only that the guard didn't fire. Use --format-style absent + ignored
    // network outcome (exit code may be 1 from real http failure under
    // sandboxed test env; what matters is exit != 2 and no audit entry).
    let (_tmp, home) = fresh_home();
    stage_wallets(&home, "alice@example.com", false);

    let output = scrubbed(&mut onchainos(), &home)
        .args(["wallet", "login", "  Alice@Example.com  "])
        .output()
        .expect("run onchainos");

    let code = output.status.code();
    assert_ne!(
        code,
        Some(2),
        "same email (case-insensitive + trimmed) must NOT fire mode-diff; got exit {code:?}"
    );

    // Audit must contain ZERO mode-diff entries — same email is a no-op.
    let lines = audit_lines(&home);
    let mode_diff_events: Vec<&Value> = lines
        .iter()
        .filter(|l| {
            let cmd = l["command"].as_str().unwrap_or("");
            cmd == "login_mode_prompt_shown" || cmd == "login_mode_prompt_user_choice"
        })
        .collect();
    assert!(
        mode_diff_events.is_empty(),
        "same-email path must NOT emit mode-diff audit; got: {mode_diff_events:?}"
    );
}
