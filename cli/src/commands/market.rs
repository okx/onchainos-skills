use anyhow::Result;
use clap::Subcommand;
use serde_json::{json, Value};

use super::Context;
use crate::client::ApiClient;
use crate::output;

#[derive(Subcommand)]
#[allow(clippy::large_enum_variant)]
pub enum MarketCommand {
    /// Get token price (by contract address)
    Price {
        /// Token contract address
        #[arg(long)]
        address: String,
        /// Chain (e.g. ethereum, solana, xlayer)
        #[arg(long)]
        chain: Option<String>,
    },
    /// Get prices for multiple tokens (POST, batch query)
    Prices {
        /// Comma-separated chainIndex:address pairs (e.g. "1:0xeee...,501:1111...")
        #[arg(long)]
        tokens: String,
        /// Default chain if not specified per token
        #[arg(long)]
        chain: Option<String>,
    },
    /// Get K-line / candlestick data
    Kline {
        /// Token contract address
        #[arg(long)]
        address: String,
        /// Bar size: 1s, 1m, 5m, 15m, 30m, 1H, 4H, 1D, 1W, etc.
        #[arg(long, default_value = "1H")]
        bar: String,
        /// Number of data points (max 299)
        #[arg(long, default_value = "100")]
        limit: u32,
        /// Chain
        #[arg(long)]
        chain: Option<String>,
    },
    /// Get index price (aggregated from multiple sources)
    Index {
        /// Token contract address (empty string for native token)
        #[arg(long)]
        address: String,
        /// Chain
        #[arg(long)]
        chain: Option<String>,
    },
    /// Get supported chains for portfolio PnL endpoints
    PortfolioSupportedChains,
    /// Get wallet portfolio overview: realized/unrealized PnL, win rate, trading stats
    PortfolioOverview {
        /// Wallet address
        #[arg(long)]
        address: String,
        /// Chain name or ID (e.g. ethereum, solana, xlayer)
        #[arg(long)]
        chain: String,
        /// Time frame: 1=1D, 2=3D, 3=7D, 4=1M, 5=3M
        #[arg(long)]
        time_frame: String,
    },
    /// Get wallet DEX transaction history (paginated)
    PortfolioDexHistory {
        /// Wallet address
        #[arg(long)]
        address: String,
        /// Chain name or ID (e.g. ethereum, solana, xlayer)
        #[arg(long)]
        chain: String,
        /// Start timestamp (milliseconds)
        #[arg(long)]
        begin: String,
        /// End timestamp (milliseconds)
        #[arg(long)]
        end: String,
        /// Page size (1-100, default 20)
        #[arg(long)]
        limit: Option<String>,
        /// Pagination cursor from previous response
        #[arg(long)]
        cursor: Option<String>,
        /// Filter by token contract address
        #[arg(long)]
        token: Option<String>,
        /// Transaction type: 1=BUY, 2=SELL, 3=Transfer In, 4=Transfer Out (comma-separated)
        #[arg(long = "tx-type")]
        tx_type: Option<String>,
    },
    /// Get recent token PnL records for a wallet (paginated)
    PortfolioRecentPnl {
        /// Wallet address
        #[arg(long)]
        address: String,
        /// Chain name or ID (e.g. ethereum, solana, xlayer)
        #[arg(long)]
        chain: String,
        /// Page size (1-100, default 20)
        #[arg(long)]
        limit: Option<String>,
        /// Pagination cursor from previous response
        #[arg(long)]
        cursor: Option<String>,
    },
    /// Get latest PnL snapshot for a specific token in a wallet
    PortfolioTokenPnl {
        /// Wallet address
        #[arg(long)]
        address: String,
        /// Chain name or ID (e.g. ethereum, solana, xlayer)
        #[arg(long)]
        chain: String,
        /// Token contract address
        #[arg(long)]
        token: String,
    },
}

