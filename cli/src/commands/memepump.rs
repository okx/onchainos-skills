use anyhow::Result;
use clap::Subcommand;
use serde_json::Value;

use super::Context;
use crate::client::ApiClient;
use crate::output;

#[derive(Subcommand)]
#[allow(clippy::large_enum_variant)]
pub enum MemepumpCommand {
    /// Get supported chains and protocols for Meme Pump
    Chains,
    /// Get Meme Pump token list (filtered)
    Tokens {
        /// Chain (e.g. solana, bsc). Required.
        #[arg(long)]
        chain: String,
        /// Token stage: NEW, MIGRATING, or MIGRATED (required by API)
        #[arg(long)]
        stage: String,
        /// Wallet address for position-specific data
        #[arg(long)]
        wallet_address: Option<String>,
        /// Comma-separated protocol IDs to filter tokens
        #[arg(long)]
        protocol_id_list: Option<String>,
        /// Comma-separated quote token addresses
        #[arg(long)]
        quote_token_address_list: Option<String>,
        // ── Holder analysis ──
        /// Minimum top 10 holders percentage (0-100)
        #[arg(long)]
        min_top10_holdings_percent: Option<String>,
        /// Maximum top 10 holders percentage (0-100)
        #[arg(long)]
        max_top10_holdings_percent: Option<String>,
        /// Minimum developer holdings percentage
        #[arg(long)]
        min_dev_holdings_percent: Option<String>,
        /// Maximum developer holdings percentage
        #[arg(long)]
        max_dev_holdings_percent: Option<String>,
        /// Minimum insider wallet percentage
        #[arg(long)]
        min_insiders_percent: Option<String>,
        /// Maximum insider wallet percentage
        #[arg(long)]
        max_insiders_percent: Option<String>,
        /// Minimum bundler wallet percentage
        #[arg(long)]
        min_bundlers_percent: Option<String>,
        /// Maximum bundler wallet percentage
        #[arg(long)]
        max_bundlers_percent: Option<String>,
        /// Minimum sniper wallet percentage
        #[arg(long)]
        min_snipers_percent: Option<String>,
        /// Maximum sniper wallet percentage
        #[arg(long)]
        max_snipers_percent: Option<String>,
        // ── Wallet analysis ──
        /// Minimum newly-created wallet percentage
        #[arg(long)]
        min_fresh_wallets_percent: Option<String>,
        /// Maximum newly-created wallet percentage
        #[arg(long)]
        max_fresh_wallets_percent: Option<String>,
        /// Minimum phishing wallet percentage
        #[arg(long)]
        min_suspected_phishing_wallet_percent: Option<String>,
        /// Maximum phishing wallet percentage
        #[arg(long)]
        max_suspected_phishing_wallet_percent: Option<String>,
        /// Minimum bot trader wallet count
        #[arg(long)]
        min_bot_traders: Option<String>,
        /// Maximum bot trader wallet count
        #[arg(long)]
        max_bot_traders: Option<String>,
        // ── Dev history ──
        /// Minimum tokens migrated by developer
        #[arg(long)]
        min_dev_migrated: Option<String>,
        /// Maximum tokens migrated by developer
        #[arg(long)]
        max_dev_migrated: Option<String>,
        // ── Market data ──
        /// Minimum market cap in USD
        #[arg(long)]
        min_market_cap: Option<String>,
        /// Maximum market cap in USD
        #[arg(long)]
        max_market_cap: Option<String>,
        /// Minimum 24h volume in USD
        #[arg(long)]
        min_volume: Option<String>,
        /// Maximum 24h volume in USD
        #[arg(long)]
        max_volume: Option<String>,
        /// Minimum transaction count
        #[arg(long)]
        min_tx_count: Option<String>,
        /// Maximum transaction count
        #[arg(long)]
        max_tx_count: Option<String>,
        /// Minimum bonding curve completion (0-100)
        #[arg(long)]
        min_bonding_percent: Option<String>,
        /// Maximum bonding curve completion (0-100)
        #[arg(long)]
        max_bonding_percent: Option<String>,
        /// Minimum unique holder count
        #[arg(long)]
        min_holders: Option<String>,
        /// Maximum unique holder count
        #[arg(long)]
        max_holders: Option<String>,
        /// Minimum token age in minutes
        #[arg(long)]
        min_token_age: Option<String>,
        /// Maximum token age in minutes
        #[arg(long)]
        max_token_age: Option<String>,
        /// Minimum buy transactions (last 1 hour)
        #[arg(long)]
        min_buy_tx_count: Option<String>,
        /// Maximum buy transactions (last 1 hour)
        #[arg(long)]
        max_buy_tx_count: Option<String>,
        /// Minimum sell transactions (last 1 hour)
        #[arg(long)]
        min_sell_tx_count: Option<String>,
        /// Maximum sell transactions (last 1 hour)
        #[arg(long)]
        max_sell_tx_count: Option<String>,
        // ── Token metadata ──
        /// Minimum ticker symbol length
        #[arg(long)]
        min_token_symbol_length: Option<String>,
        /// Maximum ticker symbol length
        #[arg(long)]
        max_token_symbol_length: Option<String>,
        // ── Social filters ──
        /// Require at least one social media link (true/false)
        #[arg(long)]
        has_at_least_one_social_link: Option<String>,
        /// Require X (Twitter) link (true/false)
        #[arg(long)]
        has_x: Option<String>,
        /// Require Telegram link (true/false)
        #[arg(long)]
        has_telegram: Option<String>,
        /// Require website link (true/false)
        #[arg(long)]
        has_website: Option<String>,
        /// Website types: 0=official, 1=YouTube, 2=Twitch, etc.
        #[arg(long)]
        website_type_list: Option<String>,
        /// Filter by DexScreener promotion status (true/false)
        #[arg(long)]
        dex_screener_paid: Option<String>,
        /// Filter by PumpFun live stream status (true/false)
        #[arg(long)]
        live_on_pump_fun: Option<String>,
        // ── Dev status ──
        /// Filter by developer liquidation status (true/false)
        #[arg(long)]
        dev_sell_all: Option<String>,
        /// Filter by developer holding status (true/false)
        #[arg(long)]
        dev_still_holding: Option<String>,
        // ── Other ──
        /// Filter by community takeover status (true/false)
        #[arg(long)]
        community_takeover: Option<String>,
        /// Filter by fee claim status (true/false)
        #[arg(long)]
        bags_fee_claimed: Option<String>,
        /// Minimum fees in native currency
        #[arg(long)]
        min_fees_native: Option<String>,
        /// Maximum fees in native currency
        #[arg(long)]
        max_fees_native: Option<String>,
        /// Include tokens matching keyword (case-insensitive)
        #[arg(long)]
        keywords_include: Option<String>,
        /// Exclude tokens matching keyword (case-insensitive)
        #[arg(long)]
        keywords_exclude: Option<String>,
    },
    /// Get Meme Pump token details
    TokenDetails {
        /// Token contract address
        #[arg(long)]
        address: String,
        /// Chain (e.g. solana, bsc)
        #[arg(long)]
        chain: Option<String>,
        /// User wallet address (for position and P&L data)
        #[arg(long)]
        wallet: Option<String>,
    },
    /// Get Meme Pump token developer info
    TokenDevInfo {
        /// Token contract address
        #[arg(long)]
        address: String,
        /// Chain (e.g. solana, bsc)
        #[arg(long)]
        chain: Option<String>,
    },
    /// Get similar tokens for a Meme Pump token
    SimilarTokens {
        /// Token contract address
        #[arg(long)]
        address: String,
        /// Chain (e.g. solana, bsc)
        #[arg(long)]
        chain: Option<String>,
    },
    /// Get Meme Pump token bundle (bundler/sniper) info
    TokenBundleInfo {
        /// Token contract address
        #[arg(long)]
        address: String,
        /// Chain (e.g. solana, bsc)
        #[arg(long)]
        chain: Option<String>,
    },
    /// Get Meme Pump aped (co-invested) wallet data
    ApedWallet {
        /// Token contract address
        #[arg(long)]
        address: String,
        /// Chain (e.g. solana, bsc)
        #[arg(long)]
        chain: Option<String>,
        /// User wallet address (to highlight if present in aped wallets)
        #[arg(long)]
        wallet: Option<String>,
    },
}

