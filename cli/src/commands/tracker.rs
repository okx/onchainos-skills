use anyhow::Result;
use clap::Subcommand;
use serde_json::Value;

use super::Context;
use crate::client::ApiClient;
use crate::output;

#[derive(Subcommand)]
pub enum TrackerCommand {
    /// Get latest DEX activities for tracked addresses (smart money, KOL, or custom multi-address)
    Activities {
        /// Tracker type: smart_money (or 1), kol (or 2), multi_address (or 3)
        #[arg(long)]
        tracker_type: String,
        /// Wallet addresses (required for multi_address), comma-separated, max 20
        #[arg(long)]
        wallet_address: Option<String>,
        /// Trade type: 0=all (default), 1=buy, 2=sell
        #[arg(long)]
        trade_type: Option<String>,
        /// Chain filter (e.g. ethereum, solana). Omit for all chains
        #[arg(long)]
        chain: Option<String>,
        /// Minimum trade volume (USD)
        #[arg(long)]
        min_volume: Option<String>,
        /// Maximum trade volume (USD)
        #[arg(long)]
        max_volume: Option<String>,
        /// Minimum number of holding addresses
        #[arg(long)]
        min_holders: Option<String>,
        /// Minimum market cap (USD)
        #[arg(long)]
        min_market_cap: Option<String>,
        /// Maximum market cap (USD)
        #[arg(long)]
        max_market_cap: Option<String>,
        /// Minimum liquidity (USD)
        #[arg(long)]
        min_liquidity: Option<String>,
        /// Maximum liquidity (USD)
        #[arg(long)]
        max_liquidity: Option<String>,
    },
}

pub async fn execute(ctx: &Context, cmd: TrackerCommand) -> Result<()> {
    match cmd {
        TrackerCommand::Activities {
            tracker_type,
            wallet_address,
            trade_type,
            chain,
            min_volume,
            max_volume,
            min_holders,
            min_market_cap,
            max_market_cap,
            min_liquidity,
            max_liquidity,
        } => {
            tracker_activities(
                ctx,
                &tracker_type,
                wallet_address.as_deref(),
                trade_type.as_deref(),
                chain.as_deref(),
                min_volume.as_deref(),
                max_volume.as_deref(),
                min_holders.as_deref(),
                min_market_cap.as_deref(),
                max_market_cap.as_deref(),
                min_liquidity.as_deref(),
                max_liquidity.as_deref(),
            )
            .await
        }
    }
}

// ── Public fetch functions (used by both CLI and MCP) ────────────────

pub fn resolve_tracker_type(t: &str) -> &str {
    match t {
        "smart_money" => "1",
        "kol" => "2",
        "multi_address" => "3",
        other => other,
    }
}

/// GET /api/v6/dex/market/address-tracker/trades
#[allow(clippy::too_many_arguments)]
pub async fn fetch_activities(
    client: &ApiClient,
    tracker_type: &str,
    wallet_address: Option<&str>,
    trade_type: Option<&str>,
    chain_index: Option<&str>,
    min_volume: Option<&str>,
    max_volume: Option<&str>,
    min_holders: Option<&str>,
    min_market_cap: Option<&str>,
    max_market_cap: Option<&str>,
    min_liquidity: Option<&str>,
    max_liquidity: Option<&str>,
) -> Result<Value> {
    let tracker_type_val = resolve_tracker_type(tracker_type);
    let mut query: Vec<(&str, &str)> = vec![("trackerType", tracker_type_val)];
    if let Some(w) = wallet_address {
        query.push(("walletAddress", w));
    }
    if let Some(t) = trade_type {
        query.push(("tradeType", t));
    }
    if let Some(c) = chain_index {
        query.push(("chainIndex", c));
    }
    if let Some(v) = min_volume {
        query.push(("minVolume", v));
    }
    if let Some(v) = max_volume {
        query.push(("maxVolume", v));
    }
    if let Some(h) = min_holders {
        query.push(("minHolders", h));
    }
    if let Some(m) = min_market_cap {
        query.push(("minMarketCap", m));
    }
    if let Some(m) = max_market_cap {
        query.push(("maxMarketCap", m));
    }
    if let Some(l) = min_liquidity {
        query.push(("minLiquidity", l));
    }
    if let Some(l) = max_liquidity {
        query.push(("maxLiquidity", l));
    }
    client
        .get("/api/v6/dex/market/address-tracker/trades", &query)
        .await
}

// ── CLI wrapper ───────────────────────────────────────────────────────

#[allow(clippy::too_many_arguments)]
async fn tracker_activities(
    ctx: &Context,
    tracker_type: &str,
    wallet_address: Option<&str>,
    trade_type: Option<&str>,
    chain: Option<&str>,
    min_volume: Option<&str>,
    max_volume: Option<&str>,
    min_holders: Option<&str>,
    min_market_cap: Option<&str>,
    max_market_cap: Option<&str>,
    min_liquidity: Option<&str>,
    max_liquidity: Option<&str>,
) -> Result<()> {
    let resolved = resolve_tracker_type(tracker_type);
    if (resolved == "3" || tracker_type == "multi_address") && wallet_address.is_none() {
        anyhow::bail!("--wallet-address is required when --tracker-type is multi_address");
    }
    let chain_index = chain.map(|c| crate::chains::resolve_chain(c).to_string());
    let client = ctx.client_async().await?;
    output::success(
        fetch_activities(
            &client,
            tracker_type,
            wallet_address,
            trade_type,
            chain_index.as_deref(),
            min_volume,
            max_volume,
            min_holders,
            min_market_cap,
            max_market_cap,
            min_liquidity,
            max_liquidity,
        )
        .await?,
    );
    Ok(())
}
