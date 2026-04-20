/// W5 — Wallet Analysis
///
/// Step 1 (parallel): portfolio overview 7d + 30d + all balances
/// Step 2 (sequential): recent token-level PnL
/// Step 3 (sequential): most recent trades via tracker activities
use anyhow::Result;
use serde_json::json;

use crate::chains;
use crate::commands::{market, portfolio, tracker};
use crate::output;

use super::{ok_or_null, Context};

pub async fn run(ctx: &Context, address: &str, chain: Option<String>) -> Result<()> {
    let client = ctx.client_async().await?;
    let chain_str = chain
        .as_deref()
        .unwrap_or_else(|| ctx.chain_override.as_deref().unwrap_or("solana"))
        .to_string();
    let chain_index = chains::resolve_chain(&chain_str).to_string();

    // ── Step 1: performance + balances (parallel) ────────────────────
    // time_frame: 3 = 7D, 4 = 1M
    let (overview_7d, overview_30d, balances) = tokio::join!(
        market::fetch_portfolio_overview(&client, &chain_index, address, "3"),
        market::fetch_portfolio_overview(&client, &chain_index, address, "4"),
        portfolio::fetch_all_balances(&client, address, &chain_index, None, None),
    );

    // ── Step 2: per-token PnL (sequential) ──────────────────────────
    let recent_pnl = ok_or_null(
        market::fetch_portfolio_recent_pnl(&client, &chain_index, address, None, None).await,
    );

    // ── Step 3: recent on-chain activity (sequential) ────────────────
    let activities = ok_or_null(
        tracker::fetch_activities(
            &client,
            "multi_address",
            Some(address),
            None,
            Some(&chain_index),
            None,
            None,
            None,
            None,
            None,
            None,
            None,
        )
        .await,
    );

    output::success(json!({
        "workflow": "wallet-analysis",
        "address":  address,
        "chain":    chain_index,
        "performance": {
            "7d":  ok_or_null(overview_7d),
            "30d": ok_or_null(overview_30d),
        },
        "balances":    ok_or_null(balances),
        "recentPnl":   recent_pnl,
        "activities":  activities,
    }));
    Ok(())
}
