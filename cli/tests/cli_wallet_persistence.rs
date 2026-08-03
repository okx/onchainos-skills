//! Integration tests for the unified `atomic_write` file-persistence &
//! permissions refactor — wallet-area rows.
//!
//! Source plan: `oli-docs/zoeiw2gxqiyzhzkaxejlhhkzgyc/integration-plan.csv`
//! rows IT-001, IT-002, IT-101, IT-102, IT-202, IT-205. Spec:
//! `oli-docs/zoeiw2gxqiyzhzkaxejlhhkzgyc/spec.md`.
//!
//! Each test runs the compiled binary against an isolated `ONCHAINOS_HOME`
//! sandbox (`common::fresh_home`) with a scrubbed environment
//! (`common::scrubbed`) so no host credentials or cwd state leak in. Sandboxes
//! live under `cli/target/test_tmp/` (NOT `tempfile::tempdir()`, which the
//! CI/agent sandbox denies writes to).
//!
//! Rows marked `network_required=mock` (IT-002, IT-205) are `#[ignore]`d stubs
//! pending the OKX mock backend harness, mirroring the existing convention in
//! `cli_wallet_login_mode.rs`.

mod common;

use common::{fresh_home, onchainos, parse_stdout_json, scrubbed};
use serde_json::{json, Value};
use std::fs;
use std::path::Path;

const SANDBOX_STEM: &str = "cli_wallet_persistence";

// ── Local staging helpers ──────────────────────────────────────────────────

/// Stage a `wallets.json` (camelCase, mirrors `WalletsJson`) so a decoupled raw
/// JSON map stands in for the binary's internal struct.
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
    let f = fs::File::create(home.join("wallets.json")).expect("create wallets.json");
    serde_json::to_writer_pretty(f, &body).expect("write wallets.json");
}

/// Stage a `session.json` with the given `sessionKeyExpireAt` (unix seconds as a
/// string). A far-future value keeps the session-key guard in
/// `ensure_tokens_refreshed` from short-circuiting so the keyring read is
/// actually reached (needed by IT-101).
fn stage_session(home: &Path, session_key_expire_at: &str) {
    let body = json!({
        "teeId": "",
        "sessionCert": "",
        "encryptedSessionSk": "",
        "sessionKeyExpireAt": session_key_expire_at,
        "apiKey": "",
    });
    let f = fs::File::create(home.join("session.json")).expect("create session.json");
    serde_json::to_writer_pretty(f, &body).expect("write session.json");
}

/// Stage a corrupt `keyring.enc`: garbage long enough to clear the
/// salt(32)+nonce(12)+1 length check but undecryptable, so `file_keyring::
/// read_blob()` fails and `keyring_store::read_blob()` surfaces the corruption
/// error instead of a silent purge (spec §3 / §8.5 #7).
fn stage_corrupt_keyring(home: &Path) {
    fs::write(
        home.join("keyring.enc"),
        b"this is not valid encrypted data at all, needs to be long enough for salt+nonce padding",
    )
    .expect("write corrupt keyring.enc");
}

// ── IT-001: wallet status on a clean install ───────────────────────────────

#[test]
fn wallet_status_it_001_clean_install_reports_ok() {
    // Baseline no-regression: config resolves via ONCHAINOS_HOME only (no
    // current_dir fallback) and `wallet status` still succeeds (spec §1, §4).
    let (_tmp, home) = fresh_home(SANDBOX_STEM);

    let output = scrubbed(&mut onchainos(), &home)
        .args(["wallet", "status"])
        .output()
        .expect("run onchainos wallet status");

    assert_eq!(output.status.code(), Some(0), "clean-install status must exit 0");
    let json = parse_stdout_json(&output);
    assert_eq!(json["ok"], Value::Bool(true));
}

// ── IT-002: login writes credential files privately (mock) ─────────────────

#[test]
#[ignore = "network_required=mock (IT-002): needs the OKX mock login backend. When wired: stage OKX_API_KEY/SECRET/PASSPHRASE, run `wallet login`, assert ok:true, then assert session.json AND wallets.json exist at mode 0600 under ONCHAINOS_HOME (spec §8.5 rows 2-3, AC#4)."]
fn wallet_login_it_002_writes_credentials_0600_against_mock_backend() {
    // Intentionally empty — see #[ignore] reason above.
}

// ── IT-101: corrupt keyring surfaces a re-login error ──────────────────────

