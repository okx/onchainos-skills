use anyhow::Result;
use clap::Subcommand;
use serde_json::{json, Value};

use super::Context;
use crate::client::ApiClient;
use crate::output;

#[derive(Subcommand)]
#[allow(clippy::large_enum_variant)]
pub enum MarketCommand {
    /// Get token price (by contract address)
    Price {
        /// Token contract address
        #[arg(long)]
        address: String,
        /// Chain (e.g. ethereum, solana, xlayer)
        #[arg(long)]
        chain: Option<String>,
    },
    /// Get prices for multiple tokens (POST, batch query)
    Prices {
        /// Comma-separated chainIndex:address pairs (e.g. "1:0xeee...,501:1111...")
        #[arg(long)]
        tokens: String,
        /// Default chain if not specified per token
        #[arg(long)]
        chain: Option<String>,
    },
    /// Get K-line / candlestick data
    Kline {
        /// Token contract address
        #[arg(long)]
        address: String,
        /// Bar size: 1s, 1m, 5m, 15m, 30m, 1H, 4H, 1D, 1W, etc.
        #[arg(long, default_value = "1H")]
        bar: String,
        /// Number of data points (max 299)
        #[arg(long, default_value = "100")]
        limit: u32,
        /// Chain
        #[arg(long)]
        chain: Option<String>,
    },
    /// Get index price (aggregated from multiple sources)
    Index {
        /// Token contract address (empty string for native token)
        #[arg(long)]
        address: String,
        /// Chain
        #[arg(long)]
        chain: Option<String>,
    },
    /// Get supported chains for portfolio PnL endpoints
    PortfolioSupportedChains,
    /// Get wallet portfolio overview: realized/unrealized PnL, win rate, trading stats
    PortfolioOverview {
        /// Wallet address
        #[arg(long)]
        address: String,
        /// Chain name or ID (e.g. ethereum, solana, xlayer)
        #[arg(long)]
        chain: String,
        /// Time frame: 1=1D, 2=3D, 3=7D, 4=1M, 5=3M (default: 4 = 1M)
        #[arg(long, default_value = "4")]
        time_frame: String,
    },
    /// Get wallet DEX transaction history (paginated)
    PortfolioDexHistory {
        /// Wallet address
        #[arg(long)]
        address: String,
        /// Chain name or ID (e.g. ethereum, solana, xlayer)
        #[arg(long)]
        chain: String,
        /// Start timestamp (milliseconds). Supply with --end, OR use --since instead.
        #[arg(long)]
        begin: Option<String>,
        /// End timestamp (milliseconds). Supply with --begin, OR use --since instead.
        #[arg(long)]
        end: Option<String>,
        /// Relative time window: <int><s|m|h|d>, e.g. 24h, 7d. Mutually exclusive with --begin/--end.
        #[arg(long)]
        since: Option<String>,
        /// Page size (1-100, default 20)
        #[arg(long)]
        limit: Option<String>,
        /// Pagination cursor from previous response
        #[arg(long)]
        cursor: Option<String>,
        /// Filter by token contract address
        #[arg(long)]
        token: Option<String>,
        /// Transaction type: 1=BUY, 2=SELL, 3=Transfer In, 4=Transfer Out (comma-separated)
        #[arg(long = "tx-type")]
        tx_type: Option<String>,
    },
    /// Get recent token PnL records for a wallet (paginated)
    PortfolioRecentPnl {
        /// Wallet address
        #[arg(long)]
        address: String,
        /// Chain name or ID (e.g. ethereum, solana, xlayer)
        #[arg(long)]
        chain: String,
        /// Page size (1-100, default 20)
        #[arg(long)]
        limit: Option<String>,
        /// Pagination cursor from previous response
        #[arg(long)]
        cursor: Option<String>,
    },
    /// Get latest PnL snapshot for a specific token in a wallet
    PortfolioTokenPnl {
        /// Wallet address
        #[arg(long)]
        address: String,
        /// Chain name or ID (e.g. ethereum, solana, xlayer)
        #[arg(long)]
        chain: String,
        /// Token contract address
        #[arg(long)]
        token: String,
    },
}