pub async fn execute(ctx: &Context, cmd: MarketCommand) -> Result<()> {
    let client = ctx.client()?;
    match cmd {
        MarketCommand::Price { address, chain } => {
            let chain_index = chain
                .map(|c| crate::chains::resolve_chain(&c).to_string())
                .unwrap_or_else(|| ctx.chain_index_or("ethereum"));
            output::success(fetch_price(&client, &address, &chain_index).await?);
        }
        MarketCommand::Prices { tokens, chain } => {
            let default_chain = chain
                .map(|c| crate::chains::resolve_chain(&c).to_string())
                .unwrap_or_else(|| ctx.chain_index_or("ethereum"));
            output::success(fetch_prices(&client, &tokens, &default_chain).await?);
        }
        MarketCommand::Kline { address, bar, limit, chain } => {
            let chain_index = chain
                .map(|c| crate::chains::resolve_chain(&c).to_string())
                .unwrap_or_else(|| ctx.chain_index_or("ethereum"));
            output::success(fetch_kline(&client, &address, &chain_index, &bar, limit).await?);
        }
        MarketCommand::Index { address, chain } => {
            let chain_index = chain
                .map(|c| crate::chains::resolve_chain(&c).to_string())
                .unwrap_or_else(|| ctx.chain_index_or("ethereum"));
            output::success(fetch_index(&client, &address, &chain_index).await?);
        }
        MarketCommand::PortfolioSupportedChains => {
            portfolio_supported_chains(ctx).await?;
        }
        MarketCommand::PortfolioOverview {
            address,
            chain,
            time_frame,
        } => {
            portfolio_overview(ctx, &address, &chain, &time_frame).await?;
        }
        MarketCommand::PortfolioDexHistory {
            address,
            chain,
            begin,
            end,
            limit,
            cursor,
            token,
            tx_type,
        } => {
            portfolio_dex_history(
                ctx,
                &address,
                &chain,
                &begin,
                &end,
                limit.as_deref(),
                cursor.as_deref(),
                token.as_deref(),
                tx_type.as_deref(),
            )
            .await?;
        }
        MarketCommand::PortfolioRecentPnl {
            address,
            chain,
            limit,
            cursor,
        } => {
            portfolio_recent_pnl(ctx, &address, &chain, limit.as_deref(), cursor.as_deref()).await?;
        }
        MarketCommand::PortfolioTokenPnl {
            address,
            chain,
            token,
        } => {
            portfolio_token_pnl(ctx, &address, &chain, &token).await?;
        }
    }
    Ok(())
}

/// POST /api/v6/dex/market/price — body is JSON array
pub async fn fetch_price(client: &ApiClient, address: &str, chain_index: &str) -> Result<Value> {
    let body = json!([{"chainIndex": chain_index, "tokenContractAddress": address}]);
    client.post("/api/v6/dex/market/price", &body).await
}

/// POST /api/v6/dex/market/price — batch query
pub async fn fetch_prices(
    client: &ApiClient,
    tokens: &str,
    default_chain_index: &str,
) -> Result<Value> {
    let items: Vec<Value> = tokens
        .split(',')
        .map(|pair| {
            let pair = pair.trim();
            if let Some((chain_part, addr)) = pair.split_once(':') {
                json!({
                    "chainIndex": crate::chains::resolve_chain(chain_part),
                    "tokenContractAddress": addr
                })
            } else {
                json!({
                    "chainIndex": default_chain_index,
                    "tokenContractAddress": pair
                })
            }
        })
        .collect();
    client.post("/api/v6/dex/market/price", &Value::Array(items)).await
}

/// GET /api/v6/dex/market/candles
pub async fn fetch_kline(
    client: &ApiClient,
    address: &str,
    chain_index: &str,
    bar: &str,
    limit: u32,
) -> Result<Value> {
    let limit_str = limit.to_string();
    client
        .get(
            "/api/v6/dex/market/candles",
            &[
                ("chainIndex", chain_index),
                ("tokenContractAddress", address),
                ("bar", bar),
                ("limit", &limit_str),
            ],
        )
        .await
}

/// POST /api/v6/dex/index/current-price — body is JSON array
pub async fn fetch_index(client: &ApiClient, address: &str, chain_index: &str) -> Result<Value> {
    let body = json!([{"chainIndex": chain_index, "tokenContractAddress": address}]);
    client.post("/api/v6/dex/index/current-price", &body).await
}

/// GET /api/v6/dex/market/memepump/supported/chainsProtocol — no parameters
pub async fn fetch_memepump_chains(client: &ApiClient) -> Result<Value> {
    client
        .get("/api/v6/dex/market/memepump/supported/chainsProtocol", &[])
        .await
}

