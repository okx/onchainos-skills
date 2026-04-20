/// W4 — New Token Screening
///
/// Step 1: fetch MIGRATED launchpad tokens
///   API failure: token_list null, Step 2 skipped entirely, returns gracefully
/// Step 2: parallel safety + dev enrichment for top 10 results
///   individual sub-call failures: field null, rest continues
use std::sync::Arc;

use anyhow::Result;
use serde_json::{json, Value};
use tokio::sync::Mutex;
use tokio::task::JoinSet;

use crate::chains;
use crate::client::ApiClient;
use crate::commands::{memepump, token};
use crate::output;

use super::{fetch_token_scan, ok_or_null, Context};

const ENRICH_TOP_N: usize = 10;

pub(crate) async fn fetch_and_assemble(
    client: Arc<Mutex<ApiClient>>,
    chain_index: &str,
    stage: &str,
) -> Result<Value> {
    // ── Step 1: fetch launchpad token list ───────────────────────────
    let token_list = {
        let mut guard = client.lock().await;
        ok_or_null(
            guard
                .get(
                    "/api/v6/dex/market/memepump/tokenList",
                    &[("chainIndex", chain_index), ("stage", stage)],
                )
                .await,
        )
    };

    let top_tokens = extract_top_tokens(&token_list, ENRICH_TOP_N);

    // ── Step 2: parallel enrichment (skipped when list empty) ────────
    let mut set: JoinSet<(String, Value)> = JoinSet::new();

    for (token_addr, token_item) in top_tokens {
        let c = Arc::clone(&client);
        let ci = chain_index.to_string();
        let addr = token_addr.clone();
        set.spawn(async move {
            let mut guard = c.lock().await;
            let security = fetch_token_scan(&mut guard, &ci, &addr).await;
            let advanced = ok_or_null(token::fetch_advanced_info(&mut guard, &addr, &ci).await);
            let dev_info = ok_or_null(
                memepump::fetch_by_address(
                    &mut guard, "/api/v6/dex/market/memepump/tokenDevInfo", &addr, &ci,
                )
                .await,
            );
            let bundle_info = ok_or_null(
                memepump::fetch_by_address(
                    &mut guard, "/api/v6/dex/market/memepump/tokenBundleInfo", &addr, &ci,
                )
                .await,
            );
            let enriched = assemble_token_result(
                token_item,
                security,
                advanced,
                dev_info,
                bundle_info,
            );
            (addr, enriched)
        });
    }

    let mut results: Vec<Value> = Vec::new();
    while let Some(join_res) = set.join_next().await {
        let (addr, data) = join_res?;
        results.push(json!({ "address": addr, "data": data }));
    }

    Ok(assemble(chain_index, stage, token_list, results))
}

pub async fn run(ctx: &Context, chain: Option<String>, stage: Option<String>) -> Result<()> {
    let chain_str = chain
        .as_deref()
        .unwrap_or_else(|| ctx.chain_override.as_deref().unwrap_or("solana"))
        .to_string();
    let chain_index = chains::resolve_chain(&chain_str).to_string();
    let stage_str = stage.unwrap_or_else(|| "MIGRATED".to_string());

    let client = Arc::new(Mutex::new(ctx.client_async().await?));
    let result = fetch_and_assemble(client, &chain_index, &stage_str).await?;
    output::success(result);
    Ok(())
}

/// Assemble the per-token enrichment object.
/// Pure function — testable without network calls.
pub(crate) fn assemble_token_result(
    token_item: Value,
    security: Value,
    advanced: Value,
    dev_info: Value,
    bundle_info: Value,
) -> Value {
    json!({
        "token":      token_item,
        "security":   security,
        "contract":   advanced,
        "devInfo":    dev_info,
        "bundleInfo": bundle_info,
    })
}

/// Assemble the top-level new-tokens output.
/// Pure function — testable without network calls.
pub(crate) fn assemble(
    chain_index: &str,
    stage: &str,
    token_list: Value,
    enriched: Vec<Value>,
) -> Value {
    json!({
        "workflow":  "new-tokens",
        "chain":     chain_index,
        "stage":     stage,
        "tokenList": token_list,
        "enriched":  enriched,
    })
}

