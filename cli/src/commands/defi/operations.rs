use anyhow::{bail, Result};
use serde_json::{json, Value};
//
use super::api::*;
use super::helpers::*;
use crate::client::ApiClient;
use serde_json::Value as JsonValue;

// ── Shared types ────────────────────────────────────────────────────

struct TokenInfo<'a> {
    address: &'a str,
    chain_index: &'a str,
    precision: u32,
    symbol: &'a str,
}

// ── Main entry point ────────────────────────────────────────────────

/// High-level invest: route to V3 or standard based on investType
#[allow(clippy::too_many_arguments)]
pub(crate) async fn cmd_invest(
    client: &ApiClient,
    investment_id: &str,
    address: &str,
    token: &str,
    amount: &str,
    token2: Option<&str>,
    amount2: Option<&str>,
    slippage: &str,
    token_id: Option<&str>,
    tick_lower: Option<i64>,
    tick_upper: Option<i64>,
    range: Option<f64>,
) -> Result<JsonValue> {
    // 1. Fetch detail → check isInvestable
    let detail = fetch_detail(client, investment_id).await?;
    if !is_investable(&detail) {
        bail!("This product is not investable (isInvestable=false). Check detail for eligibility.");
    }

    // 2. Fetch prepare → get investWithTokenList
    let prepare = fetch_prepare(client, investment_id).await?;
    let invest_tokens = prepare
        .get("investWithTokenList")
        .and_then(|v| v.as_array())
        .ok_or_else(|| anyhow::anyhow!("investWithTokenList not found in prepare response"))?;

    // 3. Resolve and validate token1
    let matched1 = find_matching_token(invest_tokens, token)?;
    let primary_token = extract_token_info(matched1, token)?;
    validate_amount(amount)?;

    // 4. Route: V3 or standard
    let invest_type = detail
        .get("investType")
        .and_then(|v| {
            v.as_u64()
                .or_else(|| v.as_str().and_then(|s| s.parse().ok()))
        })
        .unwrap_or(0);

    // Returns: (user_input_json, surplus_info, resolved_tick_lower, resolved_tick_upper)
    let (user_input_json, surplus_info, resolved_tl, resolved_tu) = if invest_type == 2 {
        // V3: resolve token2 — explicit or auto-detect from investWithTokenList
        let secondary_token_resolved: Option<(TokenInfo, &str)> =
            if let (Some(secondary_token_name), Some(secondary_amount)) = (token2, amount2) {
                let matched2 = find_matching_token(invest_tokens, secondary_token_name)?;
                let secondary_token = extract_token_info(matched2, secondary_token_name)?;
                // V3 allows amount=0 for single-sided liquidity
                validate_amount_v3(secondary_amount)?;
                Some((secondary_token, secondary_amount))
            } else if let Some(secondary_amount) = amount2 {
                validate_amount_v3(secondary_amount)?;
                let other = invest_tokens.iter().find(|t| {
                    let addr = t
                        .get("tokenAddress")
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_lowercase();
                    addr != primary_token.address.to_lowercase()
                });
                if let Some(other_token) = other {
                    let secondary_token = extract_token_info(other_token, "auto-detected")?;
                    Some((secondary_token, secondary_amount))
                } else {
                    None
                }
            } else {
                None
            };
        let (json, change, tl, tu) = invest_v3(
            client,
            investment_id,
            address,
            &primary_token,
            amount,
            secondary_token_resolved.as_ref(),
            &prepare,
            token_id,
            tick_lower,
            tick_upper,
            range,
        )
        .await?;
        (json, change, tl, tu)
    } else {
        let (json, change) = invest_standard(&primary_token, amount)?;
        (json, change, None, None)
    };

    // 6. Call fetch_enter
    let mut result = fetch_enter(
        client,
        investment_id,
        address,
        &user_input_json,
        slippage,
        token_id,
        resolved_tl,
        resolved_tu,
    )
    .await?;

    // 7. Warnings
    append_warnings(&mut result, &detail);

    // 8. Rebalance change info
    if let Some((token_sym, token_addr, human_amount)) = surplus_info {
        result["rebalance"] = json!({
            "surplusToken": token_sym,
            "surplusTokenAddress": token_addr,
            "surplusAmount": human_amount,
            "message": format!("{} {} not invested (returned to wallet)", human_amount, token_sym)
        });
    }

    Ok(result)
}

// ── Standard (non-V3) invest ────────────────────────────────────────