/// Parameters for the memepump token list query.
#[derive(serde::Deserialize, schemars::JsonSchema)]
pub struct MemepumpTokenListParams {
    pub chain: String,
    pub stage: String,
    pub wallet_address: Option<String>,
    pub protocol_id_list: Option<String>,
    pub quote_token_address_list: Option<String>,
    pub min_top10_holdings_percent: Option<String>,
    pub max_top10_holdings_percent: Option<String>,
    pub min_dev_holdings_percent: Option<String>,
    pub max_dev_holdings_percent: Option<String>,
    pub min_insiders_percent: Option<String>,
    pub max_insiders_percent: Option<String>,
    pub min_bundlers_percent: Option<String>,
    pub max_bundlers_percent: Option<String>,
    pub min_snipers_percent: Option<String>,
    pub max_snipers_percent: Option<String>,
    pub min_fresh_wallets_percent: Option<String>,
    pub max_fresh_wallets_percent: Option<String>,
    pub min_suspected_phishing_wallet_percent: Option<String>,
    pub max_suspected_phishing_wallet_percent: Option<String>,
    pub min_bot_traders: Option<String>,
    pub max_bot_traders: Option<String>,
    pub min_dev_migrated: Option<String>,
    pub max_dev_migrated: Option<String>,
    pub min_market_cap: Option<String>,
    pub max_market_cap: Option<String>,
    pub min_volume: Option<String>,
    pub max_volume: Option<String>,
    pub min_tx_count: Option<String>,
    pub max_tx_count: Option<String>,
    pub min_bonding_percent: Option<String>,
    pub max_bonding_percent: Option<String>,
    pub min_holders: Option<String>,
    pub max_holders: Option<String>,
    pub min_token_age: Option<String>,
    pub max_token_age: Option<String>,
    pub min_buy_tx_count: Option<String>,
    pub max_buy_tx_count: Option<String>,
    pub min_sell_tx_count: Option<String>,
    pub max_sell_tx_count: Option<String>,
    pub min_token_symbol_length: Option<String>,
    pub max_token_symbol_length: Option<String>,
    pub has_at_least_one_social_link: Option<String>,
    pub has_x: Option<String>,
    pub has_telegram: Option<String>,
    pub has_website: Option<String>,
    pub website_type_list: Option<String>,
    pub dex_screener_paid: Option<String>,
    pub live_on_pump_fun: Option<String>,
    pub dev_sell_all: Option<String>,
    pub dev_still_holding: Option<String>,
    pub community_takeover: Option<String>,
    pub bags_fee_claimed: Option<String>,
    pub min_fees_native: Option<String>,
    pub max_fees_native: Option<String>,
    pub keywords_include: Option<String>,
    pub keywords_exclude: Option<String>,
}

