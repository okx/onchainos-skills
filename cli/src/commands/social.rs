use anyhow::Result;
use clap::Subcommand;
use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::Value;

use super::Context;
use crate::client::ApiClient;
use crate::output;

// ── Compliance strip (PRD §3.6 / §6.3) ──────────────────────────────────
//
// DEX vibe endpoints must NOT pass tweet bodies through to the agent. The
// upstream OpenAPI shape does not currently include them, but defense-in-depth:
// recursively drop `text`, `content`, and `translatedContent` from any object
// in the response tree before returning. Tweet URLs, KOL identity fields, and
// aggregate metrics remain untouched.
const TWEET_BODY_FIELDS: &[&str] = &["text", "content", "translatedContent"];

fn strip_tweet_bodies(v: &mut Value) {
    match v {
        Value::Object(map) => {
            for f in TWEET_BODY_FIELDS {
                map.remove(*f);
            }
            for child in map.values_mut() {
                strip_tweet_bodies(child);
            }
        }
        Value::Array(arr) => {
            for item in arr.iter_mut() {
                strip_tweet_bodies(item);
            }
        }
        _ => {}
    }
}

// ── Param structs (shared with MCP) ─────────────────────────────────────

#[derive(Deserialize, JsonSchema)]
pub struct SocialNewsLatestParams {
    /// Comma-separated coin symbols to filter on, e.g. "BTC,ETH". Optional.
    pub token_symbols: Option<String>,
    /// Start of the time window (Unix milliseconds).
    pub begin: Option<String>,
    /// End of the time window (Unix milliseconds).
    pub end: Option<String>,
    /// Article importance filter: "1" (High), "2" (Medium), "3" (Low). Omit to skip.
    pub importance: Option<String>,
    /// News source platform identifier; see `social_news_platforms` for valid values.
    pub platform: Option<String>,
    /// Page size (default "10").
    pub limit: Option<String>,
    /// Pagination cursor returned by the previous page; null on first call.
    pub cursor: Option<String>,
    /// Response detail level: "1" (Summary) or "2" (Full — includes article body). Default "1".
    pub detail_level: Option<String>,
    /// Locale of returned text, e.g. "en_US" (default), "zh_CN".
    pub language: Option<String>,
}

#[derive(Deserialize, JsonSchema)]
pub struct SocialNewsBySymbolParams {
    /// Comma-separated coin symbols (required), e.g. "BTC,ETH".
    pub token_symbols: String,
    /// Result ordering: "1" (Latest, default), "2" (Hot by engagement).
    pub sort_by: Option<String>,
    /// Sentiment filter: "1" (Bullish), "2" (Bearish), "3" (Neutral). Omit to skip.
    pub sentiment: Option<String>,
    /// Importance filter: "1" (High), "2" (Medium), "3" (Low). Omit to skip.
    pub importance: Option<String>,
    /// News source platform identifier.
    pub platform: Option<String>,
    /// Page size (default "10").
    pub limit: Option<String>,
    pub cursor: Option<String>,
    /// "1" (Summary, default) or "2" (Full).
    pub detail_level: Option<String>,
    pub begin: Option<String>,
    pub end: Option<String>,
    pub language: Option<String>,
}

#[derive(Deserialize, JsonSchema)]
pub struct SocialNewsSearchParams {
    /// Free-text search keyword, e.g. "ethereum upgrade" (required).
    pub keyword: String,
    /// Result ordering: "1" (Latest, default), "2" (Hot).
    pub sort_by: Option<String>,
    /// Sentiment filter: "1" / "2" / "3".
    pub sentiment: Option<String>,
    /// Importance filter: "1" / "2" / "3".
    pub importance: Option<String>,
    pub platform: Option<String>,
    /// Comma-separated coin symbols to additionally restrict results.
    pub token_symbols: Option<String>,
    pub begin: Option<String>,
    pub end: Option<String>,
    /// "1" (Summary, default) or "2" (Full).
    pub detail_level: Option<String>,
    pub limit: Option<String>,
    pub cursor: Option<String>,
    pub language: Option<String>,
}

