//! Integration tests for `onchainos tracker` commands:
//! trades (KOL / smart money / multi-address trading activity).

mod common;

use common::{assert_ok_and_extract_data, onchainos, run_with_retry};
use predicates::prelude::*;

// ─── trades (default / KOL) ─────────────────────────────────────────

#[test]
fn tracker_trades_default_kol() {
    let output = run_with_retry(&["tracker", "trades"]);
    let data = assert_ok_and_extract_data(&output);
    assert!(
        data.is_object() || data.is_array(),
        "expected object or array: {data}"
    );
}

#[test]
fn tracker_trades_kol_explicit() {
    let output = run_with_retry(&["tracker", "trades", "--tracker-type", "kol"]);
    let data = assert_ok_and_extract_data(&output);
    assert!(
        data.is_object() || data.is_array(),
        "expected object or array: {data}"
    );
}

// ─── trades (tracker-type aliases) ──────────────────────────────────

#[test]
fn tracker_trades_smart_money_name() {
    let output = run_with_retry(&["tracker", "trades", "--tracker-type", "smart_money"]);
    let data = assert_ok_and_extract_data(&output);
    assert!(
        data.is_object() || data.is_array(),
        "expected object or array: {data}"
    );
}

#[test]
fn tracker_trades_smart_money_abbrev() {
    let output = run_with_retry(&["tracker", "trades", "--tracker-type", "sm"]);
    let data = assert_ok_and_extract_data(&output);
    assert!(
        data.is_object() || data.is_array(),
        "expected object or array: {data}"
    );
}

#[test]
fn tracker_trades_tracker_type_numeric_1() {
    let output = run_with_retry(&["tracker", "trades", "--tracker-type", "1"]);
    let data = assert_ok_and_extract_data(&output);
    assert!(
        data.is_object() || data.is_array(),
        "expected object or array: {data}"
    );
}

#[test]
fn tracker_trades_tracker_type_numeric_2() {
    let output = run_with_retry(&["tracker", "trades", "--tracker-type", "2"]);
    let data = assert_ok_and_extract_data(&output);
    assert!(
        data.is_object() || data.is_array(),
        "expected object or array: {data}"
    );
}

// ─── trades (trade-type aliases) ─────────────────────────────────────

#[test]
fn tracker_trades_buy_name() {
    let output = run_with_retry(&["tracker", "trades", "--trade-type", "buy"]);
    let data = assert_ok_and_extract_data(&output);
    assert!(
        data.is_object() || data.is_array(),
        "expected object or array: {data}"
    );
}

#[test]
fn tracker_trades_sell_name() {
    let output = run_with_retry(&["tracker", "trades", "--trade-type", "sell"]);
    let data = assert_ok_and_extract_data(&output);
    assert!(
        data.is_object() || data.is_array(),
        "expected object or array: {data}"
    );
}

#[test]
fn tracker_trades_trade_type_numeric_1() {
    let output = run_with_retry(&["tracker", "trades", "--trade-type", "1"]);
    let data = assert_ok_and_extract_data(&output);
    assert!(
        data.is_object() || data.is_array(),
        "expected object or array: {data}"
    );
}

#[test]
fn tracker_trades_trade_type_numeric_2() {
    let output = run_with_retry(&["tracker", "trades", "--trade-type", "2"]);
    let data = assert_ok_and_extract_data(&output);
    assert!(
        data.is_object() || data.is_array(),
        "expected object or array: {data}"
    );
}

// ─── trades (chain filter) ───────────────────────────────────────────

#[test]
fn tracker_trades_smart_money_buys_on_solana() {
    let output = run_with_retry(&[
        "tracker",
        "trades",
        "--tracker-type",
        "smart_money",
        "--trade-type",
        "buy",
        "--chain",
        "solana",
    ]);
    let data = assert_ok_and_extract_data(&output);
    assert!(
        data.is_object() || data.is_array(),
        "expected object or array: {data}"
    );
}

#[test]
fn tracker_trades_kol_on_ethereum() {
    let output = run_with_retry(&["tracker", "trades", "--chain", "ethereum"]);
    let data = assert_ok_and_extract_data(&output);
    assert!(
        data.is_object() || data.is_array(),
        "expected object or array: {data}"
    );
}

// ─── trades (volume / market cap / liquidity filters) ────────────────

#[test]
fn tracker_trades_with_volume_filter() {
    let output = run_with_retry(&[
        "tracker",
        "trades",
        "--min-volume",
        "1000",
        "--max-volume",
        "10000000",
    ]);
    let data = assert_ok_and_extract_data(&output);
    assert!(
        data.is_object() || data.is_array(),
        "expected object or array: {data}"
    );
}

#[test]
fn tracker_trades_with_market_cap_filter() {
    let output = run_with_retry(&[
        "tracker",
        "trades",
        "--min-market-cap",
        "100000",
        "--max-market-cap",
        "1000000000",
    ]);
    let data = assert_ok_and_extract_data(&output);
    assert!(
        data.is_object() || data.is_array(),
        "expected object or array: {data}"
    );
}

// ─── trades (multi_address) ──────────────────────────────────────────

#[test]
fn tracker_trades_multi_address() {
    let output = run_with_retry(&[
        "tracker",
        "trades",
        "--tracker-type",
        "multi_address",
        "--wallet-address",
        "0xd8dA6BF26964aF9D7eEd9e03E53415D37aA96045",
    ]);
    let data = assert_ok_and_extract_data(&output);
    assert!(
        data.is_object() || data.is_array(),
        "expected object or array: {data}"
    );
}

#[test]
fn tracker_trades_custom_alias() {
    let output = run_with_retry(&[
        "tracker",
        "trades",
        "--tracker-type",
        "custom",
        "--wallet-address",
        "0xd8dA6BF26964aF9D7eEd9e03E53415D37aA96045",
    ]);
    let data = assert_ok_and_extract_data(&output);
    assert!(
        data.is_object() || data.is_array(),
        "expected object or array: {data}"
    );
}

// ─── error cases ─────────────────────────────────────────────────────

#[test]
fn tracker_trades_multi_address_missing_wallet_address() {
    onchainos()
        .args(["tracker", "trades", "--tracker-type", "multi_address"])
        .assert()
        .failure()
        .stdout(predicate::str::contains("wallet-address"));
}

#[test]
fn tracker_trades_custom_missing_wallet_address() {
    onchainos()
        .args(["tracker", "trades", "--tracker-type", "custom"])
        .assert()
        .failure()
        .stdout(predicate::str::contains("wallet-address"));
}

#[test]
fn tracker_trades_numeric_3_missing_wallet_address() {
    onchainos()
        .args(["tracker", "trades", "--tracker-type", "3"])
        .assert()
        .failure()
        .stdout(predicate::str::contains("wallet-address"));
}
