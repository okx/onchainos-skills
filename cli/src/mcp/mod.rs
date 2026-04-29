use anyhow::Result;
use rmcp::handler::server::router::tool::ToolRouter;
use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::ServerInfo;
use rmcp::transport::io::stdio;
use rmcp::{tool, tool_handler, tool_router, ServerHandler, ServiceExt};
use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::Value;

use std::sync::Arc;
use tokio::sync::Mutex;

use crate::client::ApiClient;
use crate::commands::{
    cross_chain, defi, gateway, leaderboard, market, memepump, portfolio, signal, swap, token,
    tracker, workflows,
};

// ── DeFi ──────────────────────────────────────────────────────────────

#[derive(Deserialize, JsonSchema)]
struct DefiListParams {
    /// Page number (min 1, page size fixed at 20)
    page_num: Option<u32>,
}

#[derive(Deserialize, JsonSchema)]
struct DefiSearchParams {
    /// Comma-separated token keywords (e.g. "USDC,ETH"). At least one of token or platform is required
    token: Option<String>,
    /// Comma-separated platform keywords (e.g. "Aave,Compound")
    platform: Option<String>,
    /// Chain name (e.g. "ethereum", "avalanche")
    chain: Option<String>,
    /// Product group: SINGLE_EARN (default), DEX_POOL, LENDING
    product_group: Option<String>,
    /// Page number (min 1)
    page_num: Option<u32>,
}

#[derive(Deserialize, JsonSchema)]
struct DefiDetailParams {
    /// Investment ID from search results
    investment_id: String,
}

#[derive(Deserialize, JsonSchema)]
struct DefiPrepareParams {
    /// Investment ID from search results
    investment_id: String,
}

#[derive(Deserialize, JsonSchema)]
struct DefiRateChartParams {
    /// Investment ID
    investment_id: String,
    /// Time range: DAY (V3 only), WEEK (default), MONTH, SEASON, YEAR
    time_range: Option<String>,
}

#[derive(Deserialize, JsonSchema)]
struct DefiTvlChartParams {
    /// Investment ID
    investment_id: String,
    /// Time range: DAY (V3 only), WEEK (default), MONTH, SEASON, YEAR
    time_range: Option<String>,
}

#[derive(Deserialize, JsonSchema)]
struct DefiDepthPriceChartParams {
    /// Investment ID (V3 Pool only)
    investment_id: String,
    /// Chart type: DEPTH (default) or PRICE
    chart_type: Option<String>,
    /// Time range (only for PRICE mode): DAY (default), WEEK. Ignored in DEPTH mode
    time_range: Option<String>,
}

#[derive(Deserialize, JsonSchema)]
struct DefiEnterParams {
    /// Investment ID from search results
    investment_id: String,
    /// User wallet address
    address: String,
    /// User input tokens as JSON array. coinAmount MUST be minimal units (integer), tokenPrecision REQUIRED.
    /// Convert: userAmount × 10^tokenPrecision (e.g. 0.1 USDT with precision=6 → coinAmount="100000").
    /// Get tokenPrecision from defi_prepare → investWithTokenList[].tokenPrecision.
    /// Example: '[{"tokenAddress":"0x...","chainIndex":"1","coinAmount":"100000","tokenPrecision":"6"}]'
    user_input: String,
    /// Slippage tolerance (default "0.01" = 1%)
    slippage: Option<String>,
    /// Token ID for V3 Pool positions (required for V3 add liquidity to existing position)
    token_id: Option<String>,
    /// Lower tick for V3 Pool new position
    tick_lower: Option<i64>,
    /// Upper tick for V3 Pool new position
    tick_upper: Option<i64>,
}

#[derive(Deserialize, JsonSchema)]
struct DefiExitParams {
    /// Investment product ID (investmentId from defi_position_detail)
    product_id: String,
    /// Chain name (e.g. "ethereum", "bsc", "solana", "avax")
    chain: String,
    /// User wallet address
    address: String,
    /// Redemption ratio: "1"=full exit (100%), "0.5"=50%. Required for V3 Pool exits.
    redeem_ratio: Option<String>,
    /// V3 Pool NFT tokenId (required ONLY for V3 Pool exits)
    token_id: Option<String>,
    /// Slippage tolerance (default "0.01" = 1%)
    slippage: Option<String>,
    /// User input tokens as JSON array. coinAmount MUST be minimal units (integer), tokenPrecision REQUIRED.
    /// Convert: userAmount × 10^tokenPrecision. Get tokenPrecision from defi_position_detail → assetsTokenList[].tokenPrecision.
    /// Example: '[{"tokenAddress":"<underlying>","chainIndex":"<id>","coinAmount":"100000","tokenPrecision":"6"}]'
    /// tokenAddress: underlying token (NOT aToken/receipt token).
    /// Always pass user_input when token info is available — do not default to redeem_ratio.
    user_input: Option<String>,
    /// Single-token shorthand: LP token address (alternative to user_input)
    token_address: Option<String>,
    /// LP token symbol (used with token_address)
    token_symbol: Option<String>,
    /// Amount to redeem (used with token_address)
    amount: Option<String>,
    /// LP token decimals (used with token_address)
    token_precision: Option<u32>,
}

#[derive(Deserialize, JsonSchema)]
struct DefiClaimParams {
    /// User wallet address
    address: String,
    /// Chain name (e.g. "ethereum", "avalanche")
    chain: String,
    /// Reward type — must be one of: REWARD_PLATFORM, REWARD_INVESTMENT, REWARD_OKX_BONUS, REWARD_MERKLE_BONUS, V3_FEE, UNLOCKED_PRINCIPAL
    reward_type: String,
    /// Product ID / investmentId
    product_id: Option<String>,
    /// Protocol platform ID / analysisPlatformId
    platform_id: Option<String>,
    /// V3 Pool NFT tokenId (required for V3_FEE)
    token_id: Option<String>,
    /// Principal order index (for UNLOCKED_PRINCIPAL)
    principal_index: Option<String>,
    /// Expected output token list as JSON array
    expect_output_list: Option<String>,
}

#[derive(Deserialize, JsonSchema)]
struct DefiPositionsParams {
    /// User wallet address
    address: String,
    /// Chains to query, comma-separated (e.g. "ethereum,bsc,solana")
    chains: String,
}

#[derive(Deserialize, JsonSchema)]
struct DefiPositionDetailParams {
    /// User wallet address
    address: String,
    /// Chain name (e.g. "ethereum", "avalanche")
    chain: String,
    /// Protocol platform ID (analysisPlatformId from positions results)
    platform_id: String,
}

#[derive(Deserialize, JsonSchema)]
struct DefiCalculateEntryParams {
    /// Investment ID from search results
    id: String,
    /// User wallet address
    address: String,
    /// Input token contract address
    input_token: String,
    /// Input amount (human-readable, e.g. "100")
    input_amount: String,
    /// Token decimals
    token_decimal: String,
    /// Lower tick for V3 Pool position
    tick_lower: Option<i64>,
    /// Upper tick for V3 Pool position
    tick_upper: Option<i64>,
}

#[derive(Deserialize, JsonSchema)]
struct DefiInvestParams {
    /// Investment ID from defi_search or defi_detail
    investment_id: String,
    /// User wallet address
    address: String,
    /// Token symbol or contract address (e.g. "USDC" or "0xa0b8...")
    token: String,
    /// Amount in minimal units (integer). Convert: userAmount × 10^tokenPrecision. Example: 0.1 USDC (precision=6) → "100000"
    amount: String,
    /// Second token symbol or address (V3 dual-token entry). Auto-detected from pool if only amount2 is provided.
    token2: Option<String>,
    /// Second token amount in minimal units (V3 dual-token entry). CLI rebalances to pool ratio and returns surplus info.
    amount2: Option<String>,
    /// Chain name (optional, auto-resolved from product detail if omitted)
    chain: Option<String>,
    /// Slippage tolerance (default "0.01" = 1%)
    slippage: Option<String>,
    /// V3 Pool: NFT tokenId for adding to existing position (no tick/range needed)
    token_id: Option<String>,
    /// V3 Pool: lower tick for new position (alternative to range)
    tick_lower: Option<i64>,
    /// V3 Pool: upper tick for new position (alternative to range)
    tick_upper: Option<i64>,
    /// V3 Pool: price range percentage (e.g. 5 for ±5%). Required for V3 new position if tick_lower/tick_upper not provided.
    range: Option<f64>,
}

#[derive(Deserialize, JsonSchema)]
struct DefiWithdrawParams {
    /// Investment product ID
    investment_id: String,
    /// User wallet address
    address: String,
    /// Chain name (e.g. "ethereum", "polygon")
    chain: String,
    /// Redemption ratio: "1"=100% full exit, "0.5"=50%
    ratio: Option<String>,
    /// V3 Pool NFT tokenId
    token_id: Option<String>,
    /// Slippage tolerance (default "0.01")
    slippage: Option<String>,
    /// Partial exit amount in minimal units (integer). Convert: userAmount × 10^tokenPrecision. Get tokenPrecision from defi_position_detail.
    amount: Option<String>,
    /// Platform ID (analysisPlatformId) for auto-fetching position info
    platform_id: Option<String>,
}

#[derive(Deserialize, JsonSchema)]
struct DefiCollectParams {
    /// User wallet address
    address: String,
    /// Chain name
    chain: String,
    /// Reward type: REWARD_PLATFORM, REWARD_INVESTMENT, V3_FEE, REWARD_OKX_BONUS, REWARD_MERKLE_BONUS, UNLOCKED_PRINCIPAL
    reward_type: String,
    /// Investment product ID
    investment_id: Option<String>,
    /// Platform ID (analysisPlatformId)
    platform_id: Option<String>,
    /// V3 Pool NFT tokenId (for V3_FEE)
    token_id: Option<String>,
    /// Principal order index (for UNLOCKED_PRINCIPAL)
    principal_index: Option<String>,
}

// ── Gas Station ──────────────────────────────────────────────────────

#[derive(Deserialize, JsonSchema)]
struct GasStationUpdateDefaultTokenParams {
    /// Chain name or ID (e.g. "ethereum", "1")
    chain: String,
    /// Gas token contract address to set as default (e.g. USDT/USDC/USDG address)
    gas_token_address: String,
}

#[derive(Deserialize, JsonSchema)]
struct GasStationDisableParams {
    /// Chain name or ID (e.g. "ethereum", "1")
    chain: String,
}

