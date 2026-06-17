//! Integration tests for `onchainos cross-chain execute` (spec §1.3 / Appendix A).
//!
//! Only hermetically-runnable rows live here: clap arg validation and the local
//! input guards that bail before any network call, plus one live token-lookup
//! error wrapped in `run_with_retry`.
//!
//! The success paths and multi-step backend-error envelopes are NOT tested here
//! because they can't run hermetically: the swap broadcast needs real TEE
//! signing (`unsignedInfo` → local sign → `broadcast`), and the error envelopes
//! need a stateful mock-HTTP backend this repo has no harness for. Their
//! isolatable logic is unit-tested in `cross_chain.rs` instead (`build_execute_data`,
//! `is_no_route`, `classify_dead_end`, `tx_confirmation_timeout`). The
//! route-index bounds check is also unreachable anonymously (`/quote` → 402).
//!
//! `EVM_WALLET` is a format-valid stand-in for the CSV `0xWALLET` placeholder —
//! the CLI rejects malformed addresses client-side before the targeted logic.

mod common;

use common::{onchainos, run_with_retry};
use predicates::prelude::*;

// Format-valid stand-in for the CSV `0xWALLET` placeholder (see header note).
const EVM_WALLET: &str = "0xd8dA6BF26964aF9D7eEd9e03E53415D37aA96045"; // vitalik.eth

// ─── Local helpers ──────────────────────────────────────────────────────────

/// Assert the command failed and its stdout/stderr error envelope contains
/// `needle`. Errors are emitted as JSON on stdout with exit 1 (`main.rs`).
fn assert_error_stdout_contains(output: &std::process::Output, needle: &str) {
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        !output.status.success(),
        "expected failure, got success\nstdout: {stdout}\nstderr: {stderr}"
    );
    assert!(
        stdout.contains(needle) || stderr.contains(needle),
        "expected output to contain {needle:?}\nstdout: {stdout}\nstderr: {stderr}"
    );
}

// ════════════════════════════════════════════════════════════════════════════
// Error — local validation (offline → runs for real)
// ════════════════════════════════════════════════════════════════════════════

/// IT-101 — out-of-range slippage rejected locally before any network call.
/// spec §3.2 validate_slippage_zero_to_one, exit 1. "2" parses but is out of
/// range → range branch; "slippage must be" is the stable substring.
#[test]
fn cross_chain_execute_slippage_out_of_range_rejected() {
    let output = onchainos()
        .args([
            "cross-chain", "execute",
            "--from", "usdc", "--to", "usdc",
            "--from-chain", "ethereum", "--to-chain", "arbitrum",
            "--readable-amount", "10", "--slippage", "2", "--wallet", EVM_WALLET,
        ])
        .output()
        .expect("failed to execute onchainos");
    assert_error_stdout_contains(&output, "slippage must be");
}

/// IT-102 — empty `--readable-amount` rejected locally before the token-info
/// network call. spec §3.2 resolve_amount_arg, exit 1.
#[test]
fn cross_chain_execute_empty_readable_amount_rejected() {
    let output = onchainos()
        .args([
            "cross-chain", "execute",
            "--from", "usdc", "--to", "usdc",
            "--from-chain", "ethereum", "--to-chain", "arbitrum",
            "--readable-amount", "", "--wallet", EVM_WALLET,
        ])
        .output()
        .expect("failed to execute onchainos");
    assert_error_stdout_contains(&output, "--readable-amount must not be empty");
}

/// IT-103 — unsupported source chain rejected by ensure_supported_chain (local,
/// before any network call), exit 1. Source emits "unsupported chain: …".
#[test]
fn cross_chain_execute_unsupported_chain_rejected() {
    let output = onchainos()
        .args([
            "cross-chain", "execute",
            "--from", "usdc", "--to", "usdc",
            "--from-chain", "faketestchain", "--to-chain", "arbitrum",
            "--readable-amount", "10", "--wallet", EVM_WALLET,
        ])
        .output()
        .expect("failed to execute onchainos");
    assert_error_stdout_contains(&output, "unsupported chain");
}

