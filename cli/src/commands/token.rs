use anyhow::Result;
use clap::Subcommand;
use serde_json::{json, Value};

use super::Context;
use crate::client::ApiClient;
use crate::output;

#[derive(Subcommand)]
#[allow(clippy::large_enum_variant)]
pub enum TokenCommand {
    /// Search for tokens by name, symbol, or address
    Search {
        /// Search keyword (name, symbol, or contract address)
        #[arg(long)]
        query: String,
        /// Chains to search (comma-separated, e.g. "ethereum,solana"). Defaults to global --chain if set, otherwise "1,501"
        #[arg(long)]
        chains: Option<String>,
        /// Number of results per page (default: 20, max: 100)
        #[arg(long)]
        limit: Option<String>,
        /// Pagination cursor — pass the cursor from the last item of the previous page; omit for first page
        #[arg(long)]
        cursor: Option<String>,
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
        /// Number of results per page (default: 20, max: 100)
        #[arg(long)]
        limit: Option<String>,
        /// Pagination cursor — pass the cursor from the last item of the previous page; omit for first page
        #[arg(long)]
        cursor: Option<String>,
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
        /// Number of results per page (default: 20, max: 100)
        #[arg(long)]
        limit: Option<String>,
        /// Pagination cursor — pass the cursor from the last item of the previous page; omit for first page
        #[arg(long)]
        cursor: Option<String>,
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
        /// Number of results per page (default: 20, max: 100)
        #[arg(long)]
        limit: Option<String>,
        /// Pagination cursor — pass the cursor from the last item of the previous page; omit for first page
        #[arg(long)]
        cursor: Option<String>,
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
    /// Get token holder cluster concentration overview (cluster level, rug pull %, new address %)
    ClusterOverview {
        /// Token contract address
        #[arg(long)]
        address: String,
        /// Chain
        #[arg(long)]
        chain: Option<String>,
    },
    /// Get top 10/50/100 holder overview (holding %, trend, avg PnL, avg cost, avg sell)
    ClusterTopHolders {
        /// Token contract address
        #[arg(long)]
        address: String,
        /// Chain
        #[arg(long)]
        chain: Option<String>,
        /// Holder rank filter: 1 = top 10, 2 = top 50, 3 = top 100
        #[arg(long)]
        range_filter: String,
    },
    /// Get holder cluster list (clusters of top 300 holders with address details)
    ClusterList {
        /// Token contract address
        #[arg(long)]
        address: String,
        /// Chain
        #[arg(long)]
        chain: Option<String>,
    },
    /// Get supported chains for holder cluster analysis
    ClusterSupportedChains,

    /// Composite: token info + price-info + advanced-info + security scan in one call.
    /// Equivalent to running all 4 sub-commands in parallel.
    /// PRD Section 3.1: onchainos token report
    Report {
        /// Token contract address
        #[arg(long)]
        address: String,
        /// Chain (e.g. solana, ethereum). Defaults to global --chain or ethereum.
        #[arg(long)]
        chain: Option<String>,
    },
}

pub async fn execute(ctx: &Context, cmd: TokenCommand) -> Result<()> {
    let mut client = ctx.client_async().await?;
    match cmd {
        TokenCommand::Search {
            query,
            chains,
            limit,
            cursor,
        } => {
            if query.trim().is_empty() {
                anyhow::bail!("Parameter --query cannot be empty");
            }
            let resolved_chains = ctx.resolve_chains_or(chains, "1,501");
            output::success(
                fetch_search(
                    &mut client,
                    &query,
                    &resolved_chains,
                    limit.as_deref(),
                    cursor.as_deref(),
                )
                .await?,
            );
        }
        TokenCommand::Info { address, chain } => {
            let chain_index = chain
                .map(|c| crate::chains::resolve_chain(&c).to_string())
                .unwrap_or_else(|| ctx.chain_index_or("ethereum"));
            output::success(fetch_info(&mut client, &address, &chain_index).await?);
        }
        TokenCommand::Holders {
            address,
            chain,
            tag_filter,
            limit,
            cursor,
        } => {
            let chain_index = chain
                .map(|c| crate::chains::resolve_chain(&c).to_string())
                .unwrap_or_else(|| ctx.chain_index_or("ethereum"));
            output::success(
                fetch_holders(
                    &mut client,
                    &address,
                    &chain_index,
                    tag_filter,
                    limit.as_deref(),
                    cursor.as_deref(),
                )
                .await?,
            );
        }
        TokenCommand::PriceInfo { address, chain } => {
            let chain_index = chain
                .map(|c| crate::chains::resolve_chain(&c).to_string())
                .unwrap_or_else(|| ctx.chain_index_or("ethereum"));
            output::success(fetch_price_info(&mut client, &address, &chain_index).await?);
        }
        TokenCommand::Liquidity { address, chain } => {
            let chain_index = chain
                .map(|c| crate::chains::resolve_chain(&c).to_string())
                .unwrap_or_else(|| ctx.chain_index_or("ethereum"));
            output::success(fetch_liquidity(&mut client, &address, &chain_index).await?);
        }
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
            limit,
            cursor,
        } => {
            output::success(
                fetch_hot_tokens(
                    &mut client,
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
                        limit,
                        cursor,
                    },
                )
                .await?,
            );
        }
        TokenCommand::AdvancedInfo { address, chain } => {
            let chain_index = chain
                .map(|c| crate::chains::resolve_chain(&c).to_string())
                .unwrap_or_else(|| ctx.chain_index_or("ethereum"));
            output::success(fetch_advanced_info(&mut client, &address, &chain_index).await?);
        }
        TokenCommand::TopTrader {
            address,
            chain,
            tag_filter,
            limit,
            cursor,
        } => {
            let chain_index = chain
                .map(|c| crate::chains::resolve_chain(&c).to_string())
                .unwrap_or_else(|| ctx.chain_index_or("ethereum"));
            output::success(
                fetch_top_trader(
                    &mut client,
                    &address,
                    &chain_index,
                    tag_filter,
                    limit.as_deref(),
                    cursor.as_deref(),
                )
                .await?,
            );
        }
        TokenCommand::Trades {
            address,
            chain,
            limit,
            tag_filter,
            wallet_filter,
        } => {
            let chain_index = chain
                .map(|c| crate::chains::resolve_chain(&c).to_string())
                .unwrap_or_else(|| ctx.chain_index_or("ethereum"));
            output::success(
                fetch_token_trades(
                    &mut client,
                    &address,
                    &chain_index,
                    limit,
                    tag_filter.as_deref(),
                    wallet_filter.as_deref(),
                )
                .await?,
            );
        }
        TokenCommand::ClusterOverview { address, chain } => {
            cluster_by_address(
                ctx,
                "/api/v6/dex/market/token/cluster/overview",
                &address,
                chain,
            )
            .await?;
        }
        TokenCommand::ClusterTopHolders {
            address,
            chain,
            range_filter,
        } => cluster_top_holders(ctx, &address, chain, &range_filter).await?,
        TokenCommand::ClusterList { address, chain } => {
            cluster_by_address(
                ctx,
                "/api/v6/dex/market/token/cluster/list",
                &address,
                chain,
            )
            .await?;
        }
        TokenCommand::ClusterSupportedChains => cluster_supported_chains(ctx).await?,
        TokenCommand::Report { address, chain } => {
            let chain_index = chain
                .map(|c| crate::chains::resolve_chain(&c).to_string())
                .unwrap_or_else(|| ctx.chain_index_or("ethereum"));
            output::success(fetch_report(&mut client, &address, &chain_index).await?);
        }
    }
    Ok(())
}

