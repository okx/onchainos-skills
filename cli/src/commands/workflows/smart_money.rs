/// W3 — Smart Money Signals
///
/// Step 1: fetch signal list, aggregate by token (sort desc by SM wallet count), take top 5
/// Step 2: per-token parallel due diligence (price-info + advanced-info + security scan +
///         optional memepump dev/bundle info when protocolId is non-empty)
use anyhow::Result;
use serde_json::{json, Value};
use tokio::task::JoinSet;

use crate::chains;
use crate::commands::{memepump, signal, token};
use crate::output;

use super::{fetch_token_scan, ok_or_null, Context};

const TOP_N: usize = 5;

pub async fn run(ctx: &Context, chain: Option<String>) -> Result<()> {
    let client = ctx.client_async().await?;
    let chain_index = chain
        .as_deref()
        .map(|c| chains::resolve_chain(c).to_string())
        .unwrap_or_else(|| ctx.chain_index_or("solana"));

    // ── Step 1: collect & aggregate signals ──────────────────────────
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

    // Extract top tokens by SM wallet count from the signal list.
    // The signal list response is an array of signal objects; each has
    // tokenContractAddress and walletCount (or similar field).
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
            let protocol_id = advanced_val["protocolId"].as_str().unwrap_or("").to_string();

            let launchpad = if !protocol_id.is_empty() {
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
                "signal":   signal_item,
                "price":    ok_or_null(price),
                "contract": advanced_val,
                "security": security,
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
        "workflow": "smart-money",
        "chain":    chain_index,
        "rawSignals": raw_signals,
        "topTokens": enriched,
    }));
    Ok(())
}

/// Pull top N unique token addresses from the signal list response,
/// sorted descending by the number of SM wallets that bought each token.
fn extract_top_tokens(signals: &Value, n: usize) -> Vec<(String, Value)> {
    let arr = match signals.as_array() {
        Some(a) => a,
        // Some APIs nest under a data key
        None => match signals["data"].as_array() {
            Some(a) => a,
            None => return vec![],
        },
    };

    // Each item may have tokenContractAddress + walletCount (or addressCount).
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
    items
        .into_iter()
        .take(n)
        .map(|(_, addr, item)| (addr, item))
        .collect()
}