// ════════════════════════════════════════════════════════════════════════════
// Error — live backend (live → run_with_retry)
// ════════════════════════════════════════════════════════════════════════════

/// IT-104 — non-existent token reported as not found. spec §3.2
/// resolve_and_validate / token-decimals lookup, exit 1. Hits the live token
/// lookup, so wrap in run_with_retry for rate limits.
#[test]
fn cross_chain_execute_nonexistent_token_not_found() {
    let output = run_with_retry(&[
        "cross-chain", "execute",
        "--from", "0xdeadbeefdeadbeefdeadbeefdeadbeefdeadbeef", "--to", "usdc",
        "--from-chain", "ethereum", "--to-chain", "arbitrum",
        "--readable-amount", "10", "--wallet", EVM_WALLET,
    ]);
    assert!(
        !output.status.success(),
        "expected failure for non-existent token: {:?}",
        String::from_utf8_lossy(&output.stdout)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stdout.contains("not found") || stdout.contains("Failed to fetch token decimals")
            || stderr.contains("not found"),
        "expected a token-not-found error\nstdout: {stdout}\nstderr: {stderr}"
    );
}

// ════════════════════════════════════════════════════════════════════════════
// Edge — clap parse errors (offline → runs for real, exit 2)
// ════════════════════════════════════════════════════════════════════════════

/// IT-201 — `--confirm-approve` and `--skip-approve` are mutually exclusive
/// (clap conflicts_with), exit 2 to stderr. spec §10.1.
#[test]
fn cross_chain_execute_confirm_and_skip_approve_conflict() {
    onchainos()
        .args([
            "cross-chain", "execute",
            "--from", "usdc", "--to", "usdc",
            "--from-chain", "ethereum", "--to-chain", "arbitrum",
            "--readable-amount", "10", "--wallet", EVM_WALLET,
            "--confirm-approve", "--skip-approve",
        ])
        .assert()
        .code(2)
        .stderr(predicate::str::contains("cannot be used with"));
}

/// IT-202 — `--amount` and `--readable-amount` are mutually exclusive
/// (clap conflicts_with), exit 2.
#[test]
fn cross_chain_execute_amount_and_readable_amount_conflict() {
    onchainos()
        .args([
            "cross-chain", "execute",
            "--from", "usdc", "--to", "usdc",
            "--from-chain", "ethereum", "--to-chain", "arbitrum",
            "--amount", "1000000", "--readable-amount", "10", "--wallet", EVM_WALLET,
        ])
        .assert()
        .code(2)
        .stderr(predicate::str::contains("cannot be used with"));
}

/// IT-203 — `--bridge-id` and `--route-index` are mutually exclusive
/// (clap conflicts_with), exit 2. spec §1.2.
#[test]
fn cross_chain_execute_bridge_id_and_route_index_conflict() {
    onchainos()
        .args([
            "cross-chain", "execute",
            "--from", "usdc", "--to", "usdc",
            "--from-chain", "ethereum", "--to-chain", "arbitrum",
            "--readable-amount", "10", "--wallet", EVM_WALLET,
            "--bridge-id", "123", "--route-index", "0",
        ])
        .assert()
        .code(2)
        .stderr(predicate::str::contains("cannot be used with"));
}

/// IT-204 — invalid `--sort` value rejected by PossibleValuesParser([0,1,2]),
/// exit 2 with the valid choices.
#[test]
fn cross_chain_execute_invalid_sort_value_rejected() {
    onchainos()
        .args([
            "cross-chain", "execute",
            "--from", "usdc", "--to", "usdc",
            "--from-chain", "ethereum", "--to-chain", "arbitrum",
            "--readable-amount", "10", "--wallet", EVM_WALLET, "--sort", "9",
        ])
        .assert()
        .code(2)
        .stderr(predicate::str::contains("invalid value"));
}
