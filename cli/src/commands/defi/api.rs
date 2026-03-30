use anyhow::Result;
use serde_json::{json, Value};
//
use crate::client::ApiClient;

use super::helpers::convert_minimal_to_decimal;

/// GET /api/v6/defi/product/supported-chains
pub async fn fetch_chains(client: &ApiClient) -> Result<Value> {
    client.get("/api/v6/defi/product/supported-chains", &[]).await
}

/// GET /api/v6/defi/product/supported-platforms
pub async fn fetch_protocols(client: &ApiClient) -> Result<Value> {
    client
        .get("/api/v6/defi/product/supported-platforms", &[])
        .await
}

/// POST /api/v6/defi/product/search
pub async fn fetch_search(
    client: &ApiClient,
    token: Option<&str>,
    platform: Option<&str>,
    chain_index: Option<&str>,
    product_group: Option<&str>,
    page_num: Option<u32>,
) -> Result<Value> {
    let mut body = json!({});
    if let Some(t) = token {
        let list: Vec<&str> = t.split(',').map(|s| s.trim()).collect();
        body["tokenKeywordList"] = json!(list);
    }
    if let Some(pf) = platform {
        let list: Vec<&str> = pf.split(',').map(|s| s.trim()).collect();
        body["platformKeywordList"] = json!(list);
    }
    if let Some(ci) = chain_index {
        body["chainIndex"] = json!(ci);
    }
    if let Some(pg) = product_group {
        body["productGroup"] = json!(pg);
    }
    if let Some(p) = page_num {
        body["pageNum"] = json!(p);
    }
    client.post("/api/v6/defi/product/search", &body).await
}

/// GET /api/v6/defi/product/detail
pub async fn fetch_detail(client: &ApiClient, investment_id: &str) -> Result<Value> {
    client
        .get(
            "/api/v6/defi/product/detail",
            &[("investmentId", investment_id)],
        )
        .await
}

/// POST /api/v6/defi/product/detail/prepare
pub async fn fetch_prepare(client: &ApiClient, investment_id: &str) -> Result<Value> {
    let body = json!({ "investmentId": investment_id });
    client
        .post("/api/v6/defi/product/detail/prepare", &body)
        .await
}

/// POST /api/v6/defi/transaction/enter
#[allow(clippy::too_many_arguments)]
pub async fn fetch_enter(
    client: &ApiClient,
    investment_id: &str,
    address: &str,
    user_input: &str,
    slippage: &str,
    token_id: Option<&str>,
    tick_lower: Option<i64>,
    tick_upper: Option<i64>,
) -> Result<Value> {
    let mut user_input_list: Vec<Value> = serde_json::from_str(user_input)
        .map_err(|e| anyhow::anyhow!("failed to parse --user-input as JSON array: {e}"))?;

    // Validate: coinAmount must be integer, tokenPrecision required. Convert to decimal for API.
    convert_minimal_to_decimal(&mut user_input_list)?;

    let mut body = json!({
        "investmentId": investment_id,
        "address": address,
        "userInputList": user_input_list,
        "slippage": slippage,
    });

    // V3 Pool specific fields
    if let Some(tid) = token_id {
        body["tokenId"] = json!(tid);
    }
    if let Some(tl) = tick_lower {
        body["tickLower"] = json!(tl);
    }
    if let Some(tu) = tick_upper {
        body["tickUpper"] = json!(tu);
    }

    client.post("/api/v6/defi/transaction/enter", &body).await
}