#[derive(Deserialize, JsonSchema)]
pub struct SocialNewsDetailParams {
    /// News article id (returned by listing endpoints in `articles[].id`).
    pub article_id: String,
    /// Locale of returned text (default "en_US").
    pub language: Option<String>,
}

#[derive(Deserialize, JsonSchema)]
pub struct SocialSentimentRankingParams {
    /// Statistical period: "1" (1h, default), "2" (4h), "3" (24h).
    pub time_frame: Option<String>,
    /// Ranking criterion: "1" (Hot — by mention count). Default "1" (only value currently supported).
    pub sort_by: Option<String>,
    /// Number of results, range [1, 50]. Default "10".
    pub limit: Option<String>,
}

#[derive(Deserialize, JsonSchema)]
pub struct SocialSentimentSymbolParams {
    /// Comma-separated coin symbols (required), e.g. "BTC,ETH". Max 20 symbols.
    pub token_symbols: String,
    /// Statistical period: "1" (1h, default), "2" (4h), "3" (24h).
    pub time_frame: Option<String>,
    /// If > 0, include a `trend` series with this many equally-spaced time buckets per coin.
    pub trend_points: Option<String>,
}

#[derive(Deserialize, JsonSchema)]
pub struct SocialVibeTimelineParams {
    /// Chain name (e.g. "ethereum", "solana") or chainIndex ("1", "501", "56").
    pub chain: String,
    /// On-chain contract address of the token.
    pub token_address: String,
    /// Time window: "1" (24h, default), "2" (72h), "3" (7 Days), "4" (30 Days).
    pub time_frame: Option<String>,
}

#[derive(Deserialize, JsonSchema)]
pub struct SocialVibeTopKolsParams {
    /// Chain name (e.g. "ethereum", "solana") or chainIndex.
    pub chain: String,
    /// On-chain contract address of the token.
    pub token_address: String,
    /// Ranking criterion: "1" (Engagement, default), "2" (Mentions), "3" (Impressions).
    pub sort_by: Option<String>,
    /// Time window: "1" (24h, default), "2" (72h), "3" (7 Days), "4" (30 Days).
    pub time_frame: Option<String>,
    /// Page size (default "20"); upstream caps at TOP50.
    pub limit: Option<String>,
}

// ── CLI subcommand ──────────────────────────────────────────────────────