type InvestResult = (String, Option<(String, String, String)>);

fn invest_standard(primary_token: &TokenInfo, amount: &str) -> Result<InvestResult> {
    let user_input_list = vec![json!({
        "tokenAddress": primary_token.address,
        "chainIndex": primary_token.chain_index,
        "coinAmount": amount,
        "tokenPrecision": primary_token.precision.to_string(),
    })];
    Ok((serde_json::to_string(&user_input_list)?, None))
}

// ── V3 Pool invest ──────────────────────────────────────────────────

#[allow(clippy::too_many_arguments)]
async fn invest_v3(
    client: &ApiClient,
    investment_id: &str,
    address: &str,
    primary_token: &TokenInfo<'_>,
    amount: &str,
    secondary_token_resolved: Option<&(TokenInfo<'_>, &str)>, // (token2_info, amount2) — already validated
    prepare: &Value,
    token_id: Option<&str>,
    tick_lower: Option<i64>,
    tick_upper: Option<i64>,
    range: Option<f64>,
) -> Result<(
    String,
    Option<(String, String, String)>,
    Option<i64>,
    Option<i64>,
)> {
    // Add-to-existing position (token_id): no tick needed
    // New position: must have tick range
    let (resolved_tl, resolved_tu) = if token_id.is_some() {
        (None, None)
    } else {
        resolve_ticks(prepare, tick_lower, tick_upper, range)?
    };

    let (json, change) = if let Some((secondary_token, secondary_amount)) = secondary_token_resolved
    {
        invest_v3_dual(
            client,
            investment_id,
            address,
            primary_token,
            amount,
            secondary_token,
            secondary_amount,
            resolved_tl,
            resolved_tu,
        )
        .await?
    } else {
        invest_v3_single(
            client,
            investment_id,
            address,
            primary_token,
            amount,
            &prepare
                .get("investWithTokenList")
                .and_then(|v| v.as_array())
                .cloned()
                .unwrap_or_default(),
            resolved_tl,
            resolved_tu,
        )
        .await?
    };
    Ok((json, change, resolved_tl, resolved_tu))
}

/// V3 single-token entry: user provides one token, API calculates the other
#[allow(clippy::too_many_arguments)]
async fn invest_v3_single(
    client: &ApiClient,
    investment_id: &str,
    address: &str,
    primary_token: &TokenInfo<'_>,
    amount: &str,
    invest_tokens: &[Value],
    tick_lower: Option<i64>,
    tick_upper: Option<i64>,
) -> Result<(String, Option<(String, String, String)>)> {
    let human_amount = minimal_to_decimal_str(amount, primary_token.precision);
    let calc_result = fetch_calculate_entry(
        client,
        investment_id,
        address,
        primary_token.address,
        &human_amount,
        &primary_token.precision.to_string(),
        tick_lower,
        tick_upper,
    )
    .await?;

    let calc_tokens = calc_result
        .get("investWithTokenList")
        .and_then(|v| v.as_array())
        .ok_or_else(|| {
            anyhow::anyhow!("investWithTokenList not found in calculate-entry response")
        })?;

    let mut user_input_list: Vec<Value> = Vec::new();
    for ct in calc_tokens {
        let calc_token_address = ct
            .get("tokenAddress")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        let calc_token_chain = ct
            .get("chainIndex")
            .and_then(|v| v.as_str())
            .unwrap_or(primary_token.chain_index);
        let calc_token_amount = ct.get("coinAmount").and_then(|v| v.as_str()).unwrap_or("0");
        let calc_token_precision = find_token_precision(invest_tokens, calc_token_address);
        let calc_token_minimal = decimal_to_minimal_str(calc_token_amount, calc_token_precision);
        user_input_list.push(json!({
            "tokenAddress": calc_token_address,
            "chainIndex": calc_token_chain,
            "coinAmount": calc_token_minimal,
            "tokenPrecision": calc_token_precision.to_string(),
        }));
    }
    Ok((serde_json::to_string(&user_input_list)?, None))
}

/// V3 dual-token entry: user provides both tokens, CLI rebalances to pool ratio
#[allow(clippy::too_many_arguments)]
async fn invest_v3_dual(
    client: &ApiClient,
    investment_id: &str,
    address: &str,
    primary_token: &TokenInfo<'_>,
    amount: &str,
    secondary_token: &TokenInfo<'_>,
    secondary_amount: &str,
    tick_lower: Option<i64>,
    tick_upper: Option<i64>,
) -> Result<(String, Option<(String, String, String)>)> {
    // Rebalance: use token1 as constraint, check if token2 fits
    let human_amount1 = minimal_to_decimal_str(amount, primary_token.precision);
    let calc1 = fetch_calculate_entry(
        client,
        investment_id,
        address,
        primary_token.address,
        &human_amount1,
        &primary_token.precision.to_string(),
        tick_lower,
        tick_upper,
    )
    .await?;

    let needed_t2_human = find_token_amount_in_calc_result(&calc1, secondary_token.address)?;
    let needed_t2_minimal = decimal_to_minimal_str(&needed_t2_human, secondary_token.precision);

    let user_t2: u128 = secondary_amount.parse().unwrap_or(0);
    let needed_t2: u128 = needed_t2_minimal.parse().unwrap_or(0);

    let (final_t1, final_t2, surplus_symbol, surplus_address, surplus_amount) = if needed_t2
        <= user_t2
    {
        // token1 is the constraint, token2 has surplus
        let change = user_t2 - needed_t2;
        (
            amount.to_string(),
            needed_t2_minimal,
            secondary_token.symbol.to_string(),
            secondary_token.address.to_string(),
            minimal_to_decimal_str(&change.to_string(), secondary_token.precision),
        )
    } else {
        // token2 is the constraint, recalculate with token2
        let human_amount2 = minimal_to_decimal_str(secondary_amount, secondary_token.precision);
        let calc2 = fetch_calculate_entry(
            client,
            investment_id,
            address,
            secondary_token.address,
            &human_amount2,
            &secondary_token.precision.to_string(),
            tick_lower,
            tick_upper,
        )
        .await?;

        let needed_t1_human = find_token_amount_in_calc_result(&calc2, primary_token.address)?;
        let needed_t1_minimal = decimal_to_minimal_str(&needed_t1_human, primary_token.precision);
        let user_t1: u128 = amount.parse().unwrap_or(0);
        let needed_t1: u128 = needed_t1_minimal.parse().unwrap_or(0);
        let change = user_t1.saturating_sub(needed_t1);
        (
            needed_t1_minimal,
            secondary_amount.to_string(),
            primary_token.symbol.to_string(),
            primary_token.address.to_string(),
            minimal_to_decimal_str(&change.to_string(), primary_token.precision),
        )
    };

    let surplus_info = if surplus_amount != "0" && !surplus_amount.is_empty() {
        Some((surplus_symbol, surplus_address, surplus_amount))
    } else {
        None
    };

    let user_input_list = vec![
        json!({
            "tokenAddress": primary_token.address,
            "chainIndex": primary_token.chain_index,
            "coinAmount": final_t1,
            "tokenPrecision": primary_token.precision.to_string(),
        }),
        json!({
            "tokenAddress": secondary_token.address,
            "chainIndex": secondary_token.chain_index,
            "coinAmount": final_t2,
            "tokenPrecision": secondary_token.precision.to_string(),
        }),
    ];
    Ok((serde_json::to_string(&user_input_list)?, surplus_info))
}

// ── Helpers ─────────────────────────────────────────────────────────

fn is_investable(detail: &Value) -> bool {
    detail
        .get("isInvestable")
        .and_then(|v| {
            v.as_bool()
                .or_else(|| v.as_str().map(|s| s == "true" || s == "1"))
        })
        .unwrap_or(false)
}

fn validate_amount(amount: &str) -> Result<()> {
    if amount.contains('.') {
        bail!(
            "amount must be in minimal units (integer), got \"{}\". \
             Convert: userAmount × 10^tokenPrecision. \
             Example: 0.1 USDC (precision=6) → amount=\"100000\"",
            amount
        );
    }
    if amount.is_empty() || amount.chars().all(|c| c == '0') {
        bail!("amount cannot be zero or empty. Got \"{}\".", amount);
    }
    Ok(())
}

/// Like validate_amount but allows "0" for V3 single-sided liquidity.
fn validate_amount_v3(amount: &str) -> Result<()> {
    if amount.contains('.') {
        bail!(
            "amount must be in minimal units (integer), got \"{}\". \
             Convert: userAmount × 10^tokenPrecision. \
             Example: 0.1 USDC (precision=6) → amount=\"100000\"",
            amount
        );
    }
    if amount.is_empty() {
        bail!("amount cannot be empty.");
    }
    Ok(())
}

fn find_matching_token<'a>(invest_tokens: &'a [Value], token: &str) -> Result<&'a Value> {
    let token_lower = token.to_lowercase();
    invest_tokens
        .iter()
        .find(|t| {
            let sym = t
                .get("tokenSymbol")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_lowercase();
            let addr = t
                .get("tokenAddress")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_lowercase();
            sym == token_lower || addr == token_lower
        })
        .ok_or_else(|| {
            let available: Vec<String> = invest_tokens
                .iter()
                .filter_map(|t| {
                    t.get("tokenSymbol")
                        .and_then(|v| v.as_str())
                        .map(|s| s.to_string())
                })
                .collect();
            anyhow::anyhow!(
                "Token '{}' not found in investWithTokenList. Available: {}",
                token,
                available.join(", ")
            )
        })
}

