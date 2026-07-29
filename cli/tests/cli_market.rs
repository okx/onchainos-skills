//! Integration tests for `onchainos market` commands:
//! price, prices, kline, index, and portfolio-*.
//!
//! These tests run the compiled binary against the live OKX API,
//! so they require network access and valid API credentials.

mod common;

use common::{assert_ok_and_extract_data, onchainos, run_with_retry, tokens};
use predicates::prelude::*;

// ─── price ──────────────────────────────────────────────────────────

#[test]
fn market_price_eth_native() {
    let output = run_with_retry(&[
        "market",
        "price",
        "--address",
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
    let output = run_with_retry(&[
        "market",
        "price",
        "--address",
        tokens::SOL_WSOL,
        "--chain",
        "solana",
    ]);
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
    let output = run_with_retry(&["market", "prices", "--tokens", &tokens_arg]);
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
        "--address",
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
    // Candles are now named objects with fields: ts, o, h, l, c, vol, volUsd, confirm
    let candle = &arr[0];
    assert!(
        candle.is_object(),
        "each candle should be a named object: {candle}"
    );
    for field in ["ts", "o", "h", "l", "c", "vol", "volUsd", "confirm"] {
        assert!(
            candle.get(field).is_some(),
            "candle missing field '{field}': {candle}"
        );
    }
}

#[test]
fn market_kline_missing_address_fails() {
    onchainos()
        .args(["market", "kline"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("required"));
}

// ─── index ──────────────────────────────────────────────────────────

#[test]
fn market_index_price() {
    let output = run_with_retry(&[
        "market",
        "index",
        "--address",
        tokens::EVM_NATIVE,
        "--chain",
        "ethereum",
    ]);
    let data = assert_ok_and_extract_data(&output);
    assert!(data.is_array(), "expected array: {data}");
    let arr = data.as_array().unwrap();
    assert!(!arr.is_empty(), "expected at least one index price entry");
}

// ─── portfolio-supported-chains ─────────────────────────────────────

// Well-known Ethereum wallet (vitalik.eth) used for portfolio PnL tests
const PORTFOLIO_TEST_WALLET: &str = "0xd8dA6BF26964aF9D7eEd9e03E53415D37aA96045";

#[test]
fn market_portfolio_supported_chains_returns_list() {
    let output = run_with_retry(&["market", "portfolio-supported-chains"]);
    let data = assert_ok_and_extract_data(&output);
    assert!(data.is_array(), "expected array of chains: {data}");
    let arr = data.as_array().unwrap();
    assert!(!arr.is_empty(), "expected at least one supported chain");
    assert!(
        arr[0].get("chainIndex").is_some(),
        "chain entry missing 'chainIndex': {}",
        arr[0]
    );
}

// ─── portfolio-overview ─────────────────────────────────────────────

#[test]
fn market_portfolio_overview_ethereum() {
    let output = run_with_retry(&[
        "market",
        "portfolio-overview",
        "--address",
        PORTFOLIO_TEST_WALLET,
        "--chain",
        "ethereum",
        "--time-frame",
        "3",
    ]);
    let data = assert_ok_and_extract_data(&output);
    assert!(
        data.is_object() || data.is_array(),
        "expected portfolio overview data: {data}"
    );
}

#[test]
fn market_portfolio_overview_with_timeframe() {
    let output = run_with_retry(&[
        "market",
        "portfolio-overview",
        "--address",
        PORTFOLIO_TEST_WALLET,
        "--chain",
        "ethereum",
        "--time-frame",
        "1",
    ]);
    let data = assert_ok_and_extract_data(&output);
    assert!(
        data.is_object() || data.is_array(),
        "expected portfolio overview data: {data}"
    );
}

#[test]
fn market_portfolio_overview_missing_address_fails() {
    onchainos()
        .args([
            "market",
            "portfolio-overview",
            "--chain",
            "ethereum",
            "--time-frame",
            "3",
        ])
        .assert()
        .failure()
        .stderr(predicate::str::contains("required"));
}

#[test]
fn market_portfolio_overview_missing_chain_fails() {
    onchainos()
        .args([
            "market",
            "portfolio-overview",
            "--address",
            PORTFOLIO_TEST_WALLET,
            "--time-frame",
            "3",
        ])
        .assert()
        .failure()
        .stderr(predicate::str::contains("required"));
}

#[test]
fn market_portfolio_overview_without_time_frame_defaults_to_1m() {
    // --time-frame now defaults to "4" (1M) per user feedback (#21)
    let output = run_with_retry(&[
        "market",
        "portfolio-overview",
        "--address",
        PORTFOLIO_TEST_WALLET,
        "--chain",
        "ethereum",
    ]);
    let data = assert_ok_and_extract_data(&output);
    assert!(data.is_object(), "expected PnL overview object: {data}");
}

// ─── portfolio-dex-history ──────────────────────────────────────────

#[test]
fn market_portfolio_dex_history_ethereum() {
    let output = run_with_retry(&[
        "market",
        "portfolio-dex-history",
        "--address",
        PORTFOLIO_TEST_WALLET,
        "--chain",
        "ethereum",
        "--begin",
        "1700000000000",
        "--end",
        "1710000000000",
    ]);
    let data = assert_ok_and_extract_data(&output);
    assert!(
        data.is_object() || data.is_array(),
        "expected dex history data: {data}"
    );
}

#[test]
fn market_portfolio_dex_history_with_limit() {
    let output = run_with_retry(&[
        "market",
        "portfolio-dex-history",
        "--address",
        PORTFOLIO_TEST_WALLET,
        "--chain",
        "ethereum",
        "--begin",
        "1700000000000",
        "--end",
        "1710000000000",
        "--limit",
        "5",
    ]);
    let data = assert_ok_and_extract_data(&output);
    assert!(
        data.is_object() || data.is_array(),
        "expected dex history data: {data}"
    );
}

#[test]
fn market_portfolio_dex_history_with_token_filter() {
    let output = run_with_retry(&[
        "market",
        "portfolio-dex-history",
        "--address",
        PORTFOLIO_TEST_WALLET,
        "--chain",
        "ethereum",
        "--begin",
        "1700000000000",
        "--end",
        "1710000000000",
        "--token",
        tokens::ETH_USDC,
    ]);
    let data = assert_ok_and_extract_data(&output);
    assert!(
        data.is_object() || data.is_array(),
        "expected dex history data: {data}"
    );
}

#[test]
fn market_portfolio_dex_history_with_tx_type() {
    let output = run_with_retry(&[
        "market",
        "portfolio-dex-history",
        "--address",
        PORTFOLIO_TEST_WALLET,
        "--chain",
        "ethereum",
        "--begin",
        "1700000000000",
        "--end",
        "1710000000000",
        "--tx-type",
        "1",
    ]);
    let data = assert_ok_and_extract_data(&output);
    assert!(
        data.is_object() || data.is_array(),
        "expected dex history data: {data}"
    );
}

#[test]
fn market_portfolio_dex_history_missing_address_fails() {
    onchainos()
        .args([
            "market",
            "portfolio-dex-history",
            "--chain",
            "ethereum",
            "--begin",
            "1700000000000",
            "--end",
            "1710000000000",
        ])
        .assert()
        .failure()
        .stderr(predicate::str::contains("required"));
}

#[test]
fn market_portfolio_dex_history_missing_chain_fails() {
    onchainos()
        .args([
            "market",
            "portfolio-dex-history",
            "--address",
            PORTFOLIO_TEST_WALLET,
            "--begin",
            "1700000000000",
            "--end",
            "1710000000000",
        ])
        .assert()
        .failure()
        .stderr(predicate::str::contains("required"));
}

#[test]
fn market_portfolio_dex_history_missing_begin_fails() {
    onchainos()
        .args([
            "market",
            "portfolio-dex-history",
            "--address",
            PORTFOLIO_TEST_WALLET,
            "--chain",
            "ethereum",
            "--end",
            "1710000000000",
        ])
        .assert()
        .failure()
        .stderr(predicate::str::contains("required"));
}

#[test]
fn market_portfolio_dex_history_missing_end_fails() {
    onchainos()
        .args([
            "market",
            "portfolio-dex-history",
            "--address",
            PORTFOLIO_TEST_WALLET,
            "--chain",
            "ethereum",
            "--begin",
            "1700000000000",
        ])
        .assert()
        .failure()
        .stderr(predicate::str::contains("required"));
}

// ─── portfolio-recent-pnl ───────────────────────────────────────────

#[test]
fn market_portfolio_recent_pnl_ethereum() {
    let output = run_with_retry(&[
        "market",
        "portfolio-recent-pnl",
        "--address",
        PORTFOLIO_TEST_WALLET,
        "--chain",
        "ethereum",
    ]);
    let data = assert_ok_and_extract_data(&output);
    assert!(
        data.is_object() || data.is_array(),
        "expected recent PnL data: {data}"
    );
}

#[test]
fn market_portfolio_recent_pnl_with_limit() {
    let output = run_with_retry(&[
        "market",
        "portfolio-recent-pnl",
        "--address",
        PORTFOLIO_TEST_WALLET,
        "--chain",
        "ethereum",
        "--limit",
        "5",
    ]);
    let data = assert_ok_and_extract_data(&output);
    assert!(
        data.is_object() || data.is_array(),
        "expected recent PnL data: {data}"
    );
}

#[test]
fn market_portfolio_recent_pnl_missing_address_fails() {
    onchainos()
        .args(["market", "portfolio-recent-pnl", "--chain", "ethereum"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("required"));
}

#[test]
fn market_portfolio_recent_pnl_missing_chain_fails() {
    onchainos()
        .args([
            "market",
            "portfolio-recent-pnl",
            "--address",
            PORTFOLIO_TEST_WALLET,
        ])
        .assert()
        .failure()
        .stderr(predicate::str::contains("required"));
}

// ─── portfolio-token-pnl ────────────────────────────────────────────

#[test]
fn market_portfolio_token_pnl_usdc() {
    let output = run_with_retry(&[
        "market",
        "portfolio-token-pnl",
        "--address",
        PORTFOLIO_TEST_WALLET,
        "--chain",
        "ethereum",
        "--token",
        tokens::ETH_USDC,
    ]);
    let data = assert_ok_and_extract_data(&output);
    assert!(
        data.is_object() || data.is_array(),
        "expected token PnL data: {data}"
    );
}

#[test]
fn market_portfolio_token_pnl_native_eth() {
    let output = run_with_retry(&[
        "market",
        "portfolio-token-pnl",
        "--address",
        PORTFOLIO_TEST_WALLET,
        "--chain",
        "ethereum",
        "--token",
        tokens::EVM_NATIVE,
    ]);
    let data = assert_ok_and_extract_data(&output);
    assert!(
        data.is_object() || data.is_array(),
        "expected token PnL data: {data}"
    );
}

#[test]
fn market_portfolio_token_pnl_missing_address_fails() {
    onchainos()
        .args([
            "market",
            "portfolio-token-pnl",
            "--chain",
            "ethereum",
            "--token",
            tokens::ETH_USDC,
        ])
        .assert()
        .failure()
        .stderr(predicate::str::contains("required"));
}

#[test]
fn market_portfolio_token_pnl_missing_chain_fails() {
    onchainos()
        .args([
            "market",
            "portfolio-token-pnl",
            "--address",
            PORTFOLIO_TEST_WALLET,
            "--token",
            tokens::ETH_USDC,
        ])
        .assert()
        .failure()
        .stderr(predicate::str::contains("required"));
}

#[test]
fn market_portfolio_token_pnl_missing_token_fails() {
    onchainos()
        .args([
            "market",
            "portfolio-token-pnl",
            "--address",
            PORTFOLIO_TEST_WALLET,
            "--chain",
            "ethereum",
        ])
        .assert()
        .failure()
        .stderr(predicate::str::contains("required"));
}
