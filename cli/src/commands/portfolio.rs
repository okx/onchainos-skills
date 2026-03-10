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
