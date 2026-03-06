//! Integration tests for all `onchainos market` commands:
//! price, prices, kline, trades, index, signals, and memepump.
//!
//! These tests run the compiled binary against the live OKX API,
//! so they require network access and valid API credentials.

mod common;

use assert_cmd::cargo::cargo_bin_cmd;
use common::{assert_ok_and_extract_data, onchainos, run_with_retry, tokens};
use predicates::prelude::*;
use serde_json::Value;

// ─── price ──────────────────────────────────────────────────────────

#[test]
fn market_price_eth_native() {
    let output = run_with_retry(&[
        "market",
        "price",
        tokens::EVM_NATIVE,
        "--chain",
        "ethereum",
    ]);
    let data = assert_ok_and_extract_data(&output);
    assert!(data.is_array(), "expected array of price entries: {data}");
    let arr = data.as_array().unwrap();
    assert!(!arr.is_empty(), "expected at least one price entry");
    assert!(
        arr[0].get("price").is_some(),
        "price entry missing 'price': {}",
        arr[0]
    );
}

#[test]
fn market_price_solana_wsol() {
    let output = run_with_retry(&["market", "price", tokens::SOL_WSOL, "--chain", "solana"]);
    let data = assert_ok_and_extract_data(&output);
    assert!(data.is_array(), "expected array: {data}");
}

