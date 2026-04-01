use anyhow::{bail, Result};
use serde_json::{json, Value};
//
use crate::client::ApiClient;

use super::api::fetch_position_detail;

/// Validate and convert `coinAmount` from minimal units (integer string) to
/// human-readable decimal string using `tokenPrecision`.
///
/// Example: coinAmount="500000", tokenPrecision=6 -> "0.5"
///
/// Requirements:
/// - `tokenPrecision` is REQUIRED for every item
/// - `coinAmount` MUST be an integer (no decimal point) -- fails fast otherwise
pub(super) fn convert_minimal_to_decimal(items: &mut [Value]) -> Result<()> {
    for item in items.iter_mut() {
        let prec: Option<u32> = item.get("tokenPrecision").and_then(|v| {
            v.as_str()
                .and_then(|s| s.parse::<u32>().ok())
                .or_else(|| v.as_u64().map(|n| n as u32))
        });

        let precision = prec.ok_or_else(|| {
            anyhow::anyhow!(
                "tokenPrecision is required in --user-input for each token. \
                 Get it from `defi prepare` -> investWithTokenList[].tokenPrecision"
            )
        })?;

        if let Some(amount_str) = item.get("coinAmount").and_then(|v| v.as_str()) {
            // Reject zero or empty amounts
            if amount_str.is_empty() || amount_str.chars().all(|c| c == '0') {
                bail!(
                    "coinAmount cannot be zero or empty. Got \"{}\".",
                    amount_str
                );
            }
            if amount_str.contains('.') {
                bail!(
                    "coinAmount must be an integer (minimal units), got \"{}\". \
                     Convert: userAmount x 10^tokenPrecision. \
                     Example: 0.5 USDC (precision=6) -> coinAmount=\"500000\"",
                    amount_str
                );
            }
            let decimal = minimal_to_decimal_str(amount_str, precision);
            item["coinAmount"] = json!(decimal);
        }
        // Remove tokenPrecision before sending to backend
        item.as_object_mut().map(|m| m.remove("tokenPrecision"));
    }
    Ok(())
}

/// Convert an integer string to a decimal string given precision.
/// e.g. "500000" with precision 6 -> "0.5"
/// e.g. "1154528481238320444" with precision 18 -> "1.154528481238320444"
pub(super) fn minimal_to_decimal_str(amount: &str, precision: u32) -> String {
    let precision_digits = precision as usize;
    if precision_digits == 0 {
        return amount.to_string();
    }
    let zero_padded = if amount.len() <= precision_digits {
        format!("{:0>width$}", amount, width = precision_digits + 1)
    } else {
        amount.to_string()
    };
    let (integer_part, decimal_part) = zero_padded.split_at(zero_padded.len() - precision_digits);
    // Trim trailing zeros from decimal part
    let trimmed = decimal_part.trim_end_matches('0');
    if trimmed.is_empty() {
        integer_part.to_string()
    } else {
        format!("{}.{}", integer_part, trimmed)
    }
}

/// Convert a decimal string to an integer string (minimal units) given precision.
/// Pure string operation -- no floating point, no precision loss.
/// e.g. "0.5" with precision 6 -> "500000"
/// e.g. "226.483834" with precision 6 -> "226483834"
/// e.g. "0.005" with precision 18 -> "5000000000000000"
pub(super) fn decimal_to_minimal_str(amount: &str, precision: u32) -> String {
    let precision_digits = precision as usize;
    if precision_digits == 0 {
        // No decimal part expected; strip any decimal point
        return amount.split('.').next().unwrap_or(amount).to_string();
    }
    let (integer, decimal) = if let Some(dot_pos) = amount.find('.') {
        (&amount[..dot_pos], &amount[dot_pos + 1..])
    } else {
        (amount, "")
    };
    // Handle owned string for padding case
    let zero_padded;
    let final_decimal = if decimal.len() >= precision_digits {
        &decimal[..precision_digits]
    } else {
        zero_padded = format!("{:0<width$}", decimal, width = precision_digits);
        &zero_padded
    };
    let integer_with_decimal = format!("{}{}", integer, final_decimal);
    // Strip leading zeros but keep at least "0"
    let leading_zeros_stripped = integer_with_decimal.trim_start_matches('0');
    if leading_zeros_stripped.is_empty() {
        "0".to_string()
    } else {
        leading_zeros_stripped.to_string()
    }
}