fn extract_token_info<'a>(matched: &'a Value, token: &str) -> Result<TokenInfo<'a>> {
    let address = matched
        .get("tokenAddress")
        .and_then(|v| v.as_str())
        .filter(|s| !s.is_empty())
        .ok_or_else(|| anyhow::anyhow!("tokenAddress is empty for token '{}'", token))?;
    let chain_index = matched
        .get("chainIndex")
        .and_then(|v| v.as_str())
        .filter(|s| !s.is_empty())
        .ok_or_else(|| anyhow::anyhow!("chainIndex is empty for token '{}'", token))?;
    let precision: u32 = matched
        .get("tokenPrecision")
        .and_then(|v| {
            v.as_str()
                .and_then(|s| s.parse().ok())
                .or_else(|| v.as_u64().map(|n| n as u32))
        })
        .unwrap_or(18);
    let symbol = matched
        .get("tokenSymbol")
        .and_then(|v| v.as_str())
        .unwrap_or("UNKNOWN");
    Ok(TokenInfo {
        address,
        chain_index,
        precision,
        symbol,
    })
}

fn find_token_precision(invest_tokens: &[Value], token_address: &str) -> u32 {
    invest_tokens
        .iter()
        .find(|t| {
            t.get("tokenAddress")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .eq_ignore_ascii_case(token_address)
        })
        .and_then(|t| {
            t.get("tokenPrecision").and_then(|v| {
                v.as_str()
                    .and_then(|s| s.parse().ok())
                    .or_else(|| v.as_u64().map(|n| n as u32))
            })
        })
        .unwrap_or(18)
}

