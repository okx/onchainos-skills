/// W3 — Smart Money Signals
///
/// Step 1: fetch signal list → aggregate by token (sort desc by SM wallet count) → top 5
///   signal API failure: rawSignals null, topTokens empty, returns gracefully (not an error)
/// Step 2: per-token parallel due diligence
///   individual sub-call failures: field null, rest continues
///   launchpad enrichment conditional on protocolId (reuses is_launchpad_token)
use anyhow::Result;
use serde_json::{json, Value};
use tokio::task::JoinSet;

use crate::chains;
use crate::client::ApiClient;
use crate::commands::{memepump, signal, token};
use crate::output;

use super::{ok_or_null, Context};
use super::token_research::is_launchpad_token;

const TOP_N: usize = 5;

pub(crate) async fn fetch_and_assemble(
    client: &mut ApiClient,
    chain_index: &str,
) -> Result<Value> {
    // ── Step 1 ───────────────────────────────────────────────────────
    let raw_signals = ok_or_null(
        signal::fetch_list(
            client, chain_index,
            None, None, None, None, None, None, None, None, None, None, None, None,
        )
        .await,
    );

    let top_tokens = extract_top_tokens(&raw_signals, TOP_N);

    // Preserve the deliberate descending-walletCount order from extract_top_tokens.
    // JoinSet::join_next yields in task-completion order, so we key results by
    // address and rebuild the output vec in the original `top_tokens` order.
    let ordered_addrs: Vec<String> = top_tokens.iter().map(|(a, _)| a.clone()).collect();

    // ── Step 2: per-token enrichment (parallel, max TOP_N) ───────────
    // Each spawned task gets its own ApiClient clone — true HTTP parallelism.
    let mut set: JoinSet<(String, Value)> = JoinSet::new();

    for (token_addr, signal_item) in top_tokens {
        let mut c = client.clone();
        let ci = chain_index.to_string();
        let addr = token_addr.clone();
        set.spawn(async move {
            let (mut c1, mut c2) = (c.clone(), c.clone());
            let sec_body = serde_json::json!({
                "source": "onchain_os_cli",
                "tokenList": [{ "chainId": ci, "contractAddress": addr }]
            });
            let (price, advanced_val, security) = tokio::join!(
                token::fetch_price_info(&mut c, &addr, &ci),
                token::fetch_advanced_info(&mut c1, &addr, &ci),
                c2.post("/api/v6/security/token-scan", &sec_body),
            );
            let price = ok_or_null(price);
            let advanced_val = ok_or_null(advanced_val);
            let security = ok_or_null(security);

            let launchpad = if is_launchpad_token(&advanced_val) {
                let (mut d1, mut d2) = (c.clone(), c.clone());
                let (dev_info, bundle_info) = tokio::join!(
                    memepump::fetch_by_address(
                        &mut d1, "/api/v6/dex/market/memepump/tokenDevInfo", &addr, &ci,
                    ),
                    memepump::fetch_by_address(
                        &mut d2, "/api/v6/dex/market/memepump/tokenBundleInfo", &addr, &ci,
                    ),
                );
                json!({ "devInfo": ok_or_null(dev_info), "bundleInfo": ok_or_null(bundle_info) })
            } else {
                Value::Null
            };

            let enriched = assemble_token_result(signal_item, price, advanced_val, security, launchpad);
            (addr, enriched)
        });
    }

    let mut results_by_addr: std::collections::HashMap<String, Value> =
        std::collections::HashMap::new();
    while let Some(join_res) = set.join_next().await {
        let (addr, data) = join_res?;
        results_by_addr.insert(addr, data);
    }

    let enriched: Vec<Value> = ordered_addrs
        .into_iter()
        .filter_map(|addr| {
            results_by_addr
                .remove(&addr)
                .map(|data| json!({ "address": addr, "data": data }))
        })
        .collect();

    Ok(assemble(chain_index, raw_signals, enriched))
}

pub async fn run(ctx: &Context, chain: Option<String>) -> Result<()> {
    let chain_index = chain
        .as_deref()
        .map(|c| chains::resolve_chain(c).to_string())
        .unwrap_or_else(|| ctx.chain_index_or("solana"));

    let mut client = ctx.client_async().await?;
    let result = fetch_and_assemble(&mut client, &chain_index).await?;
    output::success(result);
    Ok(())
}