/// GET /api/v6/dex/market/token/search
pub async fn fetch_search(
    client: &mut ApiClient,
    query: &str,
    chains: &str,
    limit: Option<&str>,
    cursor: Option<&str>,
) -> Result<Value> {
    let resolved_chains = crate::chains::resolve_chains(chains);
    if let Some(s) = limit {
        let n: u64 = s
            .parse()
            .map_err(|_| anyhow::anyhow!("--limit must be a number between 1 and 100"))?;
        anyhow::ensure!(
            (1..=100).contains(&n),
            "--limit must be between 1 and 100, got {n}"
        );
    }
    let mut params = vec![
        ("chains", resolved_chains.as_str()),
        ("search", query),
        ("limit", limit.unwrap_or("20")),
    ];
    if let Some(c) = cursor {
        params.push(("cursor", c));
    }
    client.get("/api/v6/dex/market/token/search", &params).await
}

/// POST /api/v6/dex/market/token/basic-info — body is JSON array
pub async fn fetch_info(client: &mut ApiClient, address: &str, chain_index: &str) -> Result<Value> {
    let body = json!([{"chainIndex": chain_index, "tokenContractAddress": address}]);
    client
        .post("/api/v6/dex/market/token/basic-info", &body)
        .await
}

/// GET /api/v6/dex/market/token/holder
pub async fn fetch_holders(
    client: &mut ApiClient,
    address: &str,
    chain_index: &str,
    tag_filter: Option<u8>,
    limit: Option<&str>,
    cursor: Option<&str>,
) -> Result<Value> {
    if let Some(s) = limit {
        let n: u64 = s
            .parse()
            .map_err(|_| anyhow::anyhow!("--limit must be a number between 1 and 100"))?;
        anyhow::ensure!(
            (1..=100).contains(&n),
            "--limit must be between 1 and 100, got {n}"
        );
    }
    let tag_str = tag_filter.map(|t| t.to_string()).unwrap_or_default();
    let mut params = vec![
        ("chainIndex", chain_index),
        ("tokenContractAddress", address),
        ("tagFilter", tag_str.as_str()),
        ("limit", limit.unwrap_or("20")),
    ];
    if let Some(c) = cursor {
        params.push(("cursor", c));
    }
    client.get("/api/v6/dex/market/token/holder", &params).await
}