fn find_token_amount_in_calc_result(calc_result: &Value, token_address: &str) -> Result<String> {
    let tokens = calc_result
        .get("investWithTokenList")
        .and_then(|v| v.as_array())
        .ok_or_else(|| anyhow::anyhow!("calculate-entry response missing investWithTokenList"))?;
    let amount = tokens
        .iter()
        .find(|t| {
            t.get("tokenAddress")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .eq_ignore_ascii_case(token_address)
        })
        .and_then(|t| t.get("coinAmount").and_then(|v| v.as_str()))
        .unwrap_or("0");
    Ok(amount.to_string())
}

fn resolve_ticks(
    prepare: &Value,
    tick_lower: Option<i64>,
    tick_upper: Option<i64>,
    range: Option<f64>,
) -> Result<(Option<i64>, Option<i64>)> {
    if tick_lower.is_some() && tick_upper.is_some() {
        return Ok((tick_lower, tick_upper));
    }
    if let Some(range_percent) = range {
        if range_percent <= 0.0 || range_percent > 100.0 {
            bail!(
                "--range must be between 0 and 100 (percent), got {}",
                range_percent
            );
        }
        let current_tick: i64 = prepare
            .get("currentTick")
            .and_then(|v| {
                v.as_str()
                    .and_then(|s| s.parse().ok())
                    .or_else(|| v.as_i64())
            })
            .ok_or_else(|| anyhow::anyhow!("currentTick not found in prepare response"))?;
        let tick_spacing: i64 = prepare
            .get("tickSpacing")
            .and_then(|v| {
                v.as_str()
                    .and_then(|s| s.parse().ok())
                    .or_else(|| v.as_i64())
            })
            .ok_or_else(|| anyhow::anyhow!("tickSpacing not found in prepare response"))?;
        let tick_delta = ((current_tick.abs() as f64) * range_percent / 100.0)
            .max((tick_spacing * 2) as f64) as i64;
        let tick_lower_resolved = ((current_tick - tick_delta) / tick_spacing) * tick_spacing;
        let tick_upper_resolved =
            ((current_tick + tick_delta + tick_spacing - 1) / tick_spacing) * tick_spacing;
        return Ok((Some(tick_lower_resolved), Some(tick_upper_resolved)));
    }
    bail!(
        "V3 pool requires --range (e.g. --range 5 for ±5%) or --tick-lower/--tick-upper. \
         Current tick: {}, tick spacing: {}.",
        prepare
            .get("currentTick")
            .and_then(|v| v.as_str())
            .unwrap_or("unknown"),
        prepare
            .get("tickSpacing")
            .and_then(|v| v.as_str())
            .unwrap_or("unknown")
    );
}

