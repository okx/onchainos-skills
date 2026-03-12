use anyhow::Result;
use clap::Subcommand;
use serde_json::json;

use super::Context;
use crate::output;

#[derive(Subcommand)]
#[allow(clippy::large_enum_variant)]
pub enum TokenCommand {
    /// Search for tokens by name, symbol, or address
    Search {
        /// Search keyword (name, symbol, or contract address)
        #[arg(long)]
        query: String,
        /// Chains to search (comma-separated, e.g. "ethereum,solana")
        #[arg(long, default_value = "1,501")]
        chains: String,
    },
    /// Get token basic info (name, symbol, decimals, logo)
    Info {
        /// Token contract address
        #[arg(long)]
        address: String,
        /// Chain
        #[arg(long)]
        chain: Option<String>,
    },
    /// Get token holder distribution (top 100)
    Holders {
        /// Token contract address
        #[arg(long)]
        address: String,
        /// Chain
        #[arg(long)]
        chain: Option<String>,
        /// Filter by holder tag: 1=KOL, 2=Developer, 3=Smart Money, 4=Whale, 5=Fresh Wallet, 6=Insider, 7=Sniper, 8=Suspicious Phishing, 9=Bundler
        #[arg(long)]
        tag_filter: Option<u8>,
    },
    /// Get trending / top tokens
    Trending {
        /// Chains (comma-separated)
        #[arg(long, default_value = "1,501")]
        chains: String,
        /// Sort by: 2=price change, 5=volume, 6=market cap
        #[arg(long, default_value = "5")]
        sort_by: String,
        /// Time frame: 1=5min, 2=1h, 3=4h, 4=24h
        #[arg(long, default_value = "4")]
        time_frame: String,
    },
    /// Get detailed price info (price, market cap, liquidity, volume, 24h change)
    PriceInfo {
        /// Token contract address
        #[arg(long)]
        address: String,
        /// Chain
        #[arg(long)]
        chain: Option<String>,
    },
    /// Get top 5 liquidity pools for a token
    Liquidity {
        /// Token contract address
        #[arg(long)]
        address: String,
        /// Chain (e.g. ethereum, base, bsc)
        #[arg(long)]
        chain: Option<String>,
    },
    /// Get hot token list ranked by trending score or X mentions (max 100 results)
    HotTokens {
        /// Ranking type: 4=Trending (token score), 5=Xmentioned (Twitter mentions)
        #[arg(long, default_value = "4")]
        ranking_type: String,
        /// Chain filter (e.g. ethereum, solana). Default: all chains
        #[arg(long)]
        chain: Option<String>,
        /// Sort field: 1=price, 2=price change, 3=txs, 4=unique traders, 5=volume,
        /// 6=market cap, 7=liquidity, 8=created time, 9=OKX search count,
        /// 10=holders, 11=mention count, 12=social score, 14=net inflow, 15=token score
        #[arg(long)]
        rank_by: Option<String>,
        /// Time frame: 1=5min, 2=1h, 3=4h, 4=24h
        #[arg(long)]
        time_frame: Option<String>,
        /// Hide risky tokens (true/false, default: true)
        #[arg(long)]
        risk_filter: Option<String>,
        /// Filter stable coins (true/false, default: true)
        #[arg(long)]
        stable_token_filter: Option<String>,
        /// Protocol ID filter, comma-separated (e.g. 120596 for Pump.fun)
        #[arg(long)]
        project_id: Option<String>,
        /// Min price change percent (can be negative, e.g. -5)
        #[arg(long, allow_hyphen_values = true)]
        price_change_min: Option<String>,
        /// Max price change percent (can be negative, e.g. -5 for losers)
        #[arg(long, allow_hyphen_values = true)]
        price_change_max: Option<String>,
        /// Min volume (USD)
        #[arg(long)]
        volume_min: Option<String>,
        /// Max volume (USD)
        #[arg(long)]
        volume_max: Option<String>,
        /// Min market cap (USD)
        #[arg(long)]
        market_cap_min: Option<String>,
        /// Max market cap (USD)
        #[arg(long)]
        market_cap_max: Option<String>,
        /// Min liquidity (USD)
        #[arg(long)]
        liquidity_min: Option<String>,
        /// Max liquidity (USD)
        #[arg(long)]
        liquidity_max: Option<String>,
        /// Min transaction count (tradeAmount)
        #[arg(long)]
        transaction_min: Option<String>,
        /// Max transaction count (tradeAmount)
        #[arg(long)]
        transaction_max: Option<String>,
        /// Min txs count
        #[arg(long)]
        txs_min: Option<String>,
        /// Max txs count
        #[arg(long)]
        txs_max: Option<String>,
        /// Min unique traders
        #[arg(long)]
        unique_trader_min: Option<String>,
        /// Max unique traders
        #[arg(long)]
        unique_trader_max: Option<String>,
        /// Min holders
        #[arg(long)]
        holders_min: Option<String>,
        /// Max holders
        #[arg(long)]
        holders_max: Option<String>,
        /// Min net inflow USD
        #[arg(long)]
        inflow_min: Option<String>,
        /// Max net inflow USD
        #[arg(long)]
        inflow_max: Option<String>,
        /// Min FDV (USD)
        #[arg(long)]
        fdv_min: Option<String>,
        /// Max FDV (USD)
        #[arg(long)]
        fdv_max: Option<String>,
        /// Min mention count (for Xmentioned ranking)
        #[arg(long)]
        mentioned_count_min: Option<String>,
        /// Max mention count
        #[arg(long)]
        mentioned_count_max: Option<String>,
        /// Min social score
        #[arg(long)]
        social_score_min: Option<String>,
        /// Max social score
        #[arg(long)]
        social_score_max: Option<String>,
        /// Min top-10 holder percent
        #[arg(long)]
        top10_hold_percent_min: Option<String>,
        /// Max top-10 holder percent
        #[arg(long)]
        top10_hold_percent_max: Option<String>,
        /// Min dev hold percent
        #[arg(long)]
        dev_hold_percent_min: Option<String>,
        /// Max dev hold percent
        #[arg(long)]
        dev_hold_percent_max: Option<String>,
        /// Min bundle hold percent
        #[arg(long)]
        bundle_hold_percent_min: Option<String>,
        /// Max bundle hold percent
        #[arg(long)]
        bundle_hold_percent_max: Option<String>,
        /// Min suspicious hold percent
        #[arg(long)]
        suspicious_hold_percent_min: Option<String>,
        /// Max suspicious hold percent
        #[arg(long)]
        suspicious_hold_percent_max: Option<String>,
        /// LP burned filter (true/false, default: true)
        #[arg(long)]
        is_lp_burnt: Option<String>,
        /// Mintable filter (true/false, default: true)
        #[arg(long)]
        is_mint: Option<String>,
        /// Freeze filter (true/false, default: true)
        #[arg(long)]
        is_freeze: Option<String>,
    },
    /// Get advanced token info (risk, creator, dev stats, holder concentration)
    AdvancedInfo {
        /// Token contract address
        #[arg(long)]
        address: String,
        /// Chain
        #[arg(long)]
        chain: Option<String>,
    },
    /// Get top traders (profit addresses) for a token
    TopTrader {
        /// Token contract address
        #[arg(long)]
        address: String,
        /// Chain
        #[arg(long)]
        chain: Option<String>,
        /// Filter by trader tag: 1=KOL, 2=Developer, 3=Smart Money, 4=Whale, 5=Fresh Wallet, 6=Insider, 7=Sniper, 8=Suspicious Phishing, 9=Bundler
        #[arg(long)]
        tag_filter: Option<u8>,
    },
    /// Get token trade history on DEX, with optional tag and wallet filters
    Trades {
        /// Token contract address
        #[arg(long)]
        address: String,
        /// Chain
        #[arg(long)]
        chain: Option<String>,
        /// Number of trades (max 500)
        #[arg(long, default_value = "100")]
        limit: u32,
        /// Filter by trader tag: 1=KOL, 2=Developer, 3=Smart Money, 4=Whale, 5=Fresh Wallet, 6=Insider, 7=Sniper, 8=Suspicious Phishing, 9=Bundler
        #[arg(long)]
        tag_filter: Option<String>,
        /// Filter by wallet address (comma-separated, max 10)
        #[arg(long)]
        wallet_filter: Option<String>,
    },
}