/// POST /api/v6/defi/transaction/exit
#[allow(clippy::too_many_arguments)]
pub async fn fetch_exit(
    client: &ApiClient,
    product_id: &str,
    chain_index: &str,
    wallet: &str,
    redeem_ratio: Option<&str>,
    token_address: Option<&str>,
    token_symbol: Option<&str>,
    amount: Option<&str>,
    token_precision: Option<u32>,
    token_id: Option<&str>,
    slippage: &str,
    user_input: Option<&str>,
) -> Result<Value> {
    let mut body = json!({
        "investmentId": product_id,
        "address": wallet,
        "slippage": slippage,
    });

    // redeemPercent for dynamic-balance tokens (aTokens, lending protocols)
    if let Some(pct) = redeem_ratio {
        body["redeemPercent"] = json!(pct);
    }
    if let Some(tid) = token_id {
        body["tokenId"] = json!(tid);
    }

    // user_input JSON array (required for liquid staking / other non-lending exits)
    if let Some(ui) = user_input {
        let mut list: Vec<Value> = serde_json::from_str(ui)
            .map_err(|e| anyhow::anyhow!("failed to parse --user-input as JSON array: {e}"))?;
        // Validate: coinAmount must be integer, tokenPrecision required. Convert to decimal for API.
        convert_minimal_to_decimal(&mut list)?;
        body["userInputList"] = json!(list);
    } else if let (Some(ta), Some(amt)) = (token_address, amount) {
        // Single-token shorthand: --token + --amount
        let mut token_input = json!({
            "tokenAddress": ta,
            "chainIndex": chain_index,
            "coinAmount": amt,
        });
        if let Some(sym) = token_symbol {
            token_input["tokenSymbol"] = json!(sym);
        }
        if let Some(prec) = token_precision {
            token_input["tokenPrecision"] = json!(prec);
        }
        body["userInputList"] = json!([token_input]);
    }

    client.post("/api/v6/defi/transaction/exit", &body).await
}

/// POST /api/v6/defi/transaction/claim
#[allow(clippy::too_many_arguments)]
pub async fn fetch_claim(
    client: &ApiClient,
    wallet: &str,
    chain_index: &str,
    reward_type: &str,
    product_id: Option<&str>,
    platform_id: Option<&str>,
    token_id: Option<&str>,
    principal_index: Option<&str>,
    expect_output_list: Option<&str>,
) -> Result<Value> {
    let mut body = json!({
        "address": wallet,
        "rewardType": reward_type,
    });
    if !chain_index.is_empty() {
        body["chainIndex"] = json!(chain_index.parse::<i64>().unwrap_or(0));
    }
    if let Some(pid) = product_id {
        body["investmentId"] = json!(pid);
    }
    if let Some(pfid) = platform_id {
        body["analysisPlatformId"] = json!(pfid);
    }
    if let Some(tid) = token_id {
        body["tokenId"] = json!(tid);
    }
    if let Some(pi) = principal_index {
        body["principalIndex"] = json!(pi);
    }
    if let Some(eol) = expect_output_list {
        let arr: Vec<Value> = serde_json::from_str(eol)
            .map_err(|e| anyhow::anyhow!("failed to parse --expect-output as JSON array: {e}"))?;
        body["expectOutputList"] = json!(arr);
    }

    client.post("/api/v6/defi/transaction/claim", &body).await
}

/// POST /api/v6/defi/calculator/enter/info
#[allow(clippy::too_many_arguments)]
pub async fn fetch_calculate_entry(
    client: &ApiClient,
    investment_id: &str,
    address: &str,
    input_token_address: &str,
    input_amount: &str,
    token_decimal: &str,
    tick_lower: Option<i64>,
    tick_upper: Option<i64>,
) -> Result<Value> {
    let mut body = json!({
        "investmentId": investment_id,
        "address": address,
        "inputTokenAddress": input_token_address,
        "inputAmount": input_amount,
        "tokenDecimal": token_decimal,
    });
    if let Some(tl) = tick_lower {
        body["tickLower"] = json!(tl);
    }
    if let Some(tu) = tick_upper {
        body["tickUpper"] = json!(tu);
    }
    client
        .post("/api/v6/defi/calculator/enter/info", &body)
        .await
}

/// POST /api/v6/defi/user/asset/platform/list
pub async fn fetch_positions(client: &ApiClient, wallet: &str, chains: &str) -> Result<Value> {
    let wallet_list: Vec<Value> = chains
        .split(',')
        .map(|c| {
            let idx = crate::chains::resolve_chain(c.trim());
            json!({
                "chainIndex": idx,
                "walletAddress": wallet,
            })
        })
        .collect();

    let body = json!({ "walletAddressList": wallet_list });
    client
        .post("/api/v6/defi/user/asset/platform/list", &body)
        .await
}

/// POST /api/v6/defi/user/asset/platform/detail
pub async fn fetch_position_detail(
    client: &ApiClient,
    wallet: &str,
    chain_index: &str,
    platform_id: &str,
) -> Result<Value> {
    let body = json!({
        "walletAddressList": [{
            "chainIndex": chain_index,
            "walletAddress": wallet,
        }],
        "platformList": [{
            "analysisPlatformId": platform_id,
            "chainIndex": chain_index,
        }],
    });

    client
        .post("/api/v6/defi/user/asset/platform/detail", &body)
        .await
}