/// GET /api/v6/dex/market/token/top-liquidity — top 5 liquidity pools for a token
pub async fn fetch_liquidity(
    client: &mut ApiClient,
    address: &str,
    chain_index: &str,
) -> Result<Value> {
    client
        .get(
            "/api/v6/dex/market/token/top-liquidity",
            &[
                ("chainIndex", chain_index),
                ("tokenContractAddress", address),
            ],
        )
        .await
}

/// POST /api/v6/dex/market/price-info — body is JSON array
pub async fn fetch_price_info(
    client: &mut ApiClient,
    address: &str,
    chain_index: &str,
) -> Result<Value> {
    let body = json!([{"chainIndex": chain_index, "tokenContractAddress": address}]);
    client.post("/api/v6/dex/market/price-info", &body).await
}

/// POST /api/v6/security/token-scan — single-token scan helper shared by
/// `token report` and the workflow layer. Any future schema change lands here.
pub async fn fetch_security(
    client: &mut ApiClient,
    address: &str,
    chain_index: &str,
) -> Result<Value> {
    let body = json!({
        "source": "onchain_os_cli",
        "tokenList": [{ "chainId": chain_index, "contractAddress": address }]
    });
    client.post("/api/v6/security/token-scan", &body).await
}

/// Parameters for the hot token list query.
#[derive(serde::Deserialize, schemars::JsonSchema, Default)]
pub struct HotTokensParams {
    /// Ranking type: 4=Trending (token score), 5=Xmentioned (Twitter mentions)
    pub ranking_type: String,
    /// Chain filter (e.g. ethereum, solana). Empty for all chains
    pub chain: Option<String>,
    /// Sort field: 1=price, 2=price change, 3=txs, 4=unique traders, 5=volume,
    /// 6=market cap, 7=liquidity, 8=created time, 9=OKX search count,
    /// 10=holders, 11=mention count, 12=social score, 14=net inflow, 15=token score
    pub rank_by: Option<String>,
    /// Time frame: 1=5min, 2=1h, 3=4h, 4=24h
    pub time_frame: Option<String>,
    /// Hide risky tokens (true/false)
    pub risk_filter: Option<String>,
    /// Filter stable coins (true/false)
    pub stable_token_filter: Option<String>,
    /// Protocol ID filter, comma-separated (e.g. 120596 for Pump.fun)
    pub project_id: Option<String>,
    /// Min price change percent
    pub price_change_min: Option<String>,
    /// Max price change percent
    pub price_change_max: Option<String>,
    /// Min volume (USD)
    pub volume_min: Option<String>,
    /// Max volume (USD)
    pub volume_max: Option<String>,
    /// Min market cap (USD)
    pub market_cap_min: Option<String>,
    /// Max market cap (USD)
    pub market_cap_max: Option<String>,
    /// Min liquidity (USD)
    pub liquidity_min: Option<String>,
    /// Max liquidity (USD)
    pub liquidity_max: Option<String>,
    /// Min transaction count (tradeAmount)
    pub transaction_min: Option<String>,
    /// Max transaction count
    pub transaction_max: Option<String>,
    /// Min txs count
    pub txs_min: Option<String>,
    /// Max txs count
    pub txs_max: Option<String>,
    /// Min unique traders
    pub unique_trader_min: Option<String>,
    /// Max unique traders
    pub unique_trader_max: Option<String>,
    /// Min holders
    pub holders_min: Option<String>,
    /// Max holders
    pub holders_max: Option<String>,
    /// Min net inflow USD
    pub inflow_min: Option<String>,
    /// Max net inflow USD
    pub inflow_max: Option<String>,
    /// Min FDV (USD)
    pub fdv_min: Option<String>,
    /// Max FDV (USD)
    pub fdv_max: Option<String>,
    /// Min mention count (for Xmentioned ranking)
    pub mentioned_count_min: Option<String>,
    /// Max mention count
    pub mentioned_count_max: Option<String>,
    /// Min social score
    pub social_score_min: Option<String>,
    /// Max social score
    pub social_score_max: Option<String>,
    /// Min top-10 holder percent
    pub top10_hold_percent_min: Option<String>,
    /// Max top-10 holder percent
    pub top10_hold_percent_max: Option<String>,
    /// Min dev hold percent
    pub dev_hold_percent_min: Option<String>,
    /// Max dev hold percent
    pub dev_hold_percent_max: Option<String>,
    /// Min bundle hold percent
    pub bundle_hold_percent_min: Option<String>,
    /// Max bundle hold percent
    pub bundle_hold_percent_max: Option<String>,
    /// Min suspicious hold percent
    pub suspicious_hold_percent_min: Option<String>,
    /// Max suspicious hold percent
    pub suspicious_hold_percent_max: Option<String>,
    /// LP burned filter (true/false)
    pub is_lp_burnt: Option<String>,
    /// Mintable filter (true/false)
    pub is_mint: Option<String>,
    /// Freeze filter (true/false)
    pub is_freeze: Option<String>,
    /// Number of results per page (default: 20, max: 100). Use cursor for pagination.
    pub limit: Option<String>,
    /// Pagination cursor. Pass the cursor value from the last item of the previous response to fetch the next page. Omit for first page.
    pub cursor: Option<String>,
}

