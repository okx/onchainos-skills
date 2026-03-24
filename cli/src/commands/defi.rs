use anyhow::{bail, Result};
use clap::Subcommand;
use serde_json::{json, Value};
//
use super::Context;
use crate::client::ApiClient;
use crate::output;

#[derive(Subcommand)]
pub enum DefiCommand {
    /// List all DeFi products (no filters, paginated)
    List {
        /// Page number (min 1, page size fixed at 20)
        #[arg(long)]
        page_num: Option<u32>,
    },
    /// Search DeFi products (earn, pools, lending)
    Search {
        /// Comma-separated token keywords (e.g. "USDC,ETH"). At least one of --token or --platform is required
        #[arg(long)]
        token: Option<String>,
        /// Comma-separated platform keywords (e.g. "Aave,Compound")
        #[arg(long)]
        platform: Option<String>,
        /// Chain (e.g. ethereum, avalanche, bsc)
        #[arg(long)]
        chain: Option<String>,
        /// Product group: SINGLE_EARN (default), DEX_POOL, LENDING
        #[arg(long)]
        product_group: Option<String>,
        /// Page number (min 1, page size fixed at 20)
        #[arg(long)]
        page_num: Option<u32>,
    },
    /// Get DeFi product detail and APY
    Detail {
        /// Investment ID from search results
        #[arg(long)]
        investment_id: String,
    },
    /// Get pre-investment info (allowance, limits, supported tokens)
    Prepare {
        /// Investment ID from search results
        #[arg(long)]
        investment_id: String,
    },
    /// Generate deposit calldata (subscribe, add liquidity, borrow)
    Deposit {
        /// Investment ID from search results
        #[arg(long)]
        investment_id: String,
        /// User wallet address
        #[arg(long)]
        address: String,
        /// User input tokens as JSON array (e.g. '[{"tokenAddress":"0x...","chainIndex":"1","coinAmount":"0.05"}]')
        #[arg(long)]
        user_input: String,
        /// Slippage tolerance (default "0.01" = 1%)
        #[arg(long, default_value = "0.01")]
        slippage: String,
        /// Token ID for V3 Pool positions (required for V3 add liquidity to existing position)
        #[arg(long)]
        token_id: Option<String>,
        /// Lower tick for V3 Pool new position (floor(log(price)/log(1.0001)/tickSpacing)*tickSpacing)
        #[arg(long, allow_hyphen_values = true)]
        tick_lower: Option<i64>,
        /// Upper tick for V3 Pool new position (ceil(log(price)/log(1.0001)/tickSpacing)*tickSpacing)
        #[arg(long, allow_hyphen_values = true)]
        tick_upper: Option<i64>,
    },

    /// Build redemption/withdrawal calldata for a DeFi product
    Redeem {
        /// Investment product ID
        #[arg(long)]
        id: String,
        /// User wallet address
        #[arg(long)]
        address: String,
        /// Redemption ratio: "1"=full exit (100%), "0.5"=50%. Use for full exit; for partial exit use --user-input instead
        #[arg(long)]
        ratio: Option<String>,
        /// V3 Pool: NFT tokenId (required for V3 pool redemption)
        #[arg(long)]
        token_id: Option<String>,
        /// Slippage tolerance (default "0.01")
        #[arg(long, default_value = "0.01")]
        slippage: String,
        /// Chain (for LP token input)
        #[arg(long)]
        chain: Option<String>,
        /// User input tokens as JSON array (e.g. '[{"tokenAddress":"0x...","chainIndex":"56","coinAmount":"1.0"},...]')
        /// Partial exit: REQUIRED with underlying token address and exact amount. Full exit: optional but preferred if token info available. V3 Pool: pass both underlying tokens. Takes precedence over --token/--amount.
        #[arg(long)]
        user_input: Option<String>,
        /// LP token / receipt token contract address (single-token shorthand; use --user-input for V3)
        #[arg(long)]
        token: Option<String>,
        /// LP token symbol
        #[arg(long)]
        symbol: Option<String>,
        /// LP token human-readable amount
        #[arg(long)]
        amount: Option<String>,
        /// LP token decimals
        #[arg(long)]
        precision: Option<u32>,
    },

