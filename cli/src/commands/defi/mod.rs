mod api;
mod helpers;
mod operations;

pub use api::*;
pub use helpers::extract_expect_output;
pub(crate) use operations::{cmd_collect, cmd_invest, cmd_withdraw};

use anyhow::{bail, Result};
use clap::Subcommand;
use serde_json::json;
//
use super::Context;
use crate::output;

#[derive(Subcommand)]
pub enum DefiCommand {
    /// Get supported chains for DeFi
    SupportChains,
    /// Get supported platforms for DeFi
    SupportPlatforms,
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

    /// Get historical APY chart data for a DeFi product
    RateChart {
        /// Investment ID
        #[arg(long)]
        investment_id: String,
        /// Time range: DAY (V3 only), WEEK (default), MONTH, SEASON, YEAR
        #[arg(long)]
        time_range: Option<String>,
    },

    /// Get historical TVL chart data for a DeFi product
    TvlChart {
        /// Investment ID
        #[arg(long)]
        investment_id: String,
        /// Time range: DAY (V3 only), WEEK (default), MONTH, SEASON, YEAR
        #[arg(long)]
        time_range: Option<String>,
    },

    /// Get V3 Pool depth or price history chart (V3 Pool only)
    DepthPriceChart {
        /// Investment ID
        #[arg(long)]
        investment_id: String,
        /// Chart type: DEPTH (default) or PRICE
        #[arg(long)]
        chart_type: Option<String>,
        /// Time range (only for PRICE mode): DAY (default), WEEK. Ignored in DEPTH mode
        #[arg(long)]
        time_range: Option<String>,
    },

    /// High-level invest: resolve token, convert amount, build calldata
    Invest {
        /// Investment ID from search results
        #[arg(long)]
        investment_id: String,
        /// User wallet address
        #[arg(long)]
        address: String,
        /// Token symbol or contract address (first token)
        #[arg(long)]
        token: String,
        /// Amount in minimal units for first token (e.g. "100000" for 0.1 USDC with 6 decimals)
        #[arg(long)]
        amount: String,
        /// Second token symbol or contract address (V3 dual-token entry)
        #[arg(long)]
        token2: Option<String>,
        /// Amount in minimal units for second token (V3 dual-token entry)
        #[arg(long)]
        amount2: Option<String>,
        /// Chain (resolves automatically from detail if not provided)
        #[arg(long)]
        chain: Option<String>,
        /// Slippage tolerance (default "0.01" = 1%)
        #[arg(long, default_value = "0.01")]
        slippage: String,
        /// Token ID for V3 Pool (add to existing position)
        #[arg(long)]
        token_id: Option<String>,
        /// Lower tick for V3 Pool new position
        #[arg(long, allow_hyphen_values = true)]
        tick_lower: Option<i64>,
        /// Upper tick for V3 Pool new position
        #[arg(long, allow_hyphen_values = true)]
        tick_upper: Option<i64>,
        /// Price range percentage for V3 Pool (e.g. 5 for ±5% around current price). Required for V3 if tick_lower/tick_upper not provided.
        #[arg(long)]
        range: Option<f64>,
    },

    /// High-level withdraw: resolve position, build exit calldata
    Withdraw {
        /// Investment ID from search results
        #[arg(long)]
        investment_id: String,
        /// User wallet address
        #[arg(long)]
        address: String,
        /// Chain (e.g. ethereum, bsc)
        #[arg(long)]
        chain: String,
        /// Redemption ratio ("1" for full exit, "0.5" for 50%)
        #[arg(long)]
        ratio: Option<String>,
        /// V3 Pool NFT tokenId
        #[arg(long)]
        token_id: Option<String>,
        /// Slippage tolerance (default "0.01" = 1%)
        #[arg(long, default_value = "0.01")]
        slippage: String,
        /// Human-readable partial withdrawal amount
        #[arg(long)]
        amount: Option<String>,
        /// Platform ID to auto-fetch position detail
        #[arg(long)]
        platform_id: Option<String>,
    },