fn append_warnings(result: &mut Value, detail: &Value) {
    if let Some(rate) = detail.get("rate").and_then(|v| {
        v.as_str()
            .and_then(|s| s.parse::<f64>().ok())
            .or_else(|| v.as_f64())
    }) {
        if rate > 0.5 {
            result["highApyWarning"] = json!(true);
        }
    }
    if let Some(health) = detail.get("healthRate").and_then(|v| {
        v.as_str()
            .and_then(|s| s.parse::<f64>().ok())
            .or_else(|| v.as_f64())
    }) {
        if health < 1.5 {
            result["liquidationWarning"] = json!(true);
        }
    }
}

// ── Withdraw / Collect ──────────────────────────────────────────────

/// High-level withdraw: resolve position, build exit calldata
#[allow(clippy::too_many_arguments)]
pub(crate) async fn cmd_withdraw(
    client: &ApiClient,
    investment_id: &str,
    address: &str,
    chain: &str,
    ratio: Option<&str>,
    token_id: Option<&str>,
    slippage: &str,
    amount: Option<&str>,
    platform_id: Option<&str>,
) -> Result<JsonValue> {
    let chain_index = crate::chains::resolve_chain(chain);

    // Check if redemption is supported
    let detail = fetch_detail(client, investment_id).await?;
    let is_support_redeem = detail
        .get("isSupportRedeem")
        .and_then(|v| {
            v.as_bool()
                .or_else(|| v.as_str().map(|s| s == "true" || s == "1"))
        })
        .unwrap_or(true); // default true if field missing
    if !is_support_redeem {
        bail!("This product does not support redemption (isSupportRedeem=false).");
    }

    let is_v3 = token_id.is_some();

    // ── V3 path ──
    if is_v3 {
        if ratio.is_none() {
            bail!("V3 Pool withdrawal requires --ratio (e.g. --ratio 1 for full exit).");
        }
        // V3: only needs token_id + ratio, no user_input
        return fetch_exit(
            client,
            investment_id,
            &chain_index,
            address,
            ratio,
            None,
            None,
            None,
            None,
            token_id,
            slippage,
            None,
        )
        .await;
    }

    // ── Non-V3 path ──

    // Must have at least one of ratio or amount
    if ratio.is_none() && amount.is_none() {
        bail!("Must provide --ratio (e.g. --ratio 1 for full exit) or --amount for partial exit, or both.");
    }

    // amount requires platform_id to resolve token info
    if amount.is_some() && platform_id.is_none() {
        bail!("--amount requires --platform-id to resolve token info from position-detail.");
    }

    // Validate amount is minimal units
    if let Some(amt) = amount {
        validate_amount(amt)?;
    }

    // Build user_input from position-detail
    let user_input: Option<String> = if let Some(platform_id_str) = platform_id {
        let pos_detail =
            fetch_position_detail(client, address, &chain_index, platform_id_str).await?;
        let token_info = find_position_token(&pos_detail, investment_id)?;

        if let Some(amt) = amount {
            // Partial exit: check balance
            let user_amount: u128 = amt.parse().unwrap_or(0);
            let balance_minimal = decimal_to_minimal_str(&token_info.balance, token_info.precision);
            let balance_u128: u128 = balance_minimal.parse().unwrap_or(0);
            if user_amount > balance_u128 {
                bail!(
                    "Requested amount {} exceeds current balance {} {}. Reduce amount or use --ratio 1 for full exit.",
                    minimal_to_decimal_str(amt, token_info.precision),
                    token_info.balance,
                    token_info.symbol
                );
            }
            let list = vec![json!({
                "tokenAddress": token_info.address,
                "chainIndex": token_info.chain_index,
                "coinAmount": amt,
                "tokenPrecision": token_info.precision.to_string(),
            })];
            Some(serde_json::to_string(&list)?)
        } else {
            // Full exit: auto-fill user_input with current balance
            let balance_minimal = decimal_to_minimal_str(&token_info.balance, token_info.precision);
            let list = vec![json!({
                "tokenAddress": token_info.address,
                "chainIndex": token_info.chain_index,
                "coinAmount": balance_minimal,
                "tokenPrecision": token_info.precision.to_string(),
            })];
            Some(serde_json::to_string(&list)?)
        }
    } else {
        // No platform_id: rely on ratio only (fallback)
        None
    };

    fetch_exit(
        client,
        investment_id,
        &chain_index,
        address,
        ratio,
        None,
        None,
        None,
        None,
        None,
        slippage,
        user_input.as_deref(),
    )
    .await
}

