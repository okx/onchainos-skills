use anyhow::Result;
use clap::Subcommand;
use serde_json::{json, Value};

use super::Context;
use crate::client::ApiClient;
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
    let client = ctx.client()?;
    match cmd {
        TokenCommand::Search { query, chains } => {
            output::success(fetch_search(&client, &query, &chains).await?);
        }
        TokenCommand::Info { address, chain } => {
            let chain_index = chain
                .map(|c| crate::chains::resolve_chain(&c).to_string())
                .unwrap_or_else(|| ctx.chain_index_or("ethereum"));
            output::success(fetch_info(&client, &address, &chain_index).await?);
        }
        TokenCommand::Holders { address, chain } => {
            let chain_index = chain
                .map(|c| crate::chains::resolve_chain(&c).to_string())
                .unwrap_or_else(|| ctx.chain_index_or("ethereum"));
            output::success(fetch_holders(&client, &address, &chain_index).await?);
        }
        TokenCommand::Trending { chains, sort_by, time_frame } => {
            output::success(fetch_trending(&client, &chains, &sort_by, &time_frame).await?);
        }
        TokenCommand::PriceInfo { address, chain } => {
            let chain_index = chain
                .map(|c| crate::chains::resolve_chain(&c).to_string())
                .unwrap_or_else(|| ctx.chain_index_or("ethereum"));
            output::success(fetch_price_info(&client, &address, &chain_index).await?);
        }
    }
    Ok(())
}

/// GET /api/v6/dex/market/token/search
pub async fn fetch_search(client: &ApiClient, query: &str, chains: &str) -> Result<Value> {
    let resolved_chains = crate::chains::resolve_chains(chains);
    client
        .get(
            "/api/v6/dex/market/token/search",
            &[("chains", resolved_chains.as_str()), ("search", query)],
        )
        .await
}

/// POST /api/v6/dex/market/token/basic-info — body is JSON array
pub async fn fetch_info(client: &ApiClient, address: &str, chain_index: &str) -> Result<Value> {
    let body = json!([{"chainIndex": chain_index, "tokenContractAddress": address}]);
    client.post("/api/v6/dex/market/token/basic-info", &body).await
}

/// GET /api/v6/dex/market/token/holder
pub async fn fetch_holders(client: &ApiClient, address: &str, chain_index: &str) -> Result<Value> {
    client
        .get(
            "/api/v6/dex/market/token/holder",
            &[
                ("chainIndex", chain_index),
                ("tokenContractAddress", address),
            ],
        )
        .await
}

/// GET /api/v6/dex/market/token/toplist
pub async fn fetch_trending(
    client: &ApiClient,
    chains: &str,
    sort_by: &str,
    time_frame: &str,
) -> Result<Value> {
    let resolved_chains = crate::chains::resolve_chains(chains);
    client
        .get(
            "/api/v6/dex/market/token/toplist",
            &[
                ("chains", resolved_chains.as_str()),
                ("sortBy", sort_by),
                ("timeFrame", time_frame),
            ],
        )
        .await
}

/// POST /api/v6/dex/market/price-info — body is JSON array
pub async fn fetch_price_info(
    client: &ApiClient,
    address: &str,
    chain_index: &str,
) -> Result<Value> {
    let body = json!([{"chainIndex": chain_index, "tokenContractAddress": address}]);
    client.post("/api/v6/dex/market/price-info", &body).await
}
