/// W1 — Token Research
///
/// Step 1 (parallel): token info + price-info + advanced-info + security scan
///   → PRD: single sub-call failure → field null, rest continues
///   → PRD: all Step 1 calls fail → propagate error
/// Step 2 (parallel): holders + cluster overview + top traders + signal list
///   → cluster-overview may 500 for brand-new tokens: treated as null, skipped gracefully
/// Step 3 (parallel, conditional): launchpad enrichment only when protocolId is non-empty
///   → if advanced-info itself failed (null), protocolId is absent → Step 3 skipped safely
use anyhow::Result;
use serde_json::{json, Value};

use crate::chains;
use crate::commands::{memepump, signal, token};
use crate::output;

use super::{fetch_token_scan, ok_or_null, Context};

pub async fn run(ctx: &Context, address: &str, chain: Option<String>) -> Result<()> {
    let client = ctx.client_async().await?;
    let chain_index = chain
        .as_deref()
        .map(|c| chains::resolve_chain(c).to_string())
        .unwrap_or_else(|| ctx.chain_index_or("solana"));

    // ── Step 1: core data (parallel) ─────────────────────────────────
    let (info_res, price_res, advanced_res, security) = tokio::join!(
        token::fetch_info(&client, address, &chain_index),
        token::fetch_price_info(&client, address, &chain_index),
        token::fetch_advanced_info(&client, address, &chain_index),
        fetch_token_scan(&client, &chain_index, address),
    );

    let info = ok_or_null(info_res);
    let price = ok_or_null(price_res);
    let advanced = ok_or_null(advanced_res);
    // security is already a Value — fetch_token_scan never propagates errors

    // PRD: all Step 1 core calls failed → return error rather than empty shell
    if all_null(&[&info, &price, &advanced]) && security.is_null() {
        anyhow::bail!(
            "token-research: all Step 1 sub-calls failed for address {} on chain {}",
            address,
            chain_index
        );
    }

    // ── Step 2: on-chain structure (parallel) ────────────────────────
    // cluster-overview may return 500 for brand-new tokens — ok_or_null handles it gracefully.
    let (holders, cluster, top_traders, signals) = tokio::join!(
        token::fetch_holders(&client, address, &chain_index, None, Some("100"), None),
        token::fetch_cluster_by_address(
            &client,
            "/api/v6/dex/market/token/cluster/overview",
            address,
            &chain_index,
        ),
        token::fetch_top_trader(&client, address, &chain_index, None, Some("20"), None),
        signal::fetch_list(
            &client,
            &chain_index,
            None,
            None,
            None,
            None,
            None,
            Some(address.to_string()),
            None,
            None,
            None,
            None,
            None,
            None,
        ),
    );

    // ── Step 3: launchpad supplement (conditional) ───────────────────
    // is_launchpad_token guards against both missing field and failed advanced-info (null).
    let launchpad = if is_launchpad_token(&advanced) {
        let (details, dev_info, bundle_info, similar) = tokio::join!(
            memepump::fetch_by_address(
                &client,
                "/api/v6/dex/market/memepump/tokenDetails",
                address,
                &chain_index,
            ),
            memepump::fetch_by_address(
                &client,
                "/api/v6/dex/market/memepump/tokenDevInfo",
                address,
                &chain_index,
            ),
            memepump::fetch_by_address(
                &client,
                "/api/v6/dex/market/memepump/tokenBundleInfo",
                address,
                &chain_index,
            ),
            memepump::fetch_by_address(
                &client,
                "/api/v6/dex/market/memepump/similarToken",
                address,
                &chain_index,
            ),
        );
        json!({
            "tokenDetails":  ok_or_null(details),
            "devInfo":       ok_or_null(dev_info),
            "bundleInfo":    ok_or_null(bundle_info),
            "similarTokens": ok_or_null(similar),
        })
    } else {
        Value::Null
    };

    output::success(json!({
        "workflow": "token-research",
        "address":  address,
        "chain":    chain_index,
        "core": {
            "info":     info,
            "price":    price,
            "contract": advanced,
            "security": security,
        },
        "structure": {
            "holders":    ok_or_null(holders),
            "cluster":    ok_or_null(cluster),
            "topTraders": ok_or_null(top_traders),
            "signals":    ok_or_null(signals),
        },
        "launchpad": launchpad,
    }));
    Ok(())
}

/// Returns true when the token originates from a launchpad (protocolId is present and non-empty).
/// Safe to call when `advanced` is null — returns false rather than panicking.
pub(crate) fn is_launchpad_token(advanced: &Value) -> bool {
    advanced["protocolId"]
        .as_str()
        .map(|s| !s.is_empty())
        .unwrap_or(false)
}

/// Returns true when every value in `values` is JSON null.
/// Used to detect total Step 1 failure and convert it to an error.
pub(crate) fn all_null(values: &[&Value]) -> bool {
    values.iter().all(|v| v.is_null())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    // ── is_launchpad_token ────────────────────────────────────────────

    #[test]
    fn launchpad_token_with_non_empty_protocol_id() {
        let advanced = json!({ "protocolId": "120596" });
        assert!(is_launchpad_token(&advanced));
    }

    #[test]
    fn launchpad_token_with_empty_protocol_id() {
        let advanced = json!({ "protocolId": "" });
        assert!(!is_launchpad_token(&advanced));
    }

    #[test]
    fn launchpad_token_missing_protocol_id_field() {
        let advanced = json!({ "name": "BONK", "symbol": "BONK" });
        assert!(!is_launchpad_token(&advanced));
    }

    #[test]
    fn launchpad_token_advanced_is_null() {
        // advanced-info call failed; must not panic
        assert!(!is_launchpad_token(&Value::Null));
    }

    #[test]
    fn launchpad_token_advanced_is_empty_object() {
        assert!(!is_launchpad_token(&json!({})));
    }

    #[test]
    fn launchpad_token_protocol_id_non_string_type() {
        // If the API ever returns a non-string protocolId, treat as non-launchpad
        let advanced = json!({ "protocolId": 120596 });
        assert!(!is_launchpad_token(&advanced));
    }

    // ── all_null ──────────────────────────────────────────────────────

    #[test]
    fn all_null_when_every_value_is_null() {
        assert!(all_null(&[&Value::Null, &Value::Null, &Value::Null]));
    }

    #[test]
    fn all_null_false_when_one_value_present() {
        let present = json!({ "price": "1.23" });
        assert!(!all_null(&[&Value::Null, &present, &Value::Null]));
    }

    #[test]
    fn all_null_false_when_no_values_are_null() {
        let a = json!("ok");
        let b = json!(42);
        assert!(!all_null(&[&a, &b]));
    }

    #[test]
    fn all_null_true_for_single_null() {
        assert!(all_null(&[&Value::Null]));
    }

    #[test]
    fn all_null_false_for_empty_object() {
        // An empty object {} is not null
        assert!(!all_null(&[&json!({})]));
    }

    #[test]
    fn all_null_false_for_empty_array() {
        // An empty array [] is not null
        assert!(!all_null(&[&json!([])]));
    }
}
