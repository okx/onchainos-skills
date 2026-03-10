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

struct LiveMemepumpToken {
    token_address: String,
    creator_address: Option<String>,
    protocol_id: Option<String>,
    quote_token_address: Option<String>,
}

// ─── price ──────────────────────────────────────────────────────────

#[test]
fn market_price_eth_native() {
    let output = run_with_retry(&["market", "price", tokens::EVM_NATIVE, "--chain", "ethereum"]);
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
    let output = run_with_retry(&["market", "index", tokens::EVM_NATIVE, "--chain", "ethereum"]);
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
fn market_signal_list_with_all_filters() {
    let output = run_with_retry(&[
        "market",
        "signal-list",
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
        "--token-address",
        tokens::SOL_WSOL,
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
fn memepump_tokens_with_protocol_filter() {
    let output = run_with_retry(&[
        "market",
        "memepump-tokens",
        "solana",
        "--stage",
        "NEW",
        "--protocol-id-list",
        "120596",
    ]);
    let data = assert_ok_and_extract_data(&output);
    assert!(
        data.is_array() || data.is_object(),
        "data should be array or object: {data}"
    );
}

#[test]
fn memepump_tokens_with_age_filter() {
    let output = run_with_retry(&[
        "market",
        "memepump-tokens",
        "solana",
        "--stage",
        "NEW",
        "--min-token-age",
        "5",
        "--max-token-age",
        "120",
    ]);
    let data = assert_ok_and_extract_data(&output);
    assert!(
        data.is_array() || data.is_object(),
        "data should be array or object: {data}"
    );
}

#[test]
fn memepump_tokens_with_social_filters() {
    let output = run_with_retry(&[
        "market",
        "memepump-tokens",
        "solana",
        "--stage",
        "MIGRATED",
        "--has-x",
        "true",
    ]);
    let data = assert_ok_and_extract_data(&output);
    assert!(
        data.is_array() || data.is_object(),
        "data should be array or object: {data}"
    );
}

#[test]
fn memepump_tokens_with_holder_filters() {
    let output = run_with_retry(&[
        "market",
        "memepump-tokens",
        "solana",
        "--stage",
        "MIGRATED",
        "--min-top10-holdings-percent",
        "10",
    ]);
    let data = assert_ok_and_extract_data(&output);
    assert!(
        data.is_array() || data.is_object(),
        "data should be array or object: {data}"
    );
}

#[test]
fn memepump_tokens_live_on_pump_fun() {
    let output = run_with_retry(&[
        "market",
        "memepump-tokens",
        "solana",
        "--stage",
        "NEW",
        "--live-on-pump-fun",
        "true",
    ]);
    let data = assert_ok_and_extract_data(&output);
    assert!(
        data.is_array() || data.is_object(),
        "data should be array or object: {data}"
    );
}

#[test]
fn memepump_tokens_migrating_defaults_to_bonding_desc_order() {
    let output = run_with_retry(&[
        "market",
        "memepump-tokens",
        "solana",
        "--stage",
        "MIGRATING",
        "--keywords-include",
        "bonk",
    ]);
    let data = assert_ok_and_extract_data(&output);
    let tokens = if data.is_array() {
        data.as_array()
    } else {
        data.get("data").and_then(|d| d.as_array())
    };

    let Some(tokens) = tokens else {
        panic!("data should be array or object with nested array: {data}");
    };

    if tokens.len() < 2 {
        eprintln!(
            "SKIP: expected at least two migrating bonk tokens, got {}",
            tokens.len()
        );
        return;
    }

    let first = &tokens[0];
    let second = &tokens[1];
    let first_bonding: f64 = first["bondingPercent"]
        .as_str()
        .expect("first token missing bondingPercent")
        .parse()
        .expect("first bondingPercent should parse as f64");
    let second_bonding: f64 = second["bondingPercent"]
        .as_str()
        .expect("second token missing bondingPercent")
        .parse()
        .expect("second bondingPercent should parse as f64");

    assert!(
        first_bonding >= second_bonding,
        "expected MIGRATING list to default to bondingPercent desc, got first={} second={} first_token={} second_token={}",
        first_bonding,
        second_bonding,
        first,
        second
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

fn fetch_first_memepump_token(chain: &str) -> Option<LiveMemepumpToken> {
    let output = assert_cmd::Command::from(cargo_bin_cmd!("onchainos"))
        .args(["market", "memepump-tokens", chain, "--stage", "MIGRATED"])
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

    let token = tokens.and_then(|arr| arr.first())?;

    let token_address = token
        .get("tokenAddress")
        .or_else(|| token.get("tokenContractAddress"))
        .and_then(|v| v.as_str())?
        .to_string();

    Some(LiveMemepumpToken {
        token_address,
        creator_address: token
            .get("creatorAddress")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string()),
        protocol_id: token
            .get("protocolId")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string()),
        quote_token_address: token
            .get("quoteTokenAddress")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string()),
    })
}

// ─── memepump-token-details ─────────────────────────────────────────

#[test]
fn memepump_token_details_with_real_token() {
    let token = match fetch_first_memepump_token("solana") {
        Some(token) => token,
        None => {
            eprintln!("SKIP: could not fetch a live memepump token address");
            return;
        }
    };

    let output = run_with_retry(&[
        "market",
        "memepump-token-details",
        &token.token_address,
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
fn memepump_token_details_with_wallet() {
    let token = match fetch_first_memepump_token("solana") {
        Some(token) => token,
        None => {
            eprintln!("SKIP: could not fetch a live memepump token address");
            return;
        }
    };

    let wallet = match token.creator_address.as_deref() {
        Some(wallet) => wallet,
        None => {
            eprintln!("SKIP: live memepump token missing creator address");
            return;
        }
    };

    let output = run_with_retry(&[
        "market",
        "memepump-token-details",
        &token.token_address,
        "--chain",
        "solana",
        "--wallet",
        wallet,
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
    let token = match fetch_first_memepump_token("solana") {
        Some(token) => token,
        None => {
            eprintln!("SKIP: could not fetch a live memepump token address");
            return;
        }
    };

    let output = run_with_retry(&[
        "market",
        "memepump-token-dev-info",
        &token.token_address,
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
    let token = match fetch_first_memepump_token("solana") {
        Some(token) => token,
        None => {
            eprintln!("SKIP: could not fetch a live memepump token address");
            return;
        }
    };

    let output = run_with_retry(&[
        "market",
        "memepump-similar-tokens",
        &token.token_address,
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
    let token = match fetch_first_memepump_token("solana") {
        Some(token) => token,
        None => {
            eprintln!("SKIP: could not fetch a live memepump token address");
            return;
        }
    };

    let output = run_with_retry(&[
        "market",
        "memepump-token-bundle-info",
        &token.token_address,
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
    let token = match fetch_first_memepump_token("solana") {
        Some(token) => token,
        None => {
            eprintln!("SKIP: could not fetch a live memepump token address");
            return;
        }
    };

    let output = run_with_retry(&[
        "market",
        "memepump-aped-wallet",
        &token.token_address,
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
fn memepump_aped_wallet_with_wallet() {
    let token = match fetch_first_memepump_token("solana") {
        Some(token) => token,
        None => {
            eprintln!("SKIP: could not fetch a live memepump token address");
            return;
        }
    };

    let wallet = match token.creator_address.as_deref() {
        Some(wallet) => wallet,
        None => {
            eprintln!("SKIP: live memepump token missing creator address");
            return;
        }
    };

    let output = run_with_retry(&[
        "market",
        "memepump-aped-wallet",
        &token.token_address,
        "--chain",
        "solana",
        "--wallet",
        wallet,
    ]);
    let data = assert_ok_and_extract_data(&output);
    assert!(
        data.is_object() || data.is_array(),
        "expected aped wallet data: {data}"
    );
}

#[test]
fn memepump_tokens_with_all_optional_filters() {
    let token = match fetch_first_memepump_token("solana") {
        Some(token) => token,
        None => {
            eprintln!("SKIP: could not fetch a live memepump token");
            return;
        }
    };

    let wallet = token.creator_address.as_deref().unwrap_or(tokens::SOL_WSOL);
    let protocol_id = token.protocol_id.as_deref().unwrap_or("120596");
    let quote_token = token
        .quote_token_address
        .as_deref()
        .unwrap_or(tokens::SOL_WSOL);

    let output = run_with_retry(&[
        "market",
        "memepump-tokens",
        "solana",
        "--stage",
        "MIGRATED",
        "--wallet-address",
        wallet,
        "--protocol-id-list",
        protocol_id,
        "--quote-token-address-list",
        quote_token,
        "--min-top10-holdings-percent",
        "0",
        "--max-top10-holdings-percent",
        "100",
        "--min-dev-holdings-percent",
        "0",
        "--max-dev-holdings-percent",
        "100",
        "--min-insiders-percent",
        "0",
        "--max-insiders-percent",
        "100",
        "--min-bundlers-percent",
        "0",
        "--max-bundlers-percent",
        "100",
        "--min-snipers-percent",
        "0",
        "--max-snipers-percent",
        "100",
        "--min-fresh-wallets-percent",
        "0",
        "--max-fresh-wallets-percent",
        "100",
        "--min-suspected-phishing-wallet-percent",
        "0",
        "--max-suspected-phishing-wallet-percent",
        "100",
        "--min-bot-traders",
        "0",
        "--max-bot-traders",
        "1000000",
        "--min-dev-migrated",
        "0",
        "--max-dev-migrated",
        "1000000",
        "--min-market-cap",
        "0",
        "--max-market-cap",
        "1000000000000",
        "--min-volume",
        "0",
        "--max-volume",
        "1000000000000",
        "--min-tx-count",
        "0",
        "--max-tx-count",
        "1000000000",
        "--min-bonding-percent",
        "0",
        "--max-bonding-percent",
        "100",
        "--min-holders",
        "0",
        "--max-holders",
        "1000000000",
        "--min-token-age",
        "0",
        "--max-token-age",
        "1000000000",
        "--min-buy-tx-count",
        "0",
        "--max-buy-tx-count",
        "1000000000",
        "--min-sell-tx-count",
        "0",
        "--max-sell-tx-count",
        "1000000000",
        "--min-token-symbol-length",
        "0",
        "--max-token-symbol-length",
        "100",
        "--has-at-least-one-social-link",
        "false",
        "--has-x",
        "false",
        "--has-telegram",
        "false",
        "--has-website",
        "false",
        "--website-type-list",
        "0,1",
        "--dex-screener-paid",
        "false",
        "--live-on-pump-fun",
        "false",
        "--dev-sell-all",
        "false",
        "--dev-still-holding",
        "false",
        "--community-takeover",
        "false",
        "--bags-fee-claimed",
        "false",
        "--min-fees-native",
        "0",
        "--max-fees-native",
        "1000000000",
        "--keywords-include",
        "dog wif",
        "--keywords-exclude",
        "狗",
    ]);
    let data = assert_ok_and_extract_data(&output);
    assert!(
        data.is_array() || data.is_object(),
        "data should be array or object: {data}"
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
