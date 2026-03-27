//! Integration tests for `onchainos defi` commands.
//!
//! Tests cover command registration and basic search functionality.
//! Transaction-generating commands (prepare, enter) require valid product IDs
//! and wallet addresses — add targeted tests once API docs are finalized.

mod common;

use common::{assert_ok_and_extract_data, onchainos, run_with_retry};
use predicates::prelude::*;

// ─── search ────────────────────────────────────────────────────────

#[test]
fn defi_search_returns_results() {
    let output = run_with_retry(&["defi", "search", "--token", "USDC", "--chain", "ethereum"]);
    let data = assert_ok_and_extract_data(&output);
    // search may return array or object depending on API response
    assert!(
        data.is_array() || data.is_object(),
        "expected array or object: {data}"
    );
}

#[test]
fn defi_search_with_platform_filter() {
    let output = run_with_retry(&[
        "defi",
        "search",
        "--token",
        "USDC",
        "--platform",
        "Aave",
        "--chain",
        "ethereum",
    ]);
    let data = assert_ok_and_extract_data(&output);
    assert!(
        data.is_array() || data.is_object(),
        "expected array or object: {data}"
    );
}

#[test]
fn defi_search_with_product_group() {
    let output = run_with_retry(&[
        "defi",
        "search",
        "--token",
        "USDC",
        "--product-group",
        "SINGLE_EARN",
        "--chain",
        "ethereum",
    ]);
    let data = assert_ok_and_extract_data(&output);
    assert!(
        data.is_array() || data.is_object(),
        "expected array or object: {data}"
    );
}

#[test]
fn defi_search_without_token_or_platform_fails() {
    onchainos()
        .args(["defi", "search", "--chain", "ethereum"])
        .assert()
        .failure()
        .stdout(predicate::str::contains(
            "at least one of --token or --platform",
        ));
}

// ─── detail ────────────────────────────────────────────────────────

#[test]
fn defi_detail_missing_required_args_fails() {
    onchainos()
        .args(["defi", "detail"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("required"));
}

// ─── prepare ───────────────────────────────────────────────────────

#[test]
fn defi_prepare_missing_required_args_fails() {
    onchainos()
        .args(["defi", "prepare"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("required"));
}

// ─── deposit ───────────────────────────────────────────────────────

#[test]
fn defi_deposit_missing_required_args_fails() {
    onchainos()
        .args(["defi", "deposit", "--investment-id", "123"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("required"));
}

// ─── redeem ────────────────────────────────────────────────────────

#[test]
fn defi_redeem_missing_required_args_fails() {
    // --id and --address are both required
    onchainos()
        .args(["defi", "redeem", "--id", "test123"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("required"));
}

#[test]
fn defi_redeem_missing_id_fails() {
    onchainos()
        .args([
            "defi",
            "redeem",
            "--address",
            "0xd8dA6BF26964aF9D7eEd9e03E53415D37aA96045",
        ])
        .assert()
        .failure()
        .stderr(predicate::str::contains("required"));
}

#[test]
fn defi_redeem_missing_address_fails() {
    onchainos()
        .args(["defi", "redeem", "--id", "test123"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("required"));
}

// ─── claim ─────────────────────────────────────────────────────────

#[test]
fn defi_claim_missing_required_args_fails() {
    // --address and --reward-type are both required
    onchainos()
        .args([
            "defi",
            "claim",
            "--address",
            "0xd8dA6BF26964aF9D7eEd9e03E53415D37aA96045",
        ])
        .assert()
        .failure()
        .stderr(predicate::str::contains("required"));
}

#[test]
fn defi_claim_missing_address_fails() {
    onchainos()
        .args(["defi", "claim", "--reward-type", "REWARD_INVESTMENT"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("required"));
}

#[test]
fn defi_claim_missing_reward_type_fails() {
    onchainos()
        .args([
            "defi",
            "claim",
            "--address",
            "0xd8dA6BF26964aF9D7eEd9e03E53415D37aA96045",
        ])
        .assert()
        .failure()
        .stderr(predicate::str::contains("required"));
}

// ─── positions ─────────────────────────────────────────────────────

#[test]
fn defi_positions_missing_wallet_fails() {
    onchainos()
        .args(["defi", "positions", "--chains", "ethereum"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("required"));
}

#[test]
fn defi_positions_missing_chains_fails() {
    onchainos()
        .args([
            "defi",
            "positions",
            "--address",
            "0xd8dA6BF26964aF9D7eEd9e03E53415D37aA96045",
        ])
        .assert()
        .failure()
        .stderr(predicate::str::contains("required"));
}

#[test]
fn defi_positions_single_chain_returns_ok() {
    let output = run_with_retry(&[
        "defi",
        "positions",
        "--address",
        "0xd8dA6BF26964aF9D7eEd9e03E53415D37aA96045",
        "--chains",
        "ethereum",
    ]);
    let data = assert_ok_and_extract_data(&output);
    assert!(
        data.is_array() || data.is_object(),
        "expected array or object, got: {data}"
    );
}

#[test]
fn defi_positions_multi_chain_returns_ok() {
    let output = run_with_retry(&[
        "defi",
        "positions",
        "--address",
        "0xd8dA6BF26964aF9D7eEd9e03E53415D37aA96045",
        "--chains",
        "ethereum,bsc",
    ]);
    let data = assert_ok_and_extract_data(&output);
    assert!(
        data.is_array() || data.is_object(),
        "expected array or object, got: {data}"
    );
}

// ─── position-detail ───────────────────────────────────────────────

#[test]
fn defi_position_detail_missing_wallet_fails() {
    onchainos()
        .args([
            "defi",
            "position-detail",
            "--chain",
            "ethereum",
            "--platform-id",
            "44",
        ])
        .assert()
        .failure()
        .stderr(predicate::str::contains("required"));
}

#[test]
fn defi_position_detail_missing_chain_fails() {
    onchainos()
        .args([
            "defi",
            "position-detail",
            "--address",
            "0xd8dA6BF26964aF9D7eEd9e03E53415D37aA96045",
            "--platform-id",
            "44",
        ])
        .assert()
        .failure()
        .stderr(predicate::str::contains("required"));
}

#[test]
fn defi_position_detail_missing_platform_id_fails() {
    onchainos()
        .args([
            "defi",
            "position-detail",
            "--address",
            "0xd8dA6BF26964aF9D7eEd9e03E53415D37aA96045",
            "--chain",
            "ethereum",
        ])
        .assert()
        .failure()
        .stderr(predicate::str::contains("required"));
}

// ─── help ──────────────────────────────────────────────────────────

#[test]
fn defi_help_shows_subcommands() {
    onchainos()
        .args(["defi", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("search"))
        .stdout(predicate::str::contains("detail"))
        .stdout(predicate::str::contains("prepare"))
        .stdout(predicate::str::contains("deposit"))
        .stdout(predicate::str::contains("redeem"))
        .stdout(predicate::str::contains("claim"))
        .stdout(predicate::str::contains("positions"))
        .stdout(predicate::str::contains("position-detail"));
}