    /// Generate reward-claim calldata
    Claim {
        /// User wallet address
        #[arg(long)]
        address: String,
        /// Chain (e.g. ethereum, avalanche)
        #[arg(long)]
        chain: Option<String>,
        /// Reward type: REWARD_PLATFORM, REWARD_INVESTMENT, V3_FEE, REWARD_OKX_BONUS, REWARD_MERKLE_BONUS, UNLOCKED_PRINCIPAL
        #[arg(long)]
        reward_type: String,
        /// Investment product ID (required for REWARD_INVESTMENT / V3_FEE)
        #[arg(long)]
        id: Option<String>,
        /// Protocol platform ID (required for REWARD_PLATFORM)
        #[arg(long)]
        platform_id: Option<String>,
        /// V3 Pool NFT tokenId (required for V3_FEE)
        #[arg(long)]
        token_id: Option<String>,
        /// Principal order index (for UNLOCKED_PRINCIPAL)
        #[arg(long)]
        principal_index: Option<String>,
        /// Expected output token list as JSON array (e.g. '[{"chainIndex":"1","tokenAddress":"0x...","coinAmount":"0.001"}]'). Pass directly using rewardDefiTokenInfo from position-detail (preferred); auto-fetched via --platform-id as fallback
        #[arg(long)]
        expect_output: Option<String>,
    },

    /// Calculate exact token amounts needed for V3 pool entry based on input token and amount
    CalculateEntry {
        /// Investment ID from search results
        #[arg(long)]
        id: String,
        /// User wallet address
        #[arg(long)]
        address: String,
        /// Input token contract address
        #[arg(long)]
        input_token: String,
        /// Input amount in minimal units (integer, e.g. "5000000000000000" for 0.005 ETH)
        #[arg(long)]
        input_amount: String,
        /// Token decimals
        #[arg(long)]
        token_decimal: String,
        /// Lower tick for V3 Pool position
        #[arg(long, allow_hyphen_values = true)]
        tick_lower: Option<i64>,
        /// Upper tick for V3 Pool position
        #[arg(long, allow_hyphen_values = true)]
        tick_upper: Option<i64>,
    },

    /// Get user DeFi holdings overview across protocols and chains
    Positions {
        /// User wallet address
        #[arg(long)]
        address: String,
        /// Chains to query (comma-separated, e.g. "ethereum,bsc,solana")
        #[arg(long)]
        chains: String,
    },

    /// Get detailed holdings for a specific protocol
    PositionDetail {
        /// User wallet address
        #[arg(long)]
        address: String,
        /// Chain (e.g. ethereum, avalanche)
        #[arg(long)]
        chain: String,
        /// Protocol platform ID (analysisPlatformId from positions results)
        #[arg(long)]
        platform_id: String,
    },
}

