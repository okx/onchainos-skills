/// New Token Screening
///
/// Step 1: fetch MIGRATED launchpad tokens
///   API failure: token_list null, Step 2 skipped entirely, returns gracefully
/// Step 2: parallel safety + dev enrichment for top 10 results
///   individual sub-call failures: field null, rest continues
use anyhow::Result;
use serde_json::{json, Value};
use tokio::task::JoinSet;

use crate::chains;
use crate::client::ApiClient;
use crate::commands::{memepump, token};
use crate::output;

use super::{ok_or_null, Context};

const ENRICH_TOP_N: usize = 10;
const VALID_STAGES: &[&str] = &["MIGRATED", "MIGRATING"];

pub(crate) async fn fetch_and_assemble(
    client: &mut ApiClient,
    chain_index: &str,
    stage: &str,
) -> Result<Value> {
    // Accept any case (`migrated`, `MIGRATING`, …) — normalise once at the boundary.
    let stage_norm = stage.to_ascii_uppercase();
    // Message says `stage` (not `--stage`) so it reads cleanly for both CLI
    // and MCP callers; the MCP handler exposes this as a field, not a flag.
    anyhow::ensure!(
        VALID_STAGES.contains(&stage_norm.as_str()),
        "stage must be one of {:?} (case-insensitive), got: {stage}",
        VALID_STAGES
    );

    // ── Step 1: fetch launchpad token list ───────────────────────────
    let token_list = ok_or_null(
        client
            .get(
                "/api/v6/dex/market/memepump/tokenList",
                &[("chainIndex", chain_index), ("stage", stage_norm.as_str())],
            )
            .await,
    );

    let top_tokens = extract_top_tokens(&token_list, ENRICH_TOP_N);

    // Preserve the API-returned order from `top_tokens`. `JoinSet::join_next`
    // yields in task-completion order, so we key results by address and
    // rebuild the output vec in the original order.
    let ordered_addrs: Vec<String> = top_tokens.iter().map(|(a, _)| a.clone()).collect();

    // ── Step 2: parallel enrichment — each task owns its own ApiClient clone ──
    let mut set: JoinSet<(String, Value)> = JoinSet::new();

    for (token_addr, token_item) in top_tokens {
        let mut c = client.clone();
        let ci = chain_index.to_string();
        let addr = token_addr.clone();
        set.spawn(async move {
            let (mut c1, mut c2, mut c3) = (c.clone(), c.clone(), c.clone());
            let (security, advanced, dev_info, bundle_info) = tokio::join!(
                token::fetch_security(&mut c, &addr, &ci),
                token::fetch_advanced_info(&mut c1, &addr, &ci),
                memepump::fetch_by_address(
                    &mut c2,
                    "/api/v6/dex/market/memepump/tokenDevInfo",
                    &addr,
                    &ci,
                ),
                memepump::fetch_by_address(
                    &mut c3,
                    "/api/v6/dex/market/memepump/tokenBundleInfo",
                    &addr,
                    &ci,
                ),
            );
            let enriched = assemble_token_result(
                token_item,
                ok_or_null(security),
                ok_or_null(advanced),
                ok_or_null(dev_info),
                ok_or_null(bundle_info),
            );
            (addr, enriched)
        });
    }

    let mut results_by_addr: std::collections::HashMap<String, Value> =
        std::collections::HashMap::new();
    while let Some(join_res) = set.join_next().await {
        // On JoinError (task panic/cancel) skip the entry — every other
        // completed enrichment is preserved. Matches the null-on-failure
        // spirit of `ok_or_null` used inside the task body.
        if let Ok((addr, data)) = join_res {
            results_by_addr.insert(addr, data);
        }
    }

    let results: Vec<Value> = ordered_addrs
        .into_iter()
        .filter_map(|addr| {
            results_by_addr
                .remove(&addr)
                .map(|data| json!({ "address": addr, "data": data }))
        })
        .collect();

    Ok(assemble(
        chain_index,
        stage_norm.as_str(),
        token_list,
        results,
    ))
}

