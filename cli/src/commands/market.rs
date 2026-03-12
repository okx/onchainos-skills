use anyhow::Result;
use clap::Subcommand;
use serde_json::{json, Value};

use super::Context;
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
        /// Time frame: 1=1D, 2=3D, 3=7D, 4=1M, 5=3M
        #[arg(long)]
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
        /// Start timestamp (milliseconds)
        #[arg(long)]
        begin: String,
        /// End timestamp (milliseconds)
        #[arg(long)]
        end: String,
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
    match cmd {
        MarketCommand::Price { address, chain } => price(ctx, &address, chain).await,
        MarketCommand::Prices { tokens, chain } => prices(ctx, &tokens, chain).await,
        MarketCommand::Kline {
            address,
            bar,
            limit,
            chain,
        } => kline(ctx, &address, &bar, limit, chain).await,
        MarketCommand::Index { address, chain } => index(ctx, &address, chain).await,
        MarketCommand::PortfolioSupportedChains => portfolio_supported_chains(ctx).await,
        MarketCommand::PortfolioOverview {
            address,
            chain,
            time_frame,
        } => portfolio_overview(ctx, &address, &chain, &time_frame).await,
        MarketCommand::PortfolioDexHistory {
            address,
            chain,
            begin,
            end,
            limit,
            cursor,
            token,
            tx_type,
        } => {
            portfolio_dex_history(
                ctx,
                &address,
                &chain,
                &begin,
                &end,
                limit.as_deref(),
                cursor.as_deref(),
                token.as_deref(),
                tx_type.as_deref(),
            )
            .await
        }
        MarketCommand::PortfolioRecentPnl {
            address,
            chain,
            limit,
            cursor,
        } => portfolio_recent_pnl(ctx, &address, &chain, limit.as_deref(), cursor.as_deref()).await,
        MarketCommand::PortfolioTokenPnl {
            address,
            chain,
            token,
        } => portfolio_token_pnl(ctx, &address, &chain, &token).await,
    }
}

/// POST /api/v6/dex/market/price — body is JSON array
async fn price(ctx: &Context, address: &str, chain: Option<String>) -> Result<()> {
    let chain_index = chain
        .map(|c| crate::chains::resolve_chain(&c).to_string())
        .unwrap_or_else(|| ctx.chain_index_or("ethereum"));
    let client = ctx.client()?;
    let body = json!([{
        "chainIndex": chain_index,
        "tokenContractAddress": address
    }]);
    let data = client.post("/api/v6/dex/market/price", &body).await?;
    output::success(data);
    Ok(())
}

/// POST /api/v6/dex/market/price — batch query
async fn prices(ctx: &Context, tokens: &str, chain: Option<String>) -> Result<()> {
    let default_chain = chain
        .map(|c| crate::chains::resolve_chain(&c).to_string())
        .unwrap_or_else(|| ctx.chain_index_or("ethereum"));
    let mut items: Vec<Value> = Vec::new();
    for pair in tokens.split(',') {
        let pair = pair.trim();
        if let Some((chain_part, addr)) = pair.split_once(':') {
            items.push(json!({
                "chainIndex": crate::chains::resolve_chain(chain_part),
                "tokenContractAddress": addr
            }));
        } else {
            items.push(json!({
                "chainIndex": &default_chain,
                "tokenContractAddress": pair
            }));
        }
    }
    let client = ctx.client()?;
    let data = client
        .post("/api/v6/dex/market/price", &Value::Array(items))
        .await?;
    output::success(data);
    Ok(())
}

/// GET /api/v6/dex/market/candles
async fn kline(
    ctx: &Context,
    address: &str,
    bar: &str,
    limit: u32,
    chain: Option<String>,
) -> Result<()> {
    let chain_index = chain
        .map(|c| crate::chains::resolve_chain(&c).to_string())
        .unwrap_or_else(|| ctx.chain_index_or("ethereum"));
    let limit_str = limit.to_string();
    let client = ctx.client()?;
    let data = client
        .get(
            "/api/v6/dex/market/candles",
            &[
                ("chainIndex", chain_index.as_str()),
                ("tokenContractAddress", address),
                ("bar", bar),
                ("limit", &limit_str),
            ],
        )
        .await?;
    output::success(data);
    Ok(())
}

