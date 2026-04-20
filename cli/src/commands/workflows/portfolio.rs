/// W7 — Portfolio Check
///
/// Step 1 (parallel): all balances + total value + 30d portfolio overview
use anyhow::Result;
use serde_json::json;

use crate::chains;
use crate::commands::{market, portfolio};
use crate::output;

use super::{ok_or_null, Context};

pub async fn run(ctx: &Context, address: &str, chains_arg: Option<String>) -> Result<()> {
    let client = ctx.client_async().await?;

    // Resolve chains string: explicit arg → global --chain → "1,501" (all major)
    let chains_str = chains_arg.unwrap_or_else(|| {
        ctx.chain_override
            .as_ref()
            .map(|c| chains::resolve_chain(c).to_string())
            .unwrap_or_else(|| "1,501".to_string())
    });

    // For portfolio overview we need a single chainIndex — use the first resolved chain.
    let primary_chain_index = chains_str
        .split(',')
        .next()
        .map(|c| chains::resolve_chain(c).to_string())
        .unwrap_or_else(|| "501".to_string());

    // ── Step 1: parallel overview ────────────────────────────────────
    // time_frame 4 = 1M
    let (balances, total_value, overview) = tokio::join!(
        portfolio::fetch_all_balances(&client, address, &chains_str, None, None),
        portfolio::fetch_total_value(&client, address, &chains_str, None, None),
        market::fetch_portfolio_overview(&client, &primary_chain_index, address, "4"),
    );

    output::success(json!({
        "workflow":   "portfolio",
        "address":    address,
        "chains":     chains_str,
        "balances":   ok_or_null(balances),
        "totalValue": ok_or_null(total_value),
        "overview":   ok_or_null(overview),
    }));
    Ok(())
}
