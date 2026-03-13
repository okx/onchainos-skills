use anyhow::Result;
use clap::Subcommand;

use super::Context;
use crate::output;

#[derive(Subcommand)]
#[allow(clippy::large_enum_variant)]
pub enum LeaderboardCommand {
    /// Get supported chains for the leaderboard
    Chains,
    /// Get leaderboard list (top traders ranked by PnL, win rate, or volume)
    List {
        /// Chain (e.g. ethereum, solana, base). Required.
        #[arg(long)]
        chain: String,
        /// Time frame (required): 1=1D, 2=3D, 3=7D, 4=1M, 5=3M
        #[arg(long)]
        time_frame: String,
        /// Sort by (required): 1=PnL, 2=Win Rate, 3=Tx number, 4=Volume, 5=ROI (profit rate)
        #[arg(long)]
        sort_by: String,
        /// Wallet type filter (single select): sniper, dev, fresh, pump, smartMoney, influencer
        #[arg(long)]
        wallet_type: Option<String>,
        /// Minimum realized PnL in USD
        #[arg(long)]
        min_realized_pnl_usd: Option<String>,
        /// Maximum realized PnL in USD
        #[arg(long)]
        max_realized_pnl_usd: Option<String>,
        /// Minimum win rate percentage (0-100)
        #[arg(long)]
        min_win_rate_percent: Option<String>,
        /// Maximum win rate percentage (0-100)
        #[arg(long)]
        max_win_rate_percent: Option<String>,
        /// Minimum number of transactions
        #[arg(long)]
        min_txs: Option<String>,
        /// Maximum number of transactions
        #[arg(long)]
        max_txs: Option<String>,
        /// Minimum transaction volume in USD
        #[arg(long)]
        min_tx_volume: Option<String>,
        /// Maximum transaction volume in USD
        #[arg(long)]
        max_tx_volume: Option<String>,
    },
}

pub async fn execute(ctx: &Context, cmd: LeaderboardCommand) -> Result<()> {
    match cmd {
        LeaderboardCommand::Chains => supported_chains(ctx).await,
        LeaderboardCommand::List {
            chain,
            time_frame,
            sort_by,
            wallet_type,
            min_realized_pnl_usd,
            max_realized_pnl_usd,
            min_win_rate_percent,
            max_win_rate_percent,
            min_txs,
            max_txs,
            min_tx_volume,
            max_tx_volume,
        } => {
            leaderboard_list(
                ctx,
                &chain,
                &time_frame,
                &sort_by,
                wallet_type,
                min_realized_pnl_usd,
                max_realized_pnl_usd,
                min_win_rate_percent,
                max_win_rate_percent,
                min_txs,
                max_txs,
                min_tx_volume,
                max_tx_volume,
            )
            .await
        }
    }
}

/// GET /api/v6/dex/market/leaderboard/supported/chain — no parameters
async fn supported_chains(ctx: &Context) -> Result<()> {
    let client = ctx.client()?;
    let data = client
        .get("/api/v6/dex/market/leaderboard/supported/chain", &[])
        .await?;
    output::success(data);
    Ok(())
}

/// Map human-readable wallet type names to the integer codes expected by the API.
/// Accepts either the string name (e.g. "smartMoney") or the integer directly ("1").
fn resolve_leaderboard_wallet_type(wallet_type: String) -> String {
    match wallet_type.as_str() {
        "smartMoney" => "1".to_string(),
        "influencer" => "2".to_string(),
        "sniper" => "3".to_string(),
        "dev" => "4".to_string(),
        "fresh" => "5".to_string(),
        "pump" => "6".to_string(),
        _ => wallet_type,
    }
}

/// GET /api/v6/dex/market/leaderboard/list — top trader leaderboard with optional filters
#[allow(clippy::too_many_arguments)]
async fn leaderboard_list(
    ctx: &Context,
    chain: &str,
    time_frame: &str,
    sort_by: &str,
    wallet_type: Option<String>,
    min_realized_pnl_usd: Option<String>,
    max_realized_pnl_usd: Option<String>,
    min_win_rate_percent: Option<String>,
    max_win_rate_percent: Option<String>,
    min_txs: Option<String>,
    max_txs: Option<String>,
    min_tx_volume: Option<String>,
    max_tx_volume: Option<String>,
) -> Result<()> {
    let chain_index = crate::chains::resolve_chain(chain).to_string();
    let client = ctx.client()?;

    let wallet_type = resolve_leaderboard_wallet_type(wallet_type.unwrap_or_default());
    let min_realized_pnl = min_realized_pnl_usd.unwrap_or_default();
    let max_realized_pnl = max_realized_pnl_usd.unwrap_or_default();
    let min_win_rate = min_win_rate_percent.unwrap_or_default();
    let max_win_rate = max_win_rate_percent.unwrap_or_default();
    let min_txs = min_txs.unwrap_or_default();
    let max_txs = max_txs.unwrap_or_default();
    let min_tx_volume = min_tx_volume.unwrap_or_default();
    let max_tx_volume = max_tx_volume.unwrap_or_default();

    let data = client
        .get(
            "/api/v6/dex/market/leaderboard/list",
            &[
                ("chainIndex", chain_index.as_str()),
                ("timeFrame", time_frame),
                ("sortBy", sort_by),
                ("walletType", &wallet_type),
                ("minRealizedPnlUsd", &min_realized_pnl),
                ("maxRealizedPnlUsd", &max_realized_pnl),
                ("minWinRatePercent", &min_win_rate),
                ("maxWinRatePercent", &max_win_rate),
                ("minTxs", &min_txs),
                ("maxTxs", &max_txs),
                ("minTxVolume", &min_tx_volume),
                ("maxTxVolume", &max_tx_volume),
            ],
        )
        .await?;
    output::success(data);
    Ok(())
}