pub async fn execute(ctx: &Context, cmd: MarketCommand) -> Result<()> {
    let mut client = ctx.client_async().await?;
    match cmd {
        MarketCommand::Price { address, chain } => {
            let address = address.trim().to_string();
            if address.is_empty() {
                anyhow::bail!("Parameter --address cannot be empty");
            }
            let chain_index = chain
                .map(|c| crate::chains::resolve_chain(&c).to_string())
                .unwrap_or_else(|| ctx.chain_index_or("ethereum"));
            let result = fetch_price(&mut client, &address, &chain_index).await?;
            if result.as_array().is_some_and(|a| a.is_empty()) {
                anyhow::bail!(
                    "No price data found for address {} on chain {}. Verify the token address is valid on this chain.",
                    address,
                    chain_index
                );
            }
            output::success(result);
        }
        MarketCommand::Prices { tokens, chain } => {
            let default_chain = chain
                .map(|c| crate::chains::resolve_chain(&c).to_string())
                .unwrap_or_else(|| ctx.chain_index_or("ethereum"));
            output::success(fetch_prices(&mut client, &tokens, &default_chain).await?);
        }
        MarketCommand::Kline {
            address,
            bar,
            limit,
            chain,
        } => {
            let chain_index = chain
                .map(|c| crate::chains::resolve_chain(&c).to_string())
                .unwrap_or_else(|| ctx.chain_index_or("ethereum"));
            output::success(fetch_kline(&mut client, &address, &chain_index, &bar, limit).await?);
        }
        MarketCommand::Index { address, chain } => {
            let chain_index = chain
                .map(|c| crate::chains::resolve_chain(&c).to_string())
                .unwrap_or_else(|| ctx.chain_index_or("ethereum"));
            output::success(fetch_index(&mut client, &address, &chain_index).await?);
        }
        MarketCommand::PortfolioSupportedChains => {
            portfolio_supported_chains(ctx).await?;
        }
        MarketCommand::PortfolioOverview {
            address,
            chain,
            time_frame,
        } => {
            portfolio_overview(ctx, &address, &chain, &time_frame).await?;
        }
        MarketCommand::PortfolioDexHistory {
            address,
            chain,
            begin,
            end,
            since,
            limit,
            cursor,
            token,
            tx_type,
        } => {
            portfolio_dex_history(
                ctx,
                &address,
                &chain,
                begin.as_deref(),
                end.as_deref(),
                since.as_deref(),
                limit.as_deref(),
                cursor.as_deref(),
                token.as_deref(),
                tx_type.as_deref(),
            )
            .await?;
        }
        MarketCommand::PortfolioRecentPnl {
            address,
            chain,
            limit,
            cursor,
        } => {
            portfolio_recent_pnl(ctx, &address, &chain, limit.as_deref(), cursor.as_deref())
                .await?;
        }
        MarketCommand::PortfolioTokenPnl {
            address,
            chain,
            token,
        } => {
            portfolio_token_pnl(ctx, &address, &chain, &token).await?;
        }
    }
    Ok(())
}

/// POST /api/v6/dex/market/price — body is JSON array
pub async fn fetch_price(
    client: &mut ApiClient,
    address: &str,
    chain_index: &str,
) -> Result<Value> {
    let body = json!([{"chainIndex": chain_index, "tokenContractAddress": address}]);
    client.post("/api/v6/dex/market/price", &body).await
}

/// POST /api/v6/dex/market/price — batch query
pub async fn fetch_prices(
    client: &mut ApiClient,
    tokens: &str,
    default_chain_index: &str,
) -> Result<Value> {
    let items: Vec<Value> = tokens
        .split(',')
        .map(|pair| {
            let pair = pair.trim();
            if let Some((chain_part, addr)) = pair.split_once(':') {
                json!({
                    "chainIndex": crate::chains::resolve_chain(chain_part),
                    "tokenContractAddress": addr
                })
            } else {
                json!({
                    "chainIndex": default_chain_index,
                    "tokenContractAddress": pair
                })
            }
        })
        .collect();
    client
        .post("/api/v6/dex/market/price", &Value::Array(items))
        .await
}

/// Transform kline raw arrays into named objects for LLM-friendly output.
/// API returns: [ts, open, high, low, close, vol, volUsd, confirm]
fn kline_to_named_objects(data: Value) -> Value {
    const FIELDS: &[&str] = &["ts", "o", "h", "l", "c", "vol", "volUsd", "confirm"];
    match data {
        Value::Array(candles) => Value::Array(
            candles
                .into_iter()
                .map(|candle| match candle {
                    Value::Array(values) => {
                        let mut map = serde_json::Map::new();
                        for (i, val) in values.into_iter().enumerate() {
                            let key = FIELDS.get(i).unwrap_or(&"unknown");
                            map.insert((*key).to_string(), val);
                        }
                        Value::Object(map)
                    }
                    other => other,
                })
                .collect(),
        ),
        other => other,
    }
}

