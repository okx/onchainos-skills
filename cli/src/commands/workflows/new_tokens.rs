/// W4 — New Token Screening
///
/// Step 1: fetch MIGRATED launchpad tokens
/// Step 2: parallel safety + dev enrichment for top 10 results
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

    // ── Step 1: fetch launchpad tokens (direct API call) ─────────────
    let token_list = ok_or_null(
        client
            .get(
                "/api/v6/dex/market/memepump/tokenList",
                &[("chainIndex", chain_index.as_str()), ("stage", stage_str.as_str())],
            )
            .await,
    );

    // Extract top N token addresses from the list response.
    let top_tokens: Vec<(String, Value)> = extract_top_tokens(&token_list, ENRICH_TOP_N);

    // ── Step 2: parallel enrichment ──────────────────────────────────
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

/// Extract top N token entries from the memepump token list response.
fn extract_top_tokens(list: &Value, n: usize) -> Vec<(String, Value)> {
    let arr = match list.as_array() {
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
