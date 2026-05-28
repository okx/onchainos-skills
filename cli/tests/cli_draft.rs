//! Integration tests for `onchainos agent draft` commands.
//!
//! Tests are split into two groups:
//! - **No-auth** (always run): CLI parsing and input validation that fails
//!   before `ensure_tokens_refreshed`. No wallet session needed.
//! - **Auth-required** (`#[ignore]`): CRUD lifecycle and edge cases that
//!   require a logged-in wallet + registered buyer agent. Run with:
//!   `cargo test --test cli_draft -- --ignored`

mod common;

use common::onchainos;
use predicates::prelude::*;

// ═══════════════════════════════════════════════════════════════════
// No-auth: CLI parsing errors (clap rejects before handler runs)
// ═══════════════════════════════════════════════════════════════════

#[test]
fn draft_create_missing_title_flag_fails() {
    onchainos()
        .args(["agent", "draft", "create"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("--title"));
}

#[test]
fn draft_update_missing_job_id_fails() {
    onchainos()
        .args(["agent", "draft", "update", "--title", "x"])
        .assert()
        .failure();
}

#[test]
fn draft_delete_missing_job_id_fails() {
    onchainos()
        .args(["agent", "draft", "delete"])
        .assert()
        .failure();
}

#[test]
fn draft_publish_missing_job_id_fails() {
    onchainos()
        .args(["agent", "draft", "publish"])
        .assert()
        .failure();
}

// ═══════════════════════════════════════════════════════════════════
// No-auth: input validation (handler validates before auth call)
// ═══════════════════════════════════════════════════════════════════

#[test]
fn draft_create_empty_title_rejected() {
    onchainos()
        .args(["agent", "draft", "create", "--title", ""])
        .assert()
        .failure()
        .stdout(predicate::str::contains("title must not be empty"));
}

#[test]
fn draft_create_title_too_long_rejected() {
    let long_title: String = "x".repeat(31);
    onchainos()
        .args(["agent", "draft", "create", "--title", &long_title])
        .assert()
        .failure()
        .stdout(predicate::str::contains("may not exceed"));
}

#[test]
fn draft_create_description_too_short_rejected() {
    onchainos()
        .args([
            "agent", "draft", "create",
            "--title", "ok",
            "--description", "short",
        ])
        .assert()
        .failure()
        .stdout(predicate::str::contains("too short"));
}

#[test]
fn draft_create_invalid_currency_rejected() {
    onchainos()
        .args([
            "agent", "draft", "create",
            "--title", "ok",
            "--currency", "ETH",
        ])
        .assert()
        .failure()
        .stdout(predicate::str::contains("USDT").or(predicate::str::contains("USDG")));
}

#[test]
fn draft_create_budget_too_many_decimals_rejected() {
    onchainos()
        .args([
            "agent", "draft", "create",
            "--title", "ok",
            "--budget", "1.123456",
        ])
        .assert()
        .failure()
        .stdout(predicate::str::contains("decimal"));
}

#[test]
fn draft_create_budget_negative_rejected() {
    onchainos()
        .args([
            "agent", "draft", "create",
            "--title", "ok",
            "--budget", "-1",
        ])
        .assert()
        .failure();
}

#[test]
fn draft_create_deadline_open_too_short_rejected() {
    onchainos()
        .args([
            "agent", "draft", "create",
            "--title", "ok",
            "--deadline-open", "1s",
        ])
        .assert()
        .failure()
        .stdout(predicate::str::contains("deadline-open"));
}

#[test]
fn draft_create_deadline_submit_too_short_rejected() {
    onchainos()
        .args([
            "agent", "draft", "create",
            "--title", "ok",
            "--deadline-submit", "5s",
        ])
        .assert()
        .failure()
        .stdout(predicate::str::contains("deadline-submit"));
}

#[test]
fn draft_create_max_budget_less_than_budget_rejected() {
    onchainos()
        .args([
            "agent", "draft", "create",
            "--title", "ok",
            "--budget", "100",
            "--max-budget", "50",
        ])
        .assert()
        .failure()
        .stdout(predicate::str::contains("may not be less than"));
}

#[test]
fn draft_create_budget_exceeds_max_rejected() {
    onchainos()
        .args([
            "agent", "draft", "create",
            "--title", "ok",
            "--budget", "10000001",
        ])
        .assert()
        .failure();
}

// ═══════════════════════════════════════════════════════════════════
// Auth-required: CRUD lifecycle
// ═══════════════════════════════════════════════════════════════════

/// Extract jobId from stdout text like "✓ Draft saved (jobId: abc-123)"
fn extract_job_id(stdout: &str) -> Option<String> {
    let marker = "jobId: ";
    let start = stdout.find(marker)? + marker.len();
    let rest = &stdout[start..];
    let end = rest.find([')', '\n', ' '])?;
    Some(rest[..end].to_string())
}

#[test]
#[ignore]
fn draft_lifecycle_create_list_update_delete() {
    // ── 1. create ──────────────────────────────────────────────
    let output = onchainos()
        .args([
            "agent", "draft", "create",
            "--title", "IntegTest draft",
            "--description", "Integration test draft — will be deleted automatically after test run.",
            "--budget", "1",
            "--max-budget", "2",
            "--currency", "USDT",
            "--deadline-open", "1h",
            "--deadline-submit", "2h",
        ])
        .output()
        .expect("failed to execute draft create");

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        output.status.success(),
        "draft create failed (exit={:?})\nstdout: {stdout}\nstderr: {stderr}",
        output.status.code(),
    );
    assert!(
        stdout.contains("Draft saved"),
        "expected success message, got: {stdout}",
    );

    let job_id = extract_job_id(&stdout)
        .unwrap_or_else(|| panic!("could not extract jobId from: {stdout}"));
    eprintln!("[test] created draft jobId={job_id}");

    // ── 2. list ────────────────────────────────────────────────
    let output = onchainos()
        .args(["agent", "draft", "list"])
        .output()
        .expect("failed to execute draft list");

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        output.status.success(),
        "draft list failed: {stdout}\nstderr: {}",
        String::from_utf8_lossy(&output.stderr),
    );
    assert!(
        stdout.contains(&job_id) || stdout.contains("IntegTest"),
        "created draft not found in list output: {stdout}",
    );

    // ── 3. update ──────────────────────────────────────────────
    let output = onchainos()
        .args([
            "agent", "draft", "update", &job_id,
            "--title", "IntegTest updated",
            "--budget", "1.5",
        ])
        .output()
        .expect("failed to execute draft update");

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        output.status.success(),
        "draft update failed: {stdout}\nstderr: {}",
        String::from_utf8_lossy(&output.stderr),
    );
    assert!(
        stdout.contains("Draft updated"),
        "expected update success message, got: {stdout}",
    );

    // ── 4. delete ──────────────────────────────────────────────
    let output = onchainos()
        .args(["agent", "draft", "delete", &job_id])
        .output()
        .expect("failed to execute draft delete");

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        output.status.success(),
        "draft delete failed: {stdout}\nstderr: {}",
        String::from_utf8_lossy(&output.stderr),
    );
    assert!(
        stdout.contains("Draft deleted"),
        "expected delete success message, got: {stdout}",
    );

    // ── 5. verify deletion — re-list must NOT contain the jobId ──
    let output = onchainos()
        .args(["agent", "draft", "list"])
        .output()
        .expect("failed to execute draft list after delete");

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        output.status.success(),
        "draft list after delete failed: {stdout}",
    );
    assert!(
        !stdout.contains(&job_id),
        "deleted draft still appears in list: {stdout}",
    );
}