/// GET /api/v6/dex/market/token/hot-token — hot token list by trending score or X mentions
pub async fn fetch_hot_tokens(client: &mut ApiClient, params: HotTokensParams) -> Result<Value> {
    if let Some(ref s) = params.limit {
        let n: u64 = s
            .parse()
            .map_err(|_| anyhow::anyhow!("--limit must be a number between 1 and 100"))?;
        anyhow::ensure!(
            (1..=100).contains(&n),
            "--limit must be between 1 and 100, got {n}"
        );
    }
    let hot_limit = params.limit.unwrap_or_else(|| "20".to_string());
    let hot_cursor = params.cursor;
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

    let mut req_params = vec![
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
        ("limit", hot_limit.as_str()),
    ];
    if let Some(ref c) = hot_cursor {
        req_params.push(("cursor", c.as_str()));
    }
    client
        .get("/api/v6/dex/market/token/hot-token", &req_params)
        .await
}

/// GET /api/v6/dex/market/token/advanced-info
pub async fn fetch_advanced_info(
    client: &mut ApiClient,
    address: &str,
    chain_index: &str,
) -> Result<Value> {
    client
        .get(
            "/api/v6/dex/market/token/advanced-info",
            &[
                ("chainIndex", chain_index),
                ("tokenContractAddress", address),
            ],
        )
        .await
}

/// GET /api/v6/dex/market/token/top-trader
pub async fn fetch_top_trader(
    client: &mut ApiClient,
    address: &str,
    chain_index: &str,
    tag_filter: Option<u8>,
    limit: Option<&str>,
    cursor: Option<&str>,
) -> Result<Value> {
    if let Some(s) = limit {
        let n: u64 = s
            .parse()
            .map_err(|_| anyhow::anyhow!("--limit must be a number between 1 and 100"))?;
        anyhow::ensure!(
            (1..=100).contains(&n),
            "--limit must be between 1 and 100, got {n}"
        );
    }
    let tag_str = tag_filter.map(|t| t.to_string()).unwrap_or_default();
    let mut params = vec![
        ("chainIndex", chain_index),
        ("tokenContractAddress", address),
        ("tagFilter", tag_str.as_str()),
        ("limit", limit.unwrap_or("20")),
    ];
    if let Some(c) = cursor {
        params.push(("cursor", c));
    }
    client
        .get("/api/v6/dex/market/token/top-trader", &params)
        .await
}