pub async fn execute(ctx: &Context, cmd: TokenCommand) -> Result<()> {
    match cmd {
        TokenCommand::Search { query, chains } => search(ctx, &query, &chains).await,
        TokenCommand::Info { address, chain } => info(ctx, &address, chain).await,
        TokenCommand::Holders {
            address,
            chain,
            tag_filter,
        } => holders(ctx, &address, chain, tag_filter).await,
        TokenCommand::Trending {
            chains,
            sort_by,
            time_frame,
        } => trending(ctx, &chains, &sort_by, &time_frame).await,
        TokenCommand::PriceInfo { address, chain } => price_info(ctx, &address, chain).await,
        TokenCommand::Liquidity { address, chain } => liquidity(ctx, &address, chain).await,
        TokenCommand::HotTokens {
            ranking_type,
            chain,
            rank_by,
            time_frame,
            risk_filter,
            stable_token_filter,
            project_id,
            price_change_min,
            price_change_max,
            volume_min,
            volume_max,
            market_cap_min,
            market_cap_max,
            liquidity_min,
            liquidity_max,
            transaction_min,
            transaction_max,
            txs_min,
            txs_max,
            unique_trader_min,
            unique_trader_max,
            holders_min,
            holders_max,
            inflow_min,
            inflow_max,
            fdv_min,
            fdv_max,
            mentioned_count_min,
            mentioned_count_max,
            social_score_min,
            social_score_max,
            top10_hold_percent_min,
            top10_hold_percent_max,
            dev_hold_percent_min,
            dev_hold_percent_max,
            bundle_hold_percent_min,
            bundle_hold_percent_max,
            suspicious_hold_percent_min,
            suspicious_hold_percent_max,
            is_lp_burnt,
            is_mint,
            is_freeze,
        } => {
            hot_tokens(
                ctx,
                HotTokensParams {
                    ranking_type,
                    chain,
                    rank_by,
                    time_frame,
                    risk_filter,
                    stable_token_filter,
                    project_id,
                    price_change_min,
                    price_change_max,
                    volume_min,
                    volume_max,
                    market_cap_min,
                    market_cap_max,
                    liquidity_min,
                    liquidity_max,
                    transaction_min,
                    transaction_max,
                    txs_min,
                    txs_max,
                    unique_trader_min,
                    unique_trader_max,
                    holders_min,
                    holders_max,
                    inflow_min,
                    inflow_max,
                    fdv_min,
                    fdv_max,
                    mentioned_count_min,
                    mentioned_count_max,
                    social_score_min,
                    social_score_max,
                    top10_hold_percent_min,
                    top10_hold_percent_max,
                    dev_hold_percent_min,
                    dev_hold_percent_max,
                    bundle_hold_percent_min,
                    bundle_hold_percent_max,
                    suspicious_hold_percent_min,
                    suspicious_hold_percent_max,
                    is_lp_burnt,
                    is_mint,
                    is_freeze,
                },
            )
            .await
        }
        TokenCommand::AdvancedInfo { address, chain } => advanced_info(ctx, &address, chain).await,
        TokenCommand::TopTrader {
            address,
            chain,
            tag_filter,
        } => top_trader(ctx, &address, chain, tag_filter).await,
        TokenCommand::Trades {
            address,
            chain,
            limit,
            tag_filter,
            wallet_filter,
        } => token_trades(ctx, &address, chain, limit, tag_filter, wallet_filter).await,
    }
}

