use anyhow::Result;
use clap::Subcommand;
use serde_json::json;

use super::Context;
use crate::output;

#[derive(Subcommand)]
pub enum PortfolioCommand {
    /// Get supported chains for balance queries
    Chains,
    /// Get total asset value for a wallet address
    TotalValue {
        /// Wallet address
        #[arg(long)]
        address: String,
        /// Chain IDs or names, comma-separated (e.g. "xlayer,solana,ethereum")
        #[arg(long)]
        chains: String,
        /// Asset type: 0=all (default), 1=tokens only, 2=DeFi only
        #[arg(long)]
        asset_type: Option<String>,
        /// Exclude risky tokens (default true). Only ETH/BSC/SOL/BASE
        #[arg(long)]
        exclude_risk: Option<bool>,
    },
    /// Get all token balances for a wallet address
    AllBalances {
        /// Wallet address
        #[arg(long)]
        address: String,
        /// Chain IDs or names, comma-separated (e.g. "xlayer,solana,ethereum")
        #[arg(long)]
        chains: String,
        /// Exclude risky tokens: 0=filter out (default), 1=include. Only ETH/BSC/SOL/BASE
        #[arg(long)]
        exclude_risk: Option<String>,
    },
    /// Get specific token balances for a wallet address
    TokenBalances {
        /// Wallet address
        #[arg(long)]
        address: String,
        /// Token list: "chainIndex:tokenAddress" pairs, comma-separated (e.g. "196:,196:0x74b7...")
        /// Use empty address for native token (e.g. "196:" for native OKB)
        #[arg(long)]
        tokens: String,
        /// Exclude risky tokens: 0=filter out (default), 1=include
        #[arg(long)]
        exclude_risk: Option<String>,
    },
    /// Get wallet portfolio overview: realized/unrealized PnL, win rate, trading stats
    Overview {
        /// Wallet address
        #[arg(long)]
        address: String,
        /// Chain name or ID (e.g. ethereum, solana, xlayer)
        #[arg(long)]
        chain: String,
        /// Time frame: 1d, 3d, 7d, 1m, 3m
        #[arg(long, default_value = "7d")]
        time_frame: String,
    },
    /// Get wallet DEX transaction history (paginated)
    DexHistory {
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
        /// Filter by token contract address
        #[arg(long)]
        token: Option<String>,
        /// Transaction type: 1=buy, 2=sell, 3=transfer-in, 4=transfer-out, 0=all (comma-separated)
        #[arg(long = "tx-type")]
        tx_type: Option<String>,
    },
    /// Get recent token PnL records for a wallet (paginated)
    RecentPnl {
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
    TokenPnl {
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

pub async fn execute(ctx: &Context, cmd: PortfolioCommand) -> Result<()> {
    match cmd {
        PortfolioCommand::Chains => chains(ctx).await,
        PortfolioCommand::TotalValue {
            address,
            chains,
            asset_type,
            exclude_risk,
        } => total_value(ctx, &address, &chains, asset_type.as_deref(), exclude_risk).await,
        PortfolioCommand::AllBalances {
            address,
            chains,
            exclude_risk,
        } => all_balances(ctx, &address, &chains, exclude_risk.as_deref()).await,
        PortfolioCommand::TokenBalances {
            address,
            tokens,
            exclude_risk,
        } => token_balances(ctx, &address, &tokens, exclude_risk.as_deref()).await,
        PortfolioCommand::Overview {
            address,
            chain,
            time_frame,
        } => overview(ctx, &address, &chain, &time_frame).await,
        PortfolioCommand::DexHistory {
            address,
            chain,
            limit,
            cursor,
            token,
            tx_type,
        } => {
            dex_history(
                ctx,
                &address,
                &chain,
                limit.as_deref(),
                cursor.as_deref(),
                token.as_deref(),
                tx_type.as_deref(),
            )
            .await
        }
        PortfolioCommand::RecentPnl {
            address,
            chain,
            limit,
            cursor,
        } => recent_pnl(ctx, &address, &chain, limit.as_deref(), cursor.as_deref()).await,
        PortfolioCommand::TokenPnl {
            address,
            chain,
            token,
        } => token_pnl(ctx, &address, &chain, &token).await,
    }
}

/// GET /api/v6/dex/balance/supported/chain
async fn chains(ctx: &Context) -> Result<()> {
    let client = ctx.client()?;
    let data = client
        .get("/api/v6/dex/balance/supported/chain", &[])
        .await?;
    output::success(data);
    Ok(())
}

/// GET /api/v6/dex/balance/total-value-by-address
async fn total_value(
    ctx: &Context,
    address: &str,
    chains: &str,
    asset_type: Option<&str>,
    exclude_risk: Option<bool>,
) -> Result<()> {
    let chain_indices = crate::chains::resolve_chains(chains);
    let client = ctx.client()?;
    let mut query: Vec<(&str, String)> = vec![
        ("address", address.to_string()),
        ("chains", chain_indices.clone()),
    ];
    if let Some(at) = asset_type {
        query.push(("assetType", at.to_string()));
    }
    if let Some(er) = exclude_risk {
        query.push(("excludeRiskToken", er.to_string()));
    }
    let query_refs: Vec<(&str, &str)> = query.iter().map(|(k, v)| (*k, v.as_str())).collect();
    let data = client
        .get("/api/v6/dex/balance/total-value-by-address", &query_refs)
        .await?;
    output::success(data);
    Ok(())
}

/// GET /api/v6/dex/balance/all-token-balances-by-address
async fn all_balances(
    ctx: &Context,
    address: &str,
    chains: &str,
    exclude_risk: Option<&str>,
) -> Result<()> {
    let chain_indices = crate::chains::resolve_chains(chains);
    let client = ctx.client()?;
    let mut query: Vec<(&str, String)> = vec![
        ("address", address.to_string()),
        ("chains", chain_indices.clone()),
    ];
    if let Some(er) = exclude_risk {
        query.push(("excludeRiskToken", er.to_string()));
    }
    let query_refs: Vec<(&str, &str)> = query.iter().map(|(k, v)| (*k, v.as_str())).collect();
    let data = client
        .get(
            "/api/v6/dex/balance/all-token-balances-by-address",
            &query_refs,
        )
        .await?;
    output::success(data);
    Ok(())
}

/// GET /api/v6/dex/market/portfolio/overview
async fn overview(
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
async fn dex_history(
    ctx: &Context,
    address: &str,
    chain: &str,
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
async fn recent_pnl(
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
async fn token_pnl(
    ctx: &Context,
    address: &str,
    chain: &str,
    token: &str,
) -> Result<()> {
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

/// POST /api/v6/dex/balance/token-balances-by-address
async fn token_balances(
    ctx: &Context,
    address: &str,
    tokens: &str,
    exclude_risk: Option<&str>,
) -> Result<()> {
    let client = ctx.client()?;

    // Parse "chainIndex:tokenAddress" pairs
    let token_list: Vec<serde_json::Value> = tokens
        .split(',')
        .map(|pair| {
            let parts: Vec<&str> = pair.splitn(2, ':').collect();
            let chain_index = if parts.is_empty() { "" } else { parts[0] };
            let token_address = if parts.len() > 1 { parts[1] } else { "" };
            // Resolve chain name to index if not numeric
            let resolved_chain = crate::chains::resolve_chain(chain_index);
            json!({
                "chainIndex": resolved_chain,
                "tokenContractAddress": token_address
            })
        })
        .collect();

    let mut body = json!({
        "address": address,
        "tokenContractAddresses": token_list,
    });
    if let Some(er) = exclude_risk {
        body["excludeRiskToken"] = json!(er);
    }

    let data = client
        .post("/api/v6/dex/balance/token-balances-by-address", &body)
        .await?;
    output::success(data);
    Ok(())
}
