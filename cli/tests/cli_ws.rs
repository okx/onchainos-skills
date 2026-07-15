//! Integration tests for the unified `atomic_write` file-persistence &
//! permissions refactor — `ws`-area rows.
//!
//! Source plan: `oli-docs/zoeiw2gxqiyzhzkaxejlhhkzgyc/integration-plan.csv`
//! rows IT-004, IT-103, IT-201, IT-203, IT-204. Spec:
//! `oli-docs/zoeiw2gxqiyzhzkaxejlhhkzgyc/spec.md`.
//!
//! `ws channels` / `ws channel-info` are static, offline commands used as the
//! incidental trigger — the assertions target the persistence side-effects
//! (audit-log privacy, startup permission self-heal, cwd isolation) that fire
//! on every startup. Each test uses an isolated `ONCHAINOS_HOME` sandbox with a
//! scrubbed environment (`common::fresh_home` / `common::scrubbed`).

mod common;

use common::{fresh_home, onchainos, parse_stdout_json, scrubbed};
use serde_json::Value;
use std::fs;

const SANDBOX_STEM: &str = "cli_ws";

// ── IT-004: ws channels + private audit log ────────────────────────────────

#[test]
fn ws_channels_it_004_lists_channels_and_writes_private_audit() {
    // Listing monitor channels works; its audit log lands under ONCHAINOS_HOME
    // and is created append-mode 0600 on every command (spec §5, §8.5 row 4, AC#4).
    let (_tmp, home) = fresh_home(SANDBOX_STEM);

    let output = scrubbed(&mut onchainos(), &home)
        .args(["ws", "channels"])
        .output()
        .expect("run onchainos ws channels");

    assert_eq!(output.status.code(), Some(0), "ws channels must exit 0");
    let json = parse_stdout_json(&output);
    assert_eq!(json["ok"], Value::Bool(true));
    assert!(
        json["data"].is_array(),
        "channels data should be an array: {}",
        json["data"]
    );

    let audit = home.join("audit.jsonl");
    assert!(
        audit.exists(),
        "audit.jsonl must be written under ONCHAINOS_HOME"
    );
    #[cfg(unix)]
    assert_eq!(
        common::file_mode(&audit),
        0o600,
        "audit.jsonl must be private (0600) — spec §5, §8.5 row 4, AC#4"
    );
}

// ── IT-103: corrupt watch config stops with a clear error ──────────────────

#[test]
fn ws_run_daemon_it_103_corrupt_config_errors() {
    // A watch daemon whose `watch/{id}/config.json` is corrupt must write
    // status=config_corrupt and return an error (exit 1) instead of silently
    // using a default config (spec §3, §8.5 #11, AC#5). `ws run-daemon` is the
    // hidden daemon entry.
    let (_tmp, home) = fresh_home(SANDBOX_STEM);
    let watch_dir = home.join("watch").join("it-watch-corrupt");
    fs::create_dir_all(&watch_dir).expect("create watch session dir");
    fs::write(watch_dir.join("config.json"), b"{ this is not valid json ]")
        .expect("write corrupt watch config.json");

    let output = scrubbed(&mut onchainos(), &home)
        .args(["ws", "run-daemon", "--id", "it-watch-corrupt"])
        .output()
        .expect("run onchainos ws run-daemon");

    assert_eq!(output.status.code(), Some(1), "corrupt watch config must exit 1");
    let json = parse_stdout_json(&output);
    assert_eq!(json["ok"], Value::Bool(false));

    // The config_corrupt status must be persisted so `ws poll` / `ws list`
    // surface the fault.
    let status = fs::read_to_string(watch_dir.join("status")).unwrap_or_default();
    assert!(
        status.starts_with("config_corrupt"),
        "status must be config_corrupt, got: {status:?}"
    );
}

// ── IT-201: startup self-heal of a world-readable sensitive file ───────────

#[cfg(unix)]
#[test]
fn ws_channel_info_it_201_self_heals_world_readable_session() {
    use std::os::unix::fs::PermissionsExt;

    // A mis-permissioned (0644) sensitive file must be chmod'd to 0600 by
    // self_heal_permissions() at startup, idempotently and non-fatally
    // (spec §8.3, AC#4, H2). The trigger command is incidental — self-heal
    // fires on every startup.
    let (_tmp, home) = fresh_home(SANDBOX_STEM);
    let session = home.join("session.json");
    fs::write(&session, b"{}").expect("write session.json");
    fs::set_permissions(&session, fs::Permissions::from_mode(0o644)).expect("chmod 0644");

    let output = scrubbed(&mut onchainos(), &home)
        .args(["ws", "channel-info", "--channel", "price"])
        .output()
        .expect("run onchainos ws channel-info");

    assert_eq!(output.status.code(), Some(0), "ws channel-info must exit 0");
    let json = parse_stdout_json(&output);
    assert_eq!(json["ok"], Value::Bool(true));

    assert_eq!(
        common::file_mode(&session),
        0o600,
        "session.json must be self-healed 0644 → 0600 at startup (spec §8.3, AC#4)"
    );
}

// ── IT-203: config.json is excluded from self-heal (stays 0644) ────────────

#[cfg(unix)]
#[test]
fn ws_channel_info_it_203_leaves_config_0644() {
    use std::os::unix::fs::PermissionsExt;

    // config.json is non-sensitive after the dead-field deletion and is
    // deliberately EXCLUDED from self_heal — a 0644 config.json must be left
    // untouched (spec §8.3 D2, §8.4).
    let (_tmp, home) = fresh_home(SANDBOX_STEM);
    let config = home.join("config.json");
    fs::write(&config, br#"{"base_url":""}"#).expect("write config.json");
    fs::set_permissions(&config, fs::Permissions::from_mode(0o644)).expect("chmod 0644");

    let output = scrubbed(&mut onchainos(), &home)
        .args(["ws", "channel-info", "--channel", "trades"])
        .output()
        .expect("run onchainos ws channel-info");

    assert_eq!(output.status.code(), Some(0), "ws channel-info must exit 0");
    let json = parse_stdout_json(&output);
    assert_eq!(json["ok"], Value::Bool(true));

    assert_eq!(
        common::file_mode(&config),
        0o644,
        "config.json must remain 0644 (excluded from self-heal — spec §8.3 D2, §8.4)"
    );
}

// ── IT-204: ONCHAINOS_HOME isolation — no writes leak to the cwd ───────────

#[test]
fn ws_channel_info_it_204_no_cwd_leak() {
    // With ONCHAINOS_HOME set, all runtime files land under it and none leak to
    // a cwd `./.onchainos` (spec §4, AC#6). Run from a separate cwd sandbox and
    // assert nothing was created there.
    let (_tmp, home) = fresh_home(SANDBOX_STEM);
    let (_cwd_tmp, cwd) = fresh_home("cli_ws_cwd");

    let output = scrubbed(&mut onchainos(), &home)
        .current_dir(&cwd)
        .args(["ws", "channel-info", "--channel", "kol_smartmoney-tracker-activity"])
        .output()
        .expect("run onchainos ws channel-info");

    assert_eq!(output.status.code(), Some(0), "ws channel-info must exit 0");
    let json = parse_stdout_json(&output);
    assert_eq!(json["ok"], Value::Bool(true));

    assert!(
        home.join("audit.jsonl").exists(),
        "audit.jsonl must land under ONCHAINOS_HOME"
    );
    assert!(
        !cwd.join(".onchainos").exists(),
        "no ./.onchainos may be created in the cwd (spec §4, AC#6)"
    );
}
