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
    /// Comma-separated coin symbols (e.g. "BTC,ETH"). Optional — omit for all coins.
    pub coins: Option<String>,
    /// Begin timestamp (Unix milliseconds)
    pub begin: Option<String>,
    /// End timestamp (Unix milliseconds)
    pub end: Option<String>,
    /// Importance filter: "1"=high, "2"=medium, "3"=low (per upstream codes)
    pub importance: Option<String>,
    /// Single platform identifier (e.g. "blockbeats"); see `social_news_platforms`
    pub platform: Option<String>,
    /// Page size (default "10", max 50 per PRD §7.1)
    pub limit: Option<String>,
    /// Pagination cursor from the previous response
    pub cursor: Option<String>,
    /// Article detail level: "1"=summary (default), "2"=full body
    pub detail_level: Option<String>,
    /// Locale (default "en_US"; e.g. "zh_CN", "ja_JP")
    pub language: Option<String>,
}

#[derive(Deserialize, JsonSchema)]
pub struct SocialNewsByCoinParams {
    /// Comma-separated coin symbols (required, e.g. "BTC,ETH")
    pub coins: String,
    /// Sort order: "1"=latest (default), "2"=hot
    pub sort_by: Option<String>,
    /// Sentiment filter: "1"=bullish, "2"=bearish, "3"=neutral
    pub sentiment: Option<String>,
    /// Importance filter: "1"=high, "2"=medium, "3"=low
    pub importance: Option<String>,
    /// Single platform identifier
    pub platform: Option<String>,
    /// Page size (default "10", max 50)
    pub limit: Option<String>,
    pub cursor: Option<String>,
    /// "1"=summary (default), "2"=full body
    pub detail_level: Option<String>,
    pub begin: Option<String>,
    pub end: Option<String>,
    pub language: Option<String>,
}

#[derive(Deserialize, JsonSchema)]
pub struct SocialNewsSearchParams {
    /// Search keyword (required)
    pub keyword: String,
    /// "1"=latest (default), "2"=hot
    pub sort_by: Option<String>,
    /// "1"=bullish, "2"=bearish, "3"=neutral
    pub sentiment: Option<String>,
    /// "1"=high, "2"=medium, "3"=low
    pub importance: Option<String>,
    pub platform: Option<String>,
    /// Comma-separated coin symbols
    pub coins: Option<String>,
    pub begin: Option<String>,
    pub end: Option<String>,
    /// "1"=summary (default), "2"=full body
    pub detail_level: Option<String>,
    pub limit: Option<String>,
    pub cursor: Option<String>,
    pub language: Option<String>,
}

#[derive(Deserialize, JsonSchema)]
pub struct SocialNewsDetailParams {
    /// Article id (from a previous list response)
    pub id: String,
    /// Locale (default "en_US")
    pub language: Option<String>,
}

#[derive(Deserialize, JsonSchema)]
pub struct SocialSentimentRankingParams {
    /// Window: "1"=24h (default), "2"=72h, "3"=7d, "4"=30d
    pub period: Option<String>,
    /// Sort: "1"=hot (only value currently supported)
    pub sort_by: Option<String>,
    /// Page size (default "10", max 50)
    pub limit: Option<String>,
}

#[derive(Deserialize, JsonSchema)]
pub struct SocialCoinSentimentParams {
    /// Comma-separated coin symbols (required, e.g. "BTC,ETH")
    pub coins: String,
    /// Window: "1"=24h (default), "2"=72h, "3"=7d, "4"=30d
    pub period: Option<String>,
    /// If > 0, include the `trend` array on each detail with this many buckets (max 200 per PRD §7.1)
    pub trend_points: Option<String>,
}

#[derive(Deserialize, JsonSchema)]
pub struct SocialTokenVibeTimelineParams {
    /// Chain name (e.g. "ethereum", "solana") or chainIndex (e.g. "1", "501")
    pub chain: String,
    /// Token contract address (EVM addresses lowercase)
    pub token_address: String,
    /// Window: "1"=24h (default), "2"=72h, "3"=7d, "4"=30d
    pub period: Option<String>,
}

#[derive(Deserialize, JsonSchema)]
pub struct SocialTokenTopKolsParams {
    /// Chain name (e.g. "ethereum", "solana")
    pub chain: String,
    /// Token contract address
    pub token_address: String,
    /// Sort: "1"=engagement (default), "2"=mentions, "3"=impressions
    pub sort_by: Option<String>,
    /// Window: "1"=24h (default), "2"=72h, "3"=7d, "4"=30d
    pub period: Option<String>,
    /// Page size (default "20"); upstream caps at 50
    pub limit: Option<String>,
}