/// GET /api/v6/dex/market/token/search
async fn search(ctx: &Context, query: &str, chains: &str) -> Result<()> {
    let resolved_chains = crate::chains::resolve_chains(chains);
    let client = ctx.client()?;
    let data = client
        .get(
            "/api/v6/dex/market/token/search",
            &[("chains", resolved_chains.as_str()), ("search", query)],
        )
        .await?;
    output::success(data);
    Ok(())
}

/// POST /api/v6/dex/market/token/basic-info — body is JSON array
async fn info(ctx: &Context, address: &str, chain: Option<String>) -> Result<()> {
    let chain_index = chain
        .map(|c| crate::chains::resolve_chain(&c).to_string())
        .unwrap_or_else(|| ctx.chain_index_or("ethereum"));
    let client = ctx.client()?;
    let body = json!([{
        "chainIndex": chain_index,
        "tokenContractAddress": address
    }]);
    let data = client
        .post("/api/v6/dex/market/token/basic-info", &body)
        .await?;
    output::success(data);
    Ok(())
}

/// GET /api/v6/dex/market/token/holder
async fn holders(
    ctx: &Context,
    address: &str,
    chain: Option<String>,
    tag_filter: Option<u8>,
) -> Result<()> {
    let chain_index = chain
        .map(|c| crate::chains::resolve_chain(&c).to_string())
        .unwrap_or_else(|| ctx.chain_index_or("ethereum"));
    let tag_str = tag_filter.map(|t| t.to_string()).unwrap_or_default();
    let client = ctx.client()?;
    let data = client
        .get(
            "/api/v6/dex/market/token/holder",
            &[
                ("chainIndex", chain_index.as_str()),
                ("tokenContractAddress", address),
                ("tagFilter", tag_str.as_str()),
            ],
        )
        .await?;
    output::success(data);
    Ok(())
}

