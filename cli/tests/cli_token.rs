//! Integration tests for `onchainos token` commands:
//! search, info, price-info, trending, holders, liquidity, hot-tokens, advanced-info, top-trader, trades.

mod common;

use common::{assert_ok_and_extract_data, onchainos, run_with_retry, tokens};
use predicates::prelude::*;

// ─── search ─────────────────────────────────────────────────────────

#[test]
fn token_search_by_symbol() {
    let output = run_with_retry(&["token", "search", "--query", "USDC"]);
    let data = assert_ok_and_extract_data(&output);
    assert!(data.is_array(), "expected array of search results: {data}");
    let arr = data.as_array().unwrap();
    assert!(!arr.is_empty(), "expected at least one result for USDC");
}

#[test]
fn token_search_by_address() {
    let output = run_with_retry(&[
        "token",
        "search",
        "--query",
        tokens::ETH_USDC,
        "--chains",
        "ethereum",
    ]);
    let data = assert_ok_and_extract_data(&output);
    assert!(data.is_array(), "expected array: {data}");
}

#[test]
fn token_search_cross_chain() {
    let output = run_with_retry(&[
        "token",
        "search",
        "--query",
        "SOL",
        "--chains",
        "solana,ethereum",
    ]);
    let data = assert_ok_and_extract_data(&output);
    assert!(data.is_array(), "expected array: {data}");
}

#[test]
fn token_search_phrase_query() {
    let output = run_with_retry(&[
        "token", "search", "--query", "dog wif", "--chains", "solana",
    ]);
    let data = assert_ok_and_extract_data(&output);
    assert!(data.is_array(), "expected array of search results: {data}");
}

#[test]
fn token_search_unicode_query() {
    let output = run_with_retry(&["token", "search", "--query", "狗", "--chains", "solana"]);
    let data = assert_ok_and_extract_data(&output);
    assert!(data.is_array(), "expected array of search results: {data}");
}