// ── CLI subcommand ──────────────────────────────────────────────────────

#[derive(Subcommand)]
#[allow(clippy::large_enum_variant)]
pub enum SocialCommand {
    /// Latest crypto news feed (across all coins by default)
    NewsLatest {
        /// Comma-separated coin symbols (e.g. BTC,ETH)
        #[arg(long)]
        coins: Option<String>,
        /// Begin timestamp (Unix milliseconds)
        #[arg(long)]
        begin: Option<String>,
        /// End timestamp (Unix milliseconds)
        #[arg(long)]
        end: Option<String>,
        /// Importance: 1=high, 2=medium, 3=low
        #[arg(long)]
        importance: Option<String>,
        /// Single platform identifier (see `social news-platforms`)
        #[arg(long)]
        platform: Option<String>,
        /// Page size (default 10, max 50)
        #[arg(long)]
        limit: Option<String>,
        /// Pagination cursor from the previous response
        #[arg(long)]
        cursor: Option<String>,
        /// Article detail level: 1=summary, 2=full body
        #[arg(long)]
        detail_level: Option<String>,
        /// Locale (e.g. en_US, zh_CN)
        #[arg(long)]
        language: Option<String>,
    },
    /// News filtered by coin symbol(s)
    NewsByCoin {
        /// Comma-separated coin symbols (required)
        #[arg(long)]
        coins: String,
        /// Sort: 1=latest (default), 2=hot
        #[arg(long)]
        sort_by: Option<String>,
        /// Sentiment: 1=bullish, 2=bearish, 3=neutral
        #[arg(long)]
        sentiment: Option<String>,
        /// Importance: 1=high, 2=medium, 3=low
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
    /// Full-text news search
    NewsSearch {
        /// Search keyword (required)
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
        #[arg(long)]
        coins: Option<String>,
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
    /// Get the full body of a single article by id
    NewsDetail {
        /// Article id (from a previous list response)
        #[arg(long)]
        id: String,
        #[arg(long)]
        language: Option<String>,
    },
    /// List available news source platforms (use as `--platform` filters)
    NewsPlatforms,
    /// Top coins by social activity (mention count) over a window
    SentimentRanking {
        /// Window: 1=24h (default), 2=72h, 3=7d, 4=30d
        #[arg(long)]
        period: Option<String>,
        /// Sort: 1=hot (only value supported)
        #[arg(long)]
        sort_by: Option<String>,
        #[arg(long)]
        limit: Option<String>,
    },
    /// Sentiment metrics for one or more coins (snapshot or time-bucketed trend)
    CoinSentiment {
        /// Comma-separated coin symbols (required)
        #[arg(long)]
        coins: String,
        /// Window: 1=24h (default), 2=72h, 3=7d, 4=30d
        #[arg(long)]
        period: Option<String>,
        /// If > 0, switch to trend mode and return N buckets per coin (max 200)
        #[arg(long)]
        trend_points: Option<String>,
    },
    /// Token vibe (hotness) summary + timeline + sample KOLs per bucket
    TokenVibeTimeline {
        /// Chain name (e.g. ethereum, solana) or chainIndex
        #[arg(long)]
        chain: String,
        /// Token contract address
        #[arg(long)]
        token_address: String,
        /// Window: 1=24h (default), 2=72h, 3=7d, 4=30d
        #[arg(long)]
        period: Option<String>,
    },
    /// Top KOLs discussing a token (capped at upstream TOP50)
    TokenTopKols {
        #[arg(long)]
        chain: String,
        #[arg(long)]
        token_address: String,
        /// Sort: 1=engagement (default), 2=mentions, 3=impressions
        #[arg(long)]
        sort_by: Option<String>,
        #[arg(long)]
        period: Option<String>,
        /// Page size (default 20); upstream caps at 50
        #[arg(long)]
        limit: Option<String>,
    },
}

pub async fn execute(ctx: &Context, cmd: SocialCommand) -> Result<()> {
    match cmd {
        SocialCommand::NewsLatest {
            coins,
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
                coins,
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
        SocialCommand::NewsByCoin {
            coins,
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
            let p = SocialNewsByCoinParams {
                coins,
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
            output::success(fetch_news_by_coin(&mut client, p).await?);
            Ok(())
        }
        SocialCommand::NewsSearch {
            keyword,
            sort_by,
            sentiment,
            importance,
            platform,
            coins,
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
                coins,
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
        SocialCommand::NewsDetail { id, language } => {
            let p = SocialNewsDetailParams { id, language };
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
            period,
            sort_by,
            limit,
        } => {
            let p = SocialSentimentRankingParams {
                period,
                sort_by,
                limit,
            };
            let mut client = ctx.client_async().await?;
            output::success(fetch_sentiment_ranking(&mut client, p).await?);
            Ok(())
        }
        SocialCommand::CoinSentiment {
            coins,
            period,
            trend_points,
        } => {
            let p = SocialCoinSentimentParams {
                coins,
                period,
                trend_points,
            };
            let mut client = ctx.client_async().await?;
            output::success(fetch_coin_sentiment(&mut client, p).await?);
            Ok(())
        }
        SocialCommand::TokenVibeTimeline {
            chain,
            token_address,
            period,
        } => {
            let chain_index = crate::chains::resolve_chain(&chain).to_string();
            let mut client = ctx.client_async().await?;
            output::success(
                fetch_token_vibe_timeline(
                    &mut client,
                    &chain_index,
                    &token_address,
                    period.as_deref(),
                )
                .await?,
            );
            Ok(())
        }
        SocialCommand::TokenTopKols {
            chain,
            token_address,
            sort_by,
            period,
            limit,
        } => {
            let chain_index = crate::chains::resolve_chain(&chain).to_string();
            let mut client = ctx.client_async().await?;
            output::success(
                fetch_token_top_kols(
                    &mut client,
                    &chain_index,
                    &token_address,
                    sort_by.as_deref(),
                    period.as_deref(),
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
    push_if_present(&mut q, "tokenSymbols", p.coins.as_deref());
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
pub async fn fetch_news_by_coin(
    client: &mut ApiClient,
    p: SocialNewsByCoinParams,
) -> Result<Value> {
    let mut q: Vec<(&str, &str)> = vec![("tokenSymbols", p.coins.as_str())];
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
    push_if_present(&mut q, "tokenSymbols", p.coins.as_deref());
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
    let mut q: Vec<(&str, &str)> = vec![("id", p.id.as_str())];
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
    push_if_present(&mut q, "period", p.period.as_deref());
    push_if_present(&mut q, "sortBy", p.sort_by.as_deref());
    push_if_present(&mut q, "limit", p.limit.as_deref());
    client
        .get("/api/v6/dex/market/social/sentiment/ranking", &q)
        .await
}

/// GET /api/v6/dex/market/social/sentiment/symbol
pub async fn fetch_coin_sentiment(
    client: &mut ApiClient,
    p: SocialCoinSentimentParams,
) -> Result<Value> {
    let mut q: Vec<(&str, &str)> = vec![("tokenSymbols", p.coins.as_str())];
    push_if_present(&mut q, "period", p.period.as_deref());
    push_if_present(&mut q, "trendPoints", p.trend_points.as_deref());
    client
        .get("/api/v6/dex/market/social/sentiment/symbol", &q)
        .await
}

/// GET /api/v6/dex/market/social/vibe/timeline
///
/// Compliance: any `text` / `content` / `translatedContent` fields anywhere in
/// the response tree are stripped before returning (PRD §3.6 / §6.3 red line).
pub async fn fetch_token_vibe_timeline(
    client: &mut ApiClient,
    chain_index: &str,
    token_address: &str,
    period: Option<&str>,
) -> Result<Value> {
    let mut q: Vec<(&str, &str)> = vec![
        ("chainIndex", chain_index),
        ("tokenAddress", token_address),
    ];
    push_if_present(&mut q, "period", period);
    let mut data = client
        .get("/api/v6/dex/market/social/vibe/timeline", &q)
        .await?;
    strip_tweet_bodies(&mut data);
    Ok(data)
}

/// GET /api/v6/dex/market/social/vibe/top-kols
///
/// Compliance: same tweet-body strip as `fetch_token_vibe_timeline`.
pub async fn fetch_token_top_kols(
    client: &mut ApiClient,
    chain_index: &str,
    token_address: &str,
    sort_by: Option<&str>,
    period: Option<&str>,
    limit: Option<&str>,
) -> Result<Value> {
    let mut q: Vec<(&str, &str)> = vec![
        ("chainIndex", chain_index),
        ("tokenAddress", token_address),
    ];
    push_if_present(&mut q, "sortBy", sort_by);
    push_if_present(&mut q, "period", period);
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
