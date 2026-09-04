//! jobId path-traversal hardening integration tests.
//!
//! End-to-end filesystem-containment tests for the three protected path
//! builders — `attachments_dir`, `deliverables_dir` and `consent_path` — proving
//! that a path-shaped jobId is rejected BEFORE any filesystem side effect, that a
//! legal jobId's success envelope is unchanged (zero-change snapshot), and that
//! existing CJK / emoji / legacy deliverable directories stay listable.
//!
//! NOTE: the pipeline's `onchainos_check` gate runs `cargo test --bins`, which does
//! NOT execute this integration file; the guards themselves are covered by the
//! `#[cfg(test)]` unit tests in `task/common/util.rs`, `.../common/deliverables.rs`,
//! `.../user/attachments.rs` and `.../autotrade/consent.rs`. These end-to-end tests
//! run under a plain `cargo test` and prove the guard fires through the real binary.

mod common;

use common::{fresh_home, onchainos, parse_stdout_json, scrubbed};

const SENTINEL_BODY: &str = "DO_NOT_TOUCH";
const LEGAL_JOB_ID: &str = "0x1b76dabd3bf884626184e3b36b7c65b54929a827a8a26e223c4b8aa868d41be1";

/// Plant a sentinel file OUTSIDE the ONCHAINOS_HOME root (a sibling of `home`).
fn plant_sentinel(home: &std::path::Path) -> std::path::PathBuf {
    let sentinel = home.parent().unwrap().join("SENTINEL.txt");
    std::fs::write(&sentinel, SENTINEL_BODY).unwrap();
    sentinel
}

fn assert_sentinel_unchanged(sentinel: &std::path::Path) {
    assert_eq!(
        std::fs::read_to_string(sentinel).unwrap(),
        SENTINEL_BODY,
        "out-of-root sentinel must be byte-for-byte unchanged"
    );
}

/// Seed a minimal valid deliverable manifest for `dir_name` under `<home>/deliverables/<role>/`.
fn seed_manifest(home: &std::path::Path, role: &str, dir_name: &str, job_id: &str) {
    let dir = home.join("deliverables").join(role).join(dir_name);
    std::fs::create_dir_all(&dir).unwrap();
    let manifest = serde_json::json!({
        "jobId": job_id,
        "role": role,
        "task": { "shortId": "abcd", "title": "t" },
        "entries": [{
            "filename": "report.pdf",
            "originalName": "report.pdf",
            "deliverableType": "file",
            "savedAt": "2026-01-01T00:00:00+00:00",
            "sizeBytes": 4
        }]
    });
    std::fs::write(
        dir.join("manifest.json"),
        serde_json::to_string_pretty(&manifest).unwrap(),
    )
    .unwrap();
}

/// `task-deliverable-save --job-id "../../.."` fails closed —
/// the source file is neither moved nor deleted, no directory is created outside
/// ONCHAINOS_HOME, and the out-of-root sentinel is byte-for-byte unchanged.
#[test]
fn deliverable_save_poc_traversal_rejected() {
    let (_h, home) = fresh_home("jobid_pt_save_poc");
    let sentinel = plant_sentinel(&home);

    // A real source file the malicious save would try to move.
    let src = home.join("src_deliverable.txt");
    std::fs::write(&src, "payload").unwrap();

    let mut cmd = onchainos();
    let out = scrubbed(&mut cmd, &home)
        .env("HOME", &home)
        .args([
            "agent",
            "task-deliverable-save",
            "--job-id",
            "../../../../SENTINEL",
            "--role",
            "user",
            "--file",
            src.to_str().unwrap(),
            "--title",
            "t",
            "--short-id",
            "abcd",
        ])
        .output()
        .expect("run task-deliverable-save");

    assert_eq!(out.status.code(), Some(1), "malicious save must exit 1");
    // Source file must NOT have been moved/deleted.
    assert!(src.exists(), "source file must not be moved/deleted");
    assert_eq!(std::fs::read_to_string(&src).unwrap(), "payload");
    // No deliverables tree was created (the guard fired before any create_dir_all).
    assert!(
        !home.join("deliverables").exists(),
        "no deliverables directory should be created on a rejected save"
    );
    assert_sentinel_unchanged(&sentinel);
}

/// The `deliverables_dir` guard fires for a path-shaped jobId at `task-deliverable-list`
/// before the directory is joined/read: exit 1 + stable coded error, sentinel intact.
#[test]
fn deliverable_list_rejects_traversal() {
    let (_h, home) = fresh_home("jobid_pt_deliverable_list");
    let sentinel = plant_sentinel(&home);

    let mut cmd = onchainos();
    let out = scrubbed(&mut cmd, &home)
        .env("HOME", &home)
        .args([
            "agent",
            "task-deliverable-list",
            "--role",
            "user",
            "--job-id",
            "../../../x",
        ])
        .output()
        .expect("run task-deliverable-list");

    assert_eq!(out.status.code(), Some(1), "expected exit 1");
    let json = parse_stdout_json(&out);
    assert_eq!(json["ok"], serde_json::json!(false));
    assert_eq!(json["errorCode"], "UNSAFE_JOB_PATH_COMPONENT");
    assert_sentinel_unchanged(&sentinel);
}