/// GET /api/v6/dex/market/candles — returns named objects (transformed from raw arrays).
pub async fn fetch_kline(
    client: &mut ApiClient,
    address: &str,
    chain_index: &str,
    bar: &str,
    limit: u32,
) -> Result<Value> {
    let limit_str = limit.to_string();
    let raw = client
        .get(
            "/api/v6/dex/market/candles",
            &[
                ("chainIndex", chain_index),
                ("tokenContractAddress", address),
                ("bar", bar),
                ("limit", &limit_str),
            ],
        )
        .await?;
    Ok(kline_to_named_objects(raw))
}

/// POST /api/v6/dex/index/current-price — body is JSON array
pub async fn fetch_index(
    client: &mut ApiClient,
    address: &str,
    chain_index: &str,
) -> Result<Value> {
    let body = json!([{"chainIndex": chain_index, "tokenContractAddress": address}]);
    client.post("/api/v6/dex/index/current-price", &body).await
}

/// GET /api/v6/dex/market/portfolio/supported/chain
pub async fn fetch_portfolio_supported_chains(client: &mut ApiClient) -> Result<Value> {
    client
        .get("/api/v6/dex/market/portfolio/supported/chain", &[])
        .await
}

async fn portfolio_supported_chains(ctx: &Context) -> Result<()> {
    let mut client = ctx.client_async().await?;
    output::success(fetch_portfolio_supported_chains(&mut client).await?);
    Ok(())
}

/// GET /api/v6/dex/market/portfolio/overview
pub async fn fetch_portfolio_overview(
    client: &mut ApiClient,
    chain_index: &str,
    address: &str,
    time_frame: &str,
) -> Result<Value> {
    client
        .get(
            "/api/v6/dex/market/portfolio/overview",
            &[
                ("chainIndex", chain_index),
                ("walletAddress", address),
                ("timeFrame", time_frame),
            ],
        )
        .await
}

async fn portfolio_overview(
    ctx: &Context,
    address: &str,
    chain: &str,
    time_frame: &str,
) -> Result<()> {
    let chain_index = crate::chains::resolve_chain(chain);
    let mut client = ctx.client_async().await?;
    output::success(
        fetch_portfolio_overview(&mut client, &chain_index, address, time_frame).await?,
    );
    Ok(())
}

/// GET /api/v6/dex/market/portfolio/dex-history
///
/// Window rule (validated before request): exactly one of `since` XOR (`begin` AND `end`).
/// On `since`, resolves the relative window and adds `data.resolvedWindow`.
#[allow(clippy::too_many_arguments)]
pub async fn fetch_portfolio_dex_history(
    client: &mut ApiClient,
    chain_index: &str,
    address: &str,
    begin: Option<&str>,
    end: Option<&str>,
    since: Option<&str>,
    limit: Option<&str>,
    cursor: Option<&str>,
    token: Option<&str>,
    tx_type: Option<&str>,
) -> Result<Value> {
    let begin = begin.filter(|s| !s.is_empty());
    let end = end.filter(|s| !s.is_empty());

    let mut resolved_window: Option<crate::commands::sink::ResolvedWindow> = None;
    let (begin_val, end_val) = if let Some(s) = since.filter(|s| !s.is_empty()) {
        if begin.is_some() || end.is_some() {
            return Err(crate::commands::sink::CodedError::invalid_input(
                "since",
                "--since is mutually exclusive with --begin/--end",
            )
            .into());
        }
        let now = crate::commands::sink::now_ms();
        let w = crate::commands::sink::resolve_since_window(s, now).map_err(|e| {
            crate::commands::sink::CodedError::invalid_input("since", format!("{e}"))
        })?;
        let pair = (w.begin.to_string(), w.end.to_string());
        resolved_window = Some(w);
        pair
    } else {
        match (begin, end) {
            (Some(b), Some(e)) => (b.to_string(), e.to_string()),
            _ => {
                return Err(crate::commands::sink::CodedError::invalid_input(
                    "since",
                    "supply --since <dur> OR --begin+--end",
                )
                .into())
            }
        }
    };

    let mut query: Vec<(&str, &str)> = vec![
        ("chainIndex", chain_index),
        ("walletAddress", address),
        ("begin", begin_val.as_str()),
        ("end", end_val.as_str()),
    ];
    if let Some(l) = limit {
        query.push(("limit", l));
    }
    if let Some(c) = cursor {
        query.push(("cursor", c));
    }
    if let Some(t) = token {
        query.push(("tokenContractAddress", t));
    }
    if let Some(ty) = tx_type {
        query.push(("type", ty));
    }
    let mut data = client
        .get("/api/v6/dex/market/portfolio/dex-history", &query)
        .await?;
    if let (Some(w), Some(obj)) = (resolved_window, data.as_object_mut()) {
        obj.insert("resolvedWindow".to_string(), serde_json::to_value(w)?);
    }
    Ok(data)
}