/// GET /api/v6/dex/market/token/toplist
async fn trending(ctx: &Context, chains: &str, sort_by: &str, time_frame: &str) -> Result<()> {
    let resolved_chains = crate::chains::resolve_chains(chains);
    let client = ctx.client()?;
    let data = client
        .get(
            "/api/v6/dex/market/token/toplist",
            &[
                ("chains", resolved_chains.as_str()),
                ("sortBy", sort_by),
                ("timeFrame", time_frame),
            ],
        )
        .await?;
    output::success(data);
    Ok(())
}

/// GET /api/v6/dex/market/token/top-liquidity — top 5 liquidity pools for a token
async fn liquidity(ctx: &Context, address: &str, chain: Option<String>) -> Result<()> {
    let chain_index = chain
        .map(|c| crate::chains::resolve_chain(&c).to_string())
        .unwrap_or_else(|| ctx.chain_index_or("ethereum"));
    let client = ctx.client()?;
    let data = client
        .get(
            "/api/v6/dex/market/token/top-liquidity",
            &[
                ("chainIndex", chain_index.as_str()),
                ("tokenContractAddress", address),
            ],
        )
        .await?;
    output::success(data);
    Ok(())
}

/// POST /api/v6/dex/market/price-info — body is JSON array
async fn price_info(ctx: &Context, address: &str, chain: Option<String>) -> Result<()> {
    let chain_index = chain
        .map(|c| crate::chains::resolve_chain(&c).to_string())
        .unwrap_or_else(|| ctx.chain_index_or("ethereum"));
    let client = ctx.client()?;
    let body = json!([{
        "chainIndex": chain_index,
        "tokenContractAddress": address
    }]);
    let data = client.post("/api/v6/dex/market/price-info", &body).await?;
    output::success(data);
    Ok(())
}

struct HotTokensParams {
    ranking_type: String,
    chain: Option<String>,
    rank_by: Option<String>,
    time_frame: Option<String>,
    risk_filter: Option<String>,
    stable_token_filter: Option<String>,
    project_id: Option<String>,
    price_change_min: Option<String>,
    price_change_max: Option<String>,
    volume_min: Option<String>,
    volume_max: Option<String>,
    market_cap_min: Option<String>,
    market_cap_max: Option<String>,
    liquidity_min: Option<String>,
    liquidity_max: Option<String>,
    transaction_min: Option<String>,
    transaction_max: Option<String>,
    txs_min: Option<String>,
    txs_max: Option<String>,
    unique_trader_min: Option<String>,
    unique_trader_max: Option<String>,
    holders_min: Option<String>,
    holders_max: Option<String>,
    inflow_min: Option<String>,
    inflow_max: Option<String>,
    fdv_min: Option<String>,
    fdv_max: Option<String>,
    mentioned_count_min: Option<String>,
    mentioned_count_max: Option<String>,
    social_score_min: Option<String>,
    social_score_max: Option<String>,
    top10_hold_percent_min: Option<String>,
    top10_hold_percent_max: Option<String>,
    dev_hold_percent_min: Option<String>,
    dev_hold_percent_max: Option<String>,
    bundle_hold_percent_min: Option<String>,
    bundle_hold_percent_max: Option<String>,
    suspicious_hold_percent_min: Option<String>,
    suspicious_hold_percent_max: Option<String>,
    is_lp_burnt: Option<String>,
    is_mint: Option<String>,
    is_freeze: Option<String>,
}

