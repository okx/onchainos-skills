//! Integration tests for `onchainos security` commands.
//!
//! These tests verify CLI argument validation and error messages.
//! All tests are read-only and do not modify any state.
//! Network-dependent tests (actual API calls) are skipped if the API returns auth errors.

mod common;

use common::{assert_ok_and_extract_data, onchainos, run_with_retry, tokens};
use predicates::prelude::*;

// ── token-scan: argument validation ─────────────────────────────────────────

#[test]
fn token_scan_missing_all_args_fails_with_actionable_message() {
    // No --tokens, no --address, no login → should fail with login guidance
    // (or succeed if user is actually logged in, which is fine)
    let output = onchainos()
        .args(["security", "token-scan"])
        .output()
        .unwrap();

    if !output.status.success() {
        let stdout = String::from_utf8_lossy(&output.stdout);
        assert!(
            stdout.contains("login") || stdout.contains("--address"),
            "error message should mention login or --address: {stdout}"
        );
    }
}

#[test]
fn token_scan_tokens_and_address_are_mutually_exclusive() {
    onchainos()
        .args([
            "security",
            "token-scan",
            "--tokens",
            "1:0xabc",
            "--address",
            "0xdef",
        ])
        .assert()
        .failure()
        .stderr(predicate::str::contains("cannot be used"));
}

#[test]
fn token_scan_invalid_token_format_no_colon() {
    onchainos()
        .args(["security", "token-scan", "--tokens", "1_0xabc"])
        .assert()
        .failure()
        .stdout(predicate::str::contains("Invalid token format"));
}

#[test]
fn token_scan_exceeds_batch_limit_fails() {
    // Build 51 tokens (over the BATCH_SIZE=50 limit)
    let tokens: String = (0..51)
        .map(|i| format!("1:0x{:040x}", i))
        .collect::<Vec<_>>()
        .join(",");
    onchainos()
        .args(["security", "token-scan", "--tokens", &tokens])
        .assert()
        .failure()
        .stdout(predicate::str::contains("at most 50"));
}

#[test]
fn token_scan_empty_tokens_string_fails() {
    onchainos()
        .args(["security", "token-scan", "--tokens", ""])
        .assert()
        .failure()
        .stdout(predicate::str::contains("Invalid token format"));
}

// ── dapp-scan: argument validation ──────────────────────────────────────────