/// GET /api/v6/dex/market/memepump/tokenList — filtered token list
pub async fn fetch_memepump_token_list(
    client: &ApiClient,
    p: MemepumpTokenListParams,
) -> Result<Value> {
    let chain_index = crate::chains::resolve_chain(&p.chain).to_string();

    let wallet_address = p.wallet_address.unwrap_or_default();
    let protocol_id_list = p.protocol_id_list.unwrap_or_default();
    let quote_token_address_list = p.quote_token_address_list.unwrap_or_default();
    let min_top10 = p.min_top10_holdings_percent.unwrap_or_default();
    let max_top10 = p.max_top10_holdings_percent.unwrap_or_default();
    let min_dev_hold = p.min_dev_holdings_percent.unwrap_or_default();
    let max_dev_hold = p.max_dev_holdings_percent.unwrap_or_default();
    let min_insiders = p.min_insiders_percent.unwrap_or_default();
    let max_insiders = p.max_insiders_percent.unwrap_or_default();
    let min_bundlers = p.min_bundlers_percent.unwrap_or_default();
    let max_bundlers = p.max_bundlers_percent.unwrap_or_default();
    let min_snipers = p.min_snipers_percent.unwrap_or_default();
    let max_snipers = p.max_snipers_percent.unwrap_or_default();
    let min_fresh = p.min_fresh_wallets_percent.unwrap_or_default();
    let max_fresh = p.max_fresh_wallets_percent.unwrap_or_default();
    let min_phishing = p.min_suspected_phishing_wallet_percent.unwrap_or_default();
    let max_phishing = p.max_suspected_phishing_wallet_percent.unwrap_or_default();
    let min_bots = p.min_bot_traders.unwrap_or_default();
    let max_bots = p.max_bot_traders.unwrap_or_default();
    let min_dev_migrated = p.min_dev_migrated.unwrap_or_default();
    let max_dev_migrated = p.max_dev_migrated.unwrap_or_default();
    let min_market_cap = p.min_market_cap.unwrap_or_default();
    let max_market_cap = p.max_market_cap.unwrap_or_default();
    let min_volume = p.min_volume.unwrap_or_default();
    let max_volume = p.max_volume.unwrap_or_default();
    let min_tx_count = p.min_tx_count.unwrap_or_default();
    let max_tx_count = p.max_tx_count.unwrap_or_default();
    let min_bonding = p.min_bonding_percent.unwrap_or_default();
    let max_bonding = p.max_bonding_percent.unwrap_or_default();
    let min_holders = p.min_holders.unwrap_or_default();
    let max_holders = p.max_holders.unwrap_or_default();
    let min_token_age = p.min_token_age.unwrap_or_default();
    let max_token_age = p.max_token_age.unwrap_or_default();
    let min_buy_tx = p.min_buy_tx_count.unwrap_or_default();
    let max_buy_tx = p.max_buy_tx_count.unwrap_or_default();
    let min_sell_tx = p.min_sell_tx_count.unwrap_or_default();
    let max_sell_tx = p.max_sell_tx_count.unwrap_or_default();
    let min_sym_len = p.min_token_symbol_length.unwrap_or_default();
    let max_sym_len = p.max_token_symbol_length.unwrap_or_default();
    let has_social = p.has_at_least_one_social_link.unwrap_or_default();
    let has_x = p.has_x.unwrap_or_default();
    let has_tg = p.has_telegram.unwrap_or_default();
    let has_web = p.has_website.unwrap_or_default();
    let web_types = p.website_type_list.unwrap_or_default();
    let dex_paid = p.dex_screener_paid.unwrap_or_default();
    let live_pump = p.live_on_pump_fun.unwrap_or_default();
    let dev_sell = p.dev_sell_all.unwrap_or_default();
    let dev_hold = p.dev_still_holding.unwrap_or_default();
    let cto = p.community_takeover.unwrap_or_default();
    let bags_fee = p.bags_fee_claimed.unwrap_or_default();
    let min_fees = p.min_fees_native.unwrap_or_default();
    let max_fees = p.max_fees_native.unwrap_or_default();
    let kw_include = p.keywords_include.unwrap_or_default();
    let kw_exclude = p.keywords_exclude.unwrap_or_default();

    client
        .get(
            "/api/v6/dex/market/memepump/tokenList",
            &[
                ("chainIndex", chain_index.as_str()),
                ("stage", &p.stage),
                ("walletAddress", &wallet_address),
                ("protocolIdList", &protocol_id_list),
                ("quoteTokenAddressList", &quote_token_address_list),
                ("minTop10HoldingsPercent", &min_top10),
                ("maxTop10HoldingsPercent", &max_top10),
                ("minDevHoldingsPercent", &min_dev_hold),
                ("maxDevHoldingsPercent", &max_dev_hold),
                ("minInsidersPercent", &min_insiders),
                ("maxInsidersPercent", &max_insiders),
                ("minBundlersPercent", &min_bundlers),
                ("maxBundlersPercent", &max_bundlers),
                ("minSnipersPercent", &min_snipers),
                ("maxSnipersPercent", &max_snipers),
                ("minFreshWalletsPercent", &min_fresh),
                ("maxFreshWalletsPercent", &max_fresh),
                ("minSuspectedPhishingWalletPercent", &min_phishing),
                ("maxSuspectedPhishingWalletPercent", &max_phishing),
                ("minBotTraders", &min_bots),
                ("maxBotTraders", &max_bots),
                ("minDevMigrated", &min_dev_migrated),
                ("maxDevMigrated", &max_dev_migrated),
                ("minMarketCapUsd", &min_market_cap),
                ("maxMarketCapUsd", &max_market_cap),
                ("minVolumeUsd", &min_volume),
                ("maxVolumeUsd", &max_volume),
                ("minTxCount", &min_tx_count),
                ("maxTxCount", &max_tx_count),
                ("minBondingPercent", &min_bonding),
                ("maxBondingPercent", &max_bonding),
                ("minHolders", &min_holders),
                ("maxHolders", &max_holders),
                ("minTokenAge", &min_token_age),
                ("maxTokenAge", &max_token_age),
                ("minBuyTxCount", &min_buy_tx),
                ("maxBuyTxCount", &max_buy_tx),
                ("minSellTxCount", &min_sell_tx),
                ("maxSellTxCount", &max_sell_tx),
                ("minTokenSymbolLength", &min_sym_len),
                ("maxTokenSymbolLength", &max_sym_len),
                ("hasAtLeastOneSocialLink", &has_social),
                ("hasX", &has_x),
                ("hasTelegram", &has_tg),
                ("hasWebsite", &has_web),
                ("websiteTypeList", &web_types),
                ("dexScreenerPaid", &dex_paid),
                ("liveOnPumpFun", &live_pump),
                ("devSellAll", &dev_sell),
                ("devStillHolding", &dev_hold),
                ("communityTakeover", &cto),
                ("bagsFeeClaimed", &bags_fee),
                ("minFeesNative", &min_fees),
                ("maxFeesNative", &max_fees),
                ("keywordsInclude", &kw_include),
                ("keywordsExclude", &kw_exclude),
            ],
        )
        .await
}