/// GET /api/v6/dex/market/token/hot-token — hot token list by trending score or X mentions
async fn hot_tokens(ctx: &Context, params: HotTokensParams) -> Result<()> {
    let chain_index = params
        .chain
        .map(|c| crate::chains::resolve_chain(&c).to_string())
        .unwrap_or_default();

    let rank_by = params.rank_by.unwrap_or_default();
    let time_frame = params.time_frame.unwrap_or_default();
    let risk_filter = params.risk_filter.unwrap_or_default();
    let stable_token_filter = params.stable_token_filter.unwrap_or_default();
    let project_id = params.project_id.unwrap_or_default();
    let price_change_min = params.price_change_min.unwrap_or_default();
    let price_change_max = params.price_change_max.unwrap_or_default();
    let volume_min = params.volume_min.unwrap_or_default();
    let volume_max = params.volume_max.unwrap_or_default();
    let market_cap_min = params.market_cap_min.unwrap_or_default();
    let market_cap_max = params.market_cap_max.unwrap_or_default();
    let liquidity_min = params.liquidity_min.unwrap_or_default();
    let liquidity_max = params.liquidity_max.unwrap_or_default();
    let transaction_min = params.transaction_min.unwrap_or_default();
    let transaction_max = params.transaction_max.unwrap_or_default();
    let txs_min = params.txs_min.unwrap_or_default();
    let txs_max = params.txs_max.unwrap_or_default();
    let unique_trader_min = params.unique_trader_min.unwrap_or_default();
    let unique_trader_max = params.unique_trader_max.unwrap_or_default();
    let holders_min = params.holders_min.unwrap_or_default();
    let holders_max = params.holders_max.unwrap_or_default();
    let inflow_min = params.inflow_min.unwrap_or_default();
    let inflow_max = params.inflow_max.unwrap_or_default();
    let fdv_min = params.fdv_min.unwrap_or_default();
    let fdv_max = params.fdv_max.unwrap_or_default();
    let mentioned_count_min = params.mentioned_count_min.unwrap_or_default();
    let mentioned_count_max = params.mentioned_count_max.unwrap_or_default();
    let social_score_min = params.social_score_min.unwrap_or_default();
    let social_score_max = params.social_score_max.unwrap_or_default();
    let top10_hold_percent_min = params.top10_hold_percent_min.unwrap_or_default();
    let top10_hold_percent_max = params.top10_hold_percent_max.unwrap_or_default();
    let dev_hold_percent_min = params.dev_hold_percent_min.unwrap_or_default();
    let dev_hold_percent_max = params.dev_hold_percent_max.unwrap_or_default();
    let bundle_hold_percent_min = params.bundle_hold_percent_min.unwrap_or_default();
    let bundle_hold_percent_max = params.bundle_hold_percent_max.unwrap_or_default();
    let suspicious_hold_percent_min = params.suspicious_hold_percent_min.unwrap_or_default();
    let suspicious_hold_percent_max = params.suspicious_hold_percent_max.unwrap_or_default();
    let is_lp_burnt = params.is_lp_burnt.unwrap_or_default();
    let is_mint = params.is_mint.unwrap_or_default();
    let is_freeze = params.is_freeze.unwrap_or_default();

    let client = ctx.client()?;
    let data = client
        .get(
            "/api/v6/dex/market/token/hot-token",
            &[
                ("rankingType", params.ranking_type.as_str()),
                ("chainIndex", chain_index.as_str()),
                ("rankBy", rank_by.as_str()),
                ("rankingTimeFrame", time_frame.as_str()),
                ("riskFilter", risk_filter.as_str()),
                ("stableTokenFilter", stable_token_filter.as_str()),
                ("protocolId", project_id.as_str()),
                ("priceChangePercentMin", price_change_min.as_str()),
                ("priceChangePercentMax", price_change_max.as_str()),
                ("volumeMin", volume_min.as_str()),
                ("volumeMax", volume_max.as_str()),
                ("tradeAmountMin", transaction_min.as_str()),
                ("tradeAmountMax", transaction_max.as_str()),
                ("txsMin", txs_min.as_str()),
                ("txsMax", txs_max.as_str()),
                ("uniqueTraderMin", unique_trader_min.as_str()),
                ("uniqueTraderMax", unique_trader_max.as_str()),
                ("marketCapMin", market_cap_min.as_str()),
                ("marketCapMax", market_cap_max.as_str()),
                ("liquidityMin", liquidity_min.as_str()),
                ("liquidityMax", liquidity_max.as_str()),
                ("holdersMin", holders_min.as_str()),
                ("holdersMax", holders_max.as_str()),
                ("inflowUsdMin", inflow_min.as_str()),
                ("inflowUsdMax", inflow_max.as_str()),
                ("fdvMin", fdv_min.as_str()),
                ("fdvMax", fdv_max.as_str()),
                ("mentionedCountMin", mentioned_count_min.as_str()),
                ("mentionedCountMax", mentioned_count_max.as_str()),
                ("socialScoreMin", social_score_min.as_str()),
                ("socialScoreMax", social_score_max.as_str()),
                ("top10HoldPercentMin", top10_hold_percent_min.as_str()),
                ("top10HoldPercentMax", top10_hold_percent_max.as_str()),
                ("devHoldPercentMin", dev_hold_percent_min.as_str()),
                ("devHoldPercentMax", dev_hold_percent_max.as_str()),
                ("bundleHoldPercentMin", bundle_hold_percent_min.as_str()),
                ("bundleHoldPercentMax", bundle_hold_percent_max.as_str()),
                (
                    "suspiciousHoldPercentMin",
                    suspicious_hold_percent_min.as_str(),
                ),
                (
                    "suspiciousHoldPercentMax",
                    suspicious_hold_percent_max.as_str(),
                ),
                ("isLpBurnt", is_lp_burnt.as_str()),
                ("isMint", is_mint.as_str()),
                ("isFreeze", is_freeze.as_str()),
            ],
        )
        .await?;
    output::success(data);
    Ok(())
}

