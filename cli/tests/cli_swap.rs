//! Integration tests for `onchainos swap` commands.
//!
//! Only read-only endpoints are tested (chains, liquidity, quote).
//! `swap` and `approve` are skipped as they generate real transaction data
//! and would require a valid wallet address.

mod common;

use common::{assert_ok_and_extract_data, onchainos, run_with_retry, tokens};
use predicates::prelude::*;

const VITALIK: &str = "0xd8dA6BF26964aF9D7eEd9e03E53415D37aA96045";

// ─── chains ─────────────────────────────────────────────────────────

#[test]
fn swap_chains_returns_supported_chains() {
    let output = run_with_retry(&["swap", "chains"]);
    let data = assert_ok_and_extract_data(&output);
    assert!(data.is_array(), "expected array of chains: {data}");
    let arr = data.as_array().unwrap();
    assert!(!arr.is_empty(), "expected at least one chain");
    assert!(
        arr[0].get("chainIndex").is_some(),
        "chain entry missing 'chainIndex': {}",
        arr[0]
    );
}

// ─── liquidity ──────────────────────────────────────────────────────

#[test]
fn swap_liquidity_ethereum() {
    let output = run_with_retry(&["swap", "liquidity", "--chain", "ethereum"]);
    let data = assert_ok_and_extract_data(&output);
    assert!(data.is_array(), "expected array of DEX sources: {data}");
    let arr = data.as_array().unwrap();
    assert!(!arr.is_empty(), "expected at least one liquidity source");
}

#[test]
fn swap_liquidity_solana() {
    let output = run_with_retry(&["swap", "liquidity", "--chain", "solana"]);
    let data = assert_ok_and_extract_data(&output);
    assert!(data.is_array(), "expected array: {data}");
}

#[test]
fn swap_liquidity_missing_chain_fails() {
    onchainos()
        .args(["swap", "liquidity"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("required"));
}

// ─── quote ──────────────────────────────────────────────────────────

#[test]
fn swap_quote_eth_to_usdc() {
    // Quote swapping 0.01 ETH (10^16 wei) to USDC on Ethereum
    let output = run_with_retry(&[
        "swap",
        "quote",
        "--from",
        tokens::EVM_NATIVE,
        "--to",
        tokens::ETH_USDC,
        "--amount",
        "10000000000000000",
        "--chain",
        "ethereum",
    ]);
    let data = assert_ok_and_extract_data(&output);
    assert!(data.is_array(), "expected quote data array: {data}");
    let arr = data.as_array().unwrap();
    assert!(!arr.is_empty(), "expected at least one quote route");
}

#[test]
fn swap_quote_exact_out() {
    let output = run_with_retry(&[
        "swap",
        "quote",
        "--from",
        tokens::EVM_NATIVE,
        "--to",
        tokens::ETH_USDC,
        "--amount",
        "1000000",
        "--chain",
        "ethereum",
        "--swap-mode",
        "exactOut",
    ]);
    let data = assert_ok_and_extract_data(&output);
    assert!(data.is_array(), "expected quote data array: {data}");
}

#[test]
fn swap_quote_missing_required_args_fails() {
    onchainos()
        .args(["swap", "quote", "--from", tokens::EVM_NATIVE])
        .assert()
        .failure()
        .stderr(predicate::str::contains("required"));
}

// ─── approve (read-only calldata generation) ────────────────────────

#[test]
fn swap_approve_usdc_on_ethereum() {
    let output = run_with_retry(&[
        "swap",
        "approve",
        "--token",
        tokens::ETH_USDC,
        "--amount",
        "1000000",
        "--chain",
        "ethereum",
    ]);
    let data = assert_ok_and_extract_data(&output);
    assert!(data.is_array(), "expected approve data: {data}");
}

// ─── swap (read-only tx generation) ─────────────────────────────────

#[test]
fn swap_swap_eth_to_usdc_generates_tx_data() {
    let output = run_with_retry(&[
        "swap",
        "swap",
        "--from",
        tokens::EVM_NATIVE,
        "--to",
        tokens::ETH_USDC,
        "--amount",
        "10000000000000000",
        "--chain",
        "ethereum",
        "--slippage",
        "1",
        "--wallet",
        VITALIK,
        "--swap-mode",
        "exactIn",
    ]);
    let data = assert_ok_and_extract_data(&output);
    assert!(data.is_array(), "expected swap data: {data}");
}

#[test]
fn swap_swap_missing_required_args_fails() {
    onchainos()
        .args(["swap", "swap", "--from", tokens::EVM_NATIVE])
        .assert()
        .failure()
        .stderr(predicate::str::contains("required"));
}

// ─── X Layer Testnet (chainIndex 1952) integration tests ────────────
//
// These tests cover IT-001..IT-005 from the WWINFRA-3305 integration plan:
// validate that ensure_supported_chain accepts X Layer Testnet via both
// the "xlayer_test" alias and the raw "1952" chainIndex on a cold start
// (no chain_cache.json present), and rejects truly unknown chainIndices.
//
// The cold-start condition is forced by pointing ONCHAINOS_HOME at a fresh
// empty tempdir so the dynamic chain list cache cannot leak between runs;
// this exercises the offline SUPPORTED_CHAIN_INDICES constant fallback in
// ensure_supported_chain (chains.rs:17).
//
// NOTE on stdout vs stderr: onchainos prints structured errors to STDOUT as
// `{"ok": false, "error": "..."}` (output.rs:42) with exit code 1, not to
// stderr. The CSV plan's "stderr contains" hint is therefore satisfied via
// stdout here.

/// IT-001 — `swap liquidity --chain xlayer_test` succeeds via the alias on
/// cold start. The const-array fallback in ensure_supported_chain must
/// recognise 1952 before any dynamic cache is populated.
#[test]
fn swap_liquidity_xlayer_test_alias_cold_start_ensures_supported_chain() {
    let home = tempfile::tempdir().expect("create tempdir");
    let output = onchainos()
        .env("ONCHAINOS_HOME", home.path())
        .args(["swap", "liquidity", "--chain", "xlayer_test"])
        .output()
        .expect("failed to execute");

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);

    // The chain-validation step must not reject xlayer_test. Whether the
    // downstream liquidity-fetch succeeds depends on backend coverage for
    // X Layer Testnet on web3pre; we only assert that ensure_supported_chain
    // did not block us.
    assert!(
        !stdout.contains("unsupported chain") && !stderr.contains("unsupported chain"),
        "ensure_supported_chain rejected xlayer_test alias on cold start\nstdout: {stdout}\nstderr: {stderr}",
    );
}