#[test]
#[ignore]
fn draft_publish_incomplete_draft_fails() {
    // create a minimal draft (title only, missing required publish fields)
    let output = onchainos()
        .args(["agent", "draft", "create", "--title", "IntegTest publish-fail"])
        .output()
        .expect("failed to execute draft create");

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(output.status.success(), "draft create failed: {stdout}");

    let job_id = extract_job_id(&stdout)
        .unwrap_or_else(|| panic!("could not extract jobId from: {stdout}"));

    // publish should fail — missing description, budget, currency, deadlines
    let output = onchainos()
        .args(["agent", "draft", "publish", &job_id])
        .output()
        .expect("failed to execute draft publish");

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        !output.status.success(),
        "publish should have failed for incomplete draft\nstdout: {stdout}\nstderr: {stderr}",
    );
    let combined = format!("{stdout}{stderr}");
    assert!(
        combined.contains("missing required fields") || combined.contains("description"),
        "expected missing-fields error, got:\nstdout: {stdout}\nstderr: {stderr}",
    );

    // cleanup
    let _ = onchainos()
        .args(["agent", "draft", "delete", &job_id])
        .output();
}

#[test]
#[ignore]
fn draft_update_no_fields_rejected() {
    // create a draft to get a valid job_id
    let output = onchainos()
        .args(["agent", "draft", "create", "--title", "IntegTest no-fields"])
        .output()
        .expect("failed to execute draft create");

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(output.status.success(), "draft create failed: {stdout}");

    let job_id = extract_job_id(&stdout)
        .unwrap_or_else(|| panic!("could not extract jobId from: {stdout}"));

    // update with no optional flags → "no fields specified"
    let output = onchainos()
        .args(["agent", "draft", "update", &job_id])
        .output()
        .expect("failed to execute draft update");

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        !output.status.success(),
        "update with no fields should have failed\nstdout: {stdout}\nstderr: {stderr}",
    );
    let combined = format!("{stdout}{stderr}");
    assert!(
        combined.contains("no fields specified"),
        "expected 'no fields specified' error, got:\nstdout: {stdout}\nstderr: {stderr}",
    );

    // cleanup
    let _ = onchainos()
        .args(["agent", "draft", "delete", &job_id])
        .output();
}
