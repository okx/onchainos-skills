//! Integration tests for `onchainos social` commands (DEXMARKET-7736).
//!
//! Covers the 9 social CLI subcommands: news-latest, news-by-coin, news-search,
//! news-detail, news-platforms, sentiment-ranking, coin-sentiment,
//! token-vibe-timeline, token-top-kols.
//!
//! These tests run the compiled binary against the live OKX API, so they
//! require network access and valid API credentials. Each live call goes
//! through `run_with_retry` for transient rate-limiting.

mod common;

use common::{assert_ok_and_extract_data, onchainos, run_with_retry, tokens};
use predicates::prelude::*;
use serde_json::Value;

// ─── news-latest ────────────────────────────────────────────────────────

#[test]
#[ignore = "live API; enable once Orbit + priapi openapi endpoints ship (DEXMARKET-7736 upstream)"]
fn social_news_latest_returns_articles() {
    let output = run_with_retry(&["social", "news-latest", "--limit", "5"]);
    let data = assert_ok_and_extract_data(&output);
    let articles = data
        .get("articles")
        .and_then(|v| v.as_array())
        .unwrap_or_else(|| panic!("missing 'articles' array: {data}"));
    if articles.is_empty() {
        // Empty windows are valid but rare on /latest — surface as a soft skip
        // rather than a false-negative test failure.
        return;
    }
    let first = &articles[0];
    for field in &["id", "title", "source", "timestamp"] {
        assert!(
            first.get(field).is_some(),
            "article missing '{field}': {first}"
        );
    }
}

#[test]
#[ignore = "live API; enable once Orbit + priapi openapi endpoints ship (DEXMARKET-7736 upstream)"]
fn social_news_latest_with_coin_filter() {
    let output = run_with_retry(&[
        "social",
        "news-latest",
        "--coins",
        "BTC",
        "--limit",
        "5",
    ]);
    let data = assert_ok_and_extract_data(&output);
    assert!(
        data.get("articles").is_some(),
        "expected 'articles' field: {data}"
    );
}

// ─── news-by-coin ───────────────────────────────────────────────────────

#[test]
#[ignore = "live API; enable once Orbit + priapi openapi endpoints ship (DEXMARKET-7736 upstream)"]
fn social_news_by_coin_returns_articles_for_eth() {
    let output = run_with_retry(&[
        "social",
        "news-by-coin",
        "--coins",
        "ETH",
        "--limit",
        "5",
    ]);
    let data = assert_ok_and_extract_data(&output);
    assert!(
        data.get("articles").is_some(),
        "expected 'articles' field: {data}"
    );
}