#[derive(Subcommand)]
#[allow(clippy::large_enum_variant)]
pub enum SocialCommand {
    /// Latest crypto news feed (across all coins by default).
    NewsLatest {
        /// Comma-separated coin symbols (e.g. BTC,ETH).
        #[arg(long)]
        token_symbols: Option<String>,
        /// Begin timestamp (Unix milliseconds).
        #[arg(long)]
        begin: Option<String>,
        /// End timestamp (Unix milliseconds).
        #[arg(long)]
        end: Option<String>,
        /// Importance: 1=High, 2=Medium, 3=Low.
        #[arg(long)]
        importance: Option<String>,
        /// Single platform identifier (see `social news-platforms`).
        #[arg(long)]
        platform: Option<String>,
        /// Page size (default 10).
        #[arg(long)]
        limit: Option<String>,
        /// Pagination cursor from the previous response.
        #[arg(long)]
        cursor: Option<String>,
        /// Detail level: 1=Summary, 2=Full.
        #[arg(long)]
        detail_level: Option<String>,
        /// Locale (e.g. en_US, zh_CN).
        #[arg(long)]
        language: Option<String>,
    },
    /// News filtered by coin symbol(s).
    NewsBySymbol {
        /// Comma-separated coin symbols (required).
        #[arg(long)]
        token_symbols: String,
        /// Sort: 1=Latest (default), 2=Hot.
        #[arg(long)]
        sort_by: Option<String>,
        /// Sentiment: 1=Bullish, 2=Bearish, 3=Neutral.
        #[arg(long)]
        sentiment: Option<String>,
        /// Importance: 1=High, 2=Medium, 3=Low.
        #[arg(long)]
        importance: Option<String>,
        #[arg(long)]
        platform: Option<String>,
        #[arg(long)]
        limit: Option<String>,
        #[arg(long)]
        cursor: Option<String>,
        #[arg(long)]
        detail_level: Option<String>,
        #[arg(long)]
        begin: Option<String>,
        #[arg(long)]
        end: Option<String>,
        #[arg(long)]
        language: Option<String>,
    },
    /// Full-text news search.
    NewsSearch {
        /// Search keyword (required).
        #[arg(long)]
        keyword: String,
        #[arg(long)]
        sort_by: Option<String>,
        #[arg(long)]
        sentiment: Option<String>,
        #[arg(long)]
        importance: Option<String>,
        #[arg(long)]
        platform: Option<String>,
        /// Comma-separated coin symbols (additional filter).
        #[arg(long)]
        token_symbols: Option<String>,
        #[arg(long)]
        begin: Option<String>,
        #[arg(long)]
        end: Option<String>,
        #[arg(long)]
        detail_level: Option<String>,
        #[arg(long)]
        limit: Option<String>,
        #[arg(long)]
        cursor: Option<String>,
        #[arg(long)]
        language: Option<String>,
    },
    /// Get the full body of a single article by id.
    NewsDetail {
        /// Article id (from a previous list response's `articles[].id`).
        #[arg(long)]
        article_id: String,
        #[arg(long)]
        language: Option<String>,
    },
    /// List available news source platforms (use as `--platform` filters).
    NewsPlatforms,
    /// Top coins ranked by social activity (mention count) over a window.
    SentimentRanking {
        /// Statistical period: 1=1h (default), 2=4h, 3=24h.
        #[arg(long)]
        time_frame: Option<String>,
        /// Sort: 1=Hot (only value supported).
        #[arg(long)]
        sort_by: Option<String>,
        /// Page size, range [1, 50] (default 10).
        #[arg(long)]
        limit: Option<String>,
    },
    /// Sentiment metrics for one or more coins (snapshot or time-bucketed trend).
    SentimentSymbol {
        /// Comma-separated coin symbols (required, max 20).
        #[arg(long)]
        token_symbols: String,
        /// Statistical period: 1=1h (default), 2=4h, 3=24h.
        #[arg(long)]
        time_frame: Option<String>,
        /// If > 0, switch to trend mode and return N equally-spaced buckets per coin.
        #[arg(long)]
        trend_points: Option<String>,
    },
    /// Token vibe (hotness) summary + timeline + sample KOLs per bucket.
    VibeTimeline {
        /// Chain name (e.g. ethereum, solana) or chainIndex.
        #[arg(long)]
        chain: String,
        /// Token contract address.
        #[arg(long)]
        token_address: String,
        /// Time window: 1=24h (default), 2=72h, 3=7d, 4=30d.
        #[arg(long)]
        time_frame: Option<String>,
    },
    /// Top KOLs discussing a token (capped at upstream TOP50).
    VibeTopKols {
        #[arg(long)]
        chain: String,
        #[arg(long)]
        token_address: String,
        /// Sort: 1=Engagement (default), 2=Mentions, 3=Impressions.
        #[arg(long)]
        sort_by: Option<String>,
        #[arg(long)]
        time_frame: Option<String>,
        /// Page size (default 20); upstream caps at TOP50.
        #[arg(long)]
        limit: Option<String>,
    },
}

