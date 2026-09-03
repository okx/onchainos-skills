//! Integration coverage for the session-start `preflight` command: the
//! throttle early-return, the new `--force` throttle-bypass flag, its `--help`
//! copy, the `ONCHAINOS_NO_SELF_UPDATE` env override, and the offline path.

mod common;

use common::{
    assert_ok_and_extract_data, fresh_home, onchainos, parse_stdout_json, run_with_retry, scrubbed,
};

/// Pin a dead HTTP(S) proxy on `cmd` so the update client's GitHub calls fail
/// fast, forcing `data.status == "offline"` on the full-check (`--force`) path.
///
/// `data.status == "offline"` is only emitted when the release check *fails*
/// (`upgrade.rs` `preflight` → `Err(_)` arm). The GitHub host is hard-coded and
/// there is no mock harness (spec §6), so to keep the offline rows deterministic
/// regardless of whether the runner has network we point every outbound request
/// at an unreachable proxy: reqwest honours `HTTPS_PROXY`/`ALL_PROXY` for the
/// default client built in `preflight`, and curl (used by the beta `git
/// ls-remote` path) honours `ALL_PROXY` too. Loopback port 1 refuses instantly,
/// so the check reports offline without waiting out the 30s network timeout.
/// This is environment configuration, not a code-level mock.
fn force_offline(cmd: &mut assert_cmd::Command) -> &mut assert_cmd::Command {
    const DEAD_PROXY: &str = "http://127.0.0.1:1";
    cmd.env("HTTPS_PROXY", DEAD_PROXY)
        .env("https_proxy", DEAD_PROXY)
        .env("ALL_PROXY", DEAD_PROXY)
        .env("all_proxy", DEAD_PROXY)
        .env_remove("NO_PROXY")
        .env_remove("no_proxy")
}

// IT-001: a recently-checked machine skips the online update check on the next
// session — the throttled `fresh` path returns immediately with no network call.
#[test]
fn fresh_last_check_skips_remote_preflight_work() {
    let (_guard, home) = fresh_home("preflight-throttle");
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("system clock must be after unix epoch")
        .as_secs();
    std::fs::write(home.join("last_check"), now.to_string()).expect("write last_check");

    let output = scrubbed(&mut onchainos(), &home)
        .args(["preflight", "--skill-version", env!("CARGO_PKG_VERSION")])
        .output()
        .expect("run onchainos preflight");

    assert!(
        output.status.success(),
        "preflight failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let json = parse_stdout_json(&output);
    assert_eq!(json["ok"], true);
    assert_eq!(json["data"]["status"], "fresh");
    assert_eq!(json["data"]["throttled"], true);
    assert_eq!(json["data"]["updated"], false);
    assert_eq!(json["data"]["action"], serde_json::Value::Null);
    assert_eq!(json["data"]["binaryIdentity"], serde_json::Value::Null);
    assert!(!home.join("binary_identity.json").exists());

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        !stderr.contains("GitHub"),
        "unexpected network attempt: {stderr}"
    );
    assert!(
        !stderr.contains("Updating skills"),
        "unexpected skill checkout update: {stderr}"
    );
    assert!(
        !stderr.contains("Cleaning up deprecated skills"),
        "unexpected package-manager cleanup: {stderr}"
    );
    assert!(
        !stderr.contains("[preflight timing]"),
        "unexpected preflight timing log: {stderr}"
    );
}

// IT-002: `preflight --force` ignores the recent-check throttle and runs the
// full online check again. `--force` skips the `is_throttled` early-return; the
// proof is the background-cleanup stderr line, which is emitted ONLY on the
// non-throttled path (so it holds regardless of network). With the update
// client forced offline, `data.status` is `offline` while the session still
// exits 0.
#[test]
fn preflight_force_bypasses_throttle_and_runs_full_check() {
    let (_guard, home) = fresh_home("preflight-force");
    // A non-forced call with this fresh stamp would short-circuit to `fresh`;
    // `--force` must still perform the online check.
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("system clock must be after unix epoch")
        .as_secs();
    std::fs::write(home.join("last_check"), now.to_string()).expect("write last_check");

    let output = force_offline(scrubbed(&mut onchainos(), &home))
        .args([
            "preflight",
            "--force",
            "--skill-version",
            env!("CARGO_PKG_VERSION"),
        ])
        .output()
        .expect("run onchainos preflight --force");

    assert!(
        output.status.success(),
        "preflight --force must still exit 0 (session-start advisory): {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let json = parse_stdout_json(&output);
    assert_eq!(json["ok"], true);
    assert_eq!(
        json["data"]["status"], "offline",
        "a forced check with unreachable GitHub must report offline: {json}"
    );
    // `--force` bypasses the throttle early-return, so `throttled` is never set
    // on the full-check payload.
    assert!(
        json["data"].get("throttled").is_none(),
        "`throttled` must be absent on the --force path: {json}"
    );

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("Cleaning up deprecated skills in the background"),
        "the non-throttled full-check path must emit the background-cleanup line: {stderr}"
    );
}