/// GET /api/v6/dex/market/memepump/tokenDetails — requires walletAddress
pub async fn fetch_memepump_token_details(
    client: &ApiClient,
    address: &str,
    chain_index: &str,
    wallet_address: &str,
) -> Result<Value> {
    client
        .get(
            "/api/v6/dex/market/memepump/tokenDetails",
            &[
                ("chainIndex", chain_index),
                ("tokenContractAddress", address),
                ("walletAddress", wallet_address),
            ],
        )
        .await
}

/// GET /api/v6/dex/market/memepump/apedWallet — optional walletAddress
pub async fn fetch_memepump_aped_wallet(
    client: &ApiClient,
    address: &str,
    chain_index: &str,
    wallet_address: &str,
) -> Result<Value> {
    client
        .get(
            "/api/v6/dex/market/memepump/apedWallet",
            &[
                ("chainIndex", chain_index),
                ("tokenContractAddress", address),
                ("walletAddress", wallet_address),
            ],
        )
        .await
}

/// Shared helper for memepump endpoints that take (chainIndex, tokenContractAddress).
pub async fn fetch_memepump_by_address(
    client: &ApiClient,
    path: &str,
    address: &str,
    chain_index: &str,
) -> Result<Value> {
    client
        .get(
            path,
            &[
                ("chainIndex", chain_index),
                ("tokenContractAddress", address),
            ],
        )
        .await
}

/// GET /api/v6/dex/market/signal/supported/chain — no parameters
pub async fn fetch_signal_chains(client: &ApiClient) -> Result<Value> {
    client
        .get("/api/v6/dex/market/signal/supported/chain", &[])
        .await
}

/// POST /api/v6/dex/market/signal/list — smart money / KOL / whale signals
#[allow(clippy::too_many_arguments)]
pub async fn fetch_signal_list(
    client: &ApiClient,
    chain_index: &str,
    wallet_type: Option<String>,
    min_amount_usd: Option<String>,
    max_amount_usd: Option<String>,
    min_address_count: Option<String>,
    max_address_count: Option<String>,
    token_address: Option<String>,
    min_market_cap_usd: Option<String>,
    max_market_cap_usd: Option<String>,
    min_liquidity_usd: Option<String>,
    max_liquidity_usd: Option<String>,
) -> Result<Value> {
    let mut body = json!({"chainIndex": chain_index});
    let obj = body.as_object_mut().unwrap();
    if let Some(v) = wallet_type {
        obj.insert("walletType".into(), Value::String(v));
    }
    if let Some(v) = min_amount_usd {
        obj.insert("minAmountUsd".into(), Value::String(v));
    }
    if let Some(v) = max_amount_usd {
        obj.insert("maxAmountUsd".into(), Value::String(v));
    }
    if let Some(v) = min_address_count {
        obj.insert("minAddressCount".into(), Value::String(v));
    }
    if let Some(v) = max_address_count {
        obj.insert("maxAddressCount".into(), Value::String(v));
    }
    if let Some(v) = token_address {
        obj.insert("tokenAddress".into(), Value::String(v));
    }
    if let Some(v) = min_market_cap_usd {
        obj.insert("minMarketCapUsd".into(), Value::String(v));
    }
    if let Some(v) = max_market_cap_usd {
        obj.insert("maxMarketCapUsd".into(), Value::String(v));
    }
    if let Some(v) = min_liquidity_usd {
        obj.insert("minLiquidityUsd".into(), Value::String(v));
    }
    if let Some(v) = max_liquidity_usd {
        obj.insert("maxLiquidityUsd".into(), Value::String(v));
    }
    client.post("/api/v6/dex/market/signal/list", &body).await
}


/// GET /api/v6/dex/market/portfolio/supported/chain
pub async fn fetch_portfolio_supported_chains(client: &ApiClient) -> Result<Value> {
    client
        .get("/api/v6/dex/market/portfolio/supported/chain", &[])
        .await
}