#[allow(clippy::too_many_arguments)]
async fn portfolio_dex_history(
    ctx: &Context,
    address: &str,
    chain: &str,
    begin: Option<&str>,
    end: Option<&str>,
    since: Option<&str>,
    limit: Option<&str>,
    cursor: Option<&str>,
    token: Option<&str>,
    tx_type: Option<&str>,
) -> Result<()> {
    let chain_index = crate::chains::resolve_chain(chain);
    let mut client = ctx.client_async().await?;
    output::success(
        fetch_portfolio_dex_history(
            &mut client,
            &chain_index,
            address,
            begin,
            end,
            since,
            limit,
            cursor,
            token,
            tx_type,
        )
        .await?,
    );
    Ok(())
}

/// GET /api/v6/dex/market/portfolio/recent-pnl
pub async fn fetch_portfolio_recent_pnl(
    client: &mut ApiClient,
    chain_index: &str,
    address: &str,
    limit: Option<&str>,
    cursor: Option<&str>,
) -> Result<Value> {
    let mut query: Vec<(&str, &str)> =
        vec![("chainIndex", chain_index), ("walletAddress", address)];
    if let Some(l) = limit {
        query.push(("limit", l));
    }
    if let Some(c) = cursor {
        query.push(("cursor", c));
    }
    client
        .get("/api/v6/dex/market/portfolio/recent-pnl", &query)
        .await
}

async fn portfolio_recent_pnl(
    ctx: &Context,
    address: &str,
    chain: &str,
    limit: Option<&str>,
    cursor: Option<&str>,
) -> Result<()> {
    let chain_index = crate::chains::resolve_chain(chain);
    let mut client = ctx.client_async().await?;
    output::success(
        fetch_portfolio_recent_pnl(&mut client, &chain_index, address, limit, cursor).await?,
    );
    Ok(())
}

/// GET /api/v6/dex/market/portfolio/token/latest-pnl
pub async fn fetch_portfolio_token_pnl(
    client: &mut ApiClient,
    chain_index: &str,
    address: &str,
    token: &str,
) -> Result<Value> {
    client
        .get(
            "/api/v6/dex/market/portfolio/token/latest-pnl",
            &[
                ("chainIndex", chain_index),
                ("walletAddress", address),
                ("tokenContractAddress", token),
            ],
        )
        .await
}

async fn portfolio_token_pnl(ctx: &Context, address: &str, chain: &str, token: &str) -> Result<()> {
    let chain_index = crate::chains::resolve_chain(chain);
    let mut client = ctx.client_async().await?;
    output::success(fetch_portfolio_token_pnl(&mut client, &chain_index, address, token).await?);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// FR-2: the kline transform only reshapes candle arrays; a top-level object
    /// carrying `requestTime` must pass through unchanged (guards the `other => other`
    /// branch against a future rewrite that drops sibling fields).
    #[test]
    fn request_time_survives_kline_transform() {
        let v = json!({ "requestTime": 1_721_000_000_000u64, "candles": [] });
        let out = kline_to_named_objects(v.clone());
        assert_eq!(out, v);
        assert_eq!(out["requestTime"], 1_721_000_000_000u64);
    }

    #[test]
    fn kline_transform_names_candle_array_fields() {
        let raw = json!([["1700000000000", "1.0", "2.0", "0.5", "1.5", "10", "15", "1"]]);
        let out = kline_to_named_objects(raw);
        assert_eq!(out[0]["ts"], "1700000000000");
        assert_eq!(out[0]["o"], "1.0");
        assert_eq!(out[0]["confirm"], "1");
    }
}
