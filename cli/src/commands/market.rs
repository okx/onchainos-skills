use anyhow::Result;
use clap::Subcommand;
use serde_json::{json, Value};

use super::Context;
use crate::output;

#[derive(Subcommand)]
pub enum MarketCommand {
    /// Get token price (by contract address)
    Price {
        /// Token contract address
        address: String,
        /// Chain (e.g. ethereum, solana, xlayer)
        #[arg(long)]
        chain: Option<String>,
    },
    /// Get prices for multiple tokens (POST, batch query)
    Prices {
        /// Comma-separated chainIndex:address pairs (e.g. "1:0xeee...,501:1111...")
        tokens: String,
        /// Default chain if not specified per token
        #[arg(long)]
        chain: Option<String>,
    },
    /// Get K-line / candlestick data
    Kline {
        /// Token contract address
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
    /// Get recent trades
    Trades {
        /// Token contract address
        address: String,
        /// Chain
        #[arg(long)]
        chain: Option<String>,
        /// Number of trades (max 500)
        #[arg(long, default_value = "100")]
        limit: u32,
    },
    /// Get index price (aggregated from multiple sources)
    Index {
        /// Token contract address (empty string for native token)
        address: String,
        /// Chain
        #[arg(long)]
        chain: Option<String>,
    },
    /// Get supported chains for market signals
    SignalChains,
    /// Get latest signal list (smart money / KOL / whale activity)
    SignalList {
        /// Chain (e.g. ethereum, solana, base). Required.
        chain: String,
        /// Wallet type filter: 1=Smart Money, 2=KOL/Influencer, 3=Whales (comma-separated for multiple, e.g. "1,2")
        #[arg(long)]
        wallet_type: Option<String>,
        /// Minimum transaction amount in USD
        #[arg(long)]
        min_amount_usd: Option<String>,
        /// Maximum transaction amount in USD
        #[arg(long)]
        max_amount_usd: Option<String>,
        /// Minimum triggering wallet address count
        #[arg(long)]
        min_address_count: Option<String>,
        /// Maximum triggering wallet address count
        #[arg(long)]
        max_address_count: Option<String>,
        /// Token contract address (filter signals for a specific token)
        #[arg(long)]
        token_address: Option<String>,
        /// Minimum token market cap in USD
        #[arg(long)]
        min_market_cap_usd: Option<String>,
        /// Maximum token market cap in USD
        #[arg(long)]
        max_market_cap_usd: Option<String>,
        /// Minimum token liquidity in USD
        #[arg(long)]
        min_liquidity_usd: Option<String>,
        /// Maximum token liquidity in USD
        #[arg(long)]
        max_liquidity_usd: Option<String>,
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
        MarketCommand::Trades {
            address,
            chain,
            limit,
        } => trades(ctx, &address, chain, limit).await,
        MarketCommand::Index { address, chain } => index(ctx, &address, chain).await,
        MarketCommand::SignalChains => signal_chains(ctx).await,
        MarketCommand::SignalList {
            chain,
            wallet_type,
            min_amount_usd,
            max_amount_usd,
            min_address_count,
            max_address_count,
            token_address,
            min_market_cap_usd,
            max_market_cap_usd,
            min_liquidity_usd,
            max_liquidity_usd,
        } => {
            signal_list(
                ctx,
                &chain,
                wallet_type,
                min_amount_usd,
                max_amount_usd,
                min_address_count,
                max_address_count,
                token_address,
                min_market_cap_usd,
                max_market_cap_usd,
                min_liquidity_usd,
                max_liquidity_usd,
            )
            .await
        }
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

/// GET /api/v6/dex/market/trades
async fn trades(ctx: &Context, address: &str, chain: Option<String>, limit: u32) -> Result<()> {
    let chain_index = chain
        .map(|c| crate::chains::resolve_chain(&c).to_string())
        .unwrap_or_else(|| ctx.chain_index_or("ethereum"));
    let limit_str = limit.to_string();
    let client = ctx.client()?;
    let data = client
        .get(
            "/api/v6/dex/market/trades",
            &[
                ("chainIndex", chain_index.as_str()),
                ("tokenContractAddress", address),
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

/// GET /api/v6/dex/market/signal/supported/chain — no parameters
async fn signal_chains(ctx: &Context) -> Result<()> {
    let client = ctx.client()?;
    let data = client
        .get("/api/v6/dex/market/signal/supported/chain", &[])
        .await?;
    output::success(data);
    Ok(())
}

/// POST /api/v6/dex/market/signal/list — smart money / KOL / whale signals
#[allow(clippy::too_many_arguments)]
async fn signal_list(
    ctx: &Context,
    chain: &str,
    wallet_type: Option<String>,
    min_amount_usd: Option<String>,
    max_amount_usd: Option<String>,
    min_address_count: Option<String>,
    max_address_count: Option<String>,
    token_address: Option<String>,
    min_market_cap_usd: Option<String>,
    max_market_cap_usd: Option<String>,
    min_liquidity_usd: Option<String>,
    max_liquidity_usd: Option<String>,
) -> Result<()> {
    let chain_index = crate::chains::resolve_chain(chain).to_string();
    let client = ctx.client()?;

    let mut body = json!({
        "chainIndex": chain_index
    });
    let obj = body.as_object_mut().unwrap();
    if let Some(v) = wallet_type {
        obj.insert("walletType".into(), Value::String(v));
    }
    if let Some(v) = min_amount_usd {
        obj.insert("minAmountUsd".into(), Value::String(v));
    }
    if let Some(v) = max_amount_usd {
        obj.insert("maxAmountUsd".into(), Value::String(v));
    }
    if let Some(v) = min_address_count {
        obj.insert("minAddressCount".into(), Value::String(v));
    }
    if let Some(v) = max_address_count {
        obj.insert("maxAddressCount".into(), Value::String(v));
    }
    if let Some(v) = token_address {
        obj.insert("tokenAddress".into(), Value::String(v));
    }
    if let Some(v) = min_market_cap_usd {
        obj.insert("minMarketCapUsd".into(), Value::String(v));
    }
    if let Some(v) = max_market_cap_usd {
        obj.insert("maxMarketCapUsd".into(), Value::String(v));
    }
    if let Some(v) = min_liquidity_usd {
        obj.insert("minLiquidityUsd".into(), Value::String(v));
    }
    if let Some(v) = max_liquidity_usd {
        obj.insert("maxLiquidityUsd".into(), Value::String(v));
    }

    let data = client.post("/api/v6/dex/market/signal/list", &body).await?;
    output::success(data);
    Ok(())
}