async fn portfolio_supported_chains(ctx: &Context) -> Result<()> {
    let client = ctx.client()?;
    output::success(fetch_portfolio_supported_chains(&client).await?);
    Ok(())
}

/// GET /api/v6/dex/market/portfolio/overview
pub async fn fetch_portfolio_overview(
    client: &ApiClient,
    chain_index: &str,
    address: &str,
    time_frame: &str,
) -> Result<Value> {
    client
        .get(
            "/api/v6/dex/market/portfolio/overview",
            &[
                ("chainIndex", chain_index),
                ("walletAddress", address),
                ("timeFrame", time_frame),
            ],
        )
        .await
}

async fn portfolio_overview(
    ctx: &Context,
    address: &str,
    chain: &str,
    time_frame: &str,
) -> Result<()> {
    let chain_index = crate::chains::resolve_chain(chain);
    let client = ctx.client()?;
    output::success(fetch_portfolio_overview(&client, &chain_index, address, time_frame).await?);
    Ok(())
}

/// GET /api/v6/dex/market/portfolio/dex-history
#[allow(clippy::too_many_arguments)]
pub async fn fetch_portfolio_dex_history(
    client: &ApiClient,
    chain_index: &str,
    address: &str,
    begin: &str,
    end: &str,
    limit: Option<&str>,
    cursor: Option<&str>,
    token: Option<&str>,
    tx_type: Option<&str>,
) -> Result<Value> {
    let mut query: Vec<(&str, &str)> = vec![
        ("chainIndex", chain_index),
        ("walletAddress", address),
        ("begin", begin),
        ("end", end),
    ];
    if let Some(l) = limit {
        query.push(("limit", l));
    }
    if let Some(c) = cursor {
        query.push(("cursor", c));
    }
    if let Some(t) = token {
        query.push(("tokenContractAddress", t));
    }
    if let Some(ty) = tx_type {
        query.push(("type", ty));
    }
    client
        .get("/api/v6/dex/market/portfolio/dex-history", &query)
        .await
}

#[allow(clippy::too_many_arguments)]
async fn portfolio_dex_history(
    ctx: &Context,
    address: &str,
    chain: &str,
    begin: &str,
    end: &str,
    limit: Option<&str>,
    cursor: Option<&str>,
    token: Option<&str>,
    tx_type: Option<&str>,
) -> Result<()> {
    let chain_index = crate::chains::resolve_chain(chain);
    let client = ctx.client()?;
    output::success(
        fetch_portfolio_dex_history(&client, &chain_index, address, begin, end, limit, cursor, token, tx_type).await?,
    );
    Ok(())
}

/// GET /api/v6/dex/market/portfolio/recent-pnl
pub async fn fetch_portfolio_recent_pnl(
    client: &ApiClient,
    chain_index: &str,
    address: &str,
    limit: Option<&str>,
    cursor: Option<&str>,
) -> Result<Value> {
    let mut query: Vec<(&str, &str)> = vec![
        ("chainIndex", chain_index),
        ("walletAddress", address),
    ];
    if let Some(l) = limit {
        query.push(("limit", l));
    }
    if let Some(c) = cursor {
        query.push(("cursor", c));
    }
    client
        .get("/api/v6/dex/market/portfolio/recent-pnl", &query)
        .await
}

async fn portfolio_recent_pnl(
    ctx: &Context,
    address: &str,
    chain: &str,
    limit: Option<&str>,
    cursor: Option<&str>,
) -> Result<()> {
    let chain_index = crate::chains::resolve_chain(chain);
    let client = ctx.client()?;
    output::success(fetch_portfolio_recent_pnl(&client, &chain_index, address, limit, cursor).await?);
    Ok(())
}

/// GET /api/v6/dex/market/portfolio/token/latest-pnl
pub async fn fetch_portfolio_token_pnl(
    client: &ApiClient,
    chain_index: &str,
    address: &str,
    token: &str,
) -> Result<Value> {
    client
        .get(
            "/api/v6/dex/market/portfolio/token/latest-pnl",
            &[
                ("chainIndex", chain_index),
                ("walletAddress", address),
                ("tokenContractAddress", token),
            ],
        )
        .await
}

async fn portfolio_token_pnl(ctx: &Context, address: &str, chain: &str, token: &str) -> Result<()> {
    let chain_index = crate::chains::resolve_chain(chain);
    let client = ctx.client()?;
    output::success(fetch_portfolio_token_pnl(&client, &chain_index, address, token).await?);
    Ok(())
}
