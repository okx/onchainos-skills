/// W7 — Portfolio Check
///
/// Step 1 (sequential): all balances + total value + 30d portfolio overview
///   partial failures: field null, rest continues
use anyhow::Result;
use serde_json::{json, Value};

use crate::chains;
use crate::client::ApiClient;
use crate::commands::{market, portfolio};
use crate::output;

use super::{ok_or_null, Context};

pub(crate) async fn fetch_and_assemble(
    client: &mut ApiClient,
    address: &str,
    chains_str: &str,
    primary_chain_index: &str,
) -> Result<Value> {
    // ── Step 1: sequential overview ───────────────────────────────────
    // time_frame 4 = 1M
    let balances = ok_or_null(
        portfolio::fetch_all_balances(client, address, chains_str, None, None).await,
    );
    let total_value = ok_or_null(
        portfolio::fetch_total_value(client, address, chains_str, None, None).await,
    );
    let overview = ok_or_null(
        market::fetch_portfolio_overview(client, primary_chain_index, address, "4").await,
    );

    Ok(assemble(
        address,
        chains_str,
        balances,
        total_value,
        overview,
    ))
}

pub async fn run(ctx: &Context, address: &str, chains_arg: Option<String>) -> Result<()> {
    let mut client = ctx.client_async().await?;

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

    let result = fetch_and_assemble(&mut client, address, &chains_str, &primary_chain_index).await?;
    output::success(result);
    Ok(())
}

/// Assemble portfolio output from pre-fetched data.
/// Pure function — testable without network calls.
pub(crate) fn assemble(
    address: &str,
    chains: &str,
    balances: Value,
    total_value: Value,
    overview: Value,
) -> Value {
    json!({
        "workflow":   "portfolio",
        "address":    address,
        "chains":     chains,
        "balances":   balances,
        "totalValue": total_value,
        "overview":   overview,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn some_data() -> Value { json!({ "value": "9999" }) }
    fn null() -> Value { Value::Null }

    fn full_assemble(balances: Value, total_value: Value, overview: Value) -> Value {
        assemble("0xWALLET", "1,501", balances, total_value, overview)
    }

    // ── Output structure ──────────────────────────────────────────────

    #[test]
    fn output_has_workflow_discriminator() {
        let out = full_assemble(null(), null(), null());
        assert_eq!(out["workflow"], "portfolio");
    }

    #[test]
    fn output_has_address_and_chains() {
        let out = full_assemble(null(), null(), null());
        assert_eq!(out["address"], "0xWALLET");
        assert_eq!(out["chains"], "1,501");
    }

    #[test]
    fn output_has_balances_total_value_overview() {
        let out = full_assemble(some_data(), some_data(), some_data());
        assert!(!out["balances"].is_null());
        assert!(!out["totalValue"].is_null());
        assert!(!out["overview"].is_null());
    }

    // ── PRD: partial failures → null fields, rest continues ──────────

    #[test]
    fn balances_null_others_present() {
        let out = full_assemble(null(), some_data(), some_data());
        assert!(out["balances"].is_null());
        assert!(!out["totalValue"].is_null());
        assert!(!out["overview"].is_null());
    }

    #[test]
    fn total_value_null_others_present() {
        let out = full_assemble(some_data(), null(), some_data());
        assert!(out["totalValue"].is_null());
        assert!(!out["balances"].is_null());
        assert!(!out["overview"].is_null());
    }

    #[test]
    fn overview_null_others_present() {
        let out = full_assemble(some_data(), some_data(), null());
        assert!(out["overview"].is_null());
        assert!(!out["balances"].is_null());
        assert!(!out["totalValue"].is_null());
    }

    #[test]
    fn all_null_returns_valid_shell() {
        // No "all fail → error" rule for W7 — return valid output with null fields
        let out = full_assemble(null(), null(), null());
        assert_eq!(out["workflow"], "portfolio");
        assert!(out["balances"].is_null());
        assert!(out["totalValue"].is_null());
        assert!(out["overview"].is_null());
    }

    // ── Data values preserved exactly ─────────────────────────────────

    #[test]
    fn balance_data_preserved() {
        let data = json!([{ "symbol": "SOL", "balance": "10.5" }]);
        let out = full_assemble(data, null(), null());
        assert_eq!(out["balances"][0]["symbol"], "SOL");
    }

    #[test]
    fn total_value_data_preserved() {
        let tv = json!({ "totalValue": "15234.50", "currency": "USD" });
        let out = full_assemble(null(), tv, null());
        assert_eq!(out["totalValue"]["currency"], "USD");
    }

    #[test]
    fn chains_multi_chain_preserved() {
        let out = assemble("0xW", "1,501,56", null(), null(), null());
        assert_eq!(out["chains"], "1,501,56");
    }
}
