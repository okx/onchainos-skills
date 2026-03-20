use anyhow::Result;
use clap::Subcommand;
use serde_json::{json, Value};

use super::Context;
use crate::client::ApiClient;
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
        /// Token filter level: 0=default (filters risk/custom/passive tokens), 1=return all tokens
        /// Use 1 when you need the full token list including risk tokens (e.g. for security scanning)
        #[arg(long)]
        filter: Option<String>,
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
    let client = ctx.client_async().await?;
    match cmd {
        PortfolioCommand::Chains => {
            output::success(fetch_chains(&client).await?);
        }
        PortfolioCommand::TotalValue {
            address,
            chains,
            asset_type,
            exclude_risk,
        } => {
            let er = exclude_risk.map(|b| b.to_string());
            output::success(
                fetch_total_value(
                    &client,
                    &address,
                    &chains,
                    asset_type.as_deref(),
                    er.as_deref(),
                )
                .await?,
            );
        }
        PortfolioCommand::AllBalances {
            address,
            chains,
            exclude_risk,
            filter,
        } => {
            output::success(
                fetch_all_balances(
                    &client,
                    &address,
                    &chains,
                    exclude_risk.as_deref(),
                    filter.as_deref(),
                )
                .await?,
            );
        }
        PortfolioCommand::TokenBalances {
            address,
            tokens,
            exclude_risk,
        } => {
            output::success(
                fetch_token_balances(&client, &address, &tokens, exclude_risk.as_deref()).await?,
            );
        }
    }
    Ok(())
}

/// GET /api/v6/dex/balance/supported/chain
pub async fn fetch_chains(client: &ApiClient) -> Result<Value> {
    client.get("/api/v6/dex/balance/supported/chain", &[]).await
}

/// GET /api/v6/dex/balance/total-value-by-address
pub async fn fetch_total_value(
    client: &ApiClient,
    address: &str,
    chains: &str,
    asset_type: Option<&str>,
    exclude_risk: Option<&str>,
) -> Result<Value> {
    let chain_indices = crate::chains::resolve_chains(chains);
    let mut query: Vec<(&str, String)> =
        vec![("address", address.to_string()), ("chains", chain_indices)];
    if let Some(at) = asset_type {
        query.push(("assetType", at.to_string()));
    }
    if let Some(er) = exclude_risk {
        query.push(("excludeRiskToken", er.to_string()));
    }
    let query_refs: Vec<(&str, &str)> = query.iter().map(|(k, v)| (*k, v.as_str())).collect();
    client
        .get("/api/v6/dex/balance/total-value-by-address", &query_refs)
        .await
}

/// GET /api/v6/dex/balance/all-token-balances-by-address
pub async fn fetch_all_balances(
    client: &ApiClient,
    address: &str,
    chains: &str,
    exclude_risk: Option<&str>,
    filter: Option<&str>,
) -> Result<Value> {
    let chain_indices = crate::chains::resolve_chains(chains);
    let mut query: Vec<(&str, String)> =
        vec![("address", address.to_string()), ("chains", chain_indices)];
    if let Some(er) = exclude_risk {
        query.push(("excludeRiskToken", er.to_string()));
    }
    if let Some(f) = filter {
        query.push(("filter", f.to_string()));
    }
    let query_refs: Vec<(&str, &str)> = query.iter().map(|(k, v)| (*k, v.as_str())).collect();
    client
        .get(
            "/api/v6/dex/balance/all-token-balances-by-address",
            &query_refs,
        )
        .await
}

/// POST /api/v6/dex/balance/token-balances-by-address
pub async fn fetch_token_balances(
    client: &ApiClient,
    address: &str,
    tokens: &str,
    exclude_risk: Option<&str>,
) -> Result<Value> {
    let token_list: Vec<Value> = tokens
        .split(',')
        .map(|pair| {
            let parts: Vec<&str> = pair.splitn(2, ':').collect();
            let chain_index = if parts.is_empty() { "" } else { parts[0] };
            let token_address = if parts.len() > 1 { parts[1] } else { "" };
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

    client
        .post("/api/v6/dex/balance/token-balances-by-address", &body)
        .await
}