    /// High-level collect: auto-build expectOutputList, claim rewards
    Collect {
        /// User wallet address
        #[arg(long)]
        address: String,
        /// Chain (e.g. ethereum, bsc)
        #[arg(long)]
        chain: String,
        /// Reward type: REWARD_PLATFORM, REWARD_INVESTMENT, V3_FEE, REWARD_OKX_BONUS, REWARD_MERKLE_BONUS, UNLOCKED_PRINCIPAL
        #[arg(long)]
        reward_type: String,
        /// Investment product ID
        #[arg(long)]
        investment_id: Option<String>,
        /// Protocol platform ID
        #[arg(long)]
        platform_id: Option<String>,
        /// V3 Pool NFT tokenId (for V3_FEE)
        #[arg(long)]
        token_id: Option<String>,
        /// Principal order index (for UNLOCKED_PRINCIPAL)
        #[arg(long)]
        principal_index: Option<String>,
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
    let mut client = ctx.client_async().await?;
    match cmd {
        DefiCommand::SupportChains => {
            output::success(fetch_chains(&mut client).await?);
        }
        DefiCommand::SupportPlatforms => {
            output::success(fetch_protocols(&mut client).await?);
        }
        DefiCommand::List { page_num } => {
            output::success(fetch_search(&mut client, None, None, None, None, page_num).await?);
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
                    &mut client,
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
            output::success(fetch_detail(&mut client, &investment_id).await?);
        }
        DefiCommand::Prepare { investment_id } => {
            output::success(fetch_prepare(&mut client, &investment_id).await?);
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
                    &mut client,
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
            let chain_index = chain
                .as_deref()
                .map(crate::chains::resolve_chain)
                .unwrap_or_default();
            output::success(
                fetch_exit(
                    &mut client,
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
            let chain_index = chain
                .as_deref()
                .map(crate::chains::resolve_chain)
                .unwrap_or_default();
            // Auto-fetch expectOutputList from position-detail when user didn't provide it
            let auto_expect_output: Option<String> = if expect_output.is_none() {
                if let Some(pfid) = platform_id.as_deref() {
                    extract_expect_output(
                        &mut client,
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
                    &mut client,
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
                     Convert: userAmount x 10^tokenDecimal. \
                     Example: 0.005 ETH (decimal=18) -> input-amount=\"5000000000000000\"",
                    input_amount
                );
            }
            // Convert minimal units to human-readable for API
            let precision: u32 = token_decimal.parse().map_err(|_| {
                anyhow::anyhow!(
                    "token-decimal must be a non-negative integer, got \"{}\"",
                    token_decimal
                )
            })?;
            let human_readable_amount = helpers::minimal_to_decimal_str(&input_amount, precision);
            let result = fetch_calculate_entry(
                &mut client,
                &id,
                &address,
                &input_token,
                &human_readable_amount,
                &token_decimal,
                tick_lower,
                tick_upper,
            )
            .await?;

            // Convert output: coinAmount from UI decimal -> minimal units + add tokenPrecision
            // Get tokenPrecision from prepare for each token
            let prepare_data = fetch_prepare(&mut client, &id).await?;
            let mut precision_map = std::collections::HashMap::new();
            if let Some(tokens) = prepare_data
                .get("investWithTokenList")
                .and_then(|v| v.as_array())
            {
                for t in tokens {
                    if let (Some(addr), Some(prec)) = (
                        t.get("tokenAddress").and_then(|v| v.as_str()),
                        t.get("tokenPrecision")
                            .and_then(|v| v.as_str().or_else(|| v.as_u64().map(|_| "")).and(None))
                            .or_else(|| {
                                t.get("tokenPrecision")
                                    .and_then(|v| {
                                        v.as_str()
                                            .map(|s| s.to_string())
                                            .or_else(|| v.as_u64().map(|n| n.to_string()))
                                    })
                                    .as_deref()
                                    .map(|s| s.to_string())
                            }),
                    ) {
                        precision_map.insert(addr.to_lowercase(), prec);
                    }
                }
            }
            // Simpler precision extraction
            let mut precision_map: std::collections::HashMap<String, u32> =
                std::collections::HashMap::new();
            if let Some(tokens) = prepare_data
                .get("investWithTokenList")
                .and_then(|v| v.as_array())
            {
                for t in tokens {
                    let addr = t
                        .get("tokenAddress")
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_lowercase();
                    let prec = t
                        .get("tokenPrecision")
                        .and_then(|v| {
                            v.as_str()
                                .and_then(|s| s.parse::<u32>().ok())
                                .or_else(|| v.as_u64().map(|n| n as u32))
                        })
                        .unwrap_or(18);
                    precision_map.insert(addr, prec);
                }
            }

            // Transform investWithTokenList in result
            let mut output = result.clone();
            if let Some(tokens) = output
                .get_mut("investWithTokenList")
                .and_then(|v| v.as_array_mut())
            {
                for t in tokens.iter_mut() {
                    let addr = t
                        .get("tokenAddress")
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_lowercase();
                    let prec = precision_map.get(&addr).copied().unwrap_or(18);
                    if let Some(amount_str) = t.get("coinAmount").and_then(|v| v.as_str()) {
                        let minimal = helpers::decimal_to_minimal_str(amount_str, prec);
                        t["coinAmount"] = json!(minimal);
                        t["tokenPrecision"] = json!(prec.to_string());
                    }
                }
            }

            output::success(output);
        }
        DefiCommand::RateChart {
            investment_id,
            time_range,
        } => {
            output::success(
                fetch_rate_chart(&mut client, &investment_id, time_range.as_deref()).await?,
            );
        }
        DefiCommand::TvlChart {
            investment_id,
            time_range,
        } => {
            output::success(
                fetch_tvl_chart(&mut client, &investment_id, time_range.as_deref()).await?,
            );
        }
        DefiCommand::DepthPriceChart {
            investment_id,
            chart_type,
            time_range,
        } => {
            output::success(
                fetch_depth_price_chart(
                    &mut client,
                    &investment_id,
                    chart_type.as_deref(),
                    time_range.as_deref(),
                )
                .await?,
            );
        }
        DefiCommand::Invest {
            investment_id,
            address,
            token,
            amount,
            token2,
            amount2,
            chain: _chain,
            slippage,
            token_id,
            tick_lower,
            tick_upper,
            range,
        } => {
            let result = operations::cmd_invest(
                &mut client,
                &investment_id,
                &address,
                &token,
                &amount,
                token2.as_deref(),
                amount2.as_deref(),
                &slippage,
                token_id.as_deref(),
                tick_lower,
                tick_upper,
                range,
            )
            .await?;
            output::success(result);
        }
        DefiCommand::Withdraw {
            investment_id,
            address,
            chain,
            ratio,
            token_id,
            slippage,
            amount,
            platform_id,
        } => {
            let result = operations::cmd_withdraw(
                &mut client,
                &investment_id,
                &address,
                &chain,
                ratio.as_deref(),
                token_id.as_deref(),
                &slippage,
                amount.as_deref(),
                platform_id.as_deref(),
            )
            .await?;
            output::success(result);
        }
        DefiCommand::Collect {
            address,
            chain,
            reward_type,
            investment_id,
            platform_id,
            token_id,
            principal_index,
        } => {
            let result = operations::cmd_collect(
                &mut client,
                &address,
                &chain,
                &reward_type,
                investment_id.as_deref(),
                platform_id.as_deref(),
                token_id.as_deref(),
                principal_index.as_deref(),
            )
            .await?;
            output::success(result);
        }
        DefiCommand::Positions { address, chains } => {
            let raw = fetch_positions(&mut client, &address, &chains).await?;
            output::success(raw);
        }
        DefiCommand::PositionDetail {
            address,
            chain,
            platform_id,
        } => {
            let chain_index = crate::chains::resolve_chain(&chain);
            let raw =
                fetch_position_detail(&mut client, &address, &chain_index, &platform_id).await?;
            output::success(raw);
        }
    }
    Ok(())
}
