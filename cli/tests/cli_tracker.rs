//! Integration tests for `onchainos tracker` commands:
//! trades (KOL / smart money / custom group trading activity).

mod common;

use common::{assert_ok_and_extract_data, run_with_retry};

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

// ─── trades (smart_money) ───────────────────────────────────────────

#[test]
fn tracker_trades_smart_money() {
    let output = run_with_retry(&["tracker", "trades", "--tracker-type", "smart_money"]);
    let data = assert_ok_and_extract_data(&output);
    assert!(
        data.is_object() || data.is_array(),
        "expected object or array: {data}"
    );
}

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

// ─── trades (trade-type filter) ─────────────────────────────────────

#[test]
fn tracker_trades_buy_only() {
    let output = run_with_retry(&["tracker", "trades", "--trade-type", "buy"]);
    let data = assert_ok_and_extract_data(&output);
    assert!(
        data.is_object() || data.is_array(),
        "expected object or array: {data}"
    );
}

#[test]
fn tracker_trades_sell_only() {
    let output = run_with_retry(&["tracker", "trades", "--trade-type", "sell"]);
    let data = assert_ok_and_extract_data(&output);
    assert!(
        data.is_object() || data.is_array(),
        "expected object or array: {data}"
    );
}

// ─── trades (chain filter) ──────────────────────────────────────────

#[test]
fn tracker_trades_kol_on_ethereum() {
    let output = run_with_retry(&["tracker", "trades", "--chain", "ethereum"]);
    let data = assert_ok_and_extract_data(&output);
    assert!(
        data.is_object() || data.is_array(),
        "expected object or array: {data}"
    );
}

// ─── trades (volume / market cap / liquidity filters) ───────────────

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

// ─── trades (limit) ─────────────────────────────────────────────────

#[test]
fn tracker_trades_limit_50() {
    let output = run_with_retry(&["tracker", "trades", "--limit", "50"]);
    let data = assert_ok_and_extract_data(&output);
    assert!(
        data.is_object() || data.is_array(),
        "expected object or array: {data}"
    );
}