/// Token info extracted from position-detail
struct PositionTokenInfo {
    address: String,
    chain_index: String,
    precision: u32,
    balance: String,
    symbol: String,
}

/// Find the token matching investment_id in position-detail response
fn find_position_token(pos_detail: &JsonValue, investment_id: &str) -> Result<PositionTokenInfo> {
    let platforms = pos_detail
        .as_array()
        .ok_or_else(|| anyhow::anyhow!("position-detail response is not an array"))?;

    for platform in platforms {
        let wallets = match platform
            .get("walletIdPlatformDetailList")
            .and_then(|v| v.as_array())
        {
            Some(a) => a,
            None => continue,
        };
        for w in wallets {
            let networks = match w.get("networkHoldVoList").and_then(|v| v.as_array()) {
                Some(a) => a,
                None => continue,
            };
            for net in networks {
                // Search in investTokenBalanceVoList (direct positions like SushiSwap pools)
                if let Some(invests) = net
                    .get("investTokenBalanceVoList")
                    .and_then(|v| v.as_array())
                {
                    if let Some(info) = find_token_in_invest_list(invests, investment_id) {
                        return Ok(info);
                    }
                }
                // Search in investMarketTokenBalanceVoList (market positions like Aave)
                if let Some(markets) = net
                    .get("investMarketTokenBalanceVoList")
                    .and_then(|v| v.as_array())
                {
                    if let Some(info) = find_token_in_market_list(markets, investment_id) {
                        return Ok(info);
                    }
                }
            }
        }
    }
    bail!(
        "No position found for investmentId {} in position-detail",
        investment_id
    );
}

fn find_token_in_invest_list(
    invests: &[JsonValue],
    investment_id: &str,
) -> Option<PositionTokenInfo> {
    for invest in invests {
        let iid = invest
            .get("investmentId")
            .and_then(|v| {
                v.as_i64()
                    .map(|n| n.to_string())
                    .or_else(|| v.as_str().map(|s| s.to_string()))
            })
            .unwrap_or_default();
        if iid != investment_id {
            continue;
        }
        if let Some(assets) = invest.get("assetsTokenList").and_then(|v| v.as_array()) {
            if let Some(token) = assets.first() {
                return Some(extract_position_token(token));
            }
        }
    }
    None
}

fn find_token_in_market_list(
    markets: &[JsonValue],
    investment_id: &str,
) -> Option<PositionTokenInfo> {
    for market in markets {
        let asset_map = match market.get("assetMap") {
            Some(m) => m,
            None => continue,
        };
        for side in &["SUPPLY", "BORROW"] {
            if let Some(items) = asset_map.get(side).and_then(|v| v.as_array()) {
                for item in items {
                    let iid = item
                        .get("investmentId")
                        .and_then(|v| {
                            v.as_i64()
                                .map(|n| n.to_string())
                                .or_else(|| v.as_str().map(|s| s.to_string()))
                        })
                        .unwrap_or_default();
                    if iid == investment_id {
                        // Token info is inside assetsTokenList[0], not at item top level
                        if let Some(assets) = item.get("assetsTokenList").and_then(|v| v.as_array())
                        {
                            if let Some(token) = assets.first() {
                                return Some(extract_position_token(token));
                            }
                        }
                    }
                }
            }
        }
    }
    None
}