pub async fn execute(ctx: &Context, cmd: SocialCommand) -> Result<()> {
    match cmd {
        SocialCommand::NewsLatest {
            token_symbols,
            begin,
            end,
            importance,
            platform,
            limit,
            cursor,
            detail_level,
            language,
        } => {
            let p = SocialNewsLatestParams {
                token_symbols,
                begin,
                end,
                importance,
                platform,
                limit,
                cursor,
                detail_level,
                language,
            };
            let mut client = ctx.client_async().await?;
            output::success(fetch_news_latest(&mut client, p).await?);
            Ok(())
        }
        SocialCommand::NewsBySymbol {
            token_symbols,
            sort_by,
            sentiment,
            importance,
            platform,
            limit,
            cursor,
            detail_level,
            begin,
            end,
            language,
        } => {
            let p = SocialNewsBySymbolParams {
                token_symbols,
                sort_by,
                sentiment,
                importance,
                platform,
                limit,
                cursor,
                detail_level,
                begin,
                end,
                language,
            };
            let mut client = ctx.client_async().await?;
            output::success(fetch_news_by_symbol(&mut client, p).await?);
            Ok(())
        }
        SocialCommand::NewsSearch {
            keyword,
            sort_by,
            sentiment,
            importance,
            platform,
            token_symbols,
            begin,
            end,
            detail_level,
            limit,
            cursor,
            language,
        } => {
            let p = SocialNewsSearchParams {
                keyword,
                sort_by,
                sentiment,
                importance,
                platform,
                token_symbols,
                begin,
                end,
                detail_level,
                limit,
                cursor,
                language,
            };
            let mut client = ctx.client_async().await?;
            output::success(fetch_news_search(&mut client, p).await?);
            Ok(())
        }
        SocialCommand::NewsDetail {
            article_id,
            language,
        } => {
            let p = SocialNewsDetailParams {
                article_id,
                language,
            };
            let mut client = ctx.client_async().await?;
            output::success(fetch_news_detail(&mut client, p).await?);
            Ok(())
        }
        SocialCommand::NewsPlatforms => {
            let mut client = ctx.client_async().await?;
            output::success(fetch_news_platforms(&mut client).await?);
            Ok(())
        }
        SocialCommand::SentimentRanking {
            time_frame,
            sort_by,
            limit,
        } => {
            let p = SocialSentimentRankingParams {
                time_frame,
                sort_by,
                limit,
            };
            let mut client = ctx.client_async().await?;
            output::success(fetch_sentiment_ranking(&mut client, p).await?);
            Ok(())
        }
        SocialCommand::SentimentSymbol {
            token_symbols,
            time_frame,
            trend_points,
        } => {
            let p = SocialSentimentSymbolParams {
                token_symbols,
                time_frame,
                trend_points,
            };
            let mut client = ctx.client_async().await?;
            output::success(fetch_sentiment_symbol(&mut client, p).await?);
            Ok(())
        }
        SocialCommand::VibeTimeline {
            chain,
            token_address,
            time_frame,
        } => {
            let chain_index = crate::chains::resolve_chain(&chain).to_string();
            let mut client = ctx.client_async().await?;
            output::success(
                fetch_vibe_timeline(
                    &mut client,
                    &chain_index,
                    &token_address,
                    time_frame.as_deref(),
                )
                .await?,
            );
            Ok(())
        }
        SocialCommand::VibeTopKols {
            chain,
            token_address,
            sort_by,
            time_frame,
            limit,
        } => {
            let chain_index = crate::chains::resolve_chain(&chain).to_string();
            let mut client = ctx.client_async().await?;
            output::success(
                fetch_vibe_top_kols(
                    &mut client,
                    &chain_index,
                    &token_address,
                    sort_by.as_deref(),
                    time_frame.as_deref(),
                    limit.as_deref(),
                )
                .await?,
            );
            Ok(())
        }
    }
}

// ── Public fetch functions (used by both CLI and MCP) ────────────────

fn push_if_present<'a>(query: &mut Vec<(&'a str, &'a str)>, key: &'a str, val: Option<&'a str>) {
    if let Some(v) = val {
        if !v.is_empty() {
            query.push((key, v));
        }
    }
}

