//! Integration tests for `onchainos portfolio` commands:
//! chains, total-value, all-balances, and token-balances.
//!
//! These tests run the compiled binary against the live OKX API,
//! so they require network access and valid API credentials.

mod common;

use common::{assert_ok_and_extract_data, onchainos, run_with_retry};
use predicates::prelude::*;

// Well-known Ethereum wallet (vitalik.eth) used across tests
const TEST_WALLET: &str = "0xd8dA6BF26964aF9D7eEd9e03E53415D37aA96045";
// USDC on Ethereum
const TEST_TOKEN_USDC: &str = "0xa0b86991c6218b36c1d19d4a2e9eb0ce3606eb48";

// ─── chains ─────────────────────────────────────────────────────────

#[test]
fn portfolio_chains_returns_list() {
    let output = run_with_retry(&["portfolio", "chains"]);
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

// ─── total-value ─────────────────────────────────────────────────────

#[test]
fn portfolio_total_value_single_chain() {
    let output = run_with_retry(&[
        "portfolio",
        "total-value",
        "--address",
        TEST_WALLET,
        "--chains",
        "ethereum",
    ]);
    let data = assert_ok_and_extract_data(&output);
    assert!(data.is_array(), "expected array: {data}");
    let arr = data.as_array().unwrap();
    assert!(!arr.is_empty(), "expected at least one entry");
    assert!(
        arr[0].get("totalValue").is_some(),
        "entry missing 'totalValue': {}",
        arr[0]
    );
}

#[test]
fn portfolio_total_value_multi_chain() {
    let output = run_with_retry(&[
        "portfolio",
        "total-value",
        "--address",
        TEST_WALLET,
        "--chains",
        "ethereum,base",
    ]);
    let data = assert_ok_and_extract_data(&output);
    assert!(data.is_array(), "expected array: {data}");
}

#[test]
fn portfolio_total_value_missing_address_fails() {
    onchainos()
        .args(["portfolio", "total-value", "--chains", "ethereum"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("required"));
}

#[test]
fn portfolio_total_value_missing_chains_fails() {
    onchainos()
        .args(["portfolio", "total-value", "--address", TEST_WALLET])
        .assert()
        .failure()
        .stderr(predicate::str::contains("required"));
}

// ─── all-balances ────────────────────────────────────────────────────

#[test]
fn portfolio_all_balances_ethereum() {
    let output = run_with_retry(&[
        "portfolio",
        "all-balances",
        "--address",
        TEST_WALLET,
        "--chains",
        "ethereum",
    ]);
    let data = assert_ok_and_extract_data(&output);
    assert!(data.is_array(), "expected array: {data}");
    let arr = data.as_array().unwrap();
    assert!(!arr.is_empty(), "expected at least one chain result");
    let assets = arr[0].get("tokenAssets").expect("missing 'tokenAssets'");
    assert!(assets.is_array(), "tokenAssets should be array: {assets}");
}

#[test]
fn portfolio_all_balances_with_exclude_risk() {
    let output = run_with_retry(&[
        "portfolio",
        "all-balances",
        "--address",
        TEST_WALLET,
        "--chains",
        "ethereum",
        "--exclude-risk",
        "0",
    ]);
    let data = assert_ok_and_extract_data(&output);
    assert!(data.is_array(), "expected array: {data}");
}

#[test]
fn portfolio_all_balances_missing_address_fails() {
    onchainos()
        .args(["portfolio", "all-balances", "--chains", "ethereum"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("required"));
}

#[test]
fn portfolio_all_balances_missing_chains_fails() {
    onchainos()
        .args(["portfolio", "all-balances", "--address", TEST_WALLET])
        .assert()
        .failure()
        .stderr(predicate::str::contains("required"));
}

// ─── token-balances ──────────────────────────────────────────────────

#[test]
fn portfolio_token_balances_usdc_and_native_eth() {
    let tokens_arg = format!("1:{},1:", TEST_TOKEN_USDC);
    let output = run_with_retry(&[
        "portfolio",
        "token-balances",
        "--address",
        TEST_WALLET,
        "--tokens",
        &tokens_arg,
    ]);
    let data = assert_ok_and_extract_data(&output);
    assert!(data.is_array(), "expected array: {data}");
    let arr = data.as_array().unwrap();
    assert!(!arr.is_empty(), "expected at least one result");
    let assets = arr[0].get("tokenAssets").expect("missing 'tokenAssets'");
    assert!(assets.is_array(), "tokenAssets should be array: {assets}");
}

#[test]
fn portfolio_token_balances_missing_address_fails() {
    let tokens_arg = format!("1:{}", TEST_TOKEN_USDC);
    onchainos()
        .args(["portfolio", "token-balances", "--tokens", &tokens_arg])
        .assert()
        .failure()
        .stderr(predicate::str::contains("required"));
}

#[test]
fn portfolio_token_balances_missing_tokens_fails() {
    onchainos()
        .args(["portfolio", "token-balances", "--address", TEST_WALLET])
        .assert()
        .failure()
        .stderr(predicate::str::contains("required"));
}
