use anyhow::Result;
use clap::Subcommand;
use serde_json::Value;

use super::Context;
use crate::client::ApiClient;
use crate::output;

#[derive(Subcommand)]
#[allow(clippy::large_enum_variant)]
pub enum LeaderboardCommand {
    /// Get supported chains for the leaderboard
    SupportedChains,
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
        LeaderboardCommand::SupportedChains => supported_chains(ctx).await,
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
pub async fn fetch_chains(client: &ApiClient) -> Result<Value> {
    client
        .get("/api/v6/dex/market/leaderboard/supported/chain", &[])
        .await
}

async fn supported_chains(ctx: &Context) -> Result<()> {
    let client = ctx.client_async().await?;
    output::success(fetch_chains(&client).await?);
    Ok(())
}

/// Map human-readable wallet type names to the integer codes expected by the API.
/// Accepts either the string name (e.g. "smartMoney") or the integer directly ("1").
pub fn resolve_leaderboard_wallet_type(wallet_type: String) -> String {
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
pub async fn fetch_list(
    client: &ApiClient,
    chain_index: &str,
    time_frame: &str,
    sort_by: &str,
    wallet_type: Option<&str>,
    min_realized_pnl: Option<&str>,
    max_realized_pnl: Option<&str>,
    min_win_rate: Option<&str>,
    max_win_rate: Option<&str>,
    min_txs: Option<&str>,
    max_txs: Option<&str>,
    min_tx_volume: Option<&str>,
    max_tx_volume: Option<&str>,
) -> Result<Value> {
    let mut query: Vec<(&str, &str)> = vec![
        ("chainIndex", chain_index),
        ("timeFrame", time_frame),
        ("sortBy", sort_by),
    ];
    if let Some(v) = wallet_type {
        query.push(("walletType", v));
    }
    if let Some(v) = min_realized_pnl {
        query.push(("minRealizedPnlUsd", v));
    }
    if let Some(v) = max_realized_pnl {
        query.push(("maxRealizedPnlUsd", v));
    }
    if let Some(v) = min_win_rate {
        query.push(("minWinRatePercent", v));
    }
    if let Some(v) = max_win_rate {
        query.push(("maxWinRatePercent", v));
    }
    if let Some(v) = min_txs {
        query.push(("minTxs", v));
    }
    if let Some(v) = max_txs {
        query.push(("maxTxs", v));
    }
    if let Some(v) = min_tx_volume {
        query.push(("minTxVolume", v));
    }
    if let Some(v) = max_tx_volume {
        query.push(("maxTxVolume", v));
    }
    client
        .get("/api/v6/dex/market/leaderboard/list", &query)
        .await
}

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
    let client = ctx.client_async().await?;

    let wallet_type_resolved = wallet_type.map(resolve_leaderboard_wallet_type);

    output::success(
        fetch_list(
            &client,
            &chain_index,
            time_frame,
            sort_by,
            wallet_type_resolved.as_deref(),
            min_realized_pnl_usd.as_deref(),
            max_realized_pnl_usd.as_deref(),
            min_win_rate_percent.as_deref(),
            max_win_rate_percent.as_deref(),
            min_txs.as_deref(),
            max_txs.as_deref(),
            min_tx_volume.as_deref(),
            max_tx_volume.as_deref(),
        )
        .await?,
    );
    Ok(())
}