pub async fn execute(ctx: &Context, cmd: MemepumpCommand) -> Result<()> {
    match cmd {
        MemepumpCommand::Chains => memepump_chains(ctx).await,
        MemepumpCommand::Tokens {
            chain,
            stage,
            wallet_address,
            protocol_id_list,
            quote_token_address_list,
            min_top10_holdings_percent,
            max_top10_holdings_percent,
            min_dev_holdings_percent,
            max_dev_holdings_percent,
            min_insiders_percent,
            max_insiders_percent,
            min_bundlers_percent,
            max_bundlers_percent,
            min_snipers_percent,
            max_snipers_percent,
            min_fresh_wallets_percent,
            max_fresh_wallets_percent,
            min_suspected_phishing_wallet_percent,
            max_suspected_phishing_wallet_percent,
            min_bot_traders,
            max_bot_traders,
            min_dev_migrated,
            max_dev_migrated,
            min_market_cap,
            max_market_cap,
            min_volume,
            max_volume,
            min_tx_count,
            max_tx_count,
            min_bonding_percent,
            max_bonding_percent,
            min_holders,
            max_holders,
            min_token_age,
            max_token_age,
            min_buy_tx_count,
            max_buy_tx_count,
            min_sell_tx_count,
            max_sell_tx_count,
            min_token_symbol_length,
            max_token_symbol_length,
            has_at_least_one_social_link,
            has_x,
            has_telegram,
            has_website,
            website_type_list,
            dex_screener_paid,
            live_on_pump_fun,
            dev_sell_all,
            dev_still_holding,
            community_takeover,
            bags_fee_claimed,
            min_fees_native,
            max_fees_native,
            keywords_include,
            keywords_exclude,
        } => {
            let client = ctx.client()?;
            output::success(
                fetch_token_list(
                    &client,
                    MemepumpTokenListParams {
                        chain,
                        stage,
                        wallet_address,
                        protocol_id_list,
                        quote_token_address_list,
                        min_top10_holdings_percent,
                        max_top10_holdings_percent,
                        min_dev_holdings_percent,
                        max_dev_holdings_percent,
                        min_insiders_percent,
                        max_insiders_percent,
                        min_bundlers_percent,
                        max_bundlers_percent,
                        min_snipers_percent,
                        max_snipers_percent,
                        min_fresh_wallets_percent,
                        max_fresh_wallets_percent,
                        min_suspected_phishing_wallet_percent,
                        max_suspected_phishing_wallet_percent,
                        min_bot_traders,
                        max_bot_traders,
                        min_dev_migrated,
                        max_dev_migrated,
                        min_market_cap,
                        max_market_cap,
                        min_volume,
                        max_volume,
                        min_tx_count,
                        max_tx_count,
                        min_bonding_percent,
                        max_bonding_percent,
                        min_holders,
                        max_holders,
                        min_token_age,
                        max_token_age,
                        min_buy_tx_count,
                        max_buy_tx_count,
                        min_sell_tx_count,
                        max_sell_tx_count,
                        min_token_symbol_length,
                        max_token_symbol_length,
                        has_at_least_one_social_link,
                        has_x,
                        has_telegram,
                        has_website,
                        website_type_list,
                        dex_screener_paid,
                        live_on_pump_fun,
                        dev_sell_all,
                        dev_still_holding,
                        community_takeover,
                        bags_fee_claimed,
                        min_fees_native,
                        max_fees_native,
                        keywords_include,
                        keywords_exclude,
                    },
                )
                .await?,
            );
            Ok(())
        }
        MemepumpCommand::TokenDetails {
            address,
            chain,
            wallet,
        } => memepump_token_details(ctx, &address, chain, wallet).await,
        MemepumpCommand::TokenDevInfo { address, chain } => {
            memepump_by_address(
                ctx,
                "/api/v6/dex/market/memepump/tokenDevInfo",
                &address,
                chain,
            )
            .await
        }
        MemepumpCommand::SimilarTokens { address, chain } => {
            memepump_by_address(
                ctx,
                "/api/v6/dex/market/memepump/similarToken",
                &address,
                chain,
            )
            .await
        }
        MemepumpCommand::TokenBundleInfo { address, chain } => {
            memepump_by_address(
                ctx,
                "/api/v6/dex/market/memepump/tokenBundleInfo",
                &address,
                chain,
            )
            .await
        }
        MemepumpCommand::ApedWallet {
            address,
            chain,
            wallet,
        } => memepump_aped_wallet(ctx, &address, chain, wallet).await,
    }
}

