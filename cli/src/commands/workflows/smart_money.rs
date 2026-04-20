/// W3 — Smart Money Signals
///
/// Step 1: fetch signal list, aggregate by token (sort desc by SM wallet count), take top 5
///   → signal API failure: rawSignals null, topTokens empty, returns gracefully (not an error)
/// Step 2: per-token parallel due diligence (price-info + advanced-info + security scan +
///         optional memepump dev/bundle info when protocolId non-empty)
///   → individual sub-call failures: field null, rest continues
use anyhow::Result;
use serde_json::{json, Value};
use tokio::task::JoinSet;

use crate::chains;
use crate::commands::{memepump, signal, token};
use crate::output;

use super::{fetch_token_scan, ok_or_null, Context};
use super::token_research::is_launchpad_token;

const TOP_N: usize = 5;

pub async fn run(ctx: &Context, chain: Option<String>) -> Result<()> {
    let client = ctx.client_async().await?;
    let chain_index = chain
        .as_deref()
        .map(|c| chains::resolve_chain(c).to_string())
        .unwrap_or_else(|| ctx.chain_index_or("solana"));

    // ── Step 1: collect & aggregate signals ──────────────────────────
    // A signal API failure is not fatal — return gracefully with empty results.
    let raw_signals = ok_or_null(
        signal::fetch_list(
            &client,
            &chain_index,
            None,
            None,
            None,
            None,
            None,
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

    let top_tokens: Vec<(String, Value)> = extract_top_tokens(&raw_signals, TOP_N);

    // ── Step 2: per-token due diligence (parallel, max TOP_N) ────────
    let mut set: JoinSet<(String, Value)> = JoinSet::new();

    for (token_addr, signal_item) in top_tokens {
        let c = client.clone();
        let ci = chain_index.clone();
        let addr = token_addr.clone();
        set.spawn(async move {
            let (price, advanced, security) = tokio::join!(
                token::fetch_price_info(&c, &addr, &ci),
                token::fetch_advanced_info(&c, &addr, &ci),
                fetch_token_scan(&c, &ci, &addr),
            );

            let advanced_val = ok_or_null(advanced);

            // Launchpad enrichment — only when protocolId is non-empty.
            // Uses is_launchpad_token to safely handle null advanced-info.
            let launchpad = if is_launchpad_token(&advanced_val) {
                let (dev_info, bundle_info) = tokio::join!(
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
                json!({
                    "devInfo":    ok_or_null(dev_info),
                    "bundleInfo": ok_or_null(bundle_info),
                })
            } else {
                Value::Null
            };

            let result = json!({
                "signal":    signal_item,
                "price":     ok_or_null(price),
                "contract":  advanced_val,
                "security":  security,
                "launchpad": launchpad,
            });
            (addr, result)
        });
    }

    let mut enriched: Vec<Value> = Vec::new();
    while let Some(join_res) = set.join_next().await {
        let (addr, data) = join_res?;
        enriched.push(json!({ "address": addr, "data": data }));
    }

    output::success(json!({
        "workflow":   "smart-money",
        "chain":      chain_index,
        "rawSignals": raw_signals,
        "topTokens":  enriched,
    }));
    Ok(())
}

/// Extract the top N unique tokens from a signal list response, sorted descending
/// by SM wallet count. Handles both a bare array and a `{"data": [...]}` wrapper.
/// Returns an empty vec on null, empty, or malformed input.
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
            // Accept walletCount or addressCount as the SM wallet count field.
            let count = item["walletCount"]
                .as_u64()
                .or_else(|| item["addressCount"].as_u64())
                .unwrap_or(0);
            Some((count, addr, item.clone()))
        })
        .collect();

    items.sort_by(|a, b| b.0.cmp(&a.0));
    items
        .into_iter()
        .take(n)
        .map(|(_, addr, item)| (addr, item))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    // ── extract_top_tokens ────────────────────────────────────────────

    #[test]
    fn empty_array_returns_empty() {
        let result = extract_top_tokens(&json!([]), 5);
        assert!(result.is_empty());
    }

    #[test]
    fn null_input_returns_empty() {
        let result = extract_top_tokens(&Value::Null, 5);
        assert!(result.is_empty());
    }

    #[test]
    fn plain_object_with_no_array_returns_empty() {
        let result = extract_top_tokens(&json!({ "foo": "bar" }), 5);
        assert!(result.is_empty());
    }

    #[test]
    fn bare_array_extracts_tokens() {
        let signals = json!([
            { "tokenContractAddress": "0xAAA", "walletCount": 3 },
            { "tokenContractAddress": "0xBBB", "walletCount": 7 },
        ]);
        let result = extract_top_tokens(&signals, 5);
        assert_eq!(result.len(), 2);
    }

    #[test]
    fn data_key_wrapper_extracts_tokens() {
        let signals = json!({
            "data": [
                { "tokenContractAddress": "0xAAA", "walletCount": 1 },
                { "tokenContractAddress": "0xBBB", "walletCount": 2 },
            ]
        });
        let result = extract_top_tokens(&signals, 5);
        assert_eq!(result.len(), 2);
        // sorted descending: BBB first
        assert_eq!(result[0].0, "0xBBB");
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
            { "tokenContractAddress": "0xDUP", "walletCount": 5  },  // duplicate
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
        assert_eq!(result[0].0, "0xA"); // higher addressCount wins
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
    fn skips_items_missing_address_field() {
        let signals = json!([
            { "walletCount": 10 },                               // no address field
            { "tokenContractAddress": "0xGOOD", "walletCount": 1 },
        ]);
        let result = extract_top_tokens(&signals, 5);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].0, "0xGOOD");
    }

    #[test]
    fn missing_wallet_count_defaults_to_zero() {
        // Tokens with no walletCount or addressCount still appear, sorted last
        let signals = json!([
            { "tokenContractAddress": "0xNO_COUNT" },
            { "tokenContractAddress": "0xHAS_COUNT", "walletCount": 5 },
        ]);
        let result = extract_top_tokens(&signals, 5);
        assert_eq!(result.len(), 2);
        assert_eq!(result[0].0, "0xHAS_COUNT");
    }

    #[test]
    fn uses_alternate_address_field() {
        // Some signal responses use "address" instead of "tokenContractAddress"
        let signals = json!([
            { "address": "0xALT", "walletCount": 4 },
        ]);
        let result = extract_top_tokens(&signals, 5);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].0, "0xALT");
    }
}
