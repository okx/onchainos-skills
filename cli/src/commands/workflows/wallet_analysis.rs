/// Wallet Analysis
///
/// Step 1 (parallel): portfolio overview 7d + 30d + all balances
///   partial failures: field null, rest continues (no "all fail → error" rule for wallet analysis)
/// Step 2 (sequential): recent token-level PnL
/// Step 3 (sequential): recent on-chain activity via tracker
use anyhow::Result;
use serde_json::{json, Value};

use crate::chains;
use crate::client::ApiClient;
use crate::commands::{market, portfolio, tracker};
use crate::output;

use super::{ok_or_null, Context};

pub(crate) async fn fetch_and_assemble(
    client: &mut ApiClient,
    address: &str,
    chain_index: &str,
) -> Result<Value> {
    // ── Step 1: performance + balances (parallel) ────────────────────
    // time_frame: 3 = 7D, 4 = 1M
    let (mut c1, mut c2) = (client.clone(), client.clone());
    let (overview_7d, overview_30d, balances) = tokio::join!(
        market::fetch_portfolio_overview(client, chain_index, address, "3"),
        market::fetch_portfolio_overview(&mut c1, chain_index, address, "4"),
        portfolio::fetch_all_balances(&mut c2, address, chain_index, None, None),
    );
    let overview_7d = ok_or_null(overview_7d);
    let overview_30d = ok_or_null(overview_30d);
    let balances = ok_or_null(balances);

    // ── Step 2: per-token PnL (sequential) ───────────────────────────
    let recent_pnl = ok_or_null(
        market::fetch_portfolio_recent_pnl(client, chain_index, address, None, None).await,
    );

    // ── Step 3: recent on-chain activity (sequential) ─────────────────
    let activities = ok_or_null(
        tracker::fetch_activities(
            client,
            "multi_address",
            Some(address),
            None,
            Some(chain_index),
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

    Ok(assemble(
        address,
        chain_index,
        overview_7d,
        overview_30d,
        balances,
        recent_pnl,
        activities,
    ))
}

pub async fn run(ctx: &Context, address: &str, chain: Option<String>) -> Result<()> {
    let mut client = ctx.client_async().await?;
    let chain_index = chain
        .as_deref()
        .map(|c| chains::resolve_chain(c).to_string())
        .unwrap_or_else(|| ctx.chain_index_or("solana"));

    let result = fetch_and_assemble(&mut client, address, &chain_index).await?;
    output::success(result);
    Ok(())
}

/// Assemble wallet-analysis output from pre-fetched data.
/// Pure function — testable without network calls.
#[allow(clippy::too_many_arguments)]
pub(crate) fn assemble(
    address: &str,
    chain_index: &str,
    overview_7d: Value,
    overview_30d: Value,
    balances: Value,
    recent_pnl: Value,
    activities: Value,
) -> Value {
    json!({
        "workflow": "wallet-analysis",
        "address":  address,
        "chain":    chain_index,
        "performance": {
            "7d":  overview_7d,
            "30d": overview_30d,
        },
        "balances":   balances,
        "recentPnl":  recent_pnl,
        "activities": activities,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn some_data() -> Value {
        json!({ "pnl": "100" })
    }
    fn null() -> Value {
        Value::Null
    }

    fn full_assemble(
        overview_7d: Value,
        overview_30d: Value,
        balances: Value,
        recent_pnl: Value,
        activities: Value,
    ) -> Value {
        assemble(
            "0xWALLET",
            "501",
            overview_7d,
            overview_30d,
            balances,
            recent_pnl,
            activities,
        )
    }

    // ── Output structure ──────────────────────────────────────────────

    #[test]
    fn output_has_workflow_discriminator() {
        let out = full_assemble(null(), null(), null(), null(), null());
        assert_eq!(out["workflow"], "wallet-analysis");
    }

    #[test]
    fn output_has_address_and_chain() {
        let out = full_assemble(null(), null(), null(), null(), null());
        assert_eq!(out["address"], "0xWALLET");
        assert_eq!(out["chain"], "501");
    }

    #[test]
    fn output_has_performance_nested_under_7d_and_30d() {
        let out = full_assemble(some_data(), some_data(), null(), null(), null());
        assert!(!out["performance"]["7d"].is_null());
        assert!(!out["performance"]["30d"].is_null());
    }

    #[test]
    fn output_has_balances_recent_pnl_activities() {
        let out = full_assemble(null(), null(), some_data(), some_data(), some_data());
        assert!(!out["balances"].is_null());
        assert!(!out["recentPnl"].is_null());
        assert!(!out["activities"].is_null());
    }

    // ── PRD: partial failures → null fields, rest continues ──────────

    #[test]
    fn overview_7d_null_others_present() {
        let out = full_assemble(null(), some_data(), some_data(), some_data(), some_data());
        assert!(out["performance"]["7d"].is_null());
        assert!(!out["performance"]["30d"].is_null());
    }

    #[test]
    fn overview_30d_null_others_present() {
        let out = full_assemble(some_data(), null(), some_data(), some_data(), some_data());
        assert!(out["performance"]["30d"].is_null());
        assert!(!out["performance"]["7d"].is_null());
    }

    #[test]
    fn balances_null_other_steps_still_present() {
        let out = full_assemble(some_data(), some_data(), null(), some_data(), some_data());
        assert!(out["balances"].is_null());
        assert!(!out["recentPnl"].is_null());
        assert!(!out["activities"].is_null());
    }

    #[test]
    fn recent_pnl_null_activities_still_present() {
        let out = full_assemble(some_data(), some_data(), some_data(), null(), some_data());
        assert!(out["recentPnl"].is_null());
        assert!(!out["activities"].is_null());
    }

    #[test]
    fn activities_null_pnl_still_present() {
        let out = full_assemble(some_data(), some_data(), some_data(), some_data(), null());
        assert!(out["activities"].is_null());
        assert!(!out["recentPnl"].is_null());
    }

    #[test]
    fn all_null_still_returns_ok_not_error() {
        // Wallet analysis has no "all fail → error" rule in the PRD — partial data is still useful
        let out = full_assemble(null(), null(), null(), null(), null());
        assert_eq!(out["workflow"], "wallet-analysis");
        assert!(out["performance"]["7d"].is_null());
        assert!(out["balances"].is_null());
        assert!(out["activities"].is_null());
    }

    // ── Data values preserved exactly ─────────────────────────────────

    #[test]
    fn overview_data_preserved() {
        let data = json!({ "winRate": "75%", "pnl": "1200" });
        let out = full_assemble(data.clone(), null(), null(), null(), null());
        assert_eq!(out["performance"]["7d"]["winRate"], "75%");
    }

    #[test]
    fn activities_data_preserved() {
        let acts = json!([{ "action": "buy", "token": "BONK", "amount": "500" }]);
        let out = full_assemble(null(), null(), null(), null(), acts);
        assert_eq!(out["activities"][0]["action"], "buy");
    }
}