pub async fn run(ctx: &Context, chain: Option<String>, stage: Option<String>) -> Result<()> {
    let chain_index = chain
        .as_deref()
        .map(|c| chains::resolve_chain(c).to_string())
        .unwrap_or_else(|| ctx.chain_index_or("solana"));
    let stage_str = stage.unwrap_or_else(|| "MIGRATED".to_string());

    let mut client = ctx.client_async().await?;
    let result = fetch_and_assemble(&mut client, &chain_index, &stage_str).await?;
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
///
/// Dedupes by address (first occurrence wins). API-returned ordering is
/// preserved for non-duplicates. If the memepump API ever emits the same
/// address twice, we drop the later rows rather than spawning duplicate
/// enrichment tasks whose results would silently overwrite each other via
/// `results_by_addr`.
pub(crate) fn extract_top_tokens(list: &Value, n: usize) -> Vec<(String, Value)> {
    let arr: &Vec<Value> = match list.as_array() {
        Some(a) => a,
        None => match list["data"].as_array() {
            Some(a) => a,
            None => return vec![],
        },
    };

    let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();
    let mut out: Vec<(String, Value)> = Vec::with_capacity(n);
    for item in arr {
        let addr = match item["tokenContractAddress"]
            .as_str()
            .or_else(|| item["address"].as_str())
        {
            Some(s) if !s.is_empty() => s.to_string(),
            _ => continue,
        };
        if !seen.insert(addr.clone()) {
            continue;
        }
        out.push((addr, item.clone()));
        if out.len() == n {
            break;
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn some_data() -> Value {
        json!({ "key": "value" })
    }
    fn null() -> Value {
        Value::Null
    }

    // ── assemble_token_result ─────────────────────────────────────────

    #[test]
    fn token_result_has_all_required_fields() {
        let result = assemble_token_result(
            some_data(),
            some_data(),
            some_data(),
            some_data(),
            some_data(),
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
        let result =
            assemble_token_result(some_data(), null(), some_data(), some_data(), some_data());
        assert!(result["security"].is_null());
        assert!(!result["contract"].is_null());
    }

    #[test]
    fn token_result_null_dev_info_preserved() {
        let result =
            assemble_token_result(some_data(), some_data(), some_data(), null(), some_data());
        assert!(result["devInfo"].is_null());
        assert!(!result["bundleInfo"].is_null());
    }

    #[test]
    fn token_result_null_bundle_info_preserved() {
        let result =
            assemble_token_result(some_data(), some_data(), some_data(), some_data(), null());
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
        let token =
            json!({ "tokenContractAddress": "0xABC", "symbol": "TKN", "marketCap": "500000" });
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
        let list =
            json!([{ "tokenContractAddress": "0xFULL", "symbol": "TKN", "marketCap": "1000000" }]);
        let result = extract_top_tokens(&list, 10);
        assert_eq!(result[0].1["symbol"], "TKN");
        assert_eq!(result[0].1["marketCap"], "1000000");
    }

    #[test]
    fn dedupes_by_address_keeps_first_occurrence() {
        // If the memepump API ever emits the same address twice, we keep the
        // first occurrence and drop the rest so enrichment task count matches
        // the emitted output count.
        let list = json!([
            { "tokenContractAddress": "0xDUP", "symbol": "FIRST" },
            { "tokenContractAddress": "0xOTH", "symbol": "OTHER" },
            { "tokenContractAddress": "0xDUP", "symbol": "SECOND" },
        ]);
        let result = extract_top_tokens(&list, 10);
        assert_eq!(result.len(), 2);
        assert_eq!(result[0].0, "0xDUP");
        assert_eq!(result[0].1["symbol"], "FIRST"); // first occurrence wins
        assert_eq!(result[1].0, "0xOTH");
    }
}