/// POST /api/v6/dex/index/current-price — body is JSON array
async fn index(ctx: &Context, address: &str, chain: Option<String>) -> Result<()> {
    let chain_index = chain
        .map(|c| crate::chains::resolve_chain(&c).to_string())
        .unwrap_or_else(|| ctx.chain_index_or("ethereum"));
    let client = ctx.client()?;
    let body = json!([{
        "chainIndex": chain_index,
        "tokenContractAddress": address
    }]);
    let data = client
        .post("/api/v6/dex/index/current-price", &body)
        .await?;
    output::success(data);
    Ok(())
}

/// GET /api/v6/dex/market/portfolio/supported/chain
async fn portfolio_supported_chains(ctx: &Context) -> Result<()> {
    let client = ctx.client()?;
    let data = client
        .get("/api/v6/dex/market/portfolio/supported/chain", &[])
        .await?;
    output::success(data);
    Ok(())
}

/// GET /api/v6/dex/market/portfolio/overview
async fn portfolio_overview(
    ctx: &Context,
    address: &str,
    chain: &str,
    time_frame: &str,
) -> Result<()> {
    let chain_index = crate::chains::resolve_chain(chain);
    let client = ctx.client()?;
    let query: Vec<(&str, &str)> = vec![
        ("chainIndex", chain_index.as_str()),
        ("walletAddress", address),
        ("timeFrame", time_frame),
    ];
    let data = client
        .get("/api/v6/dex/market/portfolio/overview", &query)
        .await?;
    output::success(data);
    Ok(())
}

/// GET /api/v6/dex/market/portfolio/dex-history
#[allow(clippy::too_many_arguments)]
async fn portfolio_dex_history(
    ctx: &Context,
    address: &str,
    chain: &str,
    begin: &str,
    end: &str,
    limit: Option<&str>,
    cursor: Option<&str>,
    token: Option<&str>,
    tx_type: Option<&str>,
) -> Result<()> {
    let chain_index = crate::chains::resolve_chain(chain);
    let client = ctx.client()?;
    let mut query: Vec<(&str, &str)> = vec![
        ("chainIndex", chain_index.as_str()),
        ("walletAddress", address),
        ("begin", begin),
        ("end", end),
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
    let data = client
        .get("/api/v6/dex/market/portfolio/dex-history", &query)
        .await?;
    output::success(data);
    Ok(())
}

/// GET /api/v6/dex/market/portfolio/recent-pnl
async fn portfolio_recent_pnl(
    ctx: &Context,
    address: &str,
    chain: &str,
    limit: Option<&str>,
    cursor: Option<&str>,
) -> Result<()> {
    let chain_index = crate::chains::resolve_chain(chain);
    let client = ctx.client()?;
    let mut query: Vec<(&str, &str)> = vec![
        ("chainIndex", chain_index.as_str()),
        ("walletAddress", address),
    ];
    if let Some(l) = limit {
        query.push(("limit", l));
    }
    if let Some(c) = cursor {
        query.push(("cursor", c));
    }
    let data = client
        .get("/api/v6/dex/market/portfolio/recent-pnl", &query)
        .await?;
    output::success(data);
    Ok(())
}

/// GET /api/v6/dex/market/portfolio/token/latest-pnl
async fn portfolio_token_pnl(ctx: &Context, address: &str, chain: &str, token: &str) -> Result<()> {
    let chain_index = crate::chains::resolve_chain(chain);
    let client = ctx.client()?;
    let query: Vec<(&str, &str)> = vec![
        ("chainIndex", chain_index.as_str()),
        ("walletAddress", address),
        ("tokenContractAddress", token),
    ];
    let data = client
        .get("/api/v6/dex/market/portfolio/token/latest-pnl", &query)
        .await?;
    output::success(data);
    Ok(())
}
