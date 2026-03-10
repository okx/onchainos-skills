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
