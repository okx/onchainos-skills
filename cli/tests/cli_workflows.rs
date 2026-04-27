//! Integration tests for `onchainos workflow` commands.
//!
//! These tests run the compiled binary against the live OKX API, so they
//! require network access and valid API credentials.
//!
//! Workflows tested: token-research (W1), smart-money (W3), new-tokens (W4),
//! wallet-analysis (W5), portfolio (W7).

mod common;

use common::{assert_ok_and_extract_data, onchainos, run_with_retry, tokens};

// ── W1: token-research ───────────────────────────────────────────────────────

#[test]
fn workflow_token_research_returns_ok() {
    let output = run_with_retry(&[
        "workflow",
        "token-research",
        "--address",
        tokens::SOL_BONK,
        "--chain",
        "solana",
    ]);
    assert_ok_and_extract_data(&output);
}

#[test]
fn workflow_token_research_has_workflow_discriminator() {
    let output = run_with_retry(&[
        "workflow",
        "token-research",
        "--address",
        tokens::SOL_BONK,
        "--chain",
        "solana",
    ]);
    let data = assert_ok_and_extract_data(&output);
    assert_eq!(
        data["workflow"], "token-research",
        "missing workflow discriminator"
    );
}

#[test]
fn workflow_token_research_has_core_fields() {
    let output = run_with_retry(&[
        "workflow",
        "token-research",
        "--address",
        tokens::SOL_BONK,
        "--chain",
        "solana",
    ]);
    let data = assert_ok_and_extract_data(&output);
    let core = &data["core"];
    assert!(!core.is_null(), "core block missing");
    // At least one of these should be present — a total failure would have errored
    let has_any = ["info", "price", "contract", "security"]
        .iter()
        .any(|f| !core[f].is_null());
    assert!(
        has_any,
        "all core fields null — expected at least one to succeed: {core}"
    );
}

#[test]
fn workflow_token_research_has_structure_block() {
    let output = run_with_retry(&[
        "workflow",
        "token-research",
        "--address",
        tokens::SOL_BONK,
        "--chain",
        "solana",
    ]);
    let data = assert_ok_and_extract_data(&output);
    assert!(!data["structure"].is_null(), "structure block missing");
}

#[test]
fn workflow_token_research_non_launchpad_has_null_launchpad() {
    // BONK is not a pump.fun token — Step 3 should be skipped
    let output = run_with_retry(&[
        "workflow",
        "token-research",
        "--address",
        tokens::SOL_BONK,
        "--chain",
        "solana",
    ]);
    let data = assert_ok_and_extract_data(&output);
    assert!(
        data["launchpad"].is_null(),
        "expected null launchpad for non-launchpad token, got: {}",
        data["launchpad"]
    );
}

#[test]
fn workflow_token_research_address_and_chain_in_output() {
    let output = run_with_retry(&[
        "workflow",
        "token-research",
        "--address",
        tokens::SOL_USDC,
        "--chain",
        "solana",
    ]);
    let data = assert_ok_and_extract_data(&output);
    assert_eq!(data["address"], tokens::SOL_USDC);
    assert_eq!(data["chain"], "501"); // solana resolves to 501
}

#[test]
fn workflow_token_research_missing_address_fails() {
    onchainos()
        .args(["workflow", "token-research"])
        .assert()
        .failure();
}

// ── W3: smart-money ──────────────────────────────────────────────────────────

#[test]
fn workflow_smart_money_returns_ok() {
    let output = run_with_retry(&["workflow", "smart-money", "--chain", "solana"]);
    assert_ok_and_extract_data(&output);
}

#[test]
fn workflow_smart_money_has_workflow_discriminator() {
    let output = run_with_retry(&["workflow", "smart-money", "--chain", "solana"]);
    let data = assert_ok_and_extract_data(&output);
    assert_eq!(data["workflow"], "smart-money");
}

#[test]
fn workflow_smart_money_has_top_tokens_field() {
    let output = run_with_retry(&["workflow", "smart-money", "--chain", "solana"]);
    let data = assert_ok_and_extract_data(&output);
    assert!(
        data["topTokens"].is_array(),
        "topTokens should be an array: {}",
        data["topTokens"]
    );
}

