use anyhow::Result;
use clap::Subcommand;
use serde_json::{json, Value};

use super::Context;
use crate::output;

#[derive(Subcommand)]
#[allow(clippy::large_enum_variant)]
pub enum SignalCommand {
    /// Get supported chains for market signals
    Chains,
    /// Get latest signal list (smart money / KOL / whale activity)
    List {
        /// Chain (e.g. ethereum, solana, base). Required.
        #[arg(long)]
        chain: String,
        /// Wallet type filter: 1=Smart Money, 2=KOL/Influencer, 3=Whales (comma-separated, e.g. "1,2")
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

pub async fn execute(ctx: &Context, cmd: SignalCommand) -> Result<()> {
    match cmd {
        SignalCommand::Chains => signal_chains(ctx).await,
        SignalCommand::List {
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