// IT-003: the forced-update option appears in the command help with a clear
// description — validates flag registration and help copy. clap prints help to
// stdout and exits 0.
#[test]
fn preflight_help_lists_force_flag() {
    let output = onchainos()
        .args(["preflight", "--help"])
        .output()
        .expect("run onchainos preflight --help");

    assert!(
        output.status.success(),
        "preflight --help must exit 0: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("Bypass the 12-hour throttle and always perform a fresh online check"),
        "--help must document the --force flag with its help copy: {stdout}"
    );
}

// IT-004: with self-update disabled by environment, the status check reports
// that updates are turned off. `ONCHAINOS_NO_SELF_UPDATE=1` sets
// `data.selfUpdateDisabled=true` even on the throttled `fresh` early-return path
// (env-override case); the env-unset default (=> false) is covered by IT-001.
#[test]
fn preflight_env_disables_self_update_on_fresh_path() {
    let (_guard, home) = fresh_home("preflight-no-self-update");
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("system clock must be after unix epoch")
        .as_secs();
    std::fs::write(home.join("last_check"), now.to_string()).expect("write last_check");

    let output = scrubbed(&mut onchainos(), &home)
        .env("ONCHAINOS_NO_SELF_UPDATE", "1")
        .args(["preflight", "--skill-version", env!("CARGO_PKG_VERSION")])
        .output()
        .expect("run onchainos preflight");

    assert!(
        output.status.success(),
        "preflight failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let json = parse_stdout_json(&output);
    assert_eq!(json["ok"], true);
    assert_eq!(json["data"]["status"], "fresh");
    assert_eq!(
        json["data"]["selfUpdateDisabled"], true,
        "ONCHAINOS_NO_SELF_UPDATE=1 must set selfUpdateDisabled=true: {json}"
    );
}

// IT-005 (live): a forced check reports the latest available version without
// changing the installed binary. `--force` performs a real GitHub latest-release
// check while `--no-self-update` freezes the binary. Routed through the project's
// `run_with_retry` because GitHub's unauthenticated API is rate-limited
// (60 req/h). The hard assertions (selfUpdateDisabled=true, `throttled` absent,
// status never `fresh`) hold on every path; `latestVersion` is asserted only
// when a fresh online check actually resolved a release (skipped when the runner
// is offline / rate-limited), matching spec §2.1's presence rule — this keeps
// the live row deterministic rather than flaky.
#[test]
fn preflight_force_no_self_update_reports_disabled() {
    let output = run_with_retry(&["preflight", "--force", "--no-self-update"]);
    let data = assert_ok_and_extract_data(&output);

    assert_eq!(
        data["selfUpdateDisabled"], true,
        "--no-self-update must freeze the binary and report selfUpdateDisabled=true: {data}"
    );
    assert!(
        data.get("throttled").is_none(),
        "--force bypasses the throttle, so `throttled` must be absent: {data}"
    );
    let status = data["status"].as_str().unwrap_or_default();
    assert_ne!(
        status, "fresh",
        "--force must never short-circuit to the throttled `fresh` status: {data}"
    );
    if status != "offline" {
        // A fresh online check resolved a release, so per spec §2.1 the version
        // and channel fields are present.
        assert!(
            data.get("latestVersion")
                .and_then(|v| v.as_str())
                .is_some(),
            "a resolved online check (status={status}) must carry latestVersion: {data}"
        );
    }
}

// IT-006: when the release server is unreachable, a forced check reports an
// offline status. `--force` with no reachable GitHub yields data.status=offline
// and latestVersion absent, yet the session still exits 0 — preflight is a
// session-start advisory, so failure lives in data.status, never in a non-zero
// exit code.
#[test]
fn preflight_force_offline_reports_offline_status() {
    let (_guard, home) = fresh_home("preflight-offline");

    let output = force_offline(scrubbed(&mut onchainos(), &home))
        .args(["preflight", "--force"])
        .output()
        .expect("run onchainos preflight --force");

    assert!(
        output.status.success(),
        "offline preflight must still exit 0: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let json = parse_stdout_json(&output);
    assert_eq!(json["ok"], true);
    assert_eq!(
        json["data"]["status"], "offline",
        "unreachable GitHub must yield status=offline: {json}"
    );
    assert!(
        json["data"].get("latestVersion").is_none(),
        "offline must not resolve a latestVersion: {json}"
    );

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("Warning: could not reach GitHub"),
        "the offline path must warn about GitHub unreachability: {stderr}"
    );
}