#[test]
fn market_price_missing_address_fails() {
    onchainos()
        .args(["market", "price"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("required"));
}

// ─── prices (batch) ─────────────────────────────────────────────────

#[test]
fn market_prices_batch_query() {
    let tokens_arg = format!("1:{},501:{}", tokens::EVM_NATIVE, tokens::SOL_WSOL);
    let output = run_with_retry(&["market", "prices", &tokens_arg]);
    let data = assert_ok_and_extract_data(&output);
    assert!(data.is_array(), "expected array: {data}");
    let arr = data.as_array().unwrap();
    assert!(
        arr.len() >= 2,
        "expected at least 2 price entries, got {}",
        arr.len()
    );
}

// ─── kline ──────────────────────────────────────────────────────────

#[test]
fn market_kline_returns_candles() {
    let output = run_with_retry(&[
        "market",
        "kline",
        tokens::SOL_WSOL,
        "--chain",
        "solana",
        "--bar",
        "1H",
        "--limit",
        "5",
    ]);
    let data = assert_ok_and_extract_data(&output);
    assert!(data.is_array(), "kline data should be an array: {data}");
    let arr = data.as_array().unwrap();
    assert!(!arr.is_empty(), "expected at least one candle");
    assert!(
        arr[0].is_array(),
        "each candle should be an array [ts, open, high, low, close, vol, volUsd, confirm]: {}",
        arr[0]
    );
}

#[test]
fn market_kline_missing_address_fails() {
    onchainos()
        .args(["market", "kline"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("required"));
}

// ─── trades ─────────────────────────────────────────────────────────

#[test]
fn market_trades_returns_recent_trades() {
    let output = run_with_retry(&[
        "market",
        "trades",
        tokens::SOL_WSOL,
        "--chain",
        "solana",
        "--limit",
        "5",
    ]);
    let data = assert_ok_and_extract_data(&output);
    assert!(
        data.is_array() || data.is_object(),
        "trades data should be array or object: {data}"
    );
}

// ─── index ──────────────────────────────────────────────────────────

#[test]
fn market_index_price() {
    let output = run_with_retry(&[
        "market",
        "index",
        tokens::EVM_NATIVE,
        "--chain",
        "ethereum",
    ]);
    let data = assert_ok_and_extract_data(&output);
    assert!(data.is_array(), "expected array: {data}");
    let arr = data.as_array().unwrap();
    assert!(!arr.is_empty(), "expected at least one index price entry");
}

// ─── signal-chains ──────────────────────────────────────────────────

#[test]
fn market_signal_chains_returns_list() {
    let output = run_with_retry(&["market", "signal-chains"]);
    let data = assert_ok_and_extract_data(&output);
    assert!(data.is_array(), "expected array of chains: {data}");
    let arr = data.as_array().unwrap();
    assert!(!arr.is_empty(), "expected at least one signal chain");
    assert!(
        arr[0].get("chainIndex").is_some(),
        "entry missing 'chainIndex': {}",
        arr[0]
    );
}

// ─── signal-list ────────────────────────────────────────────────────

#[test]
fn market_signal_list_ethereum() {
    let output = run_with_retry(&["market", "signal-list", "ethereum"]);
    let data = assert_ok_and_extract_data(&output);
    assert!(
        data.is_array() || data.is_object(),
        "expected signal data: {data}"
    );
}

#[test]
fn market_signal_list_with_wallet_type_filter() {
    let output = run_with_retry(&["market", "signal-list", "solana", "--wallet-type", "1"]);
    let data = assert_ok_and_extract_data(&output);
    assert!(
        data.is_array() || data.is_object(),
        "expected signal data: {data}"
    );
}

#[test]
fn market_signal_list_missing_chain_fails() {
    onchainos()
        .args(["market", "signal-list"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("required"));
}

// ─── memepump-chains ────────────────────────────────────────────────

#[test]
fn memepump_chains_returns_supported_chains() {
    let output = run_with_retry(&["market", "memepump-chains"]);
    let data = assert_ok_and_extract_data(&output);

    assert!(data.is_array(), "data should be an array");
    let chains = data.as_array().unwrap();
    assert!(!chains.is_empty(), "expected at least one supported chain");

    let first = &chains[0];
    assert!(
        first.get("chainIndex").is_some(),
        "chain entry missing 'chainIndex': {first}"
    );
}

// ─── memepump-tokens ────────────────────────────────────────────────

#[test]
fn memepump_tokens_returns_list_for_solana() {
    let output = run_with_retry(&["market", "memepump-tokens", "solana", "--stage", "NEW"]);
    let data = assert_ok_and_extract_data(&output);
    assert!(
        data.is_array() || data.is_object(),
        "data should be array or object: {data}"
    );
}

#[test]
fn memepump_tokens_with_filters() {
    let output = run_with_retry(&[
        "market",
        "memepump-tokens",
        "solana",
        "--stage",
        "MIGRATED",
        "--sort-by",
        "marketCap",
        "--sort-order",
        "desc",
    ]);
    let data = assert_ok_and_extract_data(&output);
    assert!(
        data.is_array() || data.is_object(),
        "data should be array or object: {data}"
    );
}

#[test]
fn memepump_tokens_missing_chain_arg_fails() {
    onchainos()
        .args(["market", "memepump-tokens"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("required"));
}

#[test]
fn memepump_tokens_missing_stage_arg_fails() {
    onchainos()
        .args(["market", "memepump-tokens", "solana"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("required"));
}

// ─── Helper: fetch a real memepump token address ────────────────────

fn fetch_first_memepump_token_address(chain: &str) -> Option<String> {
    let output = assert_cmd::Command::from(cargo_bin_cmd!("onchainos"))
        .args([
            "market",
            "memepump-tokens",
            chain,
            "--stage",
            "MIGRATED",
            "--sort-by",
            "marketCap",
            "--sort-order",
            "desc",
        ])
        .output()
        .ok()?;

    if !output.status.success() {
        return None;
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let json: Value = serde_json::from_str(&stdout).ok()?;
    if json["ok"] != Value::Bool(true) {
        return None;
    }

    let data = &json["data"];
    let tokens = if data.is_array() {
        data.as_array()
    } else {
        data.get("data").and_then(|d| d.as_array())
    };

    tokens
        .and_then(|arr| arr.first())
        .and_then(|t| t.get("tokenAddress").or_else(|| t.get("tokenContractAddress")))
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
}

// ─── memepump-token-details ─────────────────────────────────────────

#[test]
fn memepump_token_details_with_real_token() {
    let address = match fetch_first_memepump_token_address("solana") {
        Some(addr) => addr,
        None => {
            eprintln!("SKIP: could not fetch a live memepump token address");
            return;
        }
    };

    let output = run_with_retry(&[
        "market",
        "memepump-token-details",
        &address,
        "--chain",
        "solana",
    ]);
    let data = assert_ok_and_extract_data(&output);
    assert!(
        data.is_object() || data.is_array(),
        "expected token detail data: {data}"
    );
}

#[test]
fn memepump_token_details_missing_address_fails() {
    onchainos()
        .args(["market", "memepump-token-details"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("required"));
}

// ─── memepump-token-dev-info ────────────────────────────────────────

#[test]
fn memepump_token_dev_info_with_real_token() {
    let address = match fetch_first_memepump_token_address("solana") {
        Some(addr) => addr,
        None => {
            eprintln!("SKIP: could not fetch a live memepump token address");
            return;
        }
    };

    let output = run_with_retry(&[
        "market",
        "memepump-token-dev-info",
        &address,
        "--chain",
        "solana",
    ]);
    let data = assert_ok_and_extract_data(&output);
    assert!(
        data.is_object() || data.is_array(),
        "expected dev info data: {data}"
    );
}

#[test]
fn memepump_token_dev_info_missing_address_fails() {
    onchainos()
        .args(["market", "memepump-token-dev-info"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("required"));
}

// ─── memepump-similar-tokens ────────────────────────────────────────

#[test]
fn memepump_similar_tokens_with_real_token() {
    let address = match fetch_first_memepump_token_address("solana") {
        Some(addr) => addr,
        None => {
            eprintln!("SKIP: could not fetch a live memepump token address");
            return;
        }
    };

    let output = run_with_retry(&[
        "market",
        "memepump-similar-tokens",
        &address,
        "--chain",
        "solana",
    ]);
    let data = assert_ok_and_extract_data(&output);
    assert!(
        data.is_array() || data.is_object(),
        "expected similar tokens data: {data}"
    );
}

#[test]
fn memepump_similar_tokens_missing_address_fails() {
    onchainos()
        .args(["market", "memepump-similar-tokens"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("required"));
}

// ─── memepump-token-bundle-info ─────────────────────────────────────

#[test]
fn memepump_token_bundle_info_with_real_token() {
    let address = match fetch_first_memepump_token_address("solana") {
        Some(addr) => addr,
        None => {
            eprintln!("SKIP: could not fetch a live memepump token address");
            return;
        }
    };

    let output = run_with_retry(&[
        "market",
        "memepump-token-bundle-info",
        &address,
        "--chain",
        "solana",
    ]);
    let data = assert_ok_and_extract_data(&output);
    assert!(
        data.is_object() || data.is_array(),
        "expected bundle info data: {data}"
    );
}

#[test]
fn memepump_token_bundle_info_missing_address_fails() {
    onchainos()
        .args(["market", "memepump-token-bundle-info"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("required"));
}

// ─── memepump-aped-wallet ───────────────────────────────────────────

#[test]
fn memepump_aped_wallet_with_real_token() {
    let address = match fetch_first_memepump_token_address("solana") {
        Some(addr) => addr,
        None => {
            eprintln!("SKIP: could not fetch a live memepump token address");
            return;
        }
    };

    let output = run_with_retry(&[
        "market",
        "memepump-aped-wallet",
        &address,
        "--chain",
        "solana",
    ]);
    let data = assert_ok_and_extract_data(&output);
    assert!(
        data.is_object() || data.is_array(),
        "expected aped wallet data: {data}"
    );
}

#[test]
fn memepump_aped_wallet_missing_address_fails() {
    onchainos()
        .args(["market", "memepump-aped-wallet"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("required"));
}