#[test]
fn workflow_smart_money_has_raw_signals_field() {
    let output = run_with_retry(&["workflow", "smart-money", "--chain", "solana"]);
    let data = assert_ok_and_extract_data(&output);
    // rawSignals may be null if signal API is unavailable, but field must be present
    assert!(
        data.get("rawSignals").is_some(),
        "rawSignals field missing from output"
    );
}

#[test]
fn workflow_smart_money_top_tokens_have_required_fields() {
    let output = run_with_retry(&["workflow", "smart-money", "--chain", "solana"]);
    let data = assert_ok_and_extract_data(&output);
    let tokens = data["topTokens"].as_array().unwrap();
    if tokens.is_empty() {
        return; // no signals available — skip field validation
    }
    for token in tokens {
        assert!(
            token.get("address").is_some(),
            "token entry missing 'address'"
        );
        let d = &token["data"];
        assert!(d.get("signal").is_some(), "token data missing 'signal'");
        assert!(d.get("price").is_some(), "token data missing 'price'");
        assert!(d.get("contract").is_some(), "token data missing 'contract'");
        assert!(d.get("security").is_some(), "token data missing 'security'");
        assert!(
            d.get("launchpad").is_some(),
            "token data missing 'launchpad'"
        );
    }
}

// ── W4: new-tokens ───────────────────────────────────────────────────────────

#[test]
fn workflow_new_tokens_returns_ok() {
    let output = run_with_retry(&[
        "workflow",
        "new-tokens",
        "--chain",
        "solana",
        "--stage",
        "MIGRATED",
    ]);
    assert_ok_and_extract_data(&output);
}

#[test]
fn workflow_new_tokens_has_workflow_discriminator() {
    let output = run_with_retry(&["workflow", "new-tokens", "--chain", "solana"]);
    let data = assert_ok_and_extract_data(&output);
    assert_eq!(data["workflow"], "new-tokens");
}

#[test]
fn workflow_new_tokens_has_stage_field() {
    let output = run_with_retry(&[
        "workflow",
        "new-tokens",
        "--chain",
        "solana",
        "--stage",
        "MIGRATED",
    ]);
    let data = assert_ok_and_extract_data(&output);
    assert_eq!(data["stage"], "MIGRATED");
}

#[test]
fn workflow_new_tokens_has_enriched_array() {
    let output = run_with_retry(&["workflow", "new-tokens", "--chain", "solana"]);
    let data = assert_ok_and_extract_data(&output);
    assert!(
        data["enriched"].is_array(),
        "enriched should be an array: {}",
        data["enriched"]
    );
}

#[test]
fn workflow_new_tokens_enriched_items_have_required_fields() {
    let output = run_with_retry(&[
        "workflow",
        "new-tokens",
        "--chain",
        "solana",
        "--stage",
        "MIGRATED",
    ]);
    let data = assert_ok_and_extract_data(&output);
    let enriched = data["enriched"].as_array().unwrap();
    if enriched.is_empty() {
        return; // no migrated tokens in window — skip field validation
    }
    for item in enriched {
        assert!(
            item.get("address").is_some(),
            "enriched item missing 'address'"
        );
        let d = &item["data"];
        assert!(d.get("token").is_some(), "enriched data missing 'token'");
        assert!(
            d.get("security").is_some(),
            "enriched data missing 'security'"
        );
        assert!(
            d.get("contract").is_some(),
            "enriched data missing 'contract'"
        );
        assert!(
            d.get("devInfo").is_some(),
            "enriched data missing 'devInfo'"
        );
        assert!(
            d.get("bundleInfo").is_some(),
            "enriched data missing 'bundleInfo'"
        );
    }
}

// ── W5: wallet-analysis ──────────────────────────────────────────────────────

#[test]
fn workflow_wallet_analysis_returns_ok() {
    let output = run_with_retry(&[
        "workflow",
        "wallet-analysis",
        "--address",
        tokens::ETH_VITALIK,
        "--chain",
        "ethereum",
    ]);
    assert_ok_and_extract_data(&output);
}