pub async fn execute(ctx: &Context, cmd: DefiCommand) -> Result<()> {
    let client = ctx.client_async().await?;
    match cmd {
        DefiCommand::List { page_num } => {
            output::success(
                fetch_search(&client, None, None, None, None, page_num).await?,
            );
        }
        DefiCommand::Search {
            token,
            platform,
            chain,
            product_group,
            page_num,
        } => {
            if token.is_none() && platform.is_none() {
                bail!("at least one of --token or --platform is required");
            }
            let chain_index = chain.as_deref().map(crate::chains::resolve_chain);
            output::success(
                fetch_search(
                    &client,
                    token.as_deref(),
                    platform.as_deref(),
                    chain_index.as_deref(),
                    product_group.as_deref(),
                    page_num,
                )
                .await?,
            );
        }
        DefiCommand::Detail { investment_id } => {
            output::success(fetch_detail(&client, &investment_id).await?);
        }
        DefiCommand::Prepare { investment_id } => {
            output::success(fetch_prepare(&client, &investment_id).await?);
        }
        DefiCommand::Deposit {
            investment_id,
            address,
            user_input,
            slippage,
            token_id,
            tick_lower,
            tick_upper,
        } => {
            output::success(
                fetch_enter(
                    &client,
                    &investment_id,
                    &address,
                    &user_input,
                    &slippage,
                    token_id.as_deref(),
                    tick_lower,
                    tick_upper,
                )
                .await?,
            );
        }
        DefiCommand::Redeem {
            id,
            address,
            ratio,
            token_id,
            slippage,
            chain,
            user_input,
            token,
            symbol,
            amount,
            precision,
        } => {
            let chain_index = chain.as_deref().map(crate::chains::resolve_chain).unwrap_or_default();
            output::success(
                fetch_exit(
                    &client,
                    &id,
                    &chain_index,
                    &address,
                    ratio.as_deref(),
                    token.as_deref(),
                    symbol.as_deref(),
                    amount.as_deref(),
                    precision,
                    token_id.as_deref(),
                    &slippage,
                    user_input.as_deref(),
                )
                .await?,
            );
        }
        DefiCommand::Claim {
            address,
            chain,
            reward_type,
            id,
            platform_id,
            token_id,
            principal_index,
            expect_output,
        } => {
            let chain_index = chain.as_deref().map(crate::chains::resolve_chain).unwrap_or_default();
            // Auto-fetch expectOutputList from position-detail when user didn't provide it
            let auto_expect_output: Option<String> = if expect_output.is_none() {
                if let Some(pfid) = platform_id.as_deref() {
                    extract_expect_output(
                        &client,
                        &address,
                        &chain_index,
                        pfid,
                        &reward_type,
                        id.as_deref(),
                    )
                    .await
                    .unwrap_or(None)
                } else {
                    None
                }
            } else {
                None
            };
            let final_expect_output = expect_output.as_deref().or(auto_expect_output.as_deref());
            output::success(
                fetch_claim(
                    &client,
                    &address,
                    &chain_index,
                    &reward_type,
                    id.as_deref(),
                    platform_id.as_deref(),
                    token_id.as_deref(),
                    principal_index.as_deref(),
                    final_expect_output,
                )
                .await?,
            );
        }
        DefiCommand::CalculateEntry {
            id,
            address,
            input_token,
            input_amount,
            token_decimal,
            tick_lower,
            tick_upper,
        } => {
            // Validate: input_amount must be integer (minimal units)
            if input_amount.contains('.') {
                bail!(
                    "input-amount must be an integer (minimal units), got \"{}\". \
                     Convert: userAmount × 10^tokenDecimal. \
                     Example: 0.005 ETH (decimal=18) → input-amount=\"5000000000000000\"",
                    input_amount
                );
            }
            // Convert minimal units to human-readable for API
            let precision: u32 = token_decimal.parse().map_err(|_| {
                anyhow::anyhow!("token-decimal must be a non-negative integer, got \"{}\"", token_decimal)
            })?;
            let human_readable_amount = minimal_to_decimal_str(&input_amount, precision);
            let result = fetch_calculate_entry(
                &client,
                &id,
                &address,
                &input_token,
                &human_readable_amount,
                &token_decimal,
                tick_lower,
                tick_upper,
            )
            .await?;

            // Convert output: coinAmount from UI decimal → minimal units + add tokenPrecision
            // Get tokenPrecision from prepare for each token
            let prepare_data = fetch_prepare(&client, &id).await?;
            let mut precision_map = std::collections::HashMap::new();
            if let Some(tokens) = prepare_data.get("investWithTokenList").and_then(|v| v.as_array()) {
                for t in tokens {
                    if let (Some(addr), Some(prec)) = (
                        t.get("tokenAddress").and_then(|v| v.as_str()),
                        t.get("tokenPrecision").and_then(|v| v.as_str().or_else(|| v.as_u64().map(|_| "")).and_then(|_| None))
                            .or_else(|| t.get("tokenPrecision").and_then(|v| v.as_str().map(|s| s.to_string()).or_else(|| v.as_u64().map(|n| n.to_string()))).as_deref().map(|s| s.to_string()))
                    ) {
                        precision_map.insert(addr.to_lowercase(), prec);
                    }
                }
            }
            // Simpler precision extraction
            let mut precision_map: std::collections::HashMap<String, u32> = std::collections::HashMap::new();
            if let Some(tokens) = prepare_data.get("investWithTokenList").and_then(|v| v.as_array()) {
                for t in tokens {
                    let addr = t.get("tokenAddress").and_then(|v| v.as_str()).unwrap_or("").to_lowercase();
                    let prec = t.get("tokenPrecision")
                        .and_then(|v| v.as_str().and_then(|s| s.parse::<u32>().ok()).or_else(|| v.as_u64().map(|n| n as u32)))
                        .unwrap_or(18);
                    precision_map.insert(addr, prec);
                }
            }

            // Transform investWithTokenList in result
            let mut output = result.clone();
            if let Some(tokens) = output.get_mut("investWithTokenList").and_then(|v| v.as_array_mut()) {
                for t in tokens.iter_mut() {
                    let addr = t.get("tokenAddress").and_then(|v| v.as_str()).unwrap_or("").to_lowercase();
                    let prec = precision_map.get(&addr).copied().unwrap_or(18);
                    if let Some(amount_str) = t.get("coinAmount").and_then(|v| v.as_str()) {
                        let minimal = decimal_to_minimal_str(amount_str, prec);
                        t["coinAmount"] = json!(minimal);
                        t["tokenPrecision"] = json!(prec.to_string());
                    }
                }
            }

            output::success(output);
        }
        DefiCommand::Positions { address, chains } => {
            let raw = fetch_positions(&client, &address, &chains).await?;
            output::success(raw);
        }
        DefiCommand::PositionDetail {
            address,
            chain,
            platform_id,
        } => {
            let chain_index = crate::chains::resolve_chain(&chain);
            let raw = fetch_position_detail(&client, &address, &chain_index, &platform_id).await?;
            output::success(raw);
        }
    }
    Ok(())
}

