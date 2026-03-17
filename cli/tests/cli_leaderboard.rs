//! Integration tests for `onchainos leaderboard` commands:
//! supported-chains, list.

mod common;

use common::{assert_ok_and_extract_data, onchainos, run_with_retry};
use predicates::prelude::*;

// ─── supported-chains ───────────────────────────────────────────────

#[test]
fn leaderboard_supported_chains_returns_list() {
    let output = run_with_retry(&["leaderboard", "supported-chains"]);
    let data = assert_ok_and_extract_data(&output);
    assert!(data.is_array(), "expected array of chains: {data}");
    let arr = data.as_array().unwrap();
    assert!(!arr.is_empty(), "expected at least one supported chain");
    let first = &arr[0];
    assert!(
        first.get("chainIndex").is_some(),
        "chain entry missing 'chainIndex': {first}"
    );
    assert!(
        first.get("chainName").is_some(),
        "chain entry missing 'chainName': {first}"
    );
}

// ─── list ───────────────────────────────────────────────────────────

#[test]
fn leaderboard_list_ethereum_pnl_7d() {
    let output = run_with_retry(&[
        "leaderboard",
        "list",
        "--chain",
        "ethereum",
        "--time-frame",
        "3",
        "--sort-by",
        "1",
    ]);
    let data = assert_ok_and_extract_data(&output);
    assert!(
        data.is_array(),
        "expected array of leaderboard entries: {data}"
    );
}

#[test]
fn leaderboard_list_solana_win_rate_1d() {
    let output = run_with_retry(&[
        "leaderboard",
        "list",
        "--chain",
        "solana",
        "--time-frame",
        "1",
        "--sort-by",
        "2",
    ]);
    let data = assert_ok_and_extract_data(&output);
    assert!(
        data.is_array(),
        "expected array of leaderboard entries: {data}"
    );
}

#[test]
fn leaderboard_list_with_wallet_type_filter() {
    let output = run_with_retry(&[
        "leaderboard",
        "list",
        "--chain",
        "ethereum",
        "--time-frame",
        "3",
        "--sort-by",
        "1",
        "--wallet-type",
        "smartMoney",
    ]);
    let data = assert_ok_and_extract_data(&output);
    assert!(data.is_array(), "expected array: {data}");
}

#[test]
fn leaderboard_list_with_pnl_range_filter() {
    let output = run_with_retry(&[
        "leaderboard",
        "list",
        "--chain",
        "ethereum",
        "--time-frame",
        "3",
        "--sort-by",
        "1",
        "--min-realized-pnl-usd",
        "1000",
    ]);
    let data = assert_ok_and_extract_data(&output);
    assert!(data.is_array(), "expected array: {data}");
}

#[test]
fn leaderboard_list_missing_chain_fails() {
    onchainos()
        .args(["leaderboard", "list", "--time-frame", "3", "--sort-by", "1"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("required"));
}

#[test]
fn leaderboard_list_missing_time_frame_fails() {
    onchainos()
        .args([
            "leaderboard",
            "list",
            "--chain",
            "ethereum",
            "--sort-by",
            "1",
        ])
        .assert()
        .failure()
        .stderr(predicate::str::contains("required"));
}

#[test]
fn leaderboard_list_missing_sort_by_fails() {
    onchainos()
        .args([
            "leaderboard",
            "list",
            "--chain",
            "ethereum",
            "--time-frame",
            "3",
        ])
        .assert()
        .failure()
        .stderr(predicate::str::contains("required"));
}
