use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::Result;
use clap::Subcommand;
use serde_json::Value;

use super::Context;
use crate::client::ApiClient;
use crate::output;

/// Tokens created less than this many milliseconds ago may have unreliable
/// holder/risk tag data — zero values are not yet meaningful.
///
/// Measured from `now_ms()` *after* the API response returns, so network
/// latency eats into the window. Under multi-second roundtrips this can
/// become effectively inert; widen here if we start seeing that in the wild.
const NEW_TOKEN_THRESHOLD_MS: u64 = 2_000;

/// Tag fields whose zero value is unreliable for brand-new tokens.
const UNRELIABLE_ZERO_TAGS: &[&str] = &[
    "bundlersPercent",
    "devHoldingsPercent",
    "freshWalletsPercent",
    "insidersPercent",
    "snipersPercent",
    "suspectedPhishingWalletPercent",
];

/// Capture the current time in milliseconds. Call this immediately after a
/// successful API response so all tokens in the same batch share the same
/// reference time.
fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| u64::try_from(d.as_millis()).unwrap_or(0))
        .unwrap_or(0)
}

/// True if `v` represents a numeric zero.
///
/// Parser-based: any string that `f64::from_str` accepts and equals `0.0`
/// qualifies (e.g. `"0"`, `"0.0"`, `"0.00"`, `" 0 "`, `"0e0"`, `"-0"`,
/// `"+0"`). JSON numeric `0` / `0.0` also qualifies. Non-numeric strings
/// (`""`, `"null"`, garbage) are treated as non-zero so we never nullify
/// an unparseable value.
fn is_numeric_zero(v: &Value) -> bool {
    if let Some(s) = v.as_str() {
        return s.trim().parse::<f64>().map(|n| n == 0.0).unwrap_or(false);
    }
    v.as_f64().map(|n| n == 0.0).unwrap_or(false)
}

/// If the token was created within `NEW_TOKEN_THRESHOLD_MS` of `received_at_ms`,
/// replace any zero value in the unreliable tag fields with `null` — a zero at
/// this age means "not yet computed", not "genuinely zero".
///
/// `received_at_ms` should be captured immediately after the API response
/// returns so all tokens in the same batch share the same reference time.
///
/// `createdTimestamp` is expected in **milliseconds since Unix epoch**. If the
/// upstream ever starts returning seconds the age computation will wildly
/// exceed the threshold and the helper becomes a silent no-op — if that
/// happens, normalise the unit before calling this function.
///
/// Scope: only applied by callers whose responses carry `tags.*Percent` fields
/// for freshly-minted tokens — currently `fetch_token_list` and
/// `fetch_token_details`. If a future endpoint returns the same shape for new
/// tokens (e.g. `similarToken`), apply this helper there too; otherwise stale
/// "0" values can leak back in.
fn nullify_zero_tags_if_new(token: &mut Value, received_at_ms: u64) {
    if received_at_ms == 0 {
        // Clock error — skip to avoid false-positives.
        return;
    }

    let created_ms = token
        .get("createdTimestamp")
        .and_then(|v| {
            v.as_str()
                .and_then(|s| s.parse::<u64>().ok())
                .or_else(|| v.as_u64())
        })
        .unwrap_or(0);

    // `created_ms == 0` means "unknown/missing" here (memepump tokens are not
    // minted at the Unix epoch). `created_ms > received_at_ms` guards clock
    // skew — if the server's timestamp leads our wall clock the age signal is
    // unreliable, so leave tags as-is instead of nulling legitimate "0" values
    // on older tokens.
    if created_ms == 0
        || created_ms > received_at_ms
        || received_at_ms - created_ms >= NEW_TOKEN_THRESHOLD_MS
    {
        return;
    }

    if let Some(tags) = token.get_mut("tags").and_then(|t| t.as_object_mut()) {
        for field in UNRELIABLE_ZERO_TAGS {
            if tags.get(*field).is_some_and(is_numeric_zero) {
                tags.insert((*field).to_string(), Value::Null);
            }
        }
    }
}