// ── Token ──────────────────────────────────────────────────────────────
#[derive(Deserialize, JsonSchema)]
struct TokenSearchParams {
    /// Token name, symbol, or contract address (e.g. "ETH", "USDC", "0x...")
    query: String,
    /// Comma-separated chain names, e.g. "ethereum,solana" (optional, searches all)
    chains: Option<String>,
    /// Number of results per page (default: 20, max: 100). Use cursor for pagination.
    limit: Option<String>,
    /// Pagination cursor. Pass the cursor value from the last item of the previous response to fetch the next page. Omit for first page.
    cursor: Option<String>,
}

#[derive(Deserialize, JsonSchema)]
struct TokenAddressParams {
    /// Token contract address
    address: String,
    /// Chain name, e.g. "ethereum", "solana" (optional, defaults to ethereum)
    chain: Option<String>,
}

// ── Market ─────────────────────────────────────────────────────────────
#[derive(Deserialize, JsonSchema)]
struct MarketTokenParams {
    /// Token contract address
    address: String,
    /// Chain name (optional, defaults to ethereum)
    chain: Option<String>,
}

#[derive(Deserialize, JsonSchema)]
struct MarketPricesParams {
    /// Comma-separated "chain:address" pairs, e.g. "ethereum:0xabc...,solana:1111..."
    tokens: String,
    /// Default chain if not specified per token (optional)
    chain: Option<String>,
}

#[derive(Deserialize, JsonSchema)]
struct MarketKlineParams {
    /// Token contract address
    address: String,
    /// Chain name (optional)
    chain: Option<String>,
    /// Bar size: 1s, 1m, 5m, 15m, 30m, 1H (default), 4H, 1D, 1W
    bar: Option<String>,
    /// Number of data points, max 299 (default 100)
    limit: Option<u32>,
}

#[derive(Deserialize, JsonSchema)]
struct TokenTradesParams {
    /// Token contract address
    address: String,
    /// Chain name (optional)
    chain: Option<String>,
    /// Number of trades, max 500 (default 100)
    limit: Option<u32>,
    /// Filter by trader tag: 1=KOL, 2=Developer, 3=Smart Money, 4=Whale, 5=Fresh Wallet, 6=Insider, 7=Sniper, 8=Suspicious Phishing, 9=Bundler
    tag_filter: Option<String>,
    /// Filter by wallet address (comma-separated, max 10)
    wallet_filter: Option<String>,
}

#[derive(Deserialize, JsonSchema)]
struct TokenTagAddressParams {
    /// Token contract address
    address: String,
    /// Chain name (optional, defaults to ethereum)
    chain: Option<String>,
    /// Filter by tag: 1=KOL, 2=Developer, 3=Smart Money, 4=Whale, 5=Fresh Wallet, 6=Insider, 7=Sniper, 8=Suspicious Phishing, 9=Bundler
    tag_filter: Option<u8>,
    /// Number of results per page (default: 20, max: 100). Use cursor for pagination.
    limit: Option<String>,
    /// Pagination cursor. Pass the cursor value from the last item of the previous response to fetch the next page. Omit for first page.
    cursor: Option<String>,
}

// ── Memepump ──────────────────────────────────────────────────────────
#[derive(Deserialize, JsonSchema)]
struct MemepumpWalletParams {
    /// Token contract address
    address: String,
    /// Chain name (optional, defaults to solana)
    chain: Option<String>,
    /// Wallet address for position data (optional)
    wallet_address: Option<String>,
}

// ── Portfolio PnL ─────────────────────────────────────────────────────
#[derive(Deserialize, JsonSchema)]
struct PortfolioPnlOverviewParams {
    /// Wallet address
    address: String,
    /// Chain name (e.g. ethereum, solana)
    chain: String,
    /// Time frame: 1=1D, 2=3D, 3=7D, 4=1M, 5=3M (default: 4 = 1M)
    time_frame: Option<String>,
}

#[derive(Deserialize, JsonSchema)]
struct PortfolioPnlDexHistoryParams {
    /// Wallet address
    address: String,
    /// Chain name (e.g. ethereum, solana)
    chain: String,
    /// Start timestamp (milliseconds)
    begin: String,
    /// End timestamp (milliseconds)
    end: String,
    /// Page size (1-100, default 20)
    limit: Option<String>,
    /// Pagination cursor from previous response
    cursor: Option<String>,
    /// Filter by token contract address
    token: Option<String>,
    /// Transaction type: 1=BUY, 2=SELL, 3=Transfer In, 4=Transfer Out (comma-separated)
    tx_type: Option<String>,
}

#[derive(Deserialize, JsonSchema)]
struct PortfolioPnlRecentPnlParams {
    /// Wallet address
    address: String,
    /// Chain name (e.g. ethereum, solana)
    chain: String,
    /// Page size (1-100, default 20)
    limit: Option<String>,
    /// Pagination cursor from previous response
    cursor: Option<String>,
}

#[derive(Deserialize, JsonSchema)]
struct PortfolioPnlTokenPnlParams {
    /// Wallet address
    address: String,
    /// Chain name (e.g. ethereum, solana)
    chain: String,
    /// Token contract address
    token: String,
}

#[derive(Deserialize, JsonSchema)]
struct MarketSignalListParams {
    /// Chain name, e.g. "ethereum", "solana" (required)
    chain: String,
    /// Wallet type: 1=Smart Money, 2=KOL, 3=Whales (comma-separated, optional)
    wallet_type: Option<String>,
    /// Min transaction amount in USD (optional)
    min_amount_usd: Option<String>,
    /// Max transaction amount in USD (optional)
    max_amount_usd: Option<String>,
    /// Min triggering wallet count (optional)
    min_address_count: Option<String>,
    /// Max triggering wallet count (optional)
    max_address_count: Option<String>,
    /// Filter for a specific token address (optional)
    token_address: Option<String>,
    /// Min token market cap in USD (optional)
    min_market_cap_usd: Option<String>,
    /// Max token market cap in USD (optional)
    max_market_cap_usd: Option<String>,
    /// Min token liquidity in USD (optional)
    min_liquidity_usd: Option<String>,
    /// Max token liquidity in USD (optional)
    max_liquidity_usd: Option<String>,
    /// Number of results per page (default: 20, max: 100). Use cursor for pagination.
    limit: Option<String>,
    /// Pagination cursor. Pass the cursor value from the last item of the previous response to fetch the next page. Omit for first page.
    cursor: Option<String>,
}

#[derive(Deserialize, JsonSchema)]
struct AddressTrackerActivitiesParams {
    /// Tracker type: smart_money (or 1), kol (or 2), multi_address (or 3)
    tracker_type: String,
    /// Wallet addresses, comma-separated (required when tracker_type=multi_address, max 20)
    wallet_address: Option<String>,
    /// Trade type: 0=all (default), 1=buy, 2=sell
    trade_type: Option<String>,
    /// Chain filter (e.g. ethereum, solana). Omit for all chains
    chain: Option<String>,
    /// Minimum trade volume in USD
    min_volume: Option<String>,
    /// Maximum trade volume in USD
    max_volume: Option<String>,
    /// Minimum number of holding addresses
    min_holders: Option<String>,
    /// Minimum market cap in USD
    min_market_cap: Option<String>,
    /// Maximum market cap in USD
    max_market_cap: Option<String>,
    /// Minimum liquidity in USD
    min_liquidity: Option<String>,
    /// Maximum liquidity in USD
    max_liquidity: Option<String>,
}

// ── Swap ───────────────────────────────────────────────────────────────
#[derive(Deserialize, JsonSchema)]
struct SwapQuoteParams {
    /// Source token contract address
    from: String,
    /// Destination token contract address
    to: String,
    /// Amount in minimal units (wei/lamports)
    amount: String,
    /// Chain name, e.g. "ethereum", "solana"
    chain: String,
    /// Swap mode: exactIn (default) or exactOut
    swap_mode: Option<String>,
}

#[derive(Deserialize, JsonSchema)]
struct SwapSwapParams {
    /// Source token contract address
    from: String,
    /// Destination token contract address
    to: String,
    /// Amount in minimal units
    amount: String,
    /// Chain name
    chain: String,
    /// Slippage tolerance in percent, e.g. "1" for 1%. Omit to use autoSlippage.
    slippage: Option<String>,
    /// User wallet address
    wallet: String,
    /// Gas priority: slow, average (default), fast
    gas_level: Option<String>,
    /// Swap mode: exactIn (default) or exactOut
    swap_mode: Option<String>,
    /// Jito tips in SOL for Solana MEV protection (range: 0.0000000001–2)
    tips: Option<String>,
    /// Max auto slippage percent cap when autoSlippage is enabled (e.g. "0.5")
    max_auto_slippage: Option<String>,
}

#[derive(Deserialize, JsonSchema)]
struct SwapApproveParams {
    /// Token contract address to approve
    token: String,
    /// Approval amount in minimal units
    amount: String,
    /// Chain name
    chain: String,
}

#[derive(Deserialize, JsonSchema)]
struct ChainParam {
    /// Chain name, e.g. "ethereum", "solana", "xlayer"
    chain: String,
}

// ── Portfolio ──────────────────────────────────────────────────────────
#[derive(Deserialize, JsonSchema)]
struct PortfolioTotalValueParams {
    /// Wallet address
    address: String,
    /// Comma-separated chain names, e.g. "ethereum,solana,xlayer"
    chains: String,
    /// Asset type: 0=all (default), 1=tokens only, 2=DeFi only
    asset_type: Option<String>,
    /// Exclude risky tokens: "true"=exclude (default), "false"=include. Only ETH/BSC/SOL/BASE
    exclude_risk: Option<String>,
}

#[derive(Deserialize, JsonSchema)]
struct PortfolioAllBalancesParams {
    /// Wallet address
    address: String,
    /// Comma-separated chain names, e.g. "ethereum,solana"
    chains: String,
    /// Exclude risky tokens: 0=filter out (default), 1=include
    exclude_risk: Option<String>,
    /// Token filter level: 0=default (filters risk/custom/passive tokens), 1=return all tokens.
    /// Use 1 when you need the full token list including risk tokens (e.g. for security scanning).
    filter: Option<String>,
}

