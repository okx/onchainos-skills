use anyhow::Result;
use clap::Subcommand;
use serde_json::json;

use super::Context;
use crate::output;

#[derive(Subcommand)]
pub enum TokenCommand {
    /// Search for tokens by name, symbol, or address
    Search {
        /// Search keyword (name, symbol, or contract address)
        query: String,
        /// Chains to search (comma-separated, e.g. "ethereum,solana")
        #[arg(long, default_value = "1,501")]
        chains: String,
    },
    /// Get token basic info (name, symbol, decimals, logo)
    Info {
        /// Token contract address
        address: String,
        /// Chain
        #[arg(long)]
        chain: Option<String>,
    },
    /// Get token holder distribution (top 20)
    Holders {
        /// Token contract address
        address: String,
        /// Chain
        #[arg(long)]
        chain: Option<String>,
    },
    /// Get trending / top tokens
    Trending {
        /// Chains (comma-separated)
        #[arg(long, default_value = "1,501")]
        chains: String,
        /// Sort by: 2=price change, 5=volume, 6=market cap
        #[arg(long, default_value = "5")]
        sort_by: String,
        /// Time frame: 1=5min, 2=1h, 3=4h, 4=24h
        #[arg(long, default_value = "4")]
        time_frame: String,
    },
    /// Get detailed price info (price, market cap, liquidity, volume, 24h change)
    PriceInfo {
        /// Token contract address
        address: String,
        /// Chain
        #[arg(long)]
        chain: Option<String>,
    },
}

pub async fn execute(ctx: &Context, cmd: TokenCommand) -> Result<()> {
    match cmd {
        TokenCommand::Search { query, chains } => search(ctx, &query, &chains).await,
        TokenCommand::Info { address, chain } => info(ctx, &address, chain).await,
        TokenCommand::Holders { address, chain } => holders(ctx, &address, chain).await,
        TokenCommand::Trending {
            chains,
            sort_by,
            time_frame,
        } => trending(ctx, &chains, &sort_by, &time_frame).await,
        TokenCommand::PriceInfo { address, chain } => price_info(ctx, &address, chain).await,
    }
}

/// GET /api/v6/dex/market/token/search
async fn search(ctx: &Context, query: &str, chains: &str) -> Result<()> {
    let resolved_chains = crate::chains::resolve_chains(chains);
    let client = ctx.client()?;
    let data = client
        .get(
            "/api/v6/dex/market/token/search",
            &[("chains", resolved_chains.as_str()), ("search", query)],
        )
        .await?;
    output::success(data);
    Ok(())
}

/// POST /api/v6/dex/market/token/basic-info — body is JSON array
async fn info(ctx: &Context, address: &str, chain: Option<String>) -> Result<()> {
    let chain_index = chain
        .map(|c| crate::chains::resolve_chain(&c).to_string())
        .unwrap_or_else(|| ctx.chain_index_or("ethereum"));
    let client = ctx.client()?;
    let body = json!([{
        "chainIndex": chain_index,
        "tokenContractAddress": address
    }]);
    let data = client
        .post("/api/v6/dex/market/token/basic-info", &body)
        .await?;
    output::success(data);
    Ok(())
}

/// GET /api/v6/dex/market/token/holder
async fn holders(ctx: &Context, address: &str, chain: Option<String>) -> Result<()> {
    let chain_index = chain
        .map(|c| crate::chains::resolve_chain(&c).to_string())
        .unwrap_or_else(|| ctx.chain_index_or("ethereum"));
    let client = ctx.client()?;
    let data = client
        .get(
            "/api/v6/dex/market/token/holder",
            &[
                ("chainIndex", chain_index.as_str()),
                ("tokenContractAddress", address),
            ],
        )
        .await?;
    output::success(data);
    Ok(())
}

/// GET /api/v6/dex/market/token/toplist
async fn trending(ctx: &Context, chains: &str, sort_by: &str, time_frame: &str) -> Result<()> {
    let resolved_chains = crate::chains::resolve_chains(chains);
    let client = ctx.client()?;
    let data = client
        .get(
            "/api/v6/dex/market/token/toplist",
            &[
                ("chains", resolved_chains.as_str()),
                ("sortBy", sort_by),
                ("timeFrame", time_frame),
            ],
        )
        .await?;
    output::success(data);
    Ok(())
}

/// POST /api/v6/dex/market/price-info — body is JSON array
async fn price_info(ctx: &Context, address: &str, chain: Option<String>) -> Result<()> {
    let chain_index = chain
        .map(|c| crate::chains::resolve_chain(&c).to_string())
        .unwrap_or_else(|| ctx.chain_index_or("ethereum"));
    let client = ctx.client()?;
    let body = json!([{
        "chainIndex": chain_index,
        "tokenContractAddress": address
    }]);
    let data = client.post("/api/v6/dex/market/price-info", &body).await?;
    output::success(data);
    Ok(())
}