/// GET /api/v6/dex/market/social/news/latest
pub async fn fetch_news_latest(
    client: &mut ApiClient,
    p: SocialNewsLatestParams,
) -> Result<Value> {
    let mut q: Vec<(&str, &str)> = Vec::new();
    push_if_present(&mut q, "tokenSymbols", p.token_symbols.as_deref());
    push_if_present(&mut q, "begin", p.begin.as_deref());
    push_if_present(&mut q, "end", p.end.as_deref());
    push_if_present(&mut q, "importance", p.importance.as_deref());
    push_if_present(&mut q, "platform", p.platform.as_deref());
    push_if_present(&mut q, "limit", p.limit.as_deref());
    push_if_present(&mut q, "cursor", p.cursor.as_deref());
    push_if_present(&mut q, "detailLevel", p.detail_level.as_deref());
    push_if_present(&mut q, "language", p.language.as_deref());
    client.get("/api/v6/dex/market/social/news/latest", &q).await
}

/// GET /api/v6/dex/market/social/news/by-symbol
pub async fn fetch_news_by_symbol(
    client: &mut ApiClient,
    p: SocialNewsBySymbolParams,
) -> Result<Value> {
    let mut q: Vec<(&str, &str)> = vec![("tokenSymbols", p.token_symbols.as_str())];
    push_if_present(&mut q, "sortBy", p.sort_by.as_deref());
    push_if_present(&mut q, "sentiment", p.sentiment.as_deref());
    push_if_present(&mut q, "importance", p.importance.as_deref());
    push_if_present(&mut q, "platform", p.platform.as_deref());
    push_if_present(&mut q, "limit", p.limit.as_deref());
    push_if_present(&mut q, "cursor", p.cursor.as_deref());
    push_if_present(&mut q, "detailLevel", p.detail_level.as_deref());
    push_if_present(&mut q, "begin", p.begin.as_deref());
    push_if_present(&mut q, "end", p.end.as_deref());
    push_if_present(&mut q, "language", p.language.as_deref());
    client
        .get("/api/v6/dex/market/social/news/by-symbol", &q)
        .await
}

/// GET /api/v6/dex/market/social/news/search
pub async fn fetch_news_search(
    client: &mut ApiClient,
    p: SocialNewsSearchParams,
) -> Result<Value> {
    let mut q: Vec<(&str, &str)> = vec![("keyword", p.keyword.as_str())];
    push_if_present(&mut q, "sortBy", p.sort_by.as_deref());
    push_if_present(&mut q, "sentiment", p.sentiment.as_deref());
    push_if_present(&mut q, "importance", p.importance.as_deref());
    push_if_present(&mut q, "platform", p.platform.as_deref());
    push_if_present(&mut q, "tokenSymbols", p.token_symbols.as_deref());
    push_if_present(&mut q, "begin", p.begin.as_deref());
    push_if_present(&mut q, "end", p.end.as_deref());
    push_if_present(&mut q, "detailLevel", p.detail_level.as_deref());
    push_if_present(&mut q, "limit", p.limit.as_deref());
    push_if_present(&mut q, "cursor", p.cursor.as_deref());
    push_if_present(&mut q, "language", p.language.as_deref());
    client.get("/api/v6/dex/market/social/news/search", &q).await
}

/// GET /api/v6/dex/market/social/news/detail
pub async fn fetch_news_detail(
    client: &mut ApiClient,
    p: SocialNewsDetailParams,
) -> Result<Value> {
    let mut q: Vec<(&str, &str)> = vec![("articleId", p.article_id.as_str())];
    push_if_present(&mut q, "language", p.language.as_deref());
    client.get("/api/v6/dex/market/social/news/detail", &q).await
}

/// GET /api/v6/dex/market/social/news/platforms
pub async fn fetch_news_platforms(client: &mut ApiClient) -> Result<Value> {
    client
        .get("/api/v6/dex/market/social/news/platforms", &[])
        .await
}