#[derive(Deserialize, JsonSchema)]
struct PortfolioTokenBalancesParams {
    /// Wallet address
    address: String,
    /// Comma-separated "chainName:tokenAddress" pairs, e.g. "ethereum:0xabc...,xlayer:"
    /// Use empty address for native token (e.g. "xlayer:")
    tokens: String,
    /// Exclude risky tokens: 0=filter out (default), 1=include
    exclude_risk: Option<String>,
}

// ── Leaderboard ────────────────────────────────────────────────────────
#[derive(Deserialize, JsonSchema)]
struct LeaderboardListParams {
    /// Chain name, e.g. "ethereum", "solana" (required)
    chain: String,
    /// Time frame (required): 1=1D, 2=3D, 3=7D, 4=1M, 5=3M
    time_frame: String,
    /// Sort by (required): 1=PnL, 2=Win Rate, 3=Tx count, 4=Volume, 5=ROI
    sort_by: String,
    /// Wallet type (optional, single select): smartMoney, influencer, sniper, dev, fresh, pump
    wallet_type: Option<String>,
    /// Minimum realized PnL in USD (optional)
    min_realized_pnl_usd: Option<String>,
    /// Maximum realized PnL in USD (optional)
    max_realized_pnl_usd: Option<String>,
    /// Minimum win rate percentage 0-100 (optional)
    min_win_rate_percent: Option<String>,
    /// Maximum win rate percentage 0-100 (optional)
    max_win_rate_percent: Option<String>,
    /// Minimum number of transactions (optional)
    min_txs: Option<String>,
    /// Maximum number of transactions (optional)
    max_txs: Option<String>,
    /// Minimum transaction volume in USD (optional)
    min_tx_volume: Option<String>,
    /// Maximum transaction volume in USD (optional)
    max_tx_volume: Option<String>,
}

// ── Token Cluster ───────────────────────────────────────────────────────
#[derive(Deserialize, JsonSchema)]
struct ClusterAddressParams {
    /// Token contract address
    address: String,
    /// Chain name (optional, defaults to ethereum)
    chain: Option<String>,
}

#[derive(Deserialize, JsonSchema)]
struct ClusterTopHoldersParams {
    /// Token contract address
    address: String,
    /// Chain name (optional, defaults to ethereum)
    chain: Option<String>,
    /// Holder rank tier: 1 = top 10, 2 = top 50, 3 = top 100
    range_filter: String,
}

// ── Gateway ────────────────────────────────────────────────────────────
#[derive(Deserialize, JsonSchema)]
struct GatewayGasLimitParams {
    /// Sender address
    from: String,
    /// Recipient / contract address
    to: String,
    /// Transfer value in minimal units (default "0")
    amount: Option<String>,
    /// Encoded calldata hex for contract interactions (optional)
    data: Option<String>,
    /// Chain name
    chain: String,
}

#[derive(Deserialize, JsonSchema)]
struct GatewaySimulateParams {
    /// Sender address
    from: String,
    /// Recipient / contract address
    to: String,
    /// Transfer value in minimal units (default "0")
    amount: Option<String>,
    /// Encoded calldata hex
    data: String,
    /// Chain name
    chain: String,
}

#[derive(Deserialize, JsonSchema)]
struct GatewayBroadcastParams {
    /// Fully signed transaction (hex for EVM, base58 for Solana)
    signed_tx: String,
    /// Sender wallet address
    address: String,
    /// Chain name
    chain: String,
    /// Enable MEV protection (supported on Base and other EVM chains)
    #[serde(default)]
    mev_protection: bool,
}

#[derive(Deserialize, JsonSchema)]
struct GatewayOrdersParams {
    /// Wallet address
    address: String,
    /// Chain name
    chain: String,
    /// Specific order ID from broadcast response (optional)
    order_id: Option<String>,
}

// ── Workflow params ────────────────────────────────────────────────────────────

#[derive(Deserialize, JsonSchema)]
struct WorkflowTokenResearchParams {
    /// Token contract address (use this OR query)
    address: Option<String>,
    /// Token symbol or name to search (use this OR address).
    /// When provided without address, returns top 5 search results for user selection.
    /// After user selects, call again with the chosen address.
    query: Option<String>,
    /// Chain name (e.g. "solana", "ethereum"). Defaults to solana if omitted.
    chain: Option<String>,
}

#[derive(Deserialize, JsonSchema)]
struct WorkflowSmartMoneyParams {
    /// Chain name (e.g. "solana", "ethereum"). Defaults to solana.
    chain: Option<String>,
}

#[derive(Deserialize, JsonSchema)]
struct WorkflowNewTokensParams {
    /// Chain name (e.g. "solana"). Defaults to solana.
    chain: Option<String>,
    /// Launchpad stage: "MIGRATED" (default) or "MIGRATING"
    stage: Option<String>,
}

#[derive(Deserialize, JsonSchema)]
struct WorkflowWalletAnalysisParams {
    /// Wallet address to analyse
    address: String,
    /// Chain name (e.g. "solana", "ethereum"). Defaults to solana.
    chain: Option<String>,
}

#[derive(Deserialize, JsonSchema)]
struct WorkflowPortfolioParams {
    /// Wallet address
    address: String,
    /// Comma-separated chain names or indexes (e.g. "ethereum,solana"). Defaults to "1,501".
    chains: Option<String>,
}

// ── Cross-Chain ──────────────────────────────────────────────────────
#[derive(Deserialize, JsonSchema)]
struct CrossChainQuoteParams {
    /// Source token contract address or alias (e.g. "usdc", "eth", "0x...")
    from: String,
    /// Destination token contract address or alias
    to: String,
    /// Source chain name (e.g. "ethereum", "arbitrum", "base")
    from_chain: String,
    /// Destination chain name (e.g. "optimism", "solana")
    to_chain: String,
    /// Human-readable amount (e.g. "10" for 10 USDC)
    readable_amount: String,
    /// Optional destination receive address. The v6 /quote API does not consume
    /// this — it is validated against the destination chain's address family
    /// (so EVM↔Solana mismatches fail early) and then dropped. Use it only at
    /// `cross_chain_execute` time when broadcasting.
    receive_address: Option<String>,
    /// Sort preference: 0=optimal (default), 1=fastest, 2=max output
    sort: Option<String>,
}

#[derive(Deserialize, JsonSchema)]
struct CrossChainStatusParams {
    /// Source chain transaction hash returned by cross-chain execute. Provide this
    /// OR `order_id` (mutually exclusive).
    tx_hash: Option<String>,
    /// Order id from a prior cross_chain_execute (e.g. swapOrderId,
    /// approveOrderId). Use this OR `tx_hash`. Resolved internally to a tx hash
    /// via the authenticated wallet `/order/detail` endpoint (login required).
    order_id: Option<String>,
    /// Bridge id (required by server — returns 50014 if absent). Get it from the
    /// `bridgeId` field of the prior `cross_chain_execute` / `cross_chain_quote`
    /// result, or from `cross_chain_bridges`.
    bridge_id: String,
    /// Source chain name or chainIndex (e.g. "ethereum" or "1"). Required —
    /// server returns 50014 (chainIndex) without it.
    from_chain: String,
}

#[derive(Deserialize, JsonSchema)]
struct CrossChainBridgesParams {
    /// Source chain (independently optional). See tool description for the
    /// four combinations of from_chain / to_chain.
    from_chain: Option<String>,
    /// Destination chain (independently optional).
    to_chain: Option<String>,
}

#[derive(Clone)]
pub struct McpServer {
    tool_router: ToolRouter<Self>,
    client: Arc<Mutex<ApiClient>>,
}

impl McpServer {
    pub fn new(base_url_override: Option<&str>) -> Result<Self> {
        Ok(Self {
            tool_router: Self::tool_router(),
            client: Arc::new(Mutex::new(ApiClient::new(base_url_override)?)),
        })
    }
}

#[tool_handler]
impl ServerHandler for McpServer {
    fn get_info(&self) -> ServerInfo {
        let caps = rmcp::model::ServerCapabilities::builder()
            .enable_tools()
            .build();
        ServerInfo::new(caps).with_server_info(rmcp::model::Implementation::new(
            "onchainos",
            env!("CARGO_PKG_VERSION"),
        ))
    }
}

fn ok(data: Value) -> Result<String, String> {
    let events = crate::payment_notify::drain_events();
    let payload = if events.is_empty() {
        data
    } else {
        serde_json::json!({ "data": data, "notifications": events })
    };
    Ok(serde_json::to_string_pretty(&payload)
        .unwrap_or_else(|e| format!("failed to serialize response: {e}")))
}

fn err(e: anyhow::Error) -> Result<String, String> {
    // Always drain so events don't leak into the next tool call.
    let events = crate::payment_notify::drain_events();

    // `CliConfirming` carries structured `message` / `next` fields; surface
    // them as a JSON payload so the agent can parse them (matches the CLI
    // `output::confirming` shape). Empty strings are omitted so first-charge
    // confirmations (which rely on the notifications array for semantics)
    // stay tidy.
    if let Some(c) = e.downcast_ref::<crate::output::CliConfirming>() {
        let mut payload = serde_json::json!({ "confirming": true });
        if !c.message.is_empty() {
            payload["message"] = serde_json::Value::String(c.message.clone());
        }
        if !c.next.is_empty() {
            payload["next"] = serde_json::Value::String(c.next.clone());
        }
        if !events.is_empty() {
            payload["notifications"] = serde_json::Value::Array(events);
        }
        return Err(serde_json::to_string(&payload).unwrap_or_else(|_| c.message.clone()));
    }

    let base = format!("{e:#}");
    if events.is_empty() {
        Err(base)
    } else {
        let payload = serde_json::json!({ "error": base, "notifications": events });
        Err(serde_json::to_string(&payload).unwrap_or(base))
    }
}

