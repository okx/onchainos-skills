use anyhow::Result;
use clap::Subcommand;

use super::Context;
use crate::output;

#[derive(Subcommand)]
pub enum TrackerCommand {
    /// Get on-chain trading activity of tracked addresses (KOL / smart money / custom group)
    Trades {
        /// Tracker type: kol (default), smart_money, group
        #[arg(long)]
        tracker_type: Option<String>,
        /// Custom group name (required when --tracker-type group)
        #[arg(long)]
        group_name: Option<String>,
        /// Trade type: all (default), buy, sell
        #[arg(long)]
        trade_type: Option<String>,
        /// Chain: all (default), ethereum, solana, bsc, base, xlayer, or numeric chainIndex
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
        /// Number of results to return (default 20, max 50)
        #[arg(long)]
        limit: Option<String>,
    },
}

pub async fn execute(ctx: &Context, cmd: TrackerCommand) -> Result<()> {
    match cmd {
        TrackerCommand::Trades {
            tracker_type,
            group_name,
            trade_type,
            chain,
            min_volume,
            max_volume,
            min_holders,
            min_market_cap,
            max_market_cap,
            min_liquidity,
            max_liquidity,
            limit,
        } => {
            tracker_trades(
                ctx,
                tracker_type,
                group_name,
                trade_type,
                chain,
                min_volume,
                max_volume,
                min_holders,
                min_market_cap,
                max_market_cap,
                min_liquidity,
                max_liquidity,
                limit,
            )
            .await
        }
    }
}

/// GET /api/v6/dex/market/address-tracker/trades
#[allow(clippy::too_many_arguments)]
async fn tracker_trades(
    ctx: &Context,
    tracker_type: Option<String>,
    group_name: Option<String>,
    trade_type: Option<String>,
    chain: Option<String>,
    min_volume: Option<String>,
    max_volume: Option<String>,
    min_holders: Option<String>,
    min_market_cap: Option<String>,
    max_market_cap: Option<String>,
    min_liquidity: Option<String>,
    max_liquidity: Option<String>,
    limit: Option<String>,
) -> Result<()> {
    // chainIndex: resolve chain name or pass "all" / raw numeric as-is
    let chain_index = match chain {
        None => String::new(),
        Some(ref c) if c == "all" => String::new(),
        Some(ref c) => crate::chains::resolve_chain(c).to_string(),
    };

    let tracker_type = tracker_type.unwrap_or_default();
    let group_name = group_name.unwrap_or_default();
    let trade_type = trade_type.unwrap_or_default();
    let min_volume = min_volume.unwrap_or_default();
    let max_volume = max_volume.unwrap_or_default();
    let min_holders = min_holders.unwrap_or_default();
    let min_market_cap = min_market_cap.unwrap_or_default();
    let max_market_cap = max_market_cap.unwrap_or_default();
    let min_liquidity = min_liquidity.unwrap_or_default();
    let max_liquidity = max_liquidity.unwrap_or_default();
    let limit = limit.unwrap_or_default();

    let client = ctx.client()?;
    let data = client
        .get(
            "/api/v6/dex/market/address-tracker/trades",
            &[
                ("trackerType", tracker_type.as_str()),
                ("groupName", group_name.as_str()),
                ("tradeType", trade_type.as_str()),
                ("chainIndex", chain_index.as_str()),
                ("minVolume", min_volume.as_str()),
                ("maxVolume", max_volume.as_str()),
                ("minHolders", min_holders.as_str()),
                ("minMarketCap", min_market_cap.as_str()),
                ("maxMarketCap", max_market_cap.as_str()),
                ("minLiquidity", min_liquidity.as_str()),
                ("maxLiquidity", max_liquidity.as_str()),
                ("limit", limit.as_str()),
            ],
        )
        .await?;
    output::success(data);
    Ok(())
}