/// Extract top N token entries from a memepump token list response.
/// Handles both bare arrays and `{"data": [...]}` wrappers.
/// Returns empty vec on null/empty/malformed input → Step 2 is then skipped.
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
            if addr.is_empty() { return None; }
            Some((addr, item.clone()))
        })
        .take(n)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn some_data() -> Value { json!({ "key": "value" }) }
    fn null() -> Value { Value::Null }

    // ── assemble_token_result ─────────────────────────────────────────

    #[test]
    fn token_result_has_all_required_fields() {
        let result = assemble_token_result(
            some_data(), some_data(), some_data(), some_data(), some_data(),
        );
        assert!(!result["token"].is_null());
        assert!(!result["security"].is_null());
        assert!(!result["contract"].is_null());
        assert!(!result["devInfo"].is_null());
        assert!(!result["bundleInfo"].is_null());
    }

    #[test]
    fn token_result_null_security_preserved() {
        // security scan failed
        let result = assemble_token_result(some_data(), null(), some_data(), some_data(), some_data());
        assert!(result["security"].is_null());
        assert!(!result["contract"].is_null());
    }

    #[test]
    fn token_result_null_dev_info_preserved() {
        let result = assemble_token_result(some_data(), some_data(), some_data(), null(), some_data());
        assert!(result["devInfo"].is_null());
        assert!(!result["bundleInfo"].is_null());
    }

    #[test]
    fn token_result_null_bundle_info_preserved() {
        let result = assemble_token_result(some_data(), some_data(), some_data(), some_data(), null());
        assert!(result["bundleInfo"].is_null());
        assert!(!result["devInfo"].is_null());
    }

    #[test]
    fn token_result_all_enrichment_null_still_returns_object() {
        // All enrichment calls failed — only token item remains
        let result = assemble_token_result(some_data(), null(), null(), null(), null());
        assert!(!result["token"].is_null());
        assert!(result["security"].is_null());
        assert!(result["contract"].is_null());
        assert!(result["devInfo"].is_null());
        assert!(result["bundleInfo"].is_null());
    }

    #[test]
    fn token_item_data_preserved_in_result() {
        let token = json!({ "tokenContractAddress": "0xABC", "symbol": "TKN", "marketCap": "500000" });
        let result = assemble_token_result(token, null(), null(), null(), null());
        assert_eq!(result["token"]["symbol"], "TKN");
        assert_eq!(result["token"]["marketCap"], "500000");
    }

    // ── assemble (top-level) ──────────────────────────────────────────

    #[test]
    fn output_has_workflow_discriminator() {
        let out = assemble("501", "MIGRATED", null(), vec![]);
        assert_eq!(out["workflow"], "new-tokens");
    }

    #[test]
    fn output_has_chain_and_stage() {
        let out = assemble("501", "MIGRATED", null(), vec![]);
        assert_eq!(out["chain"], "501");
        assert_eq!(out["stage"], "MIGRATED");
    }

    #[test]
    fn output_token_list_null_when_api_failed() {
        let out = assemble("501", "MIGRATED", null(), vec![]);
        assert!(out["tokenList"].is_null());
    }

    #[test]
    fn output_enriched_empty_when_no_tokens() {
        // Step 2 was skipped (empty extract)
        let out = assemble("501", "MIGRATED", null(), vec![]);
        assert_eq!(out["enriched"], json!([]));
    }

    #[test]
    fn output_enriched_contains_results() {
        let results = vec![
            json!({ "address": "0xA", "data": {} }),
            json!({ "address": "0xB", "data": {} }),
        ];
        let out = assemble("501", "MIGRATED", some_data(), results);
        assert_eq!(out["enriched"].as_array().unwrap().len(), 2);
    }

    #[test]
    fn output_migrating_stage_reflected() {
        let out = assemble("501", "MIGRATING", null(), vec![]);
        assert_eq!(out["stage"], "MIGRATING");
    }

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
            { "tokenContractAddress": "0xAAA" },
            { "tokenContractAddress": "0xBBB" },
        ]);
        let result = extract_top_tokens(&list, 10);
        assert_eq!(result.len(), 2);
        assert_eq!(result[0].0, "0xAAA");
    }

    #[test]
    fn data_key_wrapper_extracts_tokens() {
        let list = json!({ "data": [
            { "tokenContractAddress": "0xCCC" },
            { "tokenContractAddress": "0xDDD" },
        ]});
        let result = extract_top_tokens(&list, 10);
        assert_eq!(result.len(), 2);
    }

    #[test]
    fn preserves_api_order_no_resorting() {
        // new-tokens preserves API order (unlike smart-money which sorts by wallet count)
        let list = json!([
            { "tokenContractAddress": "0xFIRST",  "marketCap": "100" },
            { "tokenContractAddress": "0xSECOND", "marketCap": "999" },
        ]);
        let result = extract_top_tokens(&list, 10);
        assert_eq!(result[0].0, "0xFIRST");
        assert_eq!(result[1].0, "0xSECOND");
    }

    #[test]
    fn respects_n_limit() {
        let list = json!([
            { "tokenContractAddress": "0xA" },
            { "tokenContractAddress": "0xB" },
            { "tokenContractAddress": "0xC" },
            { "tokenContractAddress": "0xD" },
        ]);
        assert_eq!(extract_top_tokens(&list, 2).len(), 2);
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
        let list = json!([{ "symbol": "NOADDR" }, { "tokenContractAddress": "0xGOOD" }]);
        let result = extract_top_tokens(&list, 10);
        assert_eq!(result.len(), 1);
    }

    #[test]
    fn uses_alternate_address_field() {
        let list = json!([{ "address": "0xALT" }]);
        let result = extract_top_tokens(&list, 10);
        assert_eq!(result[0].0, "0xALT");
    }

    #[test]
    fn preserves_full_token_item_in_output() {
        let list = json!([{ "tokenContractAddress": "0xFULL", "symbol": "TKN", "marketCap": "1000000" }]);
        let result = extract_top_tokens(&list, 10);
        assert_eq!(result[0].1["symbol"], "TKN");
        assert_eq!(result[0].1["marketCap"], "1000000");
    }
}