#[test]
fn workflow_wallet_analysis_has_workflow_discriminator() {
    let output = run_with_retry(&[
        "workflow",
        "wallet-analysis",
        "--address",
        tokens::ETH_VITALIK,
        "--chain",
        "ethereum",
    ]);
    let data = assert_ok_and_extract_data(&output);
    assert_eq!(data["workflow"], "wallet-analysis");
}

#[test]
fn workflow_wallet_analysis_has_performance_block() {
    let output = run_with_retry(&[
        "workflow",
        "wallet-analysis",
        "--address",
        tokens::ETH_VITALIK,
        "--chain",
        "ethereum",
    ]);
    let data = assert_ok_and_extract_data(&output);
    let perf = &data["performance"];
    assert!(!perf.is_null(), "performance block missing");
    assert!(perf.get("7d").is_some(), "performance.7d missing");
    assert!(perf.get("30d").is_some(), "performance.30d missing");
}

#[test]
fn workflow_wallet_analysis_has_address_and_chain() {
    let output = run_with_retry(&[
        "workflow",
        "wallet-analysis",
        "--address",
        tokens::ETH_VITALIK,
        "--chain",
        "ethereum",
    ]);
    let data = assert_ok_and_extract_data(&output);
    assert_eq!(data["address"], tokens::ETH_VITALIK);
    assert_eq!(data["chain"], "1"); // ethereum = chainIndex 1
}

#[test]
fn workflow_wallet_analysis_has_balances_pnl_activities() {
    let output = run_with_retry(&[
        "workflow",
        "wallet-analysis",
        "--address",
        tokens::ETH_VITALIK,
        "--chain",
        "ethereum",
    ]);
    let data = assert_ok_and_extract_data(&output);
    assert!(data.get("balances").is_some(), "balances field missing");
    assert!(data.get("recentPnl").is_some(), "recentPnl field missing");
    assert!(data.get("activities").is_some(), "activities field missing");
}

#[test]
fn workflow_wallet_analysis_missing_address_fails() {
    onchainos()
        .args(["workflow", "wallet-analysis"])
        .assert()
        .failure();
}

// ── W7: portfolio ─────────────────────────────────────────────────────────────

#[test]
fn workflow_portfolio_returns_ok() {
    let output = run_with_retry(&["workflow", "portfolio", "--address", tokens::ETH_VITALIK]);
    assert_ok_and_extract_data(&output);
}

#[test]
fn workflow_portfolio_has_workflow_discriminator() {
    let output = run_with_retry(&["workflow", "portfolio", "--address", tokens::ETH_VITALIK]);
    let data = assert_ok_and_extract_data(&output);
    assert_eq!(data["workflow"], "portfolio");
}

#[test]
fn workflow_portfolio_has_address_and_chains() {
    let output = run_with_retry(&[
        "workflow",
        "portfolio",
        "--address",
        tokens::ETH_VITALIK,
        "--chains",
        "ethereum",
    ]);
    let data = assert_ok_and_extract_data(&output);
    assert_eq!(data["address"], tokens::ETH_VITALIK);
    assert!(data.get("chains").is_some(), "chains field missing");
}

#[test]
fn workflow_portfolio_has_balances_total_value_overview() {
    let output = run_with_retry(&[
        "workflow",
        "portfolio",
        "--address",
        tokens::ETH_VITALIK,
        "--chains",
        "ethereum",
    ]);
    let data = assert_ok_and_extract_data(&output);
    assert!(data.get("balances").is_some(), "balances field missing");
    assert!(data.get("totalValue").is_some(), "totalValue field missing");
    assert!(data.get("overview").is_some(), "overview field missing");
}

#[test]
fn workflow_portfolio_missing_address_fails() {
    onchainos()
        .args(["workflow", "portfolio"])
        .assert()
        .failure();
}

// ── subcommand routing ────────────────────────────────────────────────────────

#[test]
fn workflow_unknown_subcommand_fails() {
    onchainos()
        .args(["workflow", "nonexistent"])
        .assert()
        .failure();
}

#[test]
fn workflow_with_no_subcommand_prints_help() {
    onchainos().args(["workflow"]).assert().failure(); // clap exits non-zero when no subcommand given
}