#[test]
fn social_news_by_coin_missing_coins_arg_fails() {
    onchainos()
        .args(["social", "news-by-coin"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("required"));
}

// ─── news-search ────────────────────────────────────────────────────────

#[test]
#[ignore = "live API; enable once Orbit + priapi openapi endpoints ship (DEXMARKET-7736 upstream)"]
fn social_news_search_with_keyword() {
    let output = run_with_retry(&[
        "social",
        "news-search",
        "--keyword",
        "ethereum",
        "--limit",
        "5",
    ]);
    let data = assert_ok_and_extract_data(&output);
    assert!(
        data.get("articles").is_some(),
        "expected 'articles' field: {data}"
    );
}

#[test]
fn social_news_search_missing_keyword_fails() {
    onchainos()
        .args(["social", "news-search"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("required"));
}

// ─── news-platforms ─────────────────────────────────────────────────────

#[test]
#[ignore = "live API; enable once Orbit + priapi openapi endpoints ship (DEXMARKET-7736 upstream)"]
fn social_news_platforms_returns_list() {
    let output = run_with_retry(&["social", "news-platforms"]);
    let data = assert_ok_and_extract_data(&output);
    let platforms = data
        .get("platforms")
        .and_then(|v| v.as_array())
        .unwrap_or_else(|| panic!("expected 'platforms' array: {data}"));
    assert!(
        !platforms.is_empty(),
        "expected at least one platform identifier: {data}"
    );
    // Each entry is a string identifier
    for p in platforms {
        assert!(p.is_string(), "platform entry should be a string: {p}");
    }
}

// ─── news-detail (negative path) ────────────────────────────────────────

#[test]
fn social_news_detail_missing_id_fails() {
    onchainos()
        .args(["social", "news-detail"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("required"));
}

// ─── sentiment-ranking ──────────────────────────────────────────────────

#[test]
#[ignore = "live API; enable once Orbit + priapi openapi endpoints ship (DEXMARKET-7736 upstream)"]
fn social_sentiment_ranking_returns_ranked_coins() {
    let output = run_with_retry(&["social", "sentiment-ranking", "--limit", "5"]);
    let data = assert_ok_and_extract_data(&output);
    // Response shape: { period, ts, details: [...] } OR { ranking: [...] }
    let entries = data
        .get("details")
        .and_then(|v| v.as_array())
        .or_else(|| data.get("ranking").and_then(|v| v.as_array()));
    if let Some(list) = entries {
        assert!(list.len() <= 5, "expected at most 5 entries, got {}", list.len());
    }
}

// ─── coin-sentiment ─────────────────────────────────────────────────────

#[test]
#[ignore = "live API; enable once Orbit + priapi openapi endpoints ship (DEXMARKET-7736 upstream)"]
fn social_coin_sentiment_snapshot_mode() {
    let output = run_with_retry(&[
        "social",
        "coin-sentiment",
        "--coins",
        "BTC,ETH",
        "--period",
        "1",
    ]);
    let data = assert_ok_and_extract_data(&output);
    assert!(
        data.get("details").is_some() || data.is_array(),
        "expected 'details' field or array: {data}"
    );
}

#[test]
#[ignore = "live API; enable once Orbit + priapi openapi endpoints ship (DEXMARKET-7736 upstream)"]
fn social_coin_sentiment_trend_mode_includes_trend_array() {
    let output = run_with_retry(&[
        "social",
        "coin-sentiment",
        "--coins",
        "BTC",
        "--period",
        "1",
        "--trend-points",
        "8",
    ]);
    let data = assert_ok_and_extract_data(&output);
    let details = data
        .get("details")
        .and_then(|v| v.as_array())
        .unwrap_or_else(|| panic!("expected 'details' array in trend mode: {data}"));
    if let Some(first) = details.first() {
        // Trend mode populates a `trend` array on each detail entry.
        // Empty windows can return [] but the field should be present.
        assert!(
            first.get("trend").is_some(),
            "trend mode should include 'trend' field on details[0]: {first}"
        );
    }
}

#[test]
fn social_coin_sentiment_missing_coins_arg_fails() {
    onchainos()
        .args(["social", "coin-sentiment"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("required"));
}

// ─── token-vibe-timeline ────────────────────────────────────────────────

#[test]
#[ignore = "live API; enable once Orbit + priapi openapi endpoints ship (DEXMARKET-7736 upstream)"]
fn social_token_vibe_timeline_for_wsol_strips_tweet_bodies() {
    let output = run_with_retry(&[
        "social",
        "token-vibe-timeline",
        "--chain",
        "solana",
        "--token-address",
        tokens::SOL_WSOL,
        "--period",
        "1",
    ]);
    let data = assert_ok_and_extract_data(&output);
    assert_no_tweet_bodies(&data);
}

#[test]
fn social_token_vibe_timeline_missing_args_fails() {
    onchainos()
        .args(["social", "token-vibe-timeline"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("required"));
}

// ─── token-top-kols ─────────────────────────────────────────────────────

#[test]
#[ignore = "live API; enable once Orbit + priapi openapi endpoints ship (DEXMARKET-7736 upstream)"]
fn social_token_top_kols_for_wsol_strips_tweet_bodies() {
    let output = run_with_retry(&[
        "social",
        "token-top-kols",
        "--chain",
        "solana",
        "--token-address",
        tokens::SOL_WSOL,
        "--period",
        "1",
        "--limit",
        "5",
    ]);
    let data = assert_ok_and_extract_data(&output);
    assert_no_tweet_bodies(&data);
}

#[test]
fn social_token_top_kols_missing_args_fails() {
    onchainos()
        .args(["social", "token-top-kols"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("required"));
}

// ─── compliance helper ──────────────────────────────────────────────────

/// PRD §3.6 / §6.3 red line: DEX vibe responses must not include tweet
/// bodies anywhere in the response tree. The CLI's `strip_tweet_bodies`
/// pass enforces this; this assertion verifies the contract end-to-end
/// against the live API.
fn assert_no_tweet_bodies(v: &Value) {
    match v {
        Value::Object(map) => {
            for forbidden in &["text", "content", "translatedContent"] {
                assert!(
                    !map.contains_key(*forbidden),
                    "compliance violation: response contains forbidden field '{forbidden}': {v}"
                );
            }
            for child in map.values() {
                assert_no_tweet_bodies(child);
            }
        }
        Value::Array(arr) => {
            for item in arr {
                assert_no_tweet_bodies(item);
            }
        }
        _ => {}
    }
}