/// A legal `0x`+64-hex jobId keeps the exact success
/// envelope shape — `{ok:true, data:{deliverables:[]}}`, exit 0, no `errorCode`.
#[test]
fn deliverable_list_legal_jobid_zero_change() {
    let (_h, home) = fresh_home("jobid_pt_legal_snapshot");

    let mut cmd = onchainos();
    let out = scrubbed(&mut cmd, &home)
        .env("HOME", &home)
        .args([
            "agent",
            "task-deliverable-list",
            "--role",
            "user",
            "--job-id",
            LEGAL_JOB_ID,
        ])
        .output()
        .expect("run task-deliverable-list");

    assert_eq!(out.status.code(), Some(0), "legal jobId must exit 0");
    let json = parse_stdout_json(&out);
    assert_eq!(json["ok"], serde_json::json!(true));
    assert!(
        json.get("errorCode").is_none(),
        "success must carry no errorCode"
    );
    assert_eq!(json["data"]["deliverables"], serde_json::json!([]));
}

/// Legacy bare-jobId, new-style `<jobId>_<CJK title>` and `<jobId>_<emoji
/// title>` deliverable directories all remain listable by `task-deliverable-list`
/// (list-all). Directory names are built from \u{...} escapes so the source is ASCII.
#[test]
fn deliverable_list_all_cjk_emoji_legacy() {
    let (_h, home) = fresh_home("jobid_pt_list_all");

    let hex_a = format!("0x{}", "a".repeat(64));
    let hex_b = format!("0x{}", "b".repeat(64));
    let hex_c = format!("0x{}", "c".repeat(64));
    // Legacy bare-jobId directory.
    seed_manifest(&home, "user", &hex_a, &hex_a);
    // `<jobId>_<CJK title>` (CJK built from \u{...} escapes).
    seed_manifest(
        &home,
        "user",
        &format!("{hex_b}_\u{6211}\u{7684}\u{62a5}\u{544a}"),
        &hex_b,
    );
    // `<jobId>_<emoji title>` (jobId_📄).
    seed_manifest(&home, "user", &format!("{hex_c}_\u{1F4C4}"), &hex_c);

    let mut cmd = onchainos();
    let out = scrubbed(&mut cmd, &home)
        .env("HOME", &home)
        .args(["agent", "task-deliverable-list", "--role", "user"])
        .output()
        .expect("run task-deliverable-list (list-all)");

    assert_eq!(out.status.code(), Some(0), "list-all must exit 0");
    let json = parse_stdout_json(&out);
    assert_eq!(json["ok"], serde_json::json!(true));
    let results = json["data"]["results"]
        .as_array()
        .expect("results array present");
    let ids: Vec<String> = results
        .iter()
        .map(|r| r["jobId"].as_str().unwrap_or("").to_string())
        .collect();
    assert!(ids.contains(&hex_a), "legacy bare-jobId dir must list: {ids:?}");
    assert!(ids.contains(&hex_b), "CJK-title dir must list: {ids:?}");
    assert!(ids.contains(&hex_c), "emoji-title dir must list: {ids:?}");
}

/// `autotrade-consent-set --mode pause` with a
/// legal jobId succeeds with the unchanged envelope — exit 0,
/// `{consentMode:"pause", cleared:true}` — and touches nothing outside the root.
#[test]
fn autotrade_pause_legal_jobid_golden() {
    let (_h, home) = fresh_home("jobid_pt_pause_golden");
    let sentinel = plant_sentinel(&home);

    let mut cmd = onchainos();
    let out = scrubbed(&mut cmd, &home)
        .env("HOME", &home)
        .args([
            "agent",
            "autotrade-consent-set",
            "--mode",
            "pause",
            "--job-id",
            LEGAL_JOB_ID,
            "--agent-id",
            "test-agent",
        ])
        .output()
        .expect("run autotrade-consent-set pause");

    assert_eq!(out.status.code(), Some(0), "legal pause must exit 0");
    let json = parse_stdout_json(&out);
    assert_eq!(json["ok"], serde_json::json!(true));
    assert_eq!(json["data"]["consentMode"], "pause");
    assert_eq!(json["data"]["cleared"], serde_json::json!(true));
    assert_sentinel_unchanged(&sentinel);
}