/// GET /api/v6/dex/market/social/sentiment/ranking
pub async fn fetch_sentiment_ranking(
    client: &mut ApiClient,
    p: SocialSentimentRankingParams,
) -> Result<Value> {
    let mut q: Vec<(&str, &str)> = Vec::new();
    push_if_present(&mut q, "timeFrame", p.time_frame.as_deref());
    push_if_present(&mut q, "sortBy", p.sort_by.as_deref());
    push_if_present(&mut q, "limit", p.limit.as_deref());
    client
        .get("/api/v6/dex/market/social/sentiment/ranking", &q)
        .await
}

/// GET /api/v6/dex/market/social/sentiment/symbol
pub async fn fetch_sentiment_symbol(
    client: &mut ApiClient,
    p: SocialSentimentSymbolParams,
) -> Result<Value> {
    let mut q: Vec<(&str, &str)> = vec![("tokenSymbols", p.token_symbols.as_str())];
    push_if_present(&mut q, "timeFrame", p.time_frame.as_deref());
    push_if_present(&mut q, "trendPoints", p.trend_points.as_deref());
    client
        .get("/api/v6/dex/market/social/sentiment/symbol", &q)
        .await
}

/// GET /api/v6/dex/market/social/vibe/timeline
///
/// Compliance: any `text` / `content` / `translatedContent` fields anywhere in
/// the response tree are stripped before returning (PRD §3.6 / §6.3 red line).
pub async fn fetch_vibe_timeline(
    client: &mut ApiClient,
    chain_index: &str,
    token_address: &str,
    time_frame: Option<&str>,
) -> Result<Value> {
    let mut q: Vec<(&str, &str)> = vec![
        ("chainIndex", chain_index),
        ("tokenAddress", token_address),
    ];
    push_if_present(&mut q, "timeFrame", time_frame);
    let mut data = client
        .get("/api/v6/dex/market/social/vibe/timeline", &q)
        .await?;
    strip_tweet_bodies(&mut data);
    Ok(data)
}

/// GET /api/v6/dex/market/social/vibe/top-kols
///
/// Compliance: same tweet-body strip as `fetch_vibe_timeline`.
pub async fn fetch_vibe_top_kols(
    client: &mut ApiClient,
    chain_index: &str,
    token_address: &str,
    sort_by: Option<&str>,
    time_frame: Option<&str>,
    limit: Option<&str>,
) -> Result<Value> {
    let mut q: Vec<(&str, &str)> = vec![
        ("chainIndex", chain_index),
        ("tokenAddress", token_address),
    ];
    push_if_present(&mut q, "sortBy", sort_by);
    push_if_present(&mut q, "timeFrame", time_frame);
    push_if_present(&mut q, "limit", limit);
    let mut data = client
        .get("/api/v6/dex/market/social/vibe/top-kols", &q)
        .await?;
    strip_tweet_bodies(&mut data);
    Ok(data)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn strip_removes_forbidden_fields_recursively() {
        let mut v = json!({
            "summary": { "score": "78", "text": "leak" },
            "kols": [
                { "handle": "a", "content": "leak", "tweetUrl": "https://x.com/a/1" },
                { "handle": "b", "translatedContent": "leak" },
            ],
            "ts": 1
        });
        strip_tweet_bodies(&mut v);
        assert!(v["summary"].get("text").is_none());
        assert_eq!(v["summary"]["score"], "78");
        assert!(v["kols"][0].get("content").is_none());
        assert_eq!(v["kols"][0]["tweetUrl"], "https://x.com/a/1");
        assert!(v["kols"][1].get("translatedContent").is_none());
        assert_eq!(v["ts"], 1);
    }

    #[test]
    fn strip_is_noop_on_clean_response() {
        let mut v = json!({
            "summary": { "score": "50" },
            "timeline": [{ "ts": 1, "score": "40", "kols": [{ "handle": "x" }] }]
        });
        let snapshot = v.clone();
        strip_tweet_bodies(&mut v);
        assert_eq!(v, snapshot);
    }
}
