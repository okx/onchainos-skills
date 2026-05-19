//! Integration tests for `onchainos social` commands.
//!
//! Covers the 9 social CLI subcommands: news-latest, news-by-symbol, news-search,
//! news-detail, news-platforms, sentiment-ranking, sentiment-symbol,
//! vibe-timeline, vibe-top-kols.
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
fn social_news_latest_with_coin_filter() {
    let output = run_with_retry(&[
        "social",
        "news-latest",
        "--token-symbols",
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

// ─── news-by-symbol ───────────────────────────────────────────────────────

#[test]
fn social_news_by_symbol_returns_articles_for_eth() {
    let output = run_with_retry(&[
        "social",
        "news-by-symbol",
        "--token-symbols",
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
fn social_news_by_symbol_missing_coins_arg_fails() {
    onchainos()
        .args(["social", "news-by-symbol"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("required"));
}

// ─── news-search ────────────────────────────────────────────────────────

#[test]
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

// ─── sentiment-symbol ─────────────────────────────────────────────────────

#[test]
fn social_sentiment_symbol_snapshot_mode() {
    let output = run_with_retry(&[
        "social",
        "sentiment-symbol",
        "--token-symbols",
        "BTC,ETH",
        "--time-frame",
        "1",
    ]);
    let data = assert_ok_and_extract_data(&output);
    assert!(
        data.get("details").is_some() || data.is_array(),
        "expected 'details' field or array: {data}"
    );
}

#[test]
fn social_sentiment_symbol_trend_mode_includes_trend_array() {
    let output = run_with_retry(&[
        "social",
        "sentiment-symbol",
        "--token-symbols",
        "BTC",
        "--time-frame",
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
fn social_sentiment_symbol_missing_coins_arg_fails() {
    onchainos()
        .args(["social", "sentiment-symbol"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("required"));
}

// ─── vibe-timeline ────────────────────────────────────────────────

#[test]
fn social_vibe_timeline_for_wsol_strips_tweet_bodies() {
    let output = run_with_retry(&[
        "social",
        "vibe-timeline",
        "--chain",
        "solana",
        "--token-address",
        tokens::SOL_WSOL,
        "--time-frame",
        "1",
    ]);
    let data = assert_ok_and_extract_data(&output);
    assert_no_tweet_bodies(&data);
}

#[test]
fn social_vibe_timeline_missing_args_fails() {
    onchainos()
        .args(["social", "vibe-timeline"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("required"));
}

// ─── vibe-top-kols ─────────────────────────────────────────────────────

#[test]
fn social_vibe_top_kols_for_wsol_strips_tweet_bodies() {
    let output = run_with_retry(&[
        "social",
        "vibe-top-kols",
        "--chain",
        "solana",
        "--token-address",
        tokens::SOL_WSOL,
        "--time-frame",
        "1",
        "--limit",
        "5",
    ]);
    let data = assert_ok_and_extract_data(&output);
    assert_no_tweet_bodies(&data);
}

#[test]
fn social_vibe_top_kols_missing_args_fails() {
    onchainos()
        .args(["social", "vibe-top-kols"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("required"));
}

// ─── compliance helper ──────────────────────────────────────────────────

/// Compliance red line: DEX vibe responses must not include tweet
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

// ─── Additional coverage (pagination, detail round-trip, schema, mappings) ──

/// Verifies cursor pagination: page 1 → page 2 must return different article ids.
/// Guards against the cursor being a no-op or repeating the same page.
#[test]
fn social_news_latest_pagination_advances_cursor() {
    let page1 = run_with_retry(&["social", "news-latest", "--limit", "3"]);
    let d1 = assert_ok_and_extract_data(&page1);
    let cursor = d1
        .get("cursor")
        .and_then(|v| v.as_str())
        .unwrap_or_else(|| panic!("page 1 missing cursor: {d1}"));
    let ids1: Vec<String> = d1["articles"]
        .as_array()
        .unwrap()
        .iter()
        .map(|a| a["id"].as_str().unwrap().to_string())
        .collect();
    assert!(!ids1.is_empty(), "page 1 has no articles");

    let page2 = run_with_retry(&["social", "news-latest", "--limit", "3", "--cursor", cursor]);
    let d2 = assert_ok_and_extract_data(&page2);
    let ids2: Vec<String> = d2["articles"]
        .as_array()
        .unwrap()
        .iter()
        .map(|a| a["id"].as_str().unwrap().to_string())
        .collect();
    assert_ne!(
        ids1, ids2,
        "cursor pagination did not advance: page 1 = page 2 = {ids1:?}"
    );
}

/// Round-trip: list latest → take first article id → fetch detail → assert full content.
/// Covers the chained list-then-detail flow that the SKILL.md advertises.
#[test]
fn social_news_detail_round_trip_returns_full_body() {
    let list = run_with_retry(&["social", "news-latest", "--limit", "1"]);
    let dlist = assert_ok_and_extract_data(&list);
    let id = dlist["articles"][0]["id"]
        .as_str()
        .unwrap_or_else(|| panic!("list response missing articles[0].id: {dlist}"))
        .to_string();

    let detail = run_with_retry(&["social", "news-detail", "--article-id", &id]);
    let ddetail = assert_ok_and_extract_data(&detail);
    let articles = ddetail["articles"]
        .as_array()
        .unwrap_or_else(|| panic!("detail missing articles[]: {ddetail}"));
    assert_eq!(articles.len(), 1, "detail should return exactly 1 article");
    assert_eq!(
        articles[0]["id"].as_str(),
        Some(id.as_str()),
        "detail returned a different id than requested"
    );
    // Detail always returns the full content (no detailLevel parameter).
    let content = articles[0]["content"].as_str().unwrap_or("");
    assert!(
        !content.is_empty(),
        "detail response should have non-empty content"
    );
}

/// Bogus article id returns ok=true with empty articles[], not an error.
/// Spec behavior — clean empty rather than 4xx.
#[test]
fn social_news_detail_bogus_id_returns_empty_articles() {
    let output = run_with_retry(&["social", "news-detail", "--article-id", "BOGUS_DOES_NOT_EXIST_42"]);
    let data = assert_ok_and_extract_data(&output);
    let articles = data["articles"]
        .as_array()
        .unwrap_or_else(|| panic!("expected articles[] field even for bogus id: {data}"));
    assert!(
        articles.is_empty(),
        "expected empty articles[] for bogus id, got {articles:?}"
    );
}

/// Regression guard for the sentiment time_frame mapping (1=1h, 2=4h, 3=24h).
/// The response `period` field echoes the resolved window. If the codes ever
/// drift back to the old 24h/72h/7d/30d mapping, this test catches it.
#[test]
fn social_sentiment_ranking_period_echo_matches_time_frame() {
    for (tf, expected) in [("1", "1h"), ("2", "4h"), ("3", "24h")] {
        let output = run_with_retry(&["social", "sentiment-ranking", "--time-frame", tf, "--limit", "1"]);
        let data = assert_ok_and_extract_data(&output);
        let period = data["period"].as_str().unwrap_or("");
        assert_eq!(
            period, expected,
            "time-frame={tf} expected period='{expected}', got '{period}'"
        );
    }
}

/// Multi-coin sentiment query returns one details[] entry per requested coin
/// (when all symbols are well-known).
#[test]
fn social_sentiment_symbol_multi_coin_returns_per_coin_entries() {
    let output = run_with_retry(&[
        "social",
        "sentiment-symbol",
        "--token-symbols",
        "BTC,ETH,SOL",
        "--time-frame",
        "3",
    ]);
    let data = assert_ok_and_extract_data(&output);
    let details = data["details"]
        .as_array()
        .unwrap_or_else(|| panic!("expected details[] array: {data}"));
    assert_eq!(
        details.len(),
        3,
        "expected one details[] entry per coin (BTC/ETH/SOL), got {}",
        details.len()
    );
    let symbols: Vec<&str> = details
        .iter()
        .map(|d| d["tokenSymbol"].as_str().unwrap_or(""))
        .collect();
    for c in ["BTC", "ETH", "SOL"] {
        assert!(symbols.contains(&c), "missing {c} in details: {symbols:?}");
    }
}

/// Vibe-timeline summary contains every documented top-level field.
/// Schema completeness guard — flags any spec drift where fields get renamed/dropped.
#[test]
fn social_vibe_timeline_summary_has_documented_fields() {
    let output = run_with_retry(&[
        "social",
        "vibe-timeline",
        "--chain",
        "ethereum",
        "--token-address",
        tokens::ETH_WETH,
        "--time-frame",
        "1",
    ]);
    let data = assert_ok_and_extract_data(&output);
    let summary = data
        .get("summary")
        .unwrap_or_else(|| panic!("missing summary object: {data}"));
    for f in &[
        "score",
        "scoreType",
        "scoreRange",
        "scoreChangeRate",
        "mentionsCount",
        "mentionsCountChangeRate",
        "engagement",
        "engagementChangeRate",
        "impressions",
        "impressionsChangeRate",
        "supportFirstMentioned",
    ] {
        assert!(
            summary.get(*f).is_some(),
            "summary missing documented field '{f}': {summary}"
        );
    }
    assert_eq!(
        summary["scoreType"].as_str(),
        Some("dex_vibe_hotness"),
        "scoreType should be the fixed 'dex_vibe_hotness' literal"
    );
    assert_eq!(
        summary["scoreRange"].as_str(),
        Some("0-100"),
        "scoreRange should be the fixed '0-100' literal"
    );
}