#[test]
fn token_search_missing_query_fails() {
    onchainos()
        .args(["token", "search"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("required"));
}

// ─── info ───────────────────────────────────────────────────────────

#[test]
fn token_info_usdc_on_ethereum() {
    let output = run_with_retry(&[
        "token",
        "info",
        "--address",
        tokens::ETH_USDC,
        "--chain",
        "ethereum",
    ]);
    let data = assert_ok_and_extract_data(&output);
    assert!(data.is_array(), "expected array: {data}");
    let arr = data.as_array().unwrap();
    assert!(!arr.is_empty(), "expected token info");
    let token = &arr[0];
    assert!(
        token.get("tokenSymbol").is_some(),
        "token info missing 'tokenSymbol': {token}"
    );
}

#[test]
fn token_info_wsol_on_solana() {
    let output = run_with_retry(&[
        "token",
        "info",
        "--address",
        tokens::SOL_WSOL,
        "--chain",
        "solana",
    ]);
    let data = assert_ok_and_extract_data(&output);
    assert!(data.is_array(), "expected array: {data}");
}

#[test]
fn token_info_missing_address_fails() {
    onchainos()
        .args(["token", "info"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("required"));
}

// ─── price-info ─────────────────────────────────────────────────────

#[test]
fn token_price_info_usdc() {
    let output = run_with_retry(&[
        "token",
        "price-info",
        "--address",
        tokens::ETH_USDC,
        "--chain",
        "ethereum",
    ]);
    let data = assert_ok_and_extract_data(&output);
    assert!(data.is_array(), "expected array: {data}");
    let arr = data.as_array().unwrap();
    assert!(!arr.is_empty(), "expected price info data");
}

#[test]
fn token_price_info_missing_address_fails() {
    onchainos()
        .args(["token", "price-info"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("required"));
}

// ─── trending ───────────────────────────────────────────────────────

#[test]
fn token_trending_default() {
    let output = run_with_retry(&["token", "trending"]);
    let data = assert_ok_and_extract_data(&output);
    assert!(
        data.is_array() || data.is_object(),
        "expected trending data: {data}"
    );
}

#[test]
fn token_trending_solana_by_volume() {
    let output = run_with_retry(&[
        "token",
        "trending",
        "--chains",
        "solana",
        "--sort-by",
        "5",
        "--time-frame",
        "4",
    ]);
    let data = assert_ok_and_extract_data(&output);
    assert!(
        data.is_array() || data.is_object(),
        "expected trending data: {data}"
    );
}

#[test]
fn token_trending_with_all_params() {
    let output = run_with_retry(&[
        "token",
        "trending",
        "--chains",
        "solana,ethereum",
        "--sort-by",
        "6",
        "--time-frame",
        "2",
    ]);
    let data = assert_ok_and_extract_data(&output);
    assert!(
        data.is_array() || data.is_object(),
        "expected trending data: {data}"
    );
}

// ─── holders ────────────────────────────────────────────────────────

#[test]
fn token_holders_usdc_on_ethereum() {
    let output = run_with_retry(&[
        "token",
        "holders",
        "--address",
        tokens::ETH_USDC,
        "--chain",
        "ethereum",
    ]);
    let data = assert_ok_and_extract_data(&output);
    assert!(
        data.is_array() || data.is_object(),
        "expected holder data: {data}"
    );
}

#[test]
fn token_holders_with_tag_filter() {
    let output = run_with_retry(&[
        "token",
        "holders",
        "--address",
        tokens::ETH_USDC,
        "--chain",
        "ethereum",
        "--tag-filter",
        "4",
    ]);
    let data = assert_ok_and_extract_data(&output);
    assert!(
        data.is_array() || data.is_object(),
        "expected holder data: {data}"
    );
}

#[test]
fn token_holders_missing_address_fails() {
    onchainos()
        .args(["token", "holders"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("required"));
}

// ─── liquidity ──────────────────────────────────────────────────────

#[test]
fn token_liquidity_usdc_on_ethereum() {
    let output = run_with_retry(&[
        "token",
        "liquidity",
        "--address",
        tokens::ETH_USDC,
        "--chain",
        "ethereum",
    ]);
    let data = assert_ok_and_extract_data(&output);
    assert!(
        data.is_array() || data.is_object(),
        "expected liquidity pool data: {data}"
    );
}

#[test]
fn token_liquidity_wsol_on_solana() {
    let output = run_with_retry(&[
        "token",
        "liquidity",
        "--address",
        tokens::SOL_WSOL,
        "--chain",
        "solana",
    ]);
    let data = assert_ok_and_extract_data(&output);
    assert!(
        data.is_array() || data.is_object(),
        "expected liquidity pool data: {data}"
    );
}

#[test]
fn token_liquidity_default_chain() {
    // No --chain specified; API falls back to default
    let output = run_with_retry(&["token", "liquidity", "--address", tokens::ETH_USDC]);
    let data = assert_ok_and_extract_data(&output);
    assert!(
        data.is_array() || data.is_object(),
        "expected liquidity pool data: {data}"
    );
}

#[test]
fn token_liquidity_missing_address_fails() {
    onchainos()
        .args(["token", "liquidity"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("required"));
}

// ─── hot-tokens ─────────────────────────────────────────────────────

#[test]
fn token_hot_tokens_default() {
    let output = run_with_retry(&["token", "hot-tokens"]);
    let data = assert_ok_and_extract_data(&output);
    assert!(
        data.is_array() || data.is_object(),
        "expected hot tokens data: {data}"
    );
}

#[test]
fn token_hot_tokens_solana_trending() {
    let output = run_with_retry(&["token", "hot-tokens", "--chain", "solana"]);
    let data = assert_ok_and_extract_data(&output);
    assert!(
        data.is_array() || data.is_object(),
        "expected hot tokens data: {data}"
    );
}

#[test]
fn token_hot_tokens_xmentioned_ranking() {
    let output = run_with_retry(&["token", "hot-tokens", "--ranking-type", "5"]);
    let data = assert_ok_and_extract_data(&output);
    assert!(
        data.is_array() || data.is_object(),
        "expected hot tokens data: {data}"
    );
}

#[test]
fn token_hot_tokens_with_sort_and_timeframe() {
    let output = run_with_retry(&[
        "token",
        "hot-tokens",
        "--chain",
        "solana",
        "--rank-by",
        "5",
        "--time-frame",
        "4",
    ]);
    let data = assert_ok_and_extract_data(&output);
    assert!(
        data.is_array() || data.is_object(),
        "expected hot tokens data: {data}"
    );
}

#[test]
fn token_hot_tokens_with_price_change_filters() {
    let output = run_with_retry(&[
        "token",
        "hot-tokens",
        "--chain",
        "solana",
        "--price-change-min",
        "0",
        "--price-change-max",
        "1000",
    ]);
    let data = assert_ok_and_extract_data(&output);
    assert!(
        data.is_array() || data.is_object(),
        "expected hot tokens data: {data}"
    );
}

#[test]
fn token_hot_tokens_with_negative_price_change_min() {
    let output = run_with_retry(&[
        "token",
        "hot-tokens",
        "--price-change-min",
        "-100",
        "--price-change-max",
        "-5",
    ]);
    let data = assert_ok_and_extract_data(&output);
    assert!(
        data.is_array() || data.is_object(),
        "expected hot tokens data: {data}"
    );
}

#[test]
fn token_hot_tokens_with_volume_filters() {
    let output = run_with_retry(&[
        "token",
        "hot-tokens",
        "--volume-min",
        "10000",
        "--volume-max",
        "1000000000",
    ]);
    let data = assert_ok_and_extract_data(&output);
    assert!(
        data.is_array() || data.is_object(),
        "expected hot tokens data: {data}"
    );
}

#[test]
fn token_hot_tokens_with_market_cap_filters() {
    let output = run_with_retry(&[
        "token",
        "hot-tokens",
        "--market-cap-min",
        "100000",
        "--market-cap-max",
        "1000000000",
    ]);
    let data = assert_ok_and_extract_data(&output);
    assert!(
        data.is_array() || data.is_object(),
        "expected hot tokens data: {data}"
    );
}

#[test]
fn token_hot_tokens_with_liquidity_filters() {
    let output = run_with_retry(&[
        "token",
        "hot-tokens",
        "--liquidity-min",
        "5000",
        "--liquidity-max",
        "1000000000",
    ]);
    let data = assert_ok_and_extract_data(&output);
    assert!(
        data.is_array() || data.is_object(),
        "expected hot tokens data: {data}"
    );
}

#[test]
fn token_hot_tokens_with_txs_filters() {
    let output = run_with_retry(&[
        "token",
        "hot-tokens",
        "--txs-min",
        "10",
        "--txs-max",
        "1000000",
    ]);
    let data = assert_ok_and_extract_data(&output);
    assert!(
        data.is_array() || data.is_object(),
        "expected hot tokens data: {data}"
    );
}

#[test]
fn token_hot_tokens_with_unique_trader_filters() {
    let output = run_with_retry(&[
        "token",
        "hot-tokens",
        "--unique-trader-min",
        "5",
        "--unique-trader-max",
        "1000000",
    ]);
    let data = assert_ok_and_extract_data(&output);
    assert!(
        data.is_array() || data.is_object(),
        "expected hot tokens data: {data}"
    );
}

#[test]
fn token_hot_tokens_with_holder_filters() {
    let output = run_with_retry(&[
        "token",
        "hot-tokens",
        "--holders-min",
        "100",
        "--holders-max",
        "10000000",
    ]);
    let data = assert_ok_and_extract_data(&output);
    assert!(
        data.is_array() || data.is_object(),
        "expected hot tokens data: {data}"
    );
}

#[test]
fn token_hot_tokens_with_inflow_filters() {
    let output = run_with_retry(&[
        "token",
        "hot-tokens",
        "--inflow-min",
        "0",
        "--inflow-max",
        "1000000000",
    ]);
    let data = assert_ok_and_extract_data(&output);
    assert!(
        data.is_array() || data.is_object(),
        "expected hot tokens data: {data}"
    );
}

#[test]
fn token_hot_tokens_with_fdv_filters() {
    let output = run_with_retry(&[
        "token",
        "hot-tokens",
        "--fdv-min",
        "100000",
        "--fdv-max",
        "1000000000000",
    ]);
    let data = assert_ok_and_extract_data(&output);
    assert!(
        data.is_array() || data.is_object(),
        "expected hot tokens data: {data}"
    );
}

#[test]
fn token_hot_tokens_with_holder_percent_filters() {
    let output = run_with_retry(&[
        "token",
        "hot-tokens",
        "--top10-hold-percent-min",
        "0",
        "--top10-hold-percent-max",
        "100",
        "--dev-hold-percent-min",
        "0",
        "--dev-hold-percent-max",
        "50",
        "--bundle-hold-percent-min",
        "0",
        "--bundle-hold-percent-max",
        "50",
        "--suspicious-hold-percent-min",
        "0",
        "--suspicious-hold-percent-max",
        "50",
    ]);
    let data = assert_ok_and_extract_data(&output);
    assert!(
        data.is_array() || data.is_object(),
        "expected hot tokens data: {data}"
    );
}

#[test]
fn token_hot_tokens_with_boolean_filters() {
    let output = run_with_retry(&[
        "token",
        "hot-tokens",
        "--chain",
        "solana",
        "--is-lp-burnt",
        "true",
        "--risk-filter",
        "true",
        "--stable-token-filter",
        "true",
    ]);
    let data = assert_ok_and_extract_data(&output);
    assert!(
        data.is_array() || data.is_object(),
        "expected hot tokens data: {data}"
    );
}

#[test]
fn token_hot_tokens_with_xmentioned_filters() {
    let output = run_with_retry(&[
        "token",
        "hot-tokens",
        "--ranking-type",
        "5",
        "--mentioned-count-min",
        "1",
        "--mentioned-count-max",
        "100000",
        "--social-score-min",
        "0",
        "--social-score-max",
        "1000",
    ]);
    let data = assert_ok_and_extract_data(&output);
    assert!(
        data.is_array() || data.is_object(),
        "expected hot tokens data: {data}"
    );
}

#[test]
fn token_hot_tokens_with_protocol_filter() {
    // 120596 = Pump.fun
    let output = run_with_retry(&[
        "token",
        "hot-tokens",
        "--chain",
        "solana",
        "--project-id",
        "120596",
    ]);
    let data = assert_ok_and_extract_data(&output);
    assert!(
        data.is_array() || data.is_object(),
        "expected hot tokens data: {data}"
    );
}

#[test]
fn token_hot_tokens_with_all_filters() {
    let output = run_with_retry(&[
        "token",
        "hot-tokens",
        "--ranking-type",
        "4",
        "--chain",
        "solana",
        "--rank-by",
        "5",
        "--time-frame",
        "4",
        "--risk-filter",
        "true",
        "--stable-token-filter",
        "true",
        "--project-id",
        "120596",
        "--price-change-min",
        "0",
        "--price-change-max",
        "10000",
        "--volume-min",
        "1000",
        "--volume-max",
        "1000000000",
        "--market-cap-min",
        "10000",
        "--market-cap-max",
        "1000000000000",
        "--liquidity-min",
        "1000",
        "--liquidity-max",
        "1000000000",
        "--txs-min",
        "1",
        "--txs-max",
        "10000000",
        "--unique-trader-min",
        "1",
        "--unique-trader-max",
        "1000000",
        "--holders-min",
        "10",
        "--holders-max",
        "100000000",
        "--inflow-min",
        "0",
        "--inflow-max",
        "1000000000",
        "--fdv-min",
        "10000",
        "--fdv-max",
        "1000000000000",
        "--top10-hold-percent-min",
        "0",
        "--top10-hold-percent-max",
        "100",
        "--dev-hold-percent-min",
        "0",
        "--dev-hold-percent-max",
        "100",
        "--bundle-hold-percent-min",
        "0",
        "--bundle-hold-percent-max",
        "100",
        "--suspicious-hold-percent-min",
        "0",
        "--suspicious-hold-percent-max",
        "100",
    ]);
    let data = assert_ok_and_extract_data(&output);
    assert!(
        data.is_array() || data.is_object(),
        "expected hot tokens data: {data}"
    );
}

// ─── advanced-info ─────────────────────────────────────────────────

#[test]
fn token_advanced_info_on_solana() {
    let output = run_with_retry(&[
        "token",
        "advanced-info",
        "--address",
        tokens::SOL_WSOL,
        "--chain",
        "solana",
    ]);
    let data = assert_ok_and_extract_data(&output);
    assert!(data.is_object(), "expected object: {data}");
}

#[test]
fn token_advanced_info_missing_address_fails() {
    onchainos()
        .args(["token", "advanced-info"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("required"));
}

// ─── top-trader ────────────────────────────────────────────────────

#[test]
fn token_top_trader_on_solana() {
    let output = run_with_retry(&[
        "token",
        "top-trader",
        "--address",
        tokens::SOL_WSOL,
        "--chain",
        "solana",
    ]);
    let data = assert_ok_and_extract_data(&output);
    assert!(data.is_array() || data.is_object(), "expected data: {data}");
}

#[test]
fn token_top_trader_with_tag_filter() {
    let output = run_with_retry(&[
        "token",
        "top-trader",
        "--address",
        tokens::SOL_WSOL,
        "--chain",
        "solana",
        "--tag-filter",
        "3",
    ]);
    let data = assert_ok_and_extract_data(&output);
    assert!(data.is_array() || data.is_object(), "expected data: {data}");
}

#[test]
fn token_top_trader_missing_address_fails() {
    onchainos()
        .args(["token", "top-trader"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("required"));
}

// ─── trades ──────────────────────────────────────────────────────────

#[test]
fn token_trades_returns_data() {
    let output = run_with_retry(&[
        "token",
        "trades",
        "--address",
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

#[test]
fn token_trades_with_tag_filter() {
    let output = run_with_retry(&[
        "token",
        "trades",
        "--address",
        tokens::SOL_WSOL,
        "--chain",
        "solana",
        "--limit",
        "5",
        "--tag-filter",
        "1",
    ]);
    let data = assert_ok_and_extract_data(&output);
    assert!(
        data.is_array() || data.is_object(),
        "trades data should be array or object: {data}"
    );
}

#[test]
fn token_trades_missing_address_fails() {
    onchainos()
        .args(["token", "trades"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("required"));
}