/// GET /api/v6/dex/market/trades — token trade history
pub async fn fetch_token_trades(
    client: &mut ApiClient,
    address: &str,
    chain_index: &str,
    limit: u32,
    tag_filter: Option<&str>,
    wallet_filter: Option<&str>,
) -> Result<Value> {
    let limit_str = limit.to_string();
    let tag_str = tag_filter.unwrap_or_default();
    let wallet_str = wallet_filter.unwrap_or_default();
    client
        .get(
            "/api/v6/dex/market/trades",
            &[
                ("chainIndex", chain_index),
                ("tokenContractAddress", address),
                ("limit", &limit_str),
                ("tagFilter", tag_str),
                ("walletAddressFilter", wallet_str),
            ],
        )
        .await
}

/// Shared helper for cluster endpoints that take (chainIndex, tokenContractAddress) only.
async fn cluster_by_address(
    ctx: &Context,
    path: &str,
    address: &str,
    chain: Option<String>,
) -> Result<()> {
    let chain_index = chain
        .map(|c| crate::chains::resolve_chain(&c).to_string())
        .unwrap_or_else(|| ctx.chain_index_or("ethereum"));
    let mut client = ctx.client_async().await?;
    output::success(fetch_cluster_by_address(&mut client, path, address, &chain_index).await?);
    Ok(())
}

/// GET /api/v6/dex/market/token/cluster/supported/chain — no parameters
pub async fn fetch_cluster_supported_chains(client: &mut ApiClient) -> Result<Value> {
    client
        .get("/api/v6/dex/market/token/cluster/supported/chain", &[])
        .await
}

async fn cluster_supported_chains(ctx: &Context) -> Result<()> {
    let mut client = ctx.client_async().await?;
    output::success(fetch_cluster_supported_chains(&mut client).await?);
    Ok(())
}

/// GET /api/v6/dex/market/token/cluster/overview or /cluster/list
pub async fn fetch_cluster_by_address(
    client: &mut ApiClient,
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

/// GET /api/v6/dex/market/token/cluster/top-holders
pub async fn fetch_cluster_top_holders(
    client: &mut ApiClient,
    address: &str,
    chain_index: &str,
    range_filter: &str,
) -> Result<Value> {
    client
        .get(
            "/api/v6/dex/market/token/cluster/top-holders",
            &[
                ("chainIndex", chain_index),
                ("tokenContractAddress", address),
                ("rangeFilter", range_filter),
            ],
        )
        .await
}

async fn cluster_top_holders(
    ctx: &Context,
    address: &str,
    chain: Option<String>,
    range_filter: &str,
) -> Result<()> {
    let chain_index = chain
        .map(|c| crate::chains::resolve_chain(&c).to_string())
        .unwrap_or_else(|| ctx.chain_index_or("ethereum"));
    let mut client = ctx.client_async().await?;
    output::success(
        fetch_cluster_top_holders(&mut client, address, &chain_index, range_filter).await?,
    );
    Ok(())
}

/// Composite command: runs token info + price-info + advanced-info + security scan.
/// PRD Section 3.1 — `onchainos token report`
/// Error handling: single sub-call failure → that field is null, rest continue.
///                 All sub-calls fail → returns error.
pub async fn fetch_report(
    client: &mut ApiClient,
    address: &str,
    chain_index: &str,
) -> Result<serde_json::Value> {
    // Four clones — each future gets exclusive ownership of its own ApiClient.
    // tokio::join! polls all four concurrently; HTTP I/O is truly parallel via
    // the shared reqwest::Client connection pool (Arc-backed).
    let (mut c1, mut c2, mut c3) = (client.clone(), client.clone(), client.clone());

    let (info, price, advanced, security) = tokio::join!(
        fetch_info(client, address, chain_index),
        fetch_price_info(&mut c1, address, chain_index),
        fetch_advanced_info(&mut c2, address, chain_index),
        fetch_security(&mut c3, address, chain_index),
    );

    let info = info.ok();
    let price = price.ok();
    let advanced = advanced.ok();
    let security = security.ok();

    // All failed → return error per PRD spec
    if info.is_none() && price.is_none() && advanced.is_none() && security.is_none() {
        anyhow::bail!(
            "token report: all sub-calls failed for address {} on chain {}",
            address,
            chain_index
        );
    }

    Ok(serde_json::json!({
        "address":     address,
        "chain":       chain_index,
        "info":        info,
        "priceInfo":   price,
        "advancedInfo": advanced,
        "security":    security,
    }))
}
