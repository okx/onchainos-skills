//! Integration tests for `onchainos token` commands (search, info, price-info, trending, holders).

mod common;

use common::{assert_ok_and_extract_data, onchainos, run_with_retry, tokens};
use predicates::prelude::*;

// ─── search ─────────────────────────────────────────────────────────

#[test]
fn token_search_by_symbol() {
    let output = run_with_retry(&["token", "search", "USDC"]);
    let data = assert_ok_and_extract_data(&output);
    assert!(data.is_array(), "expected array of search results: {data}");
    let arr = data.as_array().unwrap();
    assert!(!arr.is_empty(), "expected at least one result for USDC");
}

#[test]
fn token_search_by_address() {
    let output = run_with_retry(&["token", "search", tokens::ETH_USDC, "--chains", "ethereum"]);
    let data = assert_ok_and_extract_data(&output);
    assert!(data.is_array(), "expected array: {data}");
}

#[test]
fn token_search_cross_chain() {
    let output = run_with_retry(&["token", "search", "SOL", "--chains", "solana,ethereum"]);
    let data = assert_ok_and_extract_data(&output);
    assert!(data.is_array(), "expected array: {data}");
}

#[test]
fn token_search_phrase_query() {
    let output = run_with_retry(&["token", "search", "dog wif", "--chains", "solana"]);
    let data = assert_ok_and_extract_data(&output);
    assert!(data.is_array(), "expected array of search results: {data}");
}

#[test]
fn token_search_unicode_query() {
    let output = run_with_retry(&["token", "search", "狗", "--chains", "solana"]);
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
    let output = run_with_retry(&["token", "info", tokens::ETH_USDC, "--chain", "ethereum"]);
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
    let output = run_with_retry(&["token", "info", tokens::SOL_WSOL, "--chain", "solana"]);
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
    let output = run_with_retry(&["token", "holders", tokens::ETH_USDC, "--chain", "ethereum"]);
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