/// Apply `nullify_zero_tags_if_new` across the response shapes we've seen
/// from the memepump endpoints: a bare array, a bare token object, or an
/// object wrapping the payload under one of `list` / `data` / `items` /
/// `signals` (matching the keys the shared test extractor probes, so a
/// backend shape change cannot silently turn the feature into a no-op
/// while tests still pass).
///
/// The wrapper key can hold either an array of tokens or a single token
/// object; both are walked.
fn apply_nullify_to_response(data: &mut Value, received_at_ms: u64) {
    if let Some(arr) = data.as_array_mut() {
        for token in arr.iter_mut() {
            nullify_zero_tags_if_new(token, received_at_ms);
        }
        return;
    }
    for key in ["list", "data", "items", "signals"] {
        if let Some(wrapped) = data.get_mut(key) {
            if let Some(arr) = wrapped.as_array_mut() {
                for token in arr.iter_mut() {
                    nullify_zero_tags_if_new(token, received_at_ms);
                }
                return;
            }
            if wrapped.is_object() {
                nullify_zero_tags_if_new(wrapped, received_at_ms);
                return;
            }
        }
    }
    if data.is_object() {
        nullify_zero_tags_if_new(data, received_at_ms);
    }
}

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
        /// Token stage: NEW, MIGRATING, or MIGRATED (default: NEW)
        #[arg(long, default_value = "NEW")]
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
            let mut client = ctx.client_async().await?;
            output::success(
                fetch_token_list(
                    &mut client,
                    MemepumpTokenListParams {
                        chain,
                        stage: Some(stage),
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
    /// Token stage: NEW, MIGRATING, or MIGRATED (default: NEW)
    pub stage: Option<String>,
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
pub async fn fetch_chains(client: &mut ApiClient) -> Result<Value> {
    client
        .get("/api/v6/dex/market/memepump/supported/chainsProtocol", &[])
        .await
}

/// GET /api/v6/dex/market/memepump/tokenList
pub async fn fetch_token_list(client: &mut ApiClient, p: MemepumpTokenListParams) -> Result<Value> {
    let chain_index = crate::chains::resolve_chain(&p.chain).to_string();
    let stage = p.stage.unwrap_or_else(|| "NEW".to_string());

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

    let mut data = client
        .get(
            "/api/v6/dex/market/memepump/tokenList",
            &[
                ("chainIndex", chain_index.as_str()),
                ("stage", &stage),
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
        .await?;
    let received_at = now_ms();

    apply_nullify_to_response(&mut data, received_at);

    Ok(data)
}

/// GET /api/v6/dex/market/memepump/tokenDetails
pub async fn fetch_token_details(
    client: &mut ApiClient,
    address: &str,
    chain_index: &str,
    wallet_address: &str,
) -> Result<Value> {
    let mut data = client
        .get(
            "/api/v6/dex/market/memepump/tokenDetails",
            &[
                ("chainIndex", chain_index),
                ("tokenContractAddress", address),
                ("walletAddress", wallet_address),
            ],
        )
        .await?;
    let received_at = now_ms();

    apply_nullify_to_response(&mut data, received_at);

    Ok(data)
}

/// GET /api/v6/dex/market/memepump/apedWallet
pub async fn fetch_aped_wallet(
    client: &mut ApiClient,
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

// ── CLI wrappers ─────────────────────────────────────────────────────

async fn memepump_chains(ctx: &Context) -> Result<()> {
    let mut client = ctx.client_async().await?;
    output::success(fetch_chains(&mut client).await?);
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
    let mut client = ctx.client_async().await?;
    output::success(
        fetch_token_details(&mut client, address, &chain_index, &wallet_address).await?,
    );
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
    let mut client = ctx.client_async().await?;
    output::success(fetch_aped_wallet(&mut client, address, &chain_index, &wallet_address).await?);
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
    let mut client = ctx.client_async().await?;
    output::success(fetch_by_address(&mut client, path, address, &chain_index).await?);
    Ok(())
}

// ─── unit tests ──────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn new_token(created_ms: u64) -> Value {
        json!({
            "createdTimestamp": created_ms.to_string(),
            "symbol": "TEST",
            "tags": {
                "bundlersPercent": "0",
                "devHoldingsPercent": "0",
                "freshWalletsPercent": "0",
                "insidersPercent": "0",
                "snipersPercent": "0",
                "suspectedPhishingWalletPercent": "0",
                "top10HoldingsPercent": "12.5",
                "totalHolders": "4"
            }
        })
    }

    /// Token created 1 s ago — all zero tag fields must become null.
    #[test]
    fn zero_tags_nullified_within_threshold() {
        let received_at = now_ms();
        let created_ms = received_at.saturating_sub(1_000);
        let mut token = new_token(created_ms);
        nullify_zero_tags_if_new(&mut token, received_at);
        for field in UNRELIABLE_ZERO_TAGS {
            assert!(
                token["tags"][field].is_null(),
                "{field} should be null for brand-new token, got: {}",
                token["tags"][field]
            );
        }
        // Non-zero and non-tag fields must be untouched
        assert_eq!(token["tags"]["top10HoldingsPercent"], "12.5");
        assert_eq!(token["tags"]["totalHolders"], "4");
        assert_eq!(token["createdTimestamp"], created_ms.to_string());
    }

    /// Token created 5 s ago — zero tag fields must NOT be nullified.
    #[test]
    fn zero_tags_not_nullified_outside_threshold() {
        let received_at = now_ms();
        let mut token = new_token(received_at.saturating_sub(5_000));
        nullify_zero_tags_if_new(&mut token, received_at);
        for field in UNRELIABLE_ZERO_TAGS {
            assert_eq!(
                token["tags"][field], "0",
                "{field} should stay '0' for older token"
            );
        }
    }

    /// Token with createdTimestamp "0" (unknown) — tags must not be touched.
    #[test]
    fn zero_tags_not_nullified_when_created_timestamp_is_zero() {
        let received_at = now_ms();
        let mut token = new_token(0);
        nullify_zero_tags_if_new(&mut token, received_at);
        for field in UNRELIABLE_ZERO_TAGS {
            assert_eq!(
                token["tags"][field], "0",
                "{field} should stay '0' when createdTimestamp is unknown"
            );
        }
    }

    /// Clock skew: createdTimestamp is ahead of received_at (e.g. server clock
    /// leads client). We cannot trust the age signal — tags must stay as-is so
    /// legitimate "0" values on older tokens aren't wrongly nullified.
    #[test]
    fn zero_tags_not_nullified_when_created_timestamp_is_in_future() {
        let received_at = now_ms();
        let mut token = new_token(received_at + 10_000);
        nullify_zero_tags_if_new(&mut token, received_at);
        for field in UNRELIABLE_ZERO_TAGS {
            assert_eq!(
                token["tags"][field], "0",
                "{field} should stay '0' under clock skew (created_ms > received_at)"
            );
        }
    }

    /// Non-zero tag values within threshold must NOT be nullified.
    #[test]
    fn non_zero_tags_not_nullified_within_threshold() {
        let received_at = now_ms();
        let created_ms = received_at.saturating_sub(500);
        let mut token = json!({
            "createdTimestamp": created_ms.to_string(),
            "symbol": "TEST",
            "tags": {
                "bundlersPercent": "5.5",
                "devHoldingsPercent": "10.0",
                "freshWalletsPercent": "0",
                "insidersPercent": "0",
                "snipersPercent": "0",
                "suspectedPhishingWalletPercent": "0",
                "totalHolders": "8"
            }
        });
        nullify_zero_tags_if_new(&mut token, received_at);
        assert_eq!(
            token["tags"]["bundlersPercent"], "5.5",
            "non-zero must be untouched"
        );
        assert_eq!(
            token["tags"]["devHoldingsPercent"], "10.0",
            "non-zero must be untouched"
        );
        assert!(
            token["tags"]["freshWalletsPercent"].is_null(),
            "zero must be null"
        );
    }

    /// `is_numeric_zero` must match every zero-ish string shape seen in the
    /// wild: plain "0", decimal "0.0", padded "0.00", leading whitespace, and
    /// raw JSON numbers. Non-numeric strings stay non-zero (never nullified).
    #[test]
    fn is_numeric_zero_handles_common_string_shapes() {
        for v in [
            json!("0"),
            json!("0.0"),
            json!("0.00"),
            json!(" 0 "),
            json!(0),
            json!(0.0),
        ] {
            assert!(is_numeric_zero(&v), "{v} should be zero");
        }
        for v in [
            json!("1"),
            json!("0.01"),
            json!(""),
            json!("null"),
            json!(1.5),
            json!(null),
        ] {
            assert!(!is_numeric_zero(&v), "{v} should NOT be zero");
        }
    }

    /// Backend sends "0.0" (not exact "0") — must still nullify.
    #[test]
    fn zero_tags_nullified_for_decimal_zero_string() {
        let received_at = now_ms();
        let created_ms = received_at.saturating_sub(500);
        let mut token = json!({
            "createdTimestamp": created_ms.to_string(),
            "tags": {
                "bundlersPercent": "0.0",
                "devHoldingsPercent": "0.00",
                "freshWalletsPercent": "0",
                "insidersPercent": "0",
                "snipersPercent": "0",
                "suspectedPhishingWalletPercent": "0"
            }
        });
        nullify_zero_tags_if_new(&mut token, received_at);
        for field in UNRELIABLE_ZERO_TAGS {
            assert!(
                token["tags"][field].is_null(),
                "{field} should be null for decimal-zero backend string"
            );
        }
    }

    /// `apply_nullify_to_response` must walk a `{ list: [...] }` wrapper as
    /// well as a bare array / bare object, so a backend shape change does not
    /// silently turn the feature into a no-op.
    #[test]
    fn apply_nullify_to_response_walks_nested_list_shape() {
        let received_at = now_ms();
        let created_ms = received_at.saturating_sub(500);
        let mut data = json!({
            "list": [new_token(created_ms)],
            "total": 1
        });
        apply_nullify_to_response(&mut data, received_at);
        for field in UNRELIABLE_ZERO_TAGS {
            assert!(
                data["list"][0]["tags"][field].is_null(),
                "{field} should be null inside {{ list: [...] }} wrapper"
            );
        }
    }

    /// Bare-array response shape (the `fetch_token_list` default) must also
    /// be walked — regression guard for the flat-array path.
    #[test]
    fn apply_nullify_to_response_walks_bare_array() {
        let received_at = now_ms();
        let created_ms = received_at.saturating_sub(500);
        let mut data = Value::Array(vec![new_token(created_ms)]);
        apply_nullify_to_response(&mut data, received_at);
        for field in UNRELIABLE_ZERO_TAGS {
            assert!(
                data[0]["tags"][field].is_null(),
                "{field} should be null inside bare array"
            );
        }
    }

    /// Wrapper key carrying a *single-object* token (not an array) — this is
    /// the `{ data: { ...token... } }` shape `fetch_token_details` can return
    /// under some backend modes. Before the fix the helper fell through to
    /// the bare-object branch on the outer wrapper and silently no-op'd.
    #[test]
    fn apply_nullify_to_response_walks_single_object_under_wrapper_key() {
        let received_at = now_ms();
        let created_ms = received_at.saturating_sub(500);
        let mut data = json!({ "data": new_token(created_ms) });
        apply_nullify_to_response(&mut data, received_at);
        for field in UNRELIABLE_ZERO_TAGS {
            assert!(
                data["data"]["tags"][field].is_null(),
                "{field} should be null inside {{ data: {{ ...token... }} }} wrapper"
            );
        }
    }

    /// `items` is one of the wrapper keys the test extractors probe — ensure
    /// `apply_nullify_to_response` does the same so shape drift cannot silently
    /// turn the feature off while tests still pass.
    #[test]
    fn apply_nullify_to_response_walks_items_wrapper() {
        let received_at = now_ms();
        let created_ms = received_at.saturating_sub(500);
        let mut data = json!({ "items": [new_token(created_ms)] });
        apply_nullify_to_response(&mut data, received_at);
        for field in UNRELIABLE_ZERO_TAGS {
            assert!(
                data["items"][0]["tags"][field].is_null(),
                "{field} should be null inside {{ items: [...] }} wrapper"
            );
        }
    }

    /// `signals` wrapper is new in this commit — added to keep prod key
    /// coverage in lockstep with `common::extract_items` (which already
    /// probes this key for signal-list tests).
    #[test]
    fn apply_nullify_to_response_walks_signals_wrapper() {
        let received_at = now_ms();
        let created_ms = received_at.saturating_sub(500);
        let mut data = json!({ "signals": [new_token(created_ms)] });
        apply_nullify_to_response(&mut data, received_at);
        for field in UNRELIABLE_ZERO_TAGS {
            assert!(
                data["signals"][0]["tags"][field].is_null(),
                "{field} should be null inside {{ signals: [...] }} wrapper"
            );
        }
    }
}