#[test]
fn wallet_balance_it_101_corrupt_keyring_says_login_again() {
    // Corrupt keyring.enc must return an explicit error (exit 1) instead of a
    // silent purge-to-empty (spec §3, §8.5 #7, AC#5). Decryption fails before
    // any network call. A valid (non-expired) session.json is staged so the
    // session-key guard in `ensure_tokens_refreshed` does not short-circuit to
    // the anonymous/relogin path — the keyring read must be reached.
    let (_tmp, home) = fresh_home(SANDBOX_STEM);
    stage_wallets(&home, "fixture@example.com", false);
    stage_session(&home, "9999999999");
    stage_corrupt_keyring(&home);

    let output = scrubbed(&mut onchainos(), &home)
        .args(["wallet", "balance", "--chain", "ethereum"])
        .output()
        .expect("run onchainos wallet balance");

    assert_eq!(output.status.code(), Some(1), "corrupt keyring must exit 1");
    let json = parse_stdout_json(&output);
    assert_eq!(json["ok"], Value::Bool(false));
    let err = json["error"].as_str().expect("error field is a string");
    assert!(
        err.contains("Please login again"),
        "corrupt-keyring error must guide re-login (spec §3/§8.5 #7); got: {err}"
    );
}

// ── IT-102: login on a read-only home fails clearly ────────────────────────

#[cfg(unix)]
#[test]
fn wallet_login_it_102_readonly_home_fails() {
    use std::os::unix::fs::PermissionsExt;

    // A machine-identity that cannot be persisted (read-only FS) with no
    // readable identity present must fail clearly (exit 1) instead of a silent
    // volatile fallback (spec §3, §8.5 #8, AC#5). The bail is local so no
    // network is required.
    //
    // We point ONCHAINOS_HOME to a non-existent subdirectory inside a read-only
    // parent. On Linux the directory owner can always chmod their own dir, so
    // setting the existing home to 0o500 would be silently auto-repaired by
    // `ensure_dir_permissions`. Using a child path that does not exist forces
    // `create_dir_all` to fail (creating entries requires write on the parent).
    let (_tmp, parent) = fresh_home(SANDBOX_STEM);
    let home = parent.join("inner");
    // Do NOT create `inner` — it must not exist.
    fs::set_permissions(&parent, fs::Permissions::from_mode(0o500))
        .expect("chmod parent read-only");

    let output = scrubbed(&mut onchainos(), &home)
        .args(["wallet", "login"])
        .output()
        .expect("run onchainos wallet login");

    // Restore writable perms so the sandbox guard can tear the dir down.
    let _ = fs::set_permissions(&parent, fs::Permissions::from_mode(0o700));

    assert_eq!(output.status.code(), Some(1), "read-only home login must exit 1");
    let json = parse_stdout_json(&output);
    assert_eq!(json["ok"], Value::Bool(false));
}

// ── IT-202: stale cwd config migrates to the home folder ───────────────────

#[test]
fn wallet_logout_it_202_migrates_stale_cwd_config_to_home() {
    // First run after the change: a stale `./.onchainos/config.json` in the cwd
    // is auto-copied to onchainos_home() and the user is prompted on stderr to
    // delete the stale file (spec §4, AC#9). `wallet logout` is the incidental
    // trigger — the migration runs during config load, which happens for every
    // command.
    let (_tmp, home) = fresh_home(SANDBOX_STEM);
    let (_cwd_tmp, cwd) = fresh_home("cli_wallet_persistence_cwd");
    // Stale project-local config in the cwd; home has NO config yet.
    let stale_dir = cwd.join(".onchainos");
    fs::create_dir_all(&stale_dir).expect("create cwd .onchainos");
    fs::write(
        stale_dir.join("config.json"),
        br#"{"base_url":"","active_wallet":"acc-1","default_chain":"ethereum"}"#,
    )
    .expect("write stale cwd config.json");

    let output = scrubbed(&mut onchainos(), &home)
        .current_dir(&cwd)
        .args(["wallet", "logout"])
        .output()
        .expect("run onchainos wallet logout");

    assert_eq!(output.status.code(), Some(0), "logout must exit 0");
    let json = parse_stdout_json(&output);
    assert_eq!(json["ok"], Value::Bool(true));

    // Migration side-effect (spec §4, AC#9): the home-dir config now exists with
    // the migrated contents, a stderr prompt was printed, and the stale source
    // is left in place (user data safety).
    let migrated = home.join("config.json");
    assert!(
        migrated.exists(),
        "stale cwd config must be migrated into ONCHAINOS_HOME (spec §4, AC#9)"
    );
    let migrated_body = fs::read_to_string(&migrated).unwrap_or_default();
    assert!(
        migrated_body.contains("ethereum"),
        "migrated config must carry the stale contents; got: {migrated_body}"
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains(".onchainos"),
        "user must be prompted (stderr) to delete the stale cwd config; got: {stderr}"
    );
    assert!(
        stale_dir.join("config.json").exists(),
        "stale cwd config must NOT be auto-deleted (spec §4)"
    );
}

// ── IT-205: balance still works when identity re-persist fails (mock) ──────

#[test]
#[ignore = "network_required=mock (IT-205): needs the OKX mock balance backend. When wired: stage a readable machine-identity on a write-failing home, run `wallet balance --chain solana`, assert the existing identity is reused (command proceeds, ok:true) rather than bailing (spec §8.5 #8, §10 item 8)."]
fn wallet_balance_it_205_reuses_readable_identity_against_mock_backend() {
    // Intentionally empty — see #[ignore] reason above.
}
