//! Integration tests for `onchainos tracker` and `onchainos signal` commands.
//!
//! Both namespaces belong to the okx-dex-signal skill:
//! - `tracker activities` — raw DEX transaction feed for smart money / KOL / custom wallets
//! - `signal chains` / `signal list` — aggregated buy-only signal alerts
//!
//! These tests run the compiled binary against the live OKX API,
//! so they require network access and valid API credentials.

mod common;

use common::{assert_ok_and_extract_data, onchainos, run_with_retry};
use predicates::prelude::*;
use serde_json::Value;

// ─── tracker activities ──────────────────────────────────────────────

/// Verify expected fields are present in a trade entry.
fn assert_tracker_trade_fields(entry: &Value) {
    for field in &[
        "txHash",
        "walletAddress",
        "tokenSymbol",
        "tokenContractAddress",
        "chainIndex",
        "tokenPrice",
        "tradeType",
        "tradeTime",
        "trackerType",
    ] {
        assert!(
            entry.get(field).is_some(),
            "trade entry missing '{field}': {entry}"
        );
    }
}

/// Extract the trades array from either a flat array or `{ "trades": [...] }` response.
fn extract_trades(data: Value) -> Vec<Value> {
    if let Some(arr) = data.as_array() {
        arr.clone()
    } else if let Some(arr) = data.get("trades").and_then(|v| v.as_array()) {
        arr.clone()
    } else {
        panic!("expected array or object with 'trades' key: {data}");
    }
}

#[test]
fn address_tracker_smart_money_returns_trades() {
    let output = run_with_retry(&["tracker", "activities", "--tracker-type", "smart_money"]);
    let data = assert_ok_and_extract_data(&output);
    let trades = extract_trades(data);
    assert!(!trades.is_empty(), "expected at least one trade entry");
    assert_tracker_trade_fields(&trades[0]);
}

#[test]
fn address_tracker_kol_returns_trades() {
    let output = run_with_retry(&["tracker", "activities", "--tracker-type", "kol"]);
    let data = assert_ok_and_extract_data(&output);
    let trades = extract_trades(data);
    assert!(!trades.is_empty(), "expected at least one trade entry");
    assert_tracker_trade_fields(&trades[0]);
}

#[test]
fn address_tracker_smart_money_solana_buy_only() {
    let output = run_with_retry(&[
        "tracker",
        "activities",
        "--tracker-type",
        "smart_money",
        "--chain",
        "solana",
        "--trade-type",
        "1",
    ]);
    let data = assert_ok_and_extract_data(&output);
    let trades = extract_trades(data);
    // Verify all returned trades are buys (tradeType == "1")
    for trade in &trades {
        assert_eq!(
            trade.get("tradeType").and_then(|v| v.as_str()),
            Some("1"),
            "expected buy-only trades (tradeType=1): {trade}"
        );
    }
}

#[test]
fn address_tracker_smart_money_with_volume_filter() {
    let output = run_with_retry(&[
        "tracker",
        "activities",
        "--tracker-type",
        "smart_money",
        "--min-volume",
        "1000",
    ]);
    let data = assert_ok_and_extract_data(&output);
    assert!(
        data.is_array() || data.is_object(),
        "expected array or object: {data}"
    );
}

#[test]
fn address_tracker_multi_address_returns_trades() {
    // Use two well-known Ethereum addresses as custom tracked wallets
    let output = run_with_retry(&[
        "tracker",
        "activities",
        "--tracker-type",
        "multi_address",
        "--wallet-address",
        "0xd8da6bf26964af9d7eed9e03e53415d37aa96045,0xab5801a7d398351b8be11c439e05c5b3259aec9b",
    ]);
    let data = assert_ok_and_extract_data(&output);
    assert!(
        data.is_array() || data.is_object(),
        "expected array or object: {data}"
    );
}

#[test]
fn address_tracker_missing_tracker_type_fails() {
    onchainos()
        .args(["tracker", "activities"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("required"));
}

#[test]
fn address_tracker_multi_address_missing_wallet_fails() {
    onchainos()
        .args(["tracker", "activities", "--tracker-type", "multi_address"])
        .assert()
        .failure();
}

// ─── signal-chains ──────────────────────────────────────────────────

#[test]
fn signal_chains_returns_list() {
    let output = run_with_retry(&["signal", "chains"]);
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
fn signal_list_ethereum() {
    let output = run_with_retry(&["signal", "list", "--chain", "ethereum"]);
    let data = assert_ok_and_extract_data(&output);
    assert!(
        data.is_array() || data.is_object(),
        "expected signal data: {data}"
    );
}

#[test]
fn signal_list_with_wallet_type_filter() {
    let output = run_with_retry(&["signal", "list", "--chain", "solana", "--wallet-type", "1"]);
    let data = assert_ok_and_extract_data(&output);
    assert!(
        data.is_array() || data.is_object(),
        "expected signal data: {data}"
    );
}

#[test]
fn signal_list_wallet_type_values_are_numeric() {
    let output = run_with_retry(&["signal", "list", "--chain", "solana"]);
    let data = assert_ok_and_extract_data(&output);
    if let Some(arr) = data.as_array() {
        if let Some(entry) = arr.first() {
            let wt = entry
                .get("walletType")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            assert!(
                ["1", "2", "3"].contains(&wt),
                "walletType should be '1', '2', or '3', got: '{wt}'"
            );
        }
    }
}

#[test]
fn signal_list_with_all_filters() {
    let output = run_with_retry(&[
        "signal",
        "list",
        "--chain",
        "solana",
        "--wallet-type",
        "1,2,3",
        "--min-amount-usd",
        "0",
        "--max-amount-usd",
        "1000000000",
        "--min-address-count",
        "1",
        "--max-address-count",
        "1000000",
        "--min-market-cap-usd",
        "0",
        "--max-market-cap-usd",
        "1000000000000",
        "--min-liquidity-usd",
        "0",
        "--max-liquidity-usd",
        "1000000000000",
    ]);
    let data = assert_ok_and_extract_data(&output);
    assert!(
        data.is_array() || data.is_object(),
        "expected signal data: {data}"
    );
}

#[test]
fn signal_list_missing_chain_fails() {
    onchainos()
        .args(["signal", "list"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("required"));
}