// ── Helpers ──────────────────────────────────────────────────────────

/// Validate and convert `coinAmount` from minimal units (integer string) to
/// human-readable decimal string using `tokenPrecision`.
///
/// Example: coinAmount="500000", tokenPrecision=6 → "0.5"
///
/// Requirements:
/// - `tokenPrecision` is REQUIRED for every item
/// - `coinAmount` MUST be an integer (no decimal point) — fails fast otherwise
fn convert_minimal_to_decimal(items: &mut Vec<Value>) -> Result<()> {
    for item in items.iter_mut() {
        let prec: Option<u32> = item
            .get("tokenPrecision")
            .and_then(|v| {
                v.as_str()
                    .and_then(|s| s.parse::<u32>().ok())
                    .or_else(|| v.as_u64().map(|n| n as u32))
            });

        let precision = prec.ok_or_else(|| {
            anyhow::anyhow!(
                "tokenPrecision is required in --user-input for each token. \
                 Get it from `defi prepare` → investWithTokenList[].tokenPrecision"
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
                     Convert: userAmount × 10^tokenPrecision. \
                     Example: 0.5 USDC (precision=6) → coinAmount=\"500000\"",
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
/// e.g. "500000" with precision 6 → "0.5"
/// e.g. "1154528481238320444" with precision 18 → "1.154528481238320444"
fn minimal_to_decimal_str(amount: &str, precision: u32) -> String {
    let p = precision as usize;
    if p == 0 {
        return amount.to_string();
    }
    let padded = if amount.len() <= p {
        format!("{:0>width$}", amount, width = p + 1)
    } else {
        amount.to_string()
    };
    let (integer_part, decimal_part) = padded.split_at(padded.len() - p);
    // Trim trailing zeros from decimal part
    let trimmed = decimal_part.trim_end_matches('0');
    if trimmed.is_empty() {
        integer_part.to_string()
    } else {
        format!("{}.{}", integer_part, trimmed)
    }
}

/// Convert a decimal string to an integer string (minimal units) given precision.
/// Pure string operation — no floating point, no precision loss.
/// e.g. "0.5" with precision 6 → "500000"
/// e.g. "226.483834" with precision 6 → "226483834"
/// e.g. "0.005" with precision 18 → "5000000000000000"
fn decimal_to_minimal_str(amount: &str, precision: u32) -> String {
    let p = precision as usize;
    if p == 0 {
        // No decimal part expected; strip any decimal point
        return amount.split('.').next().unwrap_or(amount).to_string();
    }
    let (integer, decimal) = if let Some(dot_pos) = amount.find('.') {
        (&amount[..dot_pos], &amount[dot_pos + 1..])
    } else {
        (amount, "")
    };
    // Handle owned string for padding case
    let padded_owned;
    let final_decimal = if decimal.len() >= p {
        &decimal[..p]
    } else {
        padded_owned = format!("{:0<width$}", decimal, width = p);
        &padded_owned
    };
    let combined = format!("{}{}", integer, final_decimal);
    // Strip leading zeros but keep at least "0"
    let stripped = combined.trim_start_matches('0');
    if stripped.is_empty() { "0".to_string() } else { stripped.to_string() }
}

// ── API functions ────────────────────────────────────────────────────

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
    // analysisPlatformId is only required for REWARD_PLATFORM
    if let Some(pfid) = platform_id {
        if reward_type == "REWARD_PLATFORM" {
            body["analysisPlatformId"] = json!(pfid);
        }
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
pub async fn fetch_positions(
    client: &ApiClient,
    wallet: &str,
    chains: &str,
) -> Result<Value> {
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
                // Also search availableRewards for REWARD_PLATFORM, REWARD_OKX_BONUS, REWARD_MERKLE_BONUS
                if reward_type != "REWARD_INVESTMENT" {
                    if let Some(available) = net.get("availableRewards").and_then(|v| v.as_array()) {
                        for reward in available {
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

// ── Output helpers ────────────────────────────────────────────────────