/// Try to auto-build expectOutputList from position-detail for the given reward type.
/// Returns None silently on any error or when no matching tokens are found.
pub async fn extract_expect_output(
    client: &ApiClient,
    wallet: &str,
    chain_index: &str,
    platform_id: &str,
    reward_type: &str,
    investment_id: Option<&str>,
) -> Result<Option<String>> {
    let raw = fetch_position_detail(client, wallet, chain_index, platform_id).await?;
    let platforms = match raw.as_array() {
        Some(a) => a.clone(),
        None => return Ok(None),
    };

    let mut tokens: Vec<Value> = Vec::new();

    for platform in &platforms {
        let wallets = match platform
            .get("walletIdPlatformDetailList")
            .and_then(|v| v.as_array())
        {
            Some(a) => a.clone(),
            None => continue,
        };
        for w in &wallets {
            let networks = match w.get("networkHoldVoList").and_then(|v| v.as_array()) {
                Some(a) => a.clone(),
                None => continue,
            };
            for net in &networks {
                let search_in_market = matches!(
                    reward_type,
                    "REWARD_INVESTMENT" | "REWARD_OKX_BONUS" | "REWARD_MERKLE_BONUS"
                );
                if search_in_market {
                    // Search inside investMarketTokenBalanceVoList[].assetMap.SUPPLY/BORROW[].rewardDefiTokenInfo[]
                    // For REWARD_INVESTMENT: filter by investmentId
                    // For REWARD_OKX_BONUS / REWARD_MERKLE_BONUS: collect all matching entries
                    if let Some(markets) = net
                        .get("investMarketTokenBalanceVoList")
                        .and_then(|v| v.as_array())
                    {
                        for market in markets {
                            for side in &["SUPPLY", "BORROW"] {
                                if let Some(items) = market
                                    .get("assetMap")
                                    .and_then(|m| m.get(side))
                                    .and_then(|v| v.as_array())
                                {
                                    for item in items {
                                        // Filter by investmentId when provided
                                        if let Some(id) = investment_id {
                                            let item_id = item
                                                .get("investmentId")
                                                .and_then(|v| v.as_i64())
                                                .map(|n| n.to_string());
                                            if item_id.as_deref() != Some(id) {
                                                continue;
                                            }
                                        }
                                        if let Some(rewards) = item
                                            .get("rewardDefiTokenInfo")
                                            .and_then(|v| v.as_array())
                                        {
                                            for reward in rewards {
                                                let rt = reward
                                                    .get("rewardType")
                                                    .and_then(|v| v.as_str())
                                                    .unwrap_or("");
                                                if rt == reward_type {
                                                    if let Some(base) = reward
                                                        .get("baseDefiTokenInfos")
                                                        .and_then(|v| v.as_array())
                                                    {
                                                        for t in base {
                                                            tokens.push(json!({
                                                                "chainIndex": chain_index,
                                                                "tokenAddress": t.get("tokenAddress").and_then(|v| v.as_str()).unwrap_or(""),
                                                                "coinAmount": t.get("coinAmount").and_then(|v| v.as_str()).unwrap_or("0"),
                                                            }));
                                                        }
                                                    }
                                                }
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
                // Search investMarketTokenBalanceVoList[].marketRewards[] for REWARD_PLATFORM
                // (and other market-level reward types). Many protocols (Venus, BENQI, etc.)
                // put REWARD_PLATFORM here instead of availableRewards.
                if let Some(markets) = net
                    .get("investMarketTokenBalanceVoList")
                    .and_then(|v| v.as_array())
                {
                    for market in markets {
                        if let Some(rewards) =
                            market.get("marketRewards").and_then(|v| v.as_array())
                        {
                            for reward in rewards {
                                let rt = reward
                                    .get("rewardType")
                                    .and_then(|v| v.as_str())
                                    .unwrap_or("");
                                if rt == reward_type {
                                    if let Some(base) =
                                        reward.get("baseDefiTokenInfos").and_then(|v| v.as_array())
                                    {
                                        for t in base {
                                            tokens.push(json!({
                                                "chainIndex": chain_index,
                                                "tokenAddress": t.get("tokenAddress").and_then(|v| v.as_str()).unwrap_or(""),
                                                "coinAmount": t.get("coinAmount").and_then(|v| v.as_str()).unwrap_or("0"),
                                            }));
                                        }
                                    }
                                }
                            }
                        }
                    }
                }

                // Search investTokenBalanceVoList[].rewardDefiTokenInfo[] for standalone
                // positions (Radpie, Compound, etc. that don't use investMarketTokenBalanceVoList).
                if let Some(standalone) = net
                    .get("investTokenBalanceVoList")
                    .and_then(|v| v.as_array())
                {
                    for item in standalone {
                        // Filter by investmentId when provided
                        if let Some(id) = investment_id {
                            let item_id = item
                                .get("investmentId")
                                .and_then(|v| v.as_i64())
                                .map(|n| n.to_string());
                            if item_id.as_deref() != Some(id) {
                                continue;
                            }
                        }
                        if let Some(rewards) =
                            item.get("rewardDefiTokenInfo").and_then(|v| v.as_array())
                        {
                            for reward in rewards {
                                let rt = reward
                                    .get("rewardType")
                                    .and_then(|v| v.as_str())
                                    .unwrap_or("");
                                if rt == reward_type {
                                    if let Some(base) =
                                        reward.get("baseDefiTokenInfos").and_then(|v| v.as_array())
                                    {
                                        for t in base {
                                            tokens.push(json!({
                                                "chainIndex": chain_index,
                                                "tokenAddress": t.get("tokenAddress").and_then(|v| v.as_str()).unwrap_or(""),
                                                "coinAmount": t.get("coinAmount").and_then(|v| v.as_str()).unwrap_or("0"),
                                            }));
                                        }
                                    }
                                }
                            }
                        }
                    }
                }

                // Also search availableRewards for REWARD_PLATFORM, REWARD_OKX_BONUS, REWARD_MERKLE_BONUS
                if reward_type != "REWARD_INVESTMENT" {
                    if let Some(available) = net.get("availableRewards").and_then(|v| v.as_array())
                    {
                        for reward in available {
                            let rt = reward
                                .get("rewardType")
                                .and_then(|v| v.as_str())
                                .unwrap_or("");
                            if rt == reward_type {
                                if let Some(base) =
                                    reward.get("baseDefiTokenInfos").and_then(|v| v.as_array())
                                {
                                    for t in base {
                                        tokens.push(json!({
                                            "chainIndex": chain_index,
                                            "tokenAddress": t.get("tokenAddress").and_then(|v| v.as_str()).unwrap_or(""),
                                            "coinAmount": t.get("coinAmount").and_then(|v| v.as_str()).unwrap_or("0"),
                                        }));
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    // Deduplicate by (chainIndex, tokenAddress) to avoid double-counting tokens that appear
    // in both investMarketTokenBalanceVoList and availableRewards (e.g. REWARD_OKX_BONUS)
    let mut seen = std::collections::HashSet::new();
    tokens.retain(|t| {
        let key = format!(
            "{}:{}",
            t.get("chainIndex").and_then(|v| v.as_str()).unwrap_or(""),
            t.get("tokenAddress").and_then(|v| v.as_str()).unwrap_or(""),
        );
        seen.insert(key)
    });

    if tokens.is_empty() {
        Ok(None)
    } else {
        Ok(Some(serde_json::to_string(&tokens)?))
    }
}
