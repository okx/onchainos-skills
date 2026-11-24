//! Integration tests for `onchainos gateway` commands.
//!
//! Only safe, read-only endpoints are tested (chains, gas, gas-limit).
//! `broadcast` is skipped (requires a real signed transaction).

mod common;

use common::{assert_ok_and_extract_data, onchainos, run_with_retry};
use predicates::prelude::*;

// Well-known EOA addresses for gas estimation tests
const VITALIK: &str = "0xd8dA6BF26964aF9D7eEd9e03E53415D37aA96045";
const BURN_ADDRESS: &str = "0x000000000000000000000000000000000000dEaD";
const EMPTY_CALLDATA: &str = "0x";

// ─── chains ─────────────────────────────────────────────────────────

#[test]
fn gateway_chains_returns_supported_chains() {
    let output = run_with_retry(&["gateway", "chains"]);
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

// ─── gas ────────────────────────────────────────────────────────────

#[test]
fn gateway_gas_ethereum() {
    let output = run_with_retry(&["gateway", "gas", "--chain", "ethereum"]);
    let data = assert_ok_and_extract_data(&output);
    assert!(data.is_array(), "expected array: {data}");
    let arr = data.as_array().unwrap();
    assert!(!arr.is_empty(), "expected gas price data");
}

#[test]
fn gateway_gas_solana() {
    let output = run_with_retry(&["gateway", "gas", "--chain", "solana"]);
    let data = assert_ok_and_extract_data(&output);
    assert!(data.is_array(), "expected array: {data}");
}

#[test]
fn gateway_gas_missing_chain_fails() {
    onchainos()
        .args(["gateway", "gas"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("required"));
}

// ─── gas-limit ──────────────────────────────────────────────────────

#[test]
fn gateway_gas_limit_simple_eth_transfer() {
    let output = run_with_retry(&[
        "gateway",
        "gas-limit",
        "--from",
        VITALIK,
        "--to",
        BURN_ADDRESS,
        "--amount",
        "1000000000000",
        "--chain",
        "ethereum",
    ]);
    let data = assert_ok_and_extract_data(&output);
    assert!(data.is_array(), "expected array: {data}");
}

#[test]
fn gateway_gas_limit_with_data() {
    let output = run_with_retry(&[
        "gateway",
        "gas-limit",
        "--from",
        VITALIK,
        "--to",
        BURN_ADDRESS,
        "--amount",
        "0",
        "--data",
        EMPTY_CALLDATA,
        "--chain",
        "ethereum",
    ]);
    let data = assert_ok_and_extract_data(&output);
    assert!(data.is_array(), "expected array: {data}");
}

#[test]
fn gateway_gas_limit_missing_required_args_fails() {
    onchainos()
        .args(["gateway", "gas-limit", "--from", VITALIK])
        .assert()
        .failure()
        .stderr(predicate::str::contains("required"));
}

// ─── broadcast (argument validation only) ───────────────────────────

#[test]
fn gateway_broadcast_missing_args_fails() {
    onchainos()
        .args(["gateway", "broadcast"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("required"));
}

// ─── simulate ───────────────────────────────────────────────────────

#[test]
fn gateway_simulate_eth_transfer() {
    let output = run_with_retry(&[
        "gateway",
        "simulate",
        "--from",
        VITALIK,
        "--to",
        BURN_ADDRESS,
        "--amount",
        "0",
        "--data",
        EMPTY_CALLDATA,
        "--chain",
        "ethereum",
    ]);
    let data = assert_ok_and_extract_data(&output);
    assert!(data.is_array() || data.is_object(), "expected data: {data}");
}

#[test]
fn gateway_simulate_missing_required_args_fails() {
    onchainos()
        .args(["gateway", "simulate", "--from", VITALIK])
        .assert()
        .failure()
        .stderr(predicate::str::contains("required"));
}

// ─── orders ─────────────────────────────────────────────────────────

#[test]
fn gateway_orders_with_address() {
    let output = run_with_retry(&[
        "gateway",
        "orders",
        "--address",
        VITALIK,
        "--chain",
        "ethereum",
    ]);
    let data = assert_ok_and_extract_data(&output);
    assert!(data.is_array() || data.is_object(), "expected data: {data}");
}

#[test]
fn gateway_orders_with_order_id() {
    let output = run_with_retry(&[
        "gateway",
        "orders",
        "--address",
        VITALIK,
        "--chain",
        "ethereum",
        "--order-id",
        "dummy-order-id",
    ]);
    let data = assert_ok_and_extract_data(&output);
    assert!(data.is_array() || data.is_object(), "expected data: {data}");
}

#[test]
fn gateway_orders_missing_args_fails() {
    onchainos()
        .args(["gateway", "orders"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("required"));
}