fn extract_position_token(token: &JsonValue) -> PositionTokenInfo {
    PositionTokenInfo {
        address: token
            .get("tokenAddress")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string(),
        chain_index: token
            .get("chainIndex")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string(),
        precision: token
            .get("tokenPrecision")
            .and_then(|v| {
                v.as_u64()
                    .map(|n| n as u32)
                    .or_else(|| v.as_str().and_then(|s| s.parse().ok()))
            })
            .unwrap_or(18),
        balance: token
            .get("coinAmount")
            .and_then(|v| v.as_str())
            .unwrap_or("0")
            .to_string(),
        symbol: token
            .get("tokenSymbol")
            .and_then(|v| v.as_str())
            .unwrap_or("UNKNOWN")
            .to_string(),
    }
}

/// High-level collect: auto-build expectOutputList, claim rewards
#[allow(clippy::too_many_arguments)]
pub(crate) async fn cmd_collect(
    client: &ApiClient,
    address: &str,
    chain: &str,
    reward_type: &str,
    investment_id: Option<&str>,
    platform_id: Option<&str>,
    token_id: Option<&str>,
    principal_index: Option<&str>,
) -> Result<JsonValue> {
    let chain_index = crate::chains::resolve_chain(chain);

    // 1. Validate reward_type + required params
    match reward_type {
        "REWARD_PLATFORM" => {
            if platform_id.is_none() {
                bail!(
                    "REWARD_PLATFORM requires --platform-id (analysisPlatformId from positions)."
                );
            }
        }
        "REWARD_INVESTMENT" | "REWARD_OKX_BONUS" | "REWARD_MERKLE_BONUS" => {
            if investment_id.is_none() || platform_id.is_none() {
                bail!(
                    "{} requires both --investment-id and --platform-id.",
                    reward_type
                );
            }
        }
        "V3_FEE" => {
            if investment_id.is_none() || token_id.is_none() {
                bail!("V3_FEE requires both --investment-id and --token-id (NFT tokenId).");
            }
        }
        "UNLOCKED_PRINCIPAL" => {
            if investment_id.is_none() || principal_index.is_none() {
                bail!("UNLOCKED_PRINCIPAL requires both --investment-id and --principal-index.");
            }
        }
        _ => {
            bail!("Unknown reward_type '{}'. Must be one of: REWARD_PLATFORM, REWARD_INVESTMENT, V3_FEE, REWARD_OKX_BONUS, REWARD_MERKLE_BONUS, UNLOCKED_PRINCIPAL.", reward_type);
        }
    }

    // 2. Auto-build expectOutputList from position-detail
    // V3_FEE and UNLOCKED_PRINCIPAL never need expectOutputList — backend resolves internally
    let expect_output: Option<String> = if reward_type == "V3_FEE"
        || reward_type == "UNLOCKED_PRINCIPAL"
    {
        None
    } else if let Some(platform_id_str) = platform_id {
        let auto = extract_expect_output(
            client,
            address,
            &chain_index,
            platform_id_str,
            reward_type,
            investment_id,
        )
        .await
        .map_err(|e| anyhow::anyhow!("Failed to fetch reward info from position-detail: {}", e))?;

        // Zero reward check
        if let Some(ref eo) = auto {
            let tokens: Vec<Value> = serde_json::from_str(eo).unwrap_or_default();
            if tokens.is_empty() {
                bail!("No rewards found for {} in position-detail.", reward_type);
            }
            let all_zero = tokens.iter().all(|t| {
                let reward_amount = t.get("coinAmount").and_then(|v| v.as_str()).unwrap_or("0");
                reward_amount == "0"
                    || reward_amount.is_empty()
                    || reward_amount.chars().all(|c| c == '0' || c == '.')
            });
            if all_zero {
                bail!(
                    "No rewards available. All reward amounts are zero for {}.",
                    reward_type
                );
            }
        } else {
            bail!("No reward tokens found for {} in position-detail. Verify investment-id and platform-id.", reward_type);
        }
        auto
    } else {
        bail!(
            "--platform-id is required for {} to auto-build expectOutputList.",
            reward_type
        );
    };

    // API: investmentId and analysisPlatformId cannot both be specified.
    // When investment_id is present (V3_FEE, UNLOCKED_PRINCIPAL, REWARD_INVESTMENT),
    // omit platform_id to avoid the conflict.
    let effective_platform_id = if investment_id.is_some() {
        None
    } else {
        platform_id
    };

    fetch_claim(
        client,
        address,
        &chain_index,
        reward_type,
        investment_id,
        effective_platform_id,
        token_id,
        principal_index,
        expect_output.as_deref(),
    )
    .await
}