// ── Public fetch functions (used by both CLI and MCP) ────────────────

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

/// GET /api/v6/dex/market/memepump/supported/chainsProtocol
pub async fn fetch_chains(client: &ApiClient) -> Result<Value> {
    client
        .get("/api/v6/dex/market/memepump/supported/chainsProtocol", &[])
        .await
}

/// GET /api/v6/dex/market/memepump/tokenList
pub async fn fetch_token_list(client: &ApiClient, p: MemepumpTokenListParams) -> Result<Value> {
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

/// GET /api/v6/dex/market/memepump/tokenDetails
pub async fn fetch_token_details(
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

/// GET /api/v6/dex/market/memepump/apedWallet
pub async fn fetch_aped_wallet(
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
pub async fn fetch_by_address(
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

// ── CLI wrappers ─────────────────────────────────────────────────────

async fn memepump_chains(ctx: &Context) -> Result<()> {
    let client = ctx.client()?;
    output::success(fetch_chains(&client).await?);
    Ok(())
}

async fn memepump_token_details(
    ctx: &Context,
    address: &str,
    chain: Option<String>,
    wallet: Option<String>,
) -> Result<()> {
    let chain_index = chain
        .map(|c| crate::chains::resolve_chain(&c).to_string())
        .unwrap_or_else(|| ctx.chain_index_or("solana"));
    let wallet_address = wallet.unwrap_or_default();
    let client = ctx.client()?;
    output::success(fetch_token_details(&client, address, &chain_index, &wallet_address).await?);
    Ok(())
}

async fn memepump_aped_wallet(
    ctx: &Context,
    address: &str,
    chain: Option<String>,
    wallet: Option<String>,
) -> Result<()> {
    let chain_index = chain
        .map(|c| crate::chains::resolve_chain(&c).to_string())
        .unwrap_or_else(|| ctx.chain_index_or("solana"));
    let wallet_address = wallet.unwrap_or_default();
    let client = ctx.client()?;
    output::success(fetch_aped_wallet(&client, address, &chain_index, &wallet_address).await?);
    Ok(())
}

async fn memepump_by_address(
    ctx: &Context,
    path: &str,
    address: &str,
    chain: Option<String>,
) -> Result<()> {
    let chain_index = chain
        .map(|c| crate::chains::resolve_chain(&c).to_string())
        .unwrap_or_else(|| ctx.chain_index_or("solana"));
    let client = ctx.client()?;
    output::success(fetch_by_address(&client, path, address, &chain_index).await?);
    Ok(())
}