/// IT-002 — `swap liquidity --chain 1952` succeeds via the raw chainIndex on
/// cold start. Same const-array fallback path as IT-001 but bypasses the
/// alias table.
#[test]
fn swap_liquidity_xlayer_testnet_raw_chain_index_ensures_supported_chain() {
    let home = tempfile::tempdir().expect("create tempdir");
    let output = onchainos()
        .env("ONCHAINOS_HOME", home.path())
        .args(["swap", "liquidity", "--chain", "1952"])
        .output()
        .expect("failed to execute");

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);

    assert!(
        !stdout.contains("unsupported chain") && !stderr.contains("unsupported chain"),
        "ensure_supported_chain rejected raw chainIndex 1952 on cold start\nstdout: {stdout}\nstderr: {stderr}",
    );
}

/// IT-003 — `swap quote --chain xlayer_test` passes chain validation. We use
/// the EVM native placeholder for both `--from` and `--to`; the quote
/// pricing step may legitimately fail (no route, no liquidity, identical
/// in/out token, etc.) but the failure must NOT be an "unsupported chain"
/// rejection. Exit code is intentionally not asserted.
#[test]
fn swap_quote_xlayer_test_alias_chain_validation_passes() {
    let home = tempfile::tempdir().expect("create tempdir");
    let output = onchainos()
        .env("ONCHAINOS_HOME", home.path())
        .args([
            "swap",
            "quote",
            "--chain",
            "xlayer_test",
            "--from",
            tokens::EVM_NATIVE,
            "--to",
            tokens::EVM_NATIVE,
            "--amount",
            "1000000",
        ])
        .output()
        .expect("failed to execute");

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);

    assert!(
        !stdout.contains("unsupported chain") && !stderr.contains("unsupported chain"),
        "ensure_supported_chain rejected xlayer_test alias on quote\nstdout: {stdout}\nstderr: {stderr}",
    );
}

/// IT-004 — `swap liquidity --chain 9999` is rejected at the
/// ensure_supported_chain gate with exit code 1 and an "unsupported chain"
/// error on stdout. This is the negative case proving the const-array
/// fallback rejects unknown indices in cold-start mode.
#[test]
fn swap_liquidity_unknown_chain_9999_rejected_with_unsupported_chain_error() {
    let home = tempfile::tempdir().expect("create tempdir");
    let output = onchainos()
        .env("ONCHAINOS_HOME", home.path())
        .args(["swap", "liquidity", "--chain", "9999"])
        .output()
        .expect("failed to execute");

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);

    assert_eq!(
        output.status.code(),
        Some(1),
        "expected exit code 1 for unsupported chain\nstdout: {stdout}\nstderr: {stderr}",
    );
    // onchainos surfaces the error as JSON on stdout (output::error in
    // output.rs:42); accept either stream to remain robust if that ever
    // changes.
    assert!(
        stdout.contains("unsupported chain") || stderr.contains("unsupported chain"),
        "expected 'unsupported chain' in output for chain 9999\nstdout: {stdout}\nstderr: {stderr}",
    );
}

// IT-005 — covered by existing `swap_chains_returns_supported_chains` above
// (lines 16–28). That test asserts ok=true, that data is a non-empty array,
// and that entries carry a `chainIndex` field — a strict superset of IT-005's
// "stdout contains ok / exit 0" requirement. No new fn added.