#[tool_router]
impl McpServer {
    #[tool(
        name = "token_search",
        description = "Search tokens by name/symbol/address across chains. Default limit is 20 to prevent token overflow. Use cursor for pagination."
    )]
    async fn token_search(
        &self,
        Parameters(p): Parameters<TokenSearchParams>,
    ) -> Result<String, String> {
        let chains = p.chains.as_deref().unwrap_or("1,501");
        match token::fetch_search(
            &mut *self.client.lock().await,
            &p.query,
            chains,
            p.limit.as_deref(),
            p.cursor.as_deref(),
        )
        .await
        {
            Ok(data) => ok(data),
            Err(e) => err(e),
        }
    }

    #[tool(
        name = "token_info",
        description = "Get token metadata: name, symbol, decimals, logo"
    )]
    async fn token_info(
        &self,
        Parameters(p): Parameters<TokenAddressParams>,
    ) -> Result<String, String> {
        let chain_index = p
            .chain
            .as_deref()
            .map(crate::chains::resolve_chain)
            .unwrap_or_else(|| crate::chains::resolve_chain("ethereum").to_string());
        match token::fetch_info(&mut *self.client.lock().await, &p.address, &chain_index).await {
            Ok(data) => ok(data),
            Err(e) => err(e),
        }
    }

    #[tool(
        name = "token_holders",
        description = "Get token holder distribution. Default limit is 20 to prevent token overflow. Use cursor for pagination."
    )]
    async fn token_holders(
        &self,
        Parameters(p): Parameters<TokenTagAddressParams>,
    ) -> Result<String, String> {
        let chain_index = p
            .chain
            .as_deref()
            .map(crate::chains::resolve_chain)
            .unwrap_or_else(|| crate::chains::resolve_chain("ethereum").to_string());
        match token::fetch_holders(
            &mut *self.client.lock().await,
            &p.address,
            &chain_index,
            p.tag_filter,
            p.limit.as_deref(),
            p.cursor.as_deref(),
        )
        .await
        {
            Ok(data) => ok(data),
            Err(e) => err(e),
        }
    }

    #[tool(
        name = "token_price_info",
        description = "Get token price info: market cap, liquidity, 24h change, volume"
    )]
    async fn token_price_info(
        &self,
        Parameters(p): Parameters<TokenAddressParams>,
    ) -> Result<String, String> {
        let chain_index = p
            .chain
            .as_deref()
            .map(crate::chains::resolve_chain)
            .unwrap_or_else(|| crate::chains::resolve_chain("ethereum").to_string());
        match token::fetch_price_info(&mut *self.client.lock().await, &p.address, &chain_index)
            .await
        {
            Ok(data) => ok(data),
            Err(e) => err(e),
        }
    }

    #[tool(
        name = "market_price",
        description = "Get current price for a token by contract address"
    )]
    async fn market_price(
        &self,
        Parameters(p): Parameters<MarketTokenParams>,
    ) -> Result<String, String> {
        let chain_index = p
            .chain
            .as_deref()
            .map(crate::chains::resolve_chain)
            .unwrap_or_else(|| crate::chains::resolve_chain("ethereum").to_string());
        match market::fetch_price(&mut *self.client.lock().await, &p.address, &chain_index).await {
            Ok(data) => ok(data),
            Err(e) => err(e),
        }
    }

    #[tool(
        name = "market_prices",
        description = "Batch price query for multiple tokens"
    )]
    async fn market_prices(
        &self,
        Parameters(p): Parameters<MarketPricesParams>,
    ) -> Result<String, String> {
        let default_chain = p
            .chain
            .as_deref()
            .map(crate::chains::resolve_chain)
            .unwrap_or_else(|| crate::chains::resolve_chain("ethereum").to_string());
        match market::fetch_prices(&mut *self.client.lock().await, &p.tokens, &default_chain).await
        {
            Ok(data) => ok(data),
            Err(e) => err(e),
        }
    }

    #[tool(
        name = "market_kline",
        description = "Get candlestick / K-line data for a token"
    )]
    async fn market_kline(
        &self,
        Parameters(p): Parameters<MarketKlineParams>,
    ) -> Result<String, String> {
        let chain_index = p
            .chain
            .as_deref()
            .map(crate::chains::resolve_chain)
            .unwrap_or_else(|| crate::chains::resolve_chain("ethereum").to_string());
        let bar = p.bar.as_deref().unwrap_or("1H");
        let limit = p.limit.unwrap_or(100);
        match market::fetch_kline(
            &mut *self.client.lock().await,
            &p.address,
            &chain_index,
            bar,
            limit,
        )
        .await
        {
            Ok(data) => ok(data),
            Err(e) => err(e),
        }
    }

    #[tool(
        name = "token_trades",
        description = "Get token trade history on DEX, with optional tag and wallet filters"
    )]
    async fn token_trades(
        &self,
        Parameters(p): Parameters<TokenTradesParams>,
    ) -> Result<String, String> {
        let chain_index = p
            .chain
            .as_deref()
            .map(crate::chains::resolve_chain)
            .unwrap_or_else(|| crate::chains::resolve_chain("ethereum").to_string());
        let limit = p.limit.unwrap_or(100);
        match token::fetch_token_trades(
            &mut *self.client.lock().await,
            &p.address,
            &chain_index,
            limit,
            p.tag_filter.as_deref(),
            p.wallet_filter.as_deref(),
        )
        .await
        {
            Ok(data) => ok(data),
            Err(e) => err(e),
        }
    }

    #[tool(
        name = "market_index",
        description = "Get aggregated index price for a token"
    )]
    async fn market_index(
        &self,
        Parameters(p): Parameters<MarketTokenParams>,
    ) -> Result<String, String> {
        let chain_index = p
            .chain
            .as_deref()
            .map(crate::chains::resolve_chain)
            .unwrap_or_else(|| crate::chains::resolve_chain("ethereum").to_string());
        match market::fetch_index(&mut *self.client.lock().await, &p.address, &chain_index).await {
            Ok(data) => ok(data),
            Err(e) => err(e),
        }
    }

    #[tool(
        name = "signal_chains",
        description = "Get chains supported for smart money / KOL / whale signals"
    )]
    async fn signal_chains(&self) -> Result<String, String> {
        match signal::fetch_chains(&mut *self.client.lock().await).await {
            Ok(data) => ok(data),
            Err(e) => err(e),
        }
    }

    #[tool(
        name = "signal_list",
        description = "Get smart money / KOL / whale signal list for a chain. Default limit is 20 to prevent token overflow. Use cursor for pagination."
    )]
    async fn signal_list(
        &self,
        Parameters(p): Parameters<MarketSignalListParams>,
    ) -> Result<String, String> {
        let chain_index = crate::chains::resolve_chain(&p.chain);
        match signal::fetch_list(
            &mut *self.client.lock().await,
            &chain_index,
            p.wallet_type,
            p.min_amount_usd,
            p.max_amount_usd,
            p.min_address_count,
            p.max_address_count,
            p.token_address,
            p.min_market_cap_usd,
            p.max_market_cap_usd,
            p.min_liquidity_usd,
            p.max_liquidity_usd,
            p.limit,
            p.cursor,
        )
        .await
        {
            Ok(data) => ok(data),
            Err(e) => err(e),
        }
    }

    #[tool(
        name = "memepump_chains",
        description = "Get supported chains and protocols for Meme Pump"
    )]
    async fn memepump_chains(&self) -> Result<String, String> {
        match memepump::fetch_chains(&mut *self.client.lock().await).await {
            Ok(data) => ok(data),
            Err(e) => err(e),
        }
    }

    #[tool(
        name = "memepump_tokens",
        description = "Get filtered Meme Pump token list"
    )]
    async fn memepump_tokens(
        &self,
        Parameters(p): Parameters<memepump::MemepumpTokenListParams>,
    ) -> Result<String, String> {
        match memepump::fetch_token_list(&mut *self.client.lock().await, p).await {
            Ok(data) => ok(data),
            Err(e) => err(e),
        }
    }

    #[tool(
        name = "memepump_token_details",
        description = "Get Meme Pump token details"
    )]
    async fn memepump_token_details(
        &self,
        Parameters(p): Parameters<MemepumpWalletParams>,
    ) -> Result<String, String> {
        let chain_index = p
            .chain
            .as_deref()
            .map(crate::chains::resolve_chain)
            .unwrap_or_else(|| "501".to_string());
        match memepump::fetch_token_details(
            &mut *self.client.lock().await,
            &p.address,
            &chain_index,
            p.wallet_address.as_deref().unwrap_or(""),
        )
        .await
        {
            Ok(data) => ok(data),
            Err(e) => err(e),
        }
    }

    #[tool(
        name = "memepump_token_dev_info",
        description = "Get Meme Pump token developer info and reputation"
    )]
    async fn memepump_token_dev_info(
        &self,
        Parameters(p): Parameters<MarketTokenParams>,
    ) -> Result<String, String> {
        let chain_index = p
            .chain
            .as_deref()
            .map(crate::chains::resolve_chain)
            .unwrap_or_else(|| "501".to_string());
        match memepump::fetch_by_address(
            &mut *self.client.lock().await,
            "/api/v6/dex/market/memepump/tokenDevInfo",
            &p.address,
            &chain_index,
        )
        .await
        {
            Ok(data) => ok(data),
            Err(e) => err(e),
        }
    }

    #[tool(
        name = "memepump_similar_tokens",
        description = "Get similar tokens for a Meme Pump token"
    )]
    async fn memepump_similar_tokens(
        &self,
        Parameters(p): Parameters<MarketTokenParams>,
    ) -> Result<String, String> {
        let chain_index = p
            .chain
            .as_deref()
            .map(crate::chains::resolve_chain)
            .unwrap_or_else(|| "501".to_string());
        match memepump::fetch_by_address(
            &mut *self.client.lock().await,
            "/api/v6/dex/market/memepump/similarToken",
            &p.address,
            &chain_index,
        )
        .await
        {
            Ok(data) => ok(data),
            Err(e) => err(e),
        }
    }

    #[tool(
        name = "memepump_token_bundle_info",
        description = "Get Meme Pump token bundle/sniper info for rug detection"
    )]
    async fn memepump_token_bundle_info(
        &self,
        Parameters(p): Parameters<MarketTokenParams>,
    ) -> Result<String, String> {
        let chain_index = p
            .chain
            .as_deref()
            .map(crate::chains::resolve_chain)
            .unwrap_or_else(|| "501".to_string());
        match memepump::fetch_by_address(
            &mut *self.client.lock().await,
            "/api/v6/dex/market/memepump/tokenBundleInfo",
            &p.address,
            &chain_index,
        )
        .await
        {
            Ok(data) => ok(data),
            Err(e) => err(e),
        }
    }

    #[tool(
        name = "memepump_aped_wallet",
        description = "Get co-invested wallet data for a Meme Pump token"
    )]
    async fn memepump_aped_wallet(
        &self,
        Parameters(p): Parameters<MemepumpWalletParams>,
    ) -> Result<String, String> {
        let chain_index = p
            .chain
            .as_deref()
            .map(crate::chains::resolve_chain)
            .unwrap_or_else(|| "501".to_string());
        match memepump::fetch_aped_wallet(
            &mut *self.client.lock().await,
            &p.address,
            &chain_index,
            p.wallet_address.as_deref().unwrap_or(""),
        )
        .await
        {
            Ok(data) => ok(data),
            Err(e) => err(e),
        }
    }

    #[tool(
        name = "swap_chains",
        description = "Get supported chains for DEX aggregator swaps"
    )]
    async fn swap_chains(&self) -> Result<String, String> {
        match swap::fetch_chains(&mut *self.client.lock().await).await {
            Ok(data) => ok(data),
            Err(e) => err(e),
        }
    }

    #[tool(
        name = "swap_quote",
        description = "Get swap quote (price estimate, no transaction)"
    )]
    async fn swap_quote(
        &self,
        Parameters(p): Parameters<SwapQuoteParams>,
    ) -> Result<String, String> {
        let chain_index = crate::chains::resolve_chain(&p.chain);
        let swap_mode = p.swap_mode.as_deref().unwrap_or("exactIn");
        match swap::fetch_quote(
            &mut *self.client.lock().await,
            &chain_index,
            &p.from,
            &p.to,
            &p.amount,
            swap_mode,
        )
        .await
        {
            Ok(data) => ok(data),
            Err(e) => err(e),
        }
    }

    #[tool(
        name = "swap_swap",
        description = "Get swap transaction data (unsigned tx for signing + broadcasting)"
    )]
    async fn swap_swap(&self, Parameters(p): Parameters<SwapSwapParams>) -> Result<String, String> {
        let chain_index = crate::chains::resolve_chain(&p.chain);
        let swap_mode = p.swap_mode.as_deref().unwrap_or("exactIn");
        let gas_level = p.gas_level.as_deref().unwrap_or("average");
        match swap::fetch_swap(
            &mut *self.client.lock().await,
            &chain_index,
            &p.from,
            &p.to,
            &p.amount,
            p.slippage.as_deref(),
            &p.wallet,
            swap_mode,
            gas_level,
            p.tips.as_deref(),
            p.max_auto_slippage.as_deref(),
        )
        .await
        {
            Ok(data) => ok(data),
            Err(e) => err(e),
        }
    }

    #[tool(
        name = "swap_approve",
        description = "Get ERC-20 approval transaction data"
    )]
    async fn swap_approve(
        &self,
        Parameters(p): Parameters<SwapApproveParams>,
    ) -> Result<String, String> {
        let chain_index = crate::chains::resolve_chain(&p.chain);
        match swap::fetch_approve(
            &mut *self.client.lock().await,
            &chain_index,
            &p.token,
            &p.amount,
        )
        .await
        {
            Ok(data) => ok(data),
            Err(e) => err(e),
        }
    }

    #[tool(
        name = "swap_liquidity",
        description = "Get available liquidity sources on a chain"
    )]
    async fn swap_liquidity(
        &self,
        Parameters(p): Parameters<ChainParam>,
    ) -> Result<String, String> {
        let chain_index = crate::chains::resolve_chain(&p.chain);
        match swap::fetch_liquidity(&mut *self.client.lock().await, &chain_index).await {
            Ok(data) => ok(data),
            Err(e) => err(e),
        }
    }

    #[tool(
        name = "portfolio_chains",
        description = "Get supported chains for wallet balance queries"
    )]
    async fn portfolio_chains(&self) -> Result<String, String> {
        match portfolio::fetch_chains(&mut *self.client.lock().await).await {
            Ok(data) => ok(data),
            Err(e) => err(e),
        }
    }

    #[tool(
        name = "portfolio_total_value",
        description = "Get total portfolio value for a wallet address"
    )]
    async fn portfolio_total_value(
        &self,
        Parameters(p): Parameters<PortfolioTotalValueParams>,
    ) -> Result<String, String> {
        match portfolio::fetch_total_value(
            &mut *self.client.lock().await,
            &p.address,
            &p.chains,
            p.asset_type.as_deref(),
            p.exclude_risk.as_deref(),
        )
        .await
        {
            Ok(data) => ok(data),
            Err(e) => err(e),
        }
    }

    #[tool(
        name = "portfolio_all_balances",
        description = "Get all token balances for a wallet address"
    )]
    async fn portfolio_all_balances(
        &self,
        Parameters(p): Parameters<PortfolioAllBalancesParams>,
    ) -> Result<String, String> {
        match portfolio::fetch_all_balances(
            &mut *self.client.lock().await,
            &p.address,
            &p.chains,
            p.exclude_risk.as_deref(),
            p.filter.as_deref(),
        )
        .await
        {
            Ok(data) => ok(data),
            Err(e) => err(e),
        }
    }

    #[tool(
        name = "portfolio_token_balances",
        description = "Get specific token balances for a wallet address"
    )]
    async fn portfolio_token_balances(
        &self,
        Parameters(p): Parameters<PortfolioTokenBalancesParams>,
    ) -> Result<String, String> {
        match portfolio::fetch_token_balances(
            &mut *self.client.lock().await,
            &p.address,
            &p.tokens,
            p.exclude_risk.as_deref(),
        )
        .await
        {
            Ok(data) => ok(data),
            Err(e) => err(e),
        }
    }

    #[tool(
        name = "gateway_chains",
        description = "Get supported chains for the on-chain gateway"
    )]
    async fn gateway_chains(&self) -> Result<String, String> {
        match gateway::fetch_chains(&mut *self.client.lock().await).await {
            Ok(data) => ok(data),
            Err(e) => err(e),
        }
    }

    #[tool(
        name = "gateway_gas",
        description = "Get current gas prices for a chain"
    )]
    async fn gateway_gas(&self, Parameters(p): Parameters<ChainParam>) -> Result<String, String> {
        let chain_index = crate::chains::resolve_chain(&p.chain);
        match gateway::fetch_gas(&mut *self.client.lock().await, &chain_index).await {
            Ok(data) => ok(data),
            Err(e) => err(e),
        }
    }

    #[tool(
        name = "gateway_gas_limit",
        description = "Estimate gas limit for a transaction"
    )]
    async fn gateway_gas_limit(
        &self,
        Parameters(p): Parameters<GatewayGasLimitParams>,
    ) -> Result<String, String> {
        let chain_index = crate::chains::resolve_chain(&p.chain);
        let amount = p.amount.as_deref().unwrap_or("0");
        match gateway::fetch_gas_limit(
            &mut *self.client.lock().await,
            &chain_index,
            &p.from,
            &p.to,
            amount,
            p.data.as_deref(),
        )
        .await
        {
            Ok(data) => ok(data),
            Err(e) => err(e),
        }
    }

    #[tool(
        name = "gateway_simulate",
        description = "Simulate a transaction (dry-run, no state change)"
    )]
    async fn gateway_simulate(
        &self,
        Parameters(p): Parameters<GatewaySimulateParams>,
    ) -> Result<String, String> {
        let chain_index = crate::chains::resolve_chain(&p.chain);
        let amount = p.amount.as_deref().unwrap_or("0");
        match gateway::fetch_simulate(
            &mut *self.client.lock().await,
            &chain_index,
            &p.from,
            &p.to,
            amount,
            &p.data,
        )
        .await
        {
            Ok(data) => ok(data),
            Err(e) => err(e),
        }
    }

    #[tool(
        name = "gateway_broadcast",
        description = "Broadcast a signed transaction on-chain"
    )]
    async fn gateway_broadcast(
        &self,
        Parameters(p): Parameters<GatewayBroadcastParams>,
    ) -> Result<String, String> {
        let chain_index = crate::chains::resolve_chain(&p.chain);
        match gateway::fetch_broadcast(
            &mut *self.client.lock().await,
            &chain_index,
            &p.signed_tx,
            &p.address,
            p.mev_protection,
        )
        .await
        {
            Ok(data) => ok(data),
            Err(e) => err(e),
        }
    }

    #[tool(name = "gateway_orders", description = "Track broadcast order status")]
    async fn gateway_orders(
        &self,
        Parameters(p): Parameters<GatewayOrdersParams>,
    ) -> Result<String, String> {
        let chain_index = crate::chains::resolve_chain(&p.chain);
        let oid = p.order_id.as_deref();
        match gateway::fetch_orders(
            &mut *self.client.lock().await,
            &chain_index,
            &p.address,
            oid,
        )
        .await
        {
            Ok(data) => ok(data),
            Err(e) => err(e),
        }
    }

    // ── Token: new tools ──────────────────────────────────────────────

    #[tool(
        name = "token_liquidity",
        description = "Get top 5 liquidity pools for a token"
    )]
    async fn token_liquidity(
        &self,
        Parameters(p): Parameters<TokenAddressParams>,
    ) -> Result<String, String> {
        let chain_index = p
            .chain
            .as_deref()
            .map(crate::chains::resolve_chain)
            .unwrap_or_else(|| crate::chains::resolve_chain("ethereum").to_string());
        match token::fetch_liquidity(&mut *self.client.lock().await, &p.address, &chain_index).await
        {
            Ok(data) => ok(data),
            Err(e) => err(e),
        }
    }

    #[tool(
        name = "token_hot_tokens",
        description = "Get hot token list ranked by trending score or X mentions, with extensive filtering. Default limit is 20 to prevent token overflow. Use cursor for pagination."
    )]
    async fn token_hot_tokens(
        &self,
        Parameters(p): Parameters<token::HotTokensParams>,
    ) -> Result<String, String> {
        match token::fetch_hot_tokens(&mut *self.client.lock().await, p).await {
            Ok(data) => ok(data),
            Err(e) => err(e),
        }
    }

    #[tool(
        name = "token_advanced_info",
        description = "Get advanced token info: risk level, creator, dev stats, holder concentration"
    )]
    async fn token_advanced_info(
        &self,
        Parameters(p): Parameters<TokenAddressParams>,
    ) -> Result<String, String> {
        let chain_index = p
            .chain
            .as_deref()
            .map(crate::chains::resolve_chain)
            .unwrap_or_else(|| crate::chains::resolve_chain("ethereum").to_string());
        match token::fetch_advanced_info(&mut *self.client.lock().await, &p.address, &chain_index)
            .await
        {
            Ok(data) => ok(data),
            Err(e) => err(e),
        }
    }

    #[tool(
        name = "token_top_trader",
        description = "Get top traders (profit addresses) for a token. Default limit is 20 to prevent token overflow. Use cursor for pagination."
    )]
    async fn token_top_trader(
        &self,
        Parameters(p): Parameters<TokenTagAddressParams>,
    ) -> Result<String, String> {
        let chain_index = p
            .chain
            .as_deref()
            .map(crate::chains::resolve_chain)
            .unwrap_or_else(|| crate::chains::resolve_chain("ethereum").to_string());
        match token::fetch_top_trader(
            &mut *self.client.lock().await,
            &p.address,
            &chain_index,
            p.tag_filter,
            p.limit.as_deref(),
            p.cursor.as_deref(),
        )
        .await
        {
            Ok(data) => ok(data),
            Err(e) => err(e),
        }
    }

    // ── Portfolio PnL: new tools ──────────────────────────────────────

    #[tool(
        name = "market_portfolio_supported_chains",
        description = "Get supported chains for wallet portfolio PnL analysis"
    )]
    async fn market_portfolio_supported_chains(&self) -> Result<String, String> {
        match market::fetch_portfolio_supported_chains(&mut *self.client.lock().await).await {
            Ok(data) => ok(data),
            Err(e) => err(e),
        }
    }

    #[tool(
        name = "market_portfolio_overview",
        description = "Get wallet portfolio overview: realized/unrealized PnL, win rate, trading stats"
    )]
    async fn market_portfolio_overview(
        &self,
        Parameters(p): Parameters<PortfolioPnlOverviewParams>,
    ) -> Result<String, String> {
        let chain_index = crate::chains::resolve_chain(&p.chain);
        let time_frame = p.time_frame.as_deref().unwrap_or("4");
        match market::fetch_portfolio_overview(
            &mut *self.client.lock().await,
            &chain_index,
            &p.address,
            time_frame,
        )
        .await
        {
            Ok(data) => ok(data),
            Err(e) => err(e),
        }
    }

    #[tool(
        name = "market_portfolio_dex_history",
        description = "Get wallet DEX transaction history (paginated)"
    )]
    async fn market_portfolio_dex_history(
        &self,
        Parameters(p): Parameters<PortfolioPnlDexHistoryParams>,
    ) -> Result<String, String> {
        let chain_index = crate::chains::resolve_chain(&p.chain);
        match market::fetch_portfolio_dex_history(
            &mut *self.client.lock().await,
            &chain_index,
            &p.address,
            &p.begin,
            &p.end,
            p.limit.as_deref(),
            p.cursor.as_deref(),
            p.token.as_deref(),
            p.tx_type.as_deref(),
        )
        .await
        {
            Ok(data) => ok(data),
            Err(e) => err(e),
        }
    }

    #[tool(
        name = "market_portfolio_recent_pnl",
        description = "Get recent token PnL records for a wallet (paginated)"
    )]
    async fn market_portfolio_recent_pnl(
        &self,
        Parameters(p): Parameters<PortfolioPnlRecentPnlParams>,
    ) -> Result<String, String> {
        let chain_index = crate::chains::resolve_chain(&p.chain);
        match market::fetch_portfolio_recent_pnl(
            &mut *self.client.lock().await,
            &chain_index,
            &p.address,
            p.limit.as_deref(),
            p.cursor.as_deref(),
        )
        .await
        {
            Ok(data) => ok(data),
            Err(e) => err(e),
        }
    }

    #[tool(
        name = "market_portfolio_token_pnl",
        description = "Get latest PnL snapshot for a specific token in a wallet"
    )]
    async fn market_portfolio_token_pnl(
        &self,
        Parameters(p): Parameters<PortfolioPnlTokenPnlParams>,
    ) -> Result<String, String> {
        let chain_index = crate::chains::resolve_chain(&p.chain);
        match market::fetch_portfolio_token_pnl(
            &mut *self.client.lock().await,
            &chain_index,
            &p.address,
            &p.token,
        )
        .await
        {
            Ok(data) => ok(data),
            Err(e) => err(e),
        }
    }

    #[tool(
        name = "tracker_activities",
        description = "Get latest DEX activities for tracked addresses. trackerType: smart_money (or 1) = platform smart money, kol (or 2) = platform Top 100 KOL addresses, multi_address (or 3) = custom addresses (requires wallet_address)"
    )]
    async fn tracker_activities(
        &self,
        Parameters(p): Parameters<AddressTrackerActivitiesParams>,
    ) -> Result<String, String> {
        let resolved_type = tracker::resolve_tracker_type(&p.tracker_type);
        if (resolved_type == "3" || p.tracker_type == "multi_address") && p.wallet_address.is_none()
        {
            return Err("wallet_address is required when tracker_type is multi_address".into());
        }
        let chain_index = p
            .chain
            .as_deref()
            .map(|c| crate::chains::resolve_chain(c).to_string());
        match tracker::fetch_activities(
            &mut *self.client.lock().await,
            &p.tracker_type,
            p.wallet_address.as_deref(),
            p.trade_type.as_deref(),
            chain_index.as_deref(),
            p.min_volume.as_deref(),
            p.max_volume.as_deref(),
            p.min_holders.as_deref(),
            p.min_market_cap.as_deref(),
            p.max_market_cap.as_deref(),
            p.min_liquidity.as_deref(),
            p.max_liquidity.as_deref(),
        )
        .await
        {
            Ok(data) => ok(data),
            Err(e) => err(e),
        }
    }

    // ── Leaderboard ───────────────────────────────────────────────────

    #[tool(
        name = "leaderboard_chains",
        description = "Get supported chains for the leaderboard (top traders ranking)"
    )]
    async fn leaderboard_chains(&self) -> Result<String, String> {
        match leaderboard::fetch_chains(&mut *self.client.lock().await).await {
            Ok(data) => ok(data),
            Err(e) => err(e),
        }
    }

    #[tool(
        name = "leaderboard_list",
        description = "Get top trader leaderboard ranked by PnL, win rate, volume, or ROI (max 20 per request)"
    )]
    async fn leaderboard_list(
        &self,
        Parameters(p): Parameters<LeaderboardListParams>,
    ) -> Result<String, String> {
        let chain_index = crate::chains::resolve_chain(&p.chain).to_string();
        let wallet_type_resolved = p
            .wallet_type
            .map(leaderboard::resolve_leaderboard_wallet_type);
        match leaderboard::fetch_list(
            &mut *self.client.lock().await,
            &chain_index,
            &p.time_frame,
            &p.sort_by,
            wallet_type_resolved.as_deref(),
            p.min_realized_pnl_usd.as_deref(),
            p.max_realized_pnl_usd.as_deref(),
            p.min_win_rate_percent.as_deref(),
            p.max_win_rate_percent.as_deref(),
            p.min_txs.as_deref(),
            p.max_txs.as_deref(),
            p.min_tx_volume.as_deref(),
            p.max_tx_volume.as_deref(),
        )
        .await
        {
            Ok(data) => ok(data),
            Err(e) => err(e),
        }
    }

    // ── Token Cluster ─────────────────────────────────────────────────

    #[tool(
        name = "token_cluster_supported_chains",
        description = "Get supported chains for token holder cluster analysis"
    )]
    async fn token_cluster_supported_chains(&self) -> Result<String, String> {
        match token::fetch_cluster_supported_chains(&mut *self.client.lock().await).await {
            Ok(data) => ok(data),
            Err(e) => err(e),
        }
    }

    #[tool(
        name = "token_cluster_overview",
        description = "Get token holder cluster concentration overview (cluster level, rug pull %, new address %)"
    )]
    async fn token_cluster_overview(
        &self,
        Parameters(p): Parameters<ClusterAddressParams>,
    ) -> Result<String, String> {
        let chain_index = p
            .chain
            .as_deref()
            .map(crate::chains::resolve_chain)
            .unwrap_or_else(|| crate::chains::resolve_chain("ethereum").to_string());
        match token::fetch_cluster_by_address(
            &mut *self.client.lock().await,
            "/api/v6/dex/market/token/cluster/overview",
            &p.address,
            &chain_index,
        )
        .await
        {
            Ok(data) => ok(data),
            Err(e) => err(e),
        }
    }

    #[tool(
        name = "token_cluster_top_holders",
        description = "Get token holder cluster analysis for top holder groups (range_filter: 1 = top 10, 2 = top 50, 3 = top 100)"
    )]
    async fn token_cluster_top_holders(
        &self,
        Parameters(p): Parameters<ClusterTopHoldersParams>,
    ) -> Result<String, String> {
        let chain_index = p
            .chain
            .as_deref()
            .map(crate::chains::resolve_chain)
            .unwrap_or_else(|| crate::chains::resolve_chain("ethereum").to_string());
        match token::fetch_cluster_top_holders(
            &mut *self.client.lock().await,
            &p.address,
            &chain_index,
            &p.range_filter,
        )
        .await
        {
            Ok(data) => ok(data),
            Err(e) => err(e),
        }
    }

    #[tool(
        name = "token_cluster_list",
        description = "Get holder cluster list with address details for top 300 holders of a token"
    )]
    async fn token_cluster_list(
        &self,
        Parameters(p): Parameters<ClusterAddressParams>,
    ) -> Result<String, String> {
        let chain_index = p
            .chain
            .as_deref()
            .map(crate::chains::resolve_chain)
            .unwrap_or_else(|| crate::chains::resolve_chain("ethereum").to_string());
        match token::fetch_cluster_by_address(
            &mut *self.client.lock().await,
            "/api/v6/dex/market/token/cluster/list",
            &p.address,
            &chain_index,
        )
        .await
        {
            Ok(data) => ok(data),
            Err(e) => err(e),
        }
    }

    // ── DeFi: Support Chains / Platforms ──────────────────────────────

    // ── Cross-Chain ────────────────────────────────────────────────────

    #[tool(
        name = "cross_chain_tokens",
        description = "List bridgeable tokens (/supported/tokens). Both from_chain and to_chain are independently optional: omit both for the full catalog, pass from_chain only for tokens on that source chain, pass to_chain only for tokens that can reach that destination, pass both for tokens on the specific from→to route. Returns chainIndex / tokenContractAddress / tokenSymbol / decimals."
    )]
    async fn cross_chain_tokens(
        &self,
        Parameters(p): Parameters<CrossChainBridgesParams>,
    ) -> Result<String, String> {
        let from_idx = p.from_chain.as_deref().map(crate::chains::resolve_chain);
        let to_idx = p.to_chain.as_deref().map(crate::chains::resolve_chain);
        match cross_chain::fetch_supported_tokens(
            &mut *self.client.lock().await,
            from_idx.as_deref(),
            to_idx.as_deref(),
        )
        .await
        {
            Ok(data) => ok(data),
            Err(e) => err(e),
        }
    }

    #[tool(
        name = "cross_chain_bridges",
        description = "List bridge protocols (/supported/bridges). Both from_chain and to_chain are independently optional: omit both for the full catalog, pass from_chain only for bridges on that source chain, pass to_chain only for bridges able to reach that destination, pass both for bridges that connect the specific chain pair. Returns bridgeId / bridgeName / supportedChains[]."
    )]
    async fn cross_chain_bridges(
        &self,
        Parameters(p): Parameters<CrossChainBridgesParams>,
    ) -> Result<String, String> {
        let from_idx = p.from_chain.as_deref().map(crate::chains::resolve_chain);
        let to_idx = p.to_chain.as_deref().map(crate::chains::resolve_chain);
        match cross_chain::fetch_supported_bridges(
            &mut *self.client.lock().await,
            from_idx.as_deref(),
            to_idx.as_deref(),
        )
        .await
        {
            Ok(data) => ok(data),
            Err(e) => err(e),
        }
    }

    #[tool(
        name = "cross_chain_quote",
        description = "Get cross-chain bridge quote (/quote). Returns routerList[] with bridgeId, needApprove, minimumReceived, estimateTime, crossChainFee."
    )]
    async fn cross_chain_quote(
        &self,
        Parameters(p): Parameters<CrossChainQuoteParams>,
    ) -> Result<String, String> {
        let from_chain_index = crate::chains::resolve_chain(&p.from_chain);
        let to_chain_index = crate::chains::resolve_chain(&p.to_chain);
        // Validate (but do not forward) receive_address — v6 /quote does not
        // accept it. Family mismatch fails up front instead of at execute.
        if let Some(addr) = p.receive_address.as_deref() {
            if let Err(e) = cross_chain::validate_receive_address(addr, &to_chain_index) {
                return err(e);
            }
        }
        let from_token = crate::commands::swap::resolve_token_address(&from_chain_index, &p.from);
        let to_token = crate::commands::swap::resolve_token_address(&to_chain_index, &p.to);
        let sort = p.sort.as_deref();
        // Hold a single guard across resolve_amount_arg + fetch_quote so the
        // pair runs as one atomic unit on the shared ApiClient.
        let mut client = self.client.lock().await;
        let raw_amount = match crate::commands::swap::resolve_amount_arg(
            &mut *client,
            None,
            Some(&p.readable_amount),
            &from_token,
            &from_chain_index,
        )
        .await
        {
            Ok(v) => v,
            Err(e) => return err(e),
        };
        match cross_chain::fetch_quote(
            &mut *client,
            &from_chain_index,
            &to_chain_index,
            &from_token,
            &to_token,
            &raw_amount,
            "0.01",
            None,
            false,
            None,
            sort,
            None,
            None,
        )
        .await
        {
            Ok(data) => ok(data),
            Err(e) => err(e),
        }
    }

    #[tool(
        name = "cross_chain_status",
        description = "Query cross-chain status by source chain transaction hash (/status). `bridge_id` is REQUIRED — server returns 50014 without it. Returns SUCCESS / PENDING / NOT_FOUND."
    )]
    async fn cross_chain_status(
        &self,
        Parameters(p): Parameters<CrossChainStatusParams>,
    ) -> Result<String, String> {
        let chain_idx = crate::chains::resolve_chain(&p.from_chain).to_string();
        let tx_hash = match (p.tx_hash, p.order_id) {
            (Some(h), None) => h,
            (None, Some(oid)) => match cross_chain::resolve_order_id_to_tx_hash(&oid, &chain_idx)
                .await
            {
                Ok(h) => h,
                Err(e) => return err(e),
            },
            (Some(_), Some(_)) => {
                return Err("provide tx_hash OR order_id, not both".to_string());
            }
            (None, None) => {
                return Err("one of tx_hash or order_id is required".to_string());
            }
        };
        match cross_chain::fetch_status(
            &mut *self.client.lock().await,
            &tx_hash,
            Some(&chain_idx),
            Some(&p.bridge_id),
        )
        .await
        {
            Ok(data) => ok(data),
            Err(e) => err(e),
        }
    }

    // ── DeFi ─────────────────────────────────────────────────────────

    #[tool(
        name = "defi_support_chains",
        description = "Get supported chains for DeFi operations"
    )]
    async fn defi_support_chains(&self) -> Result<String, String> {
        match defi::fetch_chains(&mut *self.client.lock().await).await {
            Ok(data) => ok(data),
            Err(e) => err(e),
        }
    }

    #[tool(
        name = "defi_support_platforms",
        description = "Get supported platforms for DeFi operations (e.g. Aave, Lido, Compound, PancakeSwap)"
    )]
    async fn defi_support_platforms(&self) -> Result<String, String> {
        match defi::fetch_protocols(&mut *self.client.lock().await).await {
            Ok(data) => ok(data),
            Err(e) => err(e),
        }
    }

    // ── DeFi A: Search / Detail / Prepare / Deposit ────────────────────

    #[tool(
        name = "defi_list",
        description = "List top DeFi products by APY across all chains (no filters, paginated)"
    )]
    async fn defi_list(&self, Parameters(p): Parameters<DefiListParams>) -> Result<String, String> {
        match defi::fetch_search(
            &mut *self.client.lock().await,
            None,
            None,
            None,
            None,
            p.page_num,
        )
        .await
        {
            Ok(data) => ok(data),
            Err(e) => err(e),
        }
    }

    #[tool(
        name = "defi_search",
        description = "Search DeFi investment products (earn, liquidity pools, lending)"
    )]
    async fn defi_search(
        &self,
        Parameters(p): Parameters<DefiSearchParams>,
    ) -> Result<String, String> {
        let chain_index = p.chain.as_deref().map(crate::chains::resolve_chain);
        match defi::fetch_search(
            &mut *self.client.lock().await,
            p.token.as_deref(),
            p.platform.as_deref(),
            chain_index.as_deref(),
            p.product_group.as_deref(),
            p.page_num,
        )
        .await
        {
            Ok(data) => ok(data),
            Err(e) => err(e),
        }
    }

    #[tool(
        name = "defi_detail",
        description = "Get full DeFi product details (APY, TVL, fee rate, isInvestable)"
    )]
    async fn defi_detail(
        &self,
        Parameters(p): Parameters<DefiDetailParams>,
    ) -> Result<String, String> {
        match defi::fetch_detail(&mut *self.client.lock().await, &p.investment_id).await {
            Ok(data) => ok(data),
            Err(e) => err(e),
        }
    }

    // ── DeFi: Chart APIs ─────────────────────────────────────────────

    #[tool(
        name = "defi_rate_chart",
        description = "Get historical APY chart data for a DeFi product. Returns timestamped APY data points for trend visualization. Time ranges: WEEK (default), MONTH, SEASON, YEAR. DAY is V3 Pool only."
    )]
    async fn defi_rate_chart(
        &self,
        Parameters(p): Parameters<DefiRateChartParams>,
    ) -> Result<String, String> {
        match defi::fetch_rate_chart(
            &mut *self.client.lock().await,
            &p.investment_id,
            p.time_range.as_deref(),
        )
        .await
        {
            Ok(data) => ok(data),
            Err(e) => err(e),
        }
    }

    #[tool(
        name = "defi_tvl_chart",
        description = "Get historical TVL chart data for a DeFi product. Returns timestamped TVL data points for trend visualization. Time ranges: WEEK (default), MONTH, SEASON, YEAR. DAY is V3 Pool only."
    )]
    async fn defi_tvl_chart(
        &self,
        Parameters(p): Parameters<DefiTvlChartParams>,
    ) -> Result<String, String> {
        match defi::fetch_tvl_chart(
            &mut *self.client.lock().await,
            &p.investment_id,
            p.time_range.as_deref(),
        )
        .await
        {
            Ok(data) => ok(data),
            Err(e) => err(e),
        }
    }

    #[tool(
        name = "defi_depth_price_chart",
        description = "Get V3 Pool liquidity depth distribution or price history chart. V3 Pool only. Chart types: DEPTH (default, shows liquidity per tick), PRICE (shows historical token0/token1 prices). Time range only applies to PRICE mode: DAY (default), WEEK."
    )]
    async fn defi_depth_price_chart(
        &self,
        Parameters(p): Parameters<DefiDepthPriceChartParams>,
    ) -> Result<String, String> {
        match defi::fetch_depth_price_chart(
            &mut *self.client.lock().await,
            &p.investment_id,
            p.chart_type.as_deref(),
            p.time_range.as_deref(),
        )
        .await
        {
            Ok(data) => ok(data),
            Err(e) => err(e),
        }
    }

    // ── DeFi: Positions / Position Detail (query tools) ──────────────

    #[tool(
        name = "defi_positions",
        description = "Get user DeFi holdings overview across all protocols and chains. DISPLAY RULE: render ALL platforms in a markdown table with columns: # | Platform | analysisPlatformId | Chain | Positions | Value(USD). analysisPlatformId is MANDATORY."
    )]
    async fn defi_positions(
        &self,
        Parameters(p): Parameters<DefiPositionsParams>,
    ) -> Result<String, String> {
        match defi::fetch_positions(&mut *self.client.lock().await, &p.address, &p.chains).await {
            Ok(data) => ok(data),
            Err(e) => err(e),
        }
    }

    #[tool(
        name = "defi_position_detail",
        description = "Get detailed DeFi holdings for a specific protocol. Requires analysisPlatformId from defi_positions."
    )]
    async fn defi_position_detail(
        &self,
        Parameters(p): Parameters<DefiPositionDetailParams>,
    ) -> Result<String, String> {
        let chain_index = crate::chains::resolve_chain(&p.chain);
        match defi::fetch_position_detail(
            &mut *self.client.lock().await,
            &p.address,
            &chain_index,
            &p.platform_id,
        )
        .await
        {
            Ok(data) => ok(data),
            Err(e) => err(e),
        }
    }

    // ── DeFi: One-step tools (invest/withdraw/collect) ─────────────

    #[tool(
        name = "defi_invest",
        description = "One-step DeFi deposit. Internally handles: detail check, prepare, precision conversion, V3 calculate-entry, calldata generation. Amount must be in minimal units (integer). For V3 pools pass range (e.g. 5 for ±5%). Returns calldata for signing."
    )]
    async fn defi_invest(
        &self,
        Parameters(p): Parameters<DefiInvestParams>,
    ) -> Result<String, String> {
        match defi::cmd_invest(
            &mut *self.client.lock().await,
            &p.investment_id,
            &p.address,
            &p.token,
            &p.amount,
            p.token2.as_deref(),
            p.amount2.as_deref(),
            p.slippage.as_deref().unwrap_or("0.01"),
            p.token_id.as_deref(),
            p.tick_lower,
            p.tick_upper,
            p.range,
        )
        .await
        {
            Ok(data) => ok(data),
            Err(e) => err(e),
        }
    }

    #[tool(
        name = "defi_withdraw",
        description = "One-step DeFi withdrawal. Internally handles: position-detail lookup, parameter construction, calldata generation. For full exit use ratio='1'. For V3 pools pass token_id + ratio."
    )]
    async fn defi_withdraw(
        &self,
        Parameters(p): Parameters<DefiWithdrawParams>,
    ) -> Result<String, String> {
        match defi::cmd_withdraw(
            &mut *self.client.lock().await,
            &p.investment_id,
            &p.address,
            &p.chain,
            p.ratio.as_deref(),
            p.token_id.as_deref(),
            p.slippage.as_deref().unwrap_or("0.01"),
            p.amount.as_deref(),
            p.platform_id.as_deref(),
        )
        .await
        {
            Ok(data) => ok(data),
            Err(e) => err(e),
        }
    }

    #[tool(
        name = "defi_collect",
        description = "One-step DeFi reward claim. Internally handles: position-detail lookup, reward check, expectOutputList construction, calldata generation. Skips if no rewards available."
    )]
    async fn defi_collect(
        &self,
        Parameters(p): Parameters<DefiCollectParams>,
    ) -> Result<String, String> {
        match defi::cmd_collect(
            &mut *self.client.lock().await,
            &p.address,
            &p.chain,
            &p.reward_type,
            p.investment_id.as_deref(),
            p.platform_id.as_deref(),
            p.token_id.as_deref(),
            p.principal_index.as_deref(),
        )
        .await
        {
            Ok(data) => ok(data),
            Err(e) => err(e),
        }
    }

    // ── Gas Station ─────────────────────────────────────────────────

    #[tool(
        name = "gas_station_update_default_token",
        description = "Update the default Gas Token for Gas Station on a specific chain. Gas Station allows paying gas fees with stablecoins (USDT/USDC/USDG) via a third-party Relayer."
    )]
    async fn gas_station_update_default_token(
        &self,
        Parameters(p): Parameters<GasStationUpdateDefaultTokenParams>,
    ) -> Result<String, String> {
        use crate::commands::agentic_wallet::gas_station;
        match gas_station::fetch_update_default_token(&p.chain, &p.gas_token_address).await {
            Ok(data) => ok(data),
            Err(e) => err(e),
        }
    }

    #[tool(
        name = "gas_station_enable",
        description = "Enable Gas Station for a specific chain (DB flag only, no on-chain action). Requires that 7702 delegation already exists on-chain (set earlier via the first-time Gas Station flow). If the chain was never delegated, backend returns a msg in the response body explaining that a first-time enable via wallet send is required."
    )]
    async fn gas_station_enable(
        &self,
        Parameters(p): Parameters<GasStationDisableParams>,
    ) -> Result<String, String> {
        use crate::commands::agentic_wallet::gas_station;
        match gas_station::fetch_update(&p.chain, true).await {
            Ok(data) => ok(data),
            Err(e) => err(e),
        }
    }

    #[tool(
        name = "gas_station_disable",
        description = "Disable Gas Station for a specific chain (DB flag only, no on-chain action). The 7702 delegation on-chain is preserved, so re-enabling later does not require a new upgrade. To switch default gas token, use gas_station_update_default_token instead."
    )]
    async fn gas_station_disable(
        &self,
        Parameters(p): Parameters<GasStationDisableParams>,
    ) -> Result<String, String> {
        use crate::commands::agentic_wallet::gas_station;
        match gas_station::fetch_update(&p.chain, false).await {
            Ok(data) => ok(data),
            Err(e) => err(e),
        }
    }

    // ── Workflows ──────────────────────────────────────────────────────────────

    #[tool(
        name = "workflow_token_research",
        description = "Full token due diligence in one call: price, contract, security scan, \
            holder distribution, cluster overview, top traders, smart money signals. \
            Accepts either 'address' (contract address) or 'query' (symbol/name). \
            When 'query' is used without 'address', returns top 5 search results — \
            present them to the user and call again with the chosen address. \
            Step 3 adds launchpad enrichment automatically when protocolId is non-empty. \
            Returns structured JSON with core / structure / launchpad blocks. \
            Error if all Step 1 sub-calls fail."
    )]
    async fn workflow_token_research(
        &self,
        Parameters(p): Parameters<WorkflowTokenResearchParams>,
    ) -> Result<String, String> {
        let chain_index = p
            .chain
            .as_deref()
            .map(|c| crate::chains::resolve_chain(c).to_string())
            .unwrap_or_else(|| crate::chains::resolve_chain("solana").to_string());

        // Symbol/name search path — return candidates for user selection
        if p.address.is_none() {
            let query = p
                .query
                .as_deref()
                .ok_or_else(|| "Either 'address' or 'query' is required".to_string())?;
            return match workflows::token_research::search_and_select(
                &mut *self.client.lock().await,
                query,
                &chain_index,
            )
            .await
            {
                Ok(data) => ok(data),
                Err(e) => err(e),
            };
        }

        // Direct address path — full workflow
        let address = p
            .address
            .as_deref()
            .expect("address is Some after is_none() early-return guard above");
        match workflows::token_research::fetch_and_assemble(
            &mut *self.client.lock().await,
            address,
            &chain_index,
        )
        .await
        {
            Ok(data) => ok(data),
            Err(e) => err(e),
        }
    }

    #[tool(
        name = "workflow_smart_money",
        description = "Smart money signals aggregated and enriched in one call. \
            Fetches the signal list, groups by token, takes the top 5 by SM wallet count, \
            then runs per-token due diligence (price, security, contract, optional launchpad). \
            Signal API failure returns gracefully with empty topTokens."
    )]
    async fn workflow_smart_money(
        &self,
        Parameters(p): Parameters<WorkflowSmartMoneyParams>,
    ) -> Result<String, String> {
        let chain_index = p
            .chain
            .as_deref()
            .map(|c| crate::chains::resolve_chain(c).to_string())
            .unwrap_or_else(|| crate::chains::resolve_chain("solana").to_string());
        match workflows::smart_money::fetch_and_assemble(
            &mut *self.client.lock().await,
            &chain_index,
        )
        .await
        {
            Ok(data) => ok(data),
            Err(e) => err(e),
        }
    }

    #[tool(
        name = "workflow_new_tokens",
        description = "Launchpad new token screening in one call. \
            Fetches MIGRATED (default) or MIGRATING tokens, then enriches the top 10 \
            with security scan, contract info, dev history, and bundle rate in parallel. \
            Token list API failure returns gracefully with empty enriched list."
    )]
    async fn workflow_new_tokens(
        &self,
        Parameters(p): Parameters<WorkflowNewTokensParams>,
    ) -> Result<String, String> {
        let chain_index = p
            .chain
            .as_deref()
            .map(|c| crate::chains::resolve_chain(c).to_string())
            .unwrap_or_else(|| crate::chains::resolve_chain("solana").to_string());
        let stage = p.stage.unwrap_or_else(|| "MIGRATED".to_string());
        match workflows::new_tokens::fetch_and_assemble(
            &mut *self.client.lock().await,
            &chain_index,
            &stage,
        )
        .await
        {
            Ok(data) => ok(data),
            Err(e) => err(e),
        }
    }

    #[tool(
        name = "workflow_wallet_analysis",
        description = "Wallet performance, behaviour, and recent activity in one call. \
            Fetches 7d and 30d portfolio overview, all token balances, recent per-token PnL, \
            and latest on-chain activity via the address tracker. \
            Partial failures return null for that field; no all-fail error rule."
    )]
    async fn workflow_wallet_analysis(
        &self,
        Parameters(p): Parameters<WorkflowWalletAnalysisParams>,
    ) -> Result<String, String> {
        let chain_index = p
            .chain
            .as_deref()
            .map(|c| crate::chains::resolve_chain(c).to_string())
            .unwrap_or_else(|| crate::chains::resolve_chain("solana").to_string());
        match workflows::wallet_analysis::fetch_and_assemble(
            &mut *self.client.lock().await,
            &p.address,
            &chain_index,
        )
        .await
        {
            Ok(data) => ok(data),
            Err(e) => err(e),
        }
    }

    #[tool(
        name = "workflow_portfolio",
        description = "Wallet portfolio snapshot in one call: all token balances, \
            total value, and 30d PnL overview. \
            Partial failures return null for that field."
    )]
    async fn workflow_portfolio(
        &self,
        Parameters(p): Parameters<WorkflowPortfolioParams>,
    ) -> Result<String, String> {
        // Resolve chain names to indices up-front so the output's `chains` field
        // is always in indexed form ("1,501") regardless of whether the caller
        // sent names ("ethereum,solana") or indices — matches CLI output shape.
        // `fetch_and_assemble` derives `primary_chain_index` internally so the
        // MCP and CLI entry points cannot drift.
        let chains_str =
            crate::chains::resolve_chains(&p.chains.unwrap_or_else(|| "1,501".to_string()));
        match workflows::portfolio::fetch_and_assemble(
            &mut *self.client.lock().await,
            &p.address,
            &chains_str,
        )
        .await
        {
            Ok(data) => ok(data),
            Err(e) => err(e),
        }
    }
}

pub async fn serve(base_url_override: Option<&str>) -> Result<()> {
    let server = McpServer::new(base_url_override)?;
    let service = server.serve(stdio()).await?;
    service.waiting().await?;
    Ok(())
}
