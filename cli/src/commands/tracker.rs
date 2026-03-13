use anyhow::{bail, Result};
use clap::Subcommand;

use super::Context;
use crate::output;

/// Resolve tracker type alias to API integer string.
/// Accepts human-readable names, abbreviations, and raw numeric values.
fn resolve_tracker_type(s: &str) -> &str {
    match s.to_lowercase().as_str() {
        "smart_money" | "smartmoney" | "smart-money" | "sm" | "1" => "1",
        "kol" | "2" => "2",
        "multi_address" | "multi-address" | "custom" | "3" => "3",
        _ => s,
    }
}

/// Resolve trade type alias to API integer string.
/// Accepts human-readable names and raw numeric values.
fn resolve_trade_type(s: &str) -> &str {
    match s.to_lowercase().as_str() {
        "all" | "0" => "0",
        "buy" | "1" => "1",
        "sell" | "2" => "2",
        _ => s,
    }
}

#[derive(Subcommand)]
pub enum TrackerCommand {
    /// Get on-chain trading activity of tracked addresses (KOL / smart money / multi-address)
    Trades {
        /// Tracker type: kol (default), smart_money/sm, multi_address/custom. Also accepts 1/2/3.
        #[arg(long, default_value = "kol")]
        tracker_type: String,
        /// Wallet address(es) to track — required when --tracker-type is multi_address/custom/3.
        /// Comma-separated, max 20 addresses.
        #[arg(long)]
        wallet_address: Option<String>,
        /// Trade type: all/0 (default), buy/1, sell/2
        #[arg(long)]
        trade_type: Option<String>,
        /// Chain: all (default), ethereum/eth, solana/sol, bsc/bnb, base, xlayer, or numeric chainIndex
        #[arg(long)]
        chain: Option<String>,
        /// Minimum trade volume in USD
        #[arg(long)]
        min_volume: Option<String>,
        /// Maximum trade volume in USD
        #[arg(long)]
        max_volume: Option<String>,
        /// Minimum holder count of the traded token
        #[arg(long)]
        min_holders: Option<String>,
        /// Minimum market cap in USD
        #[arg(long)]
        min_market_cap: Option<String>,
        /// Maximum market cap in USD
        #[arg(long)]
        max_market_cap: Option<String>,
        /// Minimum liquidity in USD
        #[arg(long)]
        min_liquidity: Option<String>,
        /// Maximum liquidity in USD
        #[arg(long)]
        max_liquidity: Option<String>,
    },
}

pub async fn execute(ctx: &Context, cmd: TrackerCommand) -> Result<()> {
    match cmd {
        TrackerCommand::Trades {
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
            tracker_trades(
                ctx,
                &tracker_type,
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
            )
            .await
        }
    }
}

/// GET /api/v6/dex/market/address-tracker/trades
#[allow(clippy::too_many_arguments)]
async fn tracker_trades(
    ctx: &Context,
    tracker_type: &str,
    wallet_address: Option<String>,
    trade_type: Option<String>,
    chain: Option<String>,
    min_volume: Option<String>,
    max_volume: Option<String>,
    min_holders: Option<String>,
    min_market_cap: Option<String>,
    max_market_cap: Option<String>,
    min_liquidity: Option<String>,
    max_liquidity: Option<String>,
) -> Result<()> {
    let tracker_type_resolved = resolve_tracker_type(tracker_type);

    if tracker_type_resolved == "3" && wallet_address.is_none() {
        bail!("--wallet-address is required when --tracker-type is multi_address/custom/3");
    }

    let chain_index = match chain {
        None => String::new(),
        Some(ref c) if c.eq_ignore_ascii_case("all") => String::new(),
        Some(ref c) => crate::chains::resolve_chain(c),
    };

    let wallet_address = wallet_address.unwrap_or_default();
    let trade_type = trade_type
        .as_deref()
        .map(resolve_trade_type)
        .unwrap_or_default()
        .to_string();
    let min_volume = min_volume.unwrap_or_default();
    let max_volume = max_volume.unwrap_or_default();
    let min_holders = min_holders.unwrap_or_default();
    let min_market_cap = min_market_cap.unwrap_or_default();
    let max_market_cap = max_market_cap.unwrap_or_default();
    let min_liquidity = min_liquidity.unwrap_or_default();
    let max_liquidity = max_liquidity.unwrap_or_default();

    let client = ctx.client()?;
    let data = client
        .get(
            "/api/v6/dex/market/address-tracker/trades",
            &[
                ("trackerType", tracker_type_resolved),
                ("walletAddress", wallet_address.as_str()),
                ("tradeType", trade_type.as_str()),
                ("chainIndex", chain_index.as_str()),
                ("minVolume", min_volume.as_str()),
                ("maxVolume", max_volume.as_str()),
                ("minHolders", min_holders.as_str()),
                ("minMarketCap", min_market_cap.as_str()),
                ("maxMarketCap", max_market_cap.as_str()),
                ("minLiquidity", min_liquidity.as_str()),
                ("maxLiquidity", max_liquidity.as_str()),
            ],
        )
        .await?;
    output::success(data);
    Ok(())
}