/// GET /api/v6/dex/market/token/advanced-info
async fn advanced_info(ctx: &Context, address: &str, chain: Option<String>) -> Result<()> {
    let chain_index = chain
        .map(|c| crate::chains::resolve_chain(&c).to_string())
        .unwrap_or_else(|| ctx.chain_index_or("ethereum"));
    let client = ctx.client()?;
    let data = client
        .get(
            "/api/v6/dex/market/token/advanced-info",
            &[
                ("chainIndex", chain_index.as_str()),
                ("tokenContractAddress", address),
            ],
        )
        .await?;
    output::success(data);
    Ok(())
}

/// GET /api/v6/dex/market/token/top-trader
async fn top_trader(
    ctx: &Context,
    address: &str,
    chain: Option<String>,
    tag_filter: Option<u8>,
) -> Result<()> {
    let chain_index = chain
        .map(|c| crate::chains::resolve_chain(&c).to_string())
        .unwrap_or_else(|| ctx.chain_index_or("ethereum"));
    let tag_str = tag_filter.map(|t| t.to_string()).unwrap_or_default();
    let client = ctx.client()?;
    let data = client
        .get(
            "/api/v6/dex/market/token/top-trader",
            &[
                ("chainIndex", chain_index.as_str()),
                ("tokenContractAddress", address),
                ("tagFilter", tag_str.as_str()),
            ],
        )
        .await?;
    output::success(data);
    Ok(())
}

/// GET /api/v6/dex/market/trades — token trade history with optional tag/wallet filters
async fn token_trades(
    ctx: &Context,
    address: &str,
    chain: Option<String>,
    limit: u32,
    tag_filter: Option<String>,
    wallet_filter: Option<String>,
) -> Result<()> {
    let chain_index = chain
        .map(|c| crate::chains::resolve_chain(&c).to_string())
        .unwrap_or_else(|| ctx.chain_index_or("ethereum"));
    let limit_str = limit.to_string();
    let tag_str = tag_filter.unwrap_or_default();
    let wallet_str = wallet_filter.unwrap_or_default();
    let client = ctx.client()?;
    let data = client
        .get(
            "/api/v6/dex/market/trades",
            &[
                ("chainIndex", chain_index.as_str()),
                ("tokenContractAddress", address),
                ("limit", &limit_str),
                ("tagFilter", tag_str.as_str()),
                ("walletAddressFilter", wallet_str.as_str()),
            ],
        )
        .await?;
    output::success(data);
    Ok(())
}
