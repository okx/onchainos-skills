/// W1 — Token Research
///
/// Step 1 (parallel): token info + price-info + advanced-info + security scan
/// Step 2 (parallel): holders + cluster overview + top traders + signal list
/// Step 3 (parallel, conditional): launchpad data when protocolId is non-empty
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

    // ── Step 1: core data ────────────────────────────────────────────
    let (info, price, advanced, security) = tokio::join!(
        token::fetch_info(&client, address, &chain_index),
        token::fetch_price_info(&client, address, &chain_index),
        token::fetch_advanced_info(&client, address, &chain_index),
        fetch_token_scan(&client, &chain_index, address),
    );

    let info = ok_or_null(info);
    let price = ok_or_null(price);
    let advanced = ok_or_null(advanced);
    // security is already Value (never errors out)

    // ── Step 2: on-chain structure ───────────────────────────────────
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
    let protocol_id = advanced["protocolId"].as_str().unwrap_or("").to_string();
    let launchpad = if !protocol_id.is_empty() {
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
            "tokenDetails": ok_or_null(details),
            "devInfo":      ok_or_null(dev_info),
            "bundleInfo":   ok_or_null(bundle_info),
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
