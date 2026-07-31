//! Integration tests for the WBW-13651 "sink-to-CLI" parameter surface
//! (FR-1 `--since`, FR-3 `--max-results`, FR-6 `rank --all`).
//!
//! These are pure argument-validation checks: every case fails *before* any
//! upstream request is issued (mutual-exclusion / range / positive-duration
//! guards all return early), so the tests need no network and are
//! deterministic. Errors are emitted as a JSON envelope on stdout with exit 1
//! (`main.rs` → `output::error_coded`).

mod common;

use common::onchainos;

/// Assert the command failed and its stdout/stderr error envelope contains
/// every `needle`.
fn assert_error_contains(output: &std::process::Output, needles: &[&str]) {
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        !output.status.success(),
        "expected failure, got success\nstdout: {stdout}\nstderr: {stderr}"
    );
    for needle in needles {
        assert!(
            stdout.contains(needle) || stderr.contains(needle),
            "expected output to contain {needle:?}\nstdout: {stdout}\nstderr: {stderr}"
        );
    }
}

// ── FR-6: `competition rank --all` ──────────────────────────────────────

#[test]
fn competition_rank_all_conflicts_with_sort_type() {
    // `--all` and `--sort-type` are mutually exclusive; the guard fires before
    // identity resolution / any network call.
    let output = onchainos()
        .args([
            "competition",
            "rank",
            "--activity-id",
            "12345",
            "--all",
            "--sort-type",
            "1",
        ])
        .output()
        .expect("failed to execute");
    assert_error_contains(
        &output,
        &["--all is mutually exclusive with --sort-type", "invalid_input"],
    );
}

// ── FR-1: `--since` positive-only + mutual exclusion ────────────────────

#[test]
fn social_news_latest_since_zero_is_rejected() {
    // `--since 0` (and `0m`/`0h`) would produce a zero-width window; the
    // positive-only parser rejects it as invalid_input before any request.
    let output = onchainos()
        .args(["social", "news-latest", "--since", "0"])
        .output()
        .expect("failed to execute");
    assert_error_contains(&output, &["duration must be positive", "invalid_input"]);
}

#[test]
fn social_news_latest_since_conflicts_with_begin() {
    let output = onchainos()
        .args([
            "social",
            "news-latest",
            "--since",
            "24h",
            "--begin",
            "1000",
        ])
        .output()
        .expect("failed to execute");
    assert_error_contains(
        &output,
        &["--since is mutually exclusive with --begin/--end", "invalid_input"],
    );
}

// ── FR-3: `--max-results` range ─────────────────────────────────────────

#[test]
fn token_search_max_results_out_of_range_is_rejected() {
    // `--max-results` must be 1..=500; the range check runs before the request.
    let output = onchainos()
        .args([
            "token",
            "search",
            "--query",
            "btc",
            "--max-results",
            "999",
        ])
        .output()
        .expect("failed to execute");
    assert_error_contains(
        &output,
        &["--max-results must be between 1 and 500", "invalid_input"],
    );
}

#[test]
fn token_search_max_results_non_integer_is_rejected() {
    let output = onchainos()
        .args([
            "token",
            "search",
            "--query",
            "btc",
            "--max-results",
            "abc",
        ])
        .output()
        .expect("failed to execute");
    assert_error_contains(&output, &["--max-results must be an integer", "invalid_input"]);
}