/// Assemble the per-token enrichment object.
/// Pure function — testable without network calls.
pub(crate) fn assemble_token_result(
    signal_item: Value,
    price: Value,
    advanced: Value,
    security: Value,
    launchpad: Value,
) -> Value {
    json!({
        "signal":    signal_item,
        "price":     price,
        "contract":  advanced,
        "security":  security,
        "launchpad": launchpad,
    })
}

/// Assemble the top-level smart-money output.
/// Pure function — testable without network calls.
pub(crate) fn assemble(chain_index: &str, raw_signals: Value, enriched: Vec<Value>) -> Value {
    json!({
        "workflow":   "smart-money",
        "chain":      chain_index,
        "rawSignals": raw_signals,
        "topTokens":  enriched,
    })
}

/// Extract the top N unique tokens from a signal list response, sorted descending
/// by SM wallet count. Handles both a bare array and a `{"data": [...]}` wrapper.
pub(crate) fn extract_top_tokens(signals: &Value, n: usize) -> Vec<(String, Value)> {
    let arr: &Vec<Value> = match signals.as_array() {
        Some(a) => a,
        None => match signals["data"].as_array() {
            Some(a) => a,
            None => return vec![],
        },
    };

    let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();
    let mut items: Vec<(u64, String, Value)> = arr
        .iter()
        .filter_map(|item| {
            let addr = item["tokenContractAddress"]
                .as_str()
                .or_else(|| item["address"].as_str())?
                .to_string();
            if addr.is_empty() || seen.contains(&addr) {
                return None;
            }
            seen.insert(addr.clone());
            let count = item["walletCount"]
                .as_u64()
                .or_else(|| item["addressCount"].as_u64())
                .unwrap_or(0);
            Some((count, addr, item.clone()))
        })
        .collect();

    items.sort_by(|a, b| b.0.cmp(&a.0));
    items.into_iter().take(n).map(|(_, addr, item)| (addr, item)).collect()
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
            some_data(), some_data(), some_data(), some_data(), null(),
        );
        assert!(!result["signal"].is_null());
        assert!(!result["price"].is_null());
        assert!(!result["contract"].is_null());
        assert!(!result["security"].is_null());
        assert!(result["launchpad"].is_null());
    }

    #[test]
    fn token_result_launchpad_present_when_provided() {
        let lp = json!({ "devInfo": { "rugCount": 2 }, "bundleInfo": {} });
        let result = assemble_token_result(some_data(), some_data(), some_data(), some_data(), lp);
        assert_eq!(result["launchpad"]["devInfo"]["rugCount"], 2);
    }

    #[test]
    fn token_result_null_price_preserved() {
        let result = assemble_token_result(some_data(), null(), some_data(), some_data(), null());
        assert!(result["price"].is_null());
        assert!(!result["contract"].is_null());
    }

    #[test]
    fn token_result_null_security_preserved() {
        let result = assemble_token_result(some_data(), some_data(), some_data(), null(), null());
        assert!(result["security"].is_null());
    }

    #[test]
    fn token_result_launchpad_null_when_non_launchpad_token() {
        // advanced has no protocolId — launchpad null passed in from run()
        let result = assemble_token_result(
            some_data(),
            some_data(),
            json!({ "name": "BONK", "protocolId": "" }),
            some_data(),
            null(),
        );
        assert!(result["launchpad"].is_null());
    }

    // ── assemble (top-level) ──────────────────────────────────────────

    #[test]
    fn output_has_workflow_discriminator() {
        let out = assemble("501", null(), vec![]);
        assert_eq!(out["workflow"], "smart-money");
    }

    #[test]
    fn output_has_chain() {
        let out = assemble("501", null(), vec![]);
        assert_eq!(out["chain"], "501");
    }

    #[test]
    fn output_raw_signals_null_when_api_failed() {
        let out = assemble("501", null(), vec![]);
        assert!(out["rawSignals"].is_null());
    }

    #[test]
    fn output_raw_signals_present_when_api_succeeded() {
        let signals = json!([{ "tokenContractAddress": "0xAAA", "walletCount": 3 }]);
        let out = assemble("501", signals.clone(), vec![]);
        assert_eq!(out["rawSignals"], signals);
    }

    #[test]
    fn output_empty_enriched_when_no_top_tokens() {
        let out = assemble("501", null(), vec![]);
        assert_eq!(out["topTokens"], json!([]));
    }

    #[test]
    fn output_enriched_tokens_included() {
        let enriched = vec![
            json!({ "address": "0xAAA", "data": { "price": "1.0" } }),
            json!({ "address": "0xBBB", "data": { "price": "2.0" } }),
        ];
        let out = assemble("501", null(), enriched);
        assert_eq!(out["topTokens"].as_array().unwrap().len(), 2);
    }

    // ── extract_top_tokens ────────────────────────────────────────────

    #[test]
    fn empty_array_returns_empty() {
        assert!(extract_top_tokens(&json!([]), 5).is_empty());
    }

    #[test]
    fn null_input_returns_empty() {
        assert!(extract_top_tokens(&Value::Null, 5).is_empty());
    }

    #[test]
    fn plain_object_with_no_array_returns_empty() {
        assert!(extract_top_tokens(&json!({ "foo": "bar" }), 5).is_empty());
    }

    #[test]
    fn bare_array_extracts_tokens() {
        let signals = json!([
            { "tokenContractAddress": "0xAAA", "walletCount": 3 },
            { "tokenContractAddress": "0xBBB", "walletCount": 7 },
        ]);
        assert_eq!(extract_top_tokens(&signals, 5).len(), 2);
    }

    #[test]
    fn data_key_wrapper_extracts_tokens() {
        let signals = json!({ "data": [
            { "tokenContractAddress": "0xAAA", "walletCount": 1 },
            { "tokenContractAddress": "0xBBB", "walletCount": 2 },
        ]});
        let result = extract_top_tokens(&signals, 5);
        assert_eq!(result.len(), 2);
        assert_eq!(result[0].0, "0xBBB"); // higher count first
    }

    #[test]
    fn sorts_descending_by_wallet_count() {
        let signals = json!([
            { "tokenContractAddress": "0xLOW",  "walletCount": 1  },
            { "tokenContractAddress": "0xHIGH", "walletCount": 99 },
            { "tokenContractAddress": "0xMID",  "walletCount": 10 },
        ]);
        let result = extract_top_tokens(&signals, 5);
        assert_eq!(result[0].0, "0xHIGH");
        assert_eq!(result[1].0, "0xMID");
        assert_eq!(result[2].0, "0xLOW");
    }

    #[test]
    fn respects_n_limit() {
        let signals = json!([
            { "tokenContractAddress": "0xA", "walletCount": 5 },
            { "tokenContractAddress": "0xB", "walletCount": 4 },
            { "tokenContractAddress": "0xC", "walletCount": 3 },
            { "tokenContractAddress": "0xD", "walletCount": 2 },
            { "tokenContractAddress": "0xE", "walletCount": 1 },
            { "tokenContractAddress": "0xF", "walletCount": 0 },
        ]);
        let result = extract_top_tokens(&signals, 3);
        assert_eq!(result.len(), 3);
        assert_eq!(result[0].0, "0xA");
    }

    #[test]
    fn deduplicates_by_address() {
        let signals = json!([
            { "tokenContractAddress": "0xDUP", "walletCount": 10 },
            { "tokenContractAddress": "0xDUP", "walletCount": 5  },
            { "tokenContractAddress": "0xUNI", "walletCount": 3  },
        ]);
        let result = extract_top_tokens(&signals, 5);
        assert_eq!(result.len(), 2);
        assert_eq!(result[0].0, "0xDUP");
    }

    #[test]
    fn falls_back_to_address_count_field() {
        let signals = json!([
            { "tokenContractAddress": "0xA", "addressCount": 8 },
            { "tokenContractAddress": "0xB", "addressCount": 3 },
        ]);
        let result = extract_top_tokens(&signals, 5);
        assert_eq!(result[0].0, "0xA");
    }

    #[test]
    fn skips_items_with_empty_address() {
        let signals = json!([
            { "tokenContractAddress": "",     "walletCount": 99 },
            { "tokenContractAddress": "0xOK", "walletCount": 1  },
        ]);
        let result = extract_top_tokens(&signals, 5);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].0, "0xOK");
    }

    #[test]
    fn uses_alternate_address_field() {
        let signals = json!([{ "address": "0xALT", "walletCount": 4 }]);
        let result = extract_top_tokens(&signals, 5);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].0, "0xALT");
    }
}
