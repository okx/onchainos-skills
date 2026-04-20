/// W4 — New Token Screening
///
/// Step 1: fetch MIGRATED launchpad tokens (direct API call — avoids large params struct)
///   → API failure: token_list null, Step 2 skipped, returns gracefully
/// Step 2: parallel safety + dev enrichment for top 10 results
///   → individual sub-call failures: field null, rest continues
use anyhow::Result;
use serde_json::{json, Value};
use tokio::task::JoinSet;

use crate::chains;
use crate::commands::{memepump, token};
use crate::output;

use super::{fetch_token_scan, ok_or_null, Context};

const ENRICH_TOP_N: usize = 10;

pub async fn run(ctx: &Context, chain: Option<String>, stage: Option<String>) -> Result<()> {
    let client = ctx.client_async().await?;
    let chain_str = chain
        .as_deref()
        .unwrap_or_else(|| ctx.chain_override.as_deref().unwrap_or("solana"))
        .to_string();
    let chain_index = chains::resolve_chain(&chain_str).to_string();
    let stage_str = stage.unwrap_or_else(|| "MIGRATED".to_string());

    // ── Step 1: fetch launchpad token list ───────────────────────────
    // Direct GET rather than MemepumpTokenListParams (which has ~50 fields and no Default).
    // A failure here is non-fatal — we return an empty enriched list.
    let token_list = ok_or_null(
        client
            .get(
                "/api/v6/dex/market/memepump/tokenList",
                &[("chainIndex", chain_index.as_str()), ("stage", stage_str.as_str())],
            )
            .await,
    );

    let top_tokens: Vec<(String, Value)> = extract_top_tokens(&token_list, ENRICH_TOP_N);

    // ── Step 2: parallel enrichment (skip entirely when list is empty) ──
    let mut set: JoinSet<(String, Value)> = JoinSet::new();

    for (token_addr, token_item) in top_tokens {
        let c = client.clone();
        let ci = chain_index.clone();
        let addr = token_addr.clone();
        set.spawn(async move {
            let (security, advanced, dev_info, bundle_info) = tokio::join!(
                fetch_token_scan(&c, &ci, &addr),
                token::fetch_advanced_info(&c, &addr, &ci),
                memepump::fetch_by_address(
                    &c,
                    "/api/v6/dex/market/memepump/tokenDevInfo",
                    &addr,
                    &ci,
                ),
                memepump::fetch_by_address(
                    &c,
                    "/api/v6/dex/market/memepump/tokenBundleInfo",
                    &addr,
                    &ci,
                ),
            );

            let enriched = json!({
                "token":      token_item,
                "security":   security,
                "contract":   ok_or_null(advanced),
                "devInfo":    ok_or_null(dev_info),
                "bundleInfo": ok_or_null(bundle_info),
            });
            (addr, enriched)
        });
    }

    let mut results: Vec<Value> = Vec::new();
    while let Some(join_res) = set.join_next().await {
        let (addr, data) = join_res?;
        results.push(json!({ "address": addr, "data": data }));
    }

    output::success(json!({
        "workflow":  "new-tokens",
        "chain":     chain_index,
        "stage":     stage_str,
        "tokenList": token_list,
        "enriched":  results,
    }));
    Ok(())
}

/// Extract top N token entries from a memepump token list response.
/// Handles both bare arrays and `{"data": [...]}` wrappers.
/// Returns empty vec on null, empty, or malformed input — Step 2 is then skipped.
pub(crate) fn extract_top_tokens(list: &Value, n: usize) -> Vec<(String, Value)> {
    let arr: &Vec<Value> = match list.as_array() {
        Some(a) => a,
        None => match list["data"].as_array() {
            Some(a) => a,
            None => return vec![],
        },
    };

    arr.iter()
        .filter_map(|item| {
            let addr = item["tokenContractAddress"]
                .as_str()
                .or_else(|| item["address"].as_str())?
                .to_string();
            if addr.is_empty() {
                return None;
            }
            Some((addr, item.clone()))
        })
        .take(n)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    // ── extract_top_tokens ────────────────────────────────────────────

    #[test]
    fn null_input_returns_empty() {
        assert!(extract_top_tokens(&Value::Null, 10).is_empty());
    }

    #[test]
    fn empty_array_returns_empty() {
        assert!(extract_top_tokens(&json!([]), 10).is_empty());
    }

    #[test]
    fn plain_object_not_array_returns_empty() {
        assert!(extract_top_tokens(&json!({ "foo": "bar" }), 10).is_empty());
    }

    #[test]
    fn bare_array_extracts_addresses() {
        let list = json!([
            { "tokenContractAddress": "0xAAA", "symbol": "AAA" },
            { "tokenContractAddress": "0xBBB", "symbol": "BBB" },
        ]);
        let result = extract_top_tokens(&list, 10);
        assert_eq!(result.len(), 2);
        assert_eq!(result[0].0, "0xAAA");
        assert_eq!(result[1].0, "0xBBB");
    }

    #[test]
    fn data_key_wrapper_extracts_tokens() {
        let list = json!({
            "data": [
                { "tokenContractAddress": "0xCCC" },
                { "tokenContractAddress": "0xDDD" },
            ]
        });
        let result = extract_top_tokens(&list, 10);
        assert_eq!(result.len(), 2);
        assert_eq!(result[0].0, "0xCCC");
    }

    #[test]
    fn respects_n_limit() {
        let list = json!([
            { "tokenContractAddress": "0xA" },
            { "tokenContractAddress": "0xB" },
            { "tokenContractAddress": "0xC" },
            { "tokenContractAddress": "0xD" },
        ]);
        let result = extract_top_tokens(&list, 2);
        assert_eq!(result.len(), 2);
        // Preserves API order (no re-sorting in this workflow)
        assert_eq!(result[0].0, "0xA");
        assert_eq!(result[1].0, "0xB");
    }

    #[test]
    fn skips_items_with_empty_address() {
        let list = json!([
            { "tokenContractAddress": "" },
            { "tokenContractAddress": "0xOK" },
        ]);
        let result = extract_top_tokens(&list, 10);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].0, "0xOK");
    }

    #[test]
    fn skips_items_missing_address_field() {
        let list = json!([
            { "symbol": "NOADDR" },
            { "tokenContractAddress": "0xGOOD" },
        ]);
        let result = extract_top_tokens(&list, 10);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].0, "0xGOOD");
    }

    #[test]
    fn uses_alternate_address_field() {
        let list = json!([
            { "address": "0xALT", "symbol": "ALT" },
        ]);
        let result = extract_top_tokens(&list, 10);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].0, "0xALT");
    }

    #[test]
    fn preserves_full_token_item_in_output() {
        let list = json!([
            { "tokenContractAddress": "0xFULL", "symbol": "TKN", "marketCap": "1000000" },
        ]);
        let result = extract_top_tokens(&list, 10);
        assert_eq!(result[0].1["symbol"], "TKN");
        assert_eq!(result[0].1["marketCap"], "1000000");
    }

    #[test]
    fn n_zero_returns_empty() {
        let list = json!([{ "tokenContractAddress": "0xA" }]);
        let result = extract_top_tokens(&list, 0);
        assert!(result.is_empty());
    }
}