#[test]
fn dapp_scan_missing_domain_fails() {
    onchainos()
        .args(["security", "dapp-scan"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("--domain"));
}

// ── tx-scan: argument validation ─────────────────────────────────────────────

#[test]
fn tx_scan_missing_required_args_fails() {
    onchainos()
        .args(["security", "tx-scan"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("required"));
}

#[test]
fn tx_scan_evm_missing_data_fails() {
    onchainos()
        .args([
            "security",
            "tx-scan",
            "--chain",
            "ethereum",
            "--from",
            "0xd8dA6BF26964aF9D7eEd9e03E53415D37aA96045",
        ])
        .assert()
        .failure()
        .stdout(predicate::str::contains("--data is required"));
}

#[test]
fn tx_scan_solana_missing_encoding_fails() {
    onchainos()
        .args([
            "security",
            "tx-scan",
            "--chain",
            "solana",
            "--from",
            "EeBCkp5j17U5Fg4bEiboHvRrUvQ4LP9AdioQwPg5wF43",
        ])
        .assert()
        .failure()
        .stdout(predicate::str::contains("--encoding is required"));
}

#[test]
fn tx_scan_unsupported_chain_fails() {
    onchainos()
        .args([
            "security", "tx-scan", "--chain", "sui", "--from", "0xabc", "--data", "0x",
        ])
        .assert()
        .failure()
        .stdout(predicate::str::contains("not supported"));
}

// ── sig-scan: argument validation ────────────────────────────────────────────

#[test]
fn sig_scan_missing_required_args_fails() {
    onchainos()
        .args(["security", "sig-scan"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("required"));
}

#[test]
fn sig_scan_invalid_method_fails() {
    onchainos()
        .args([
            "security",
            "sig-scan",
            "--chain",
            "ethereum",
            "--from",
            "0xd8dA6BF26964aF9D7eEd9e03E53415D37aA96045",
            "--sig-method",
            "invalid_method",
            "--message",
            "hello",
        ])
        .assert()
        .failure()
        .stdout(predicate::str::contains("Invalid --sig-method"));
}

#[test]
fn sig_scan_solana_chain_rejected() {
    onchainos()
        .args([
            "security",
            "sig-scan",
            "--chain",
            "solana",
            "--from",
            "EeBCkp5j17U5Fg4bEiboHvRrUvQ4LP9AdioQwPg5wF43",
            "--sig-method",
            "personal_sign",
            "--message",
            "hello",
        ])
        .assert()
        .failure()
        .stdout(predicate::str::contains("not supported"));
}

// ── token-scan: --trade-direction action classification (WWINFRA-3509 SEC) ──
//
// Integration-plan rows IT-001 … IT-004. `security token-scan` is an
// anonymous/AK endpoint, so no wallet login is required. IT-001/002/003 are
// `live` golden/edge rows routed through the shared `run_with_retry` helper;
// IT-004 is an `offline` clap value-validation error (no retry helper).

/// `chainIndex:address` token arg for USDC on ethereum, as consumed by
/// `security token-scan --tokens`.
fn usdc_eth_token_arg() -> String {
    format!("1:{}", tokens::ETH_USDC)
}

/// IT-001 — buying a token returns the object wrapper with `tradeDirection`
/// echoed as `"buy"`. Verifies the SEC classification wiring (spec §2.1); the
/// riskLevel×direction matrix values themselves are unit-tested in
/// `risk_classify.rs`.
#[test]
fn token_scan_trade_direction_buy_echoes_object_wrapper() {
    let token_arg = usdc_eth_token_arg();
    let output = run_with_retry(&[
        "security",
        "token-scan",
        "--tokens",
        token_arg.as_str(),
        "--trade-direction",
        "buy",
    ]);
    let data = assert_ok_and_extract_data(&output);
    assert_eq!(
        data.get("tradeDirection").and_then(|v| v.as_str()),
        Some("buy"),
        "expected data.tradeDirection == \"buy\", got: {data}"
    );
}

/// IT-002 — selling a token returns the object wrapper with `tradeDirection`
/// echoed as `"sell"`. Sell-direction wrapper + combinedAction reduction wiring
/// (spec §2.1); matrix values are unit-tested.
#[test]
fn token_scan_trade_direction_sell_echoes_object_wrapper() {
    let token_arg = usdc_eth_token_arg();
    let output = run_with_retry(&[
        "security",
        "token-scan",
        "--tokens",
        token_arg.as_str(),
        "--trade-direction",
        "sell",
    ]);
    let data = assert_ok_and_extract_data(&output);
    assert_eq!(
        data.get("tradeDirection").and_then(|v| v.as_str()),
        Some("sell"),
        "expected data.tradeDirection == \"sell\", got: {data}"
    );
}

/// IT-003 — without `--trade-direction`, the scan returns the raw backend array
/// unchanged (backward-compat guard, spec §1.1/§2.1): `data` is an array with
/// no object wrapper.
#[test]
fn token_scan_without_trade_direction_returns_raw_array() {
    let token_arg = usdc_eth_token_arg();
    let output = run_with_retry(&["security", "token-scan", "--tokens", token_arg.as_str()]);
    let data = assert_ok_and_extract_data(&output);
    let arr = data
        .as_array()
        .unwrap_or_else(|| panic!("expected data to be a raw array, got: {data}"));
    assert!(
        !arr.is_empty(),
        "expected at least one scan result, got: {data}"
    );
}

/// IT-004 — an unknown `--trade-direction` value is rejected by the clap
/// `parse_trade_direction_value` value-parser with a clear input error on
/// stderr. Offline: fails at arg parse before any network call. Exit code is
/// clap's value-validation code (non-zero); we assert failure + the stderr
/// substring per the plan note (the CSV's exit-code column is unreliable here).
#[test]
fn token_scan_invalid_trade_direction_rejected() {
    let token_arg = usdc_eth_token_arg();
    onchainos()
        .args([
            "security",
            "token-scan",
            "--tokens",
            token_arg.as_str(),
            "--trade-direction",
            "sideways",
        ])
        .assert()
        .failure()
        .stderr(predicate::str::contains("invalid value"));
}
