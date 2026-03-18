use anyhow::Result;
use rmcp::handler::server::router::tool::ToolRouter;
use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::ServerInfo;
use rmcp::transport::io::stdio;
use rmcp::{tool, tool_handler, tool_router, ServerHandler, ServiceExt};
use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::Value;

use crate::client::ApiClient;
use crate::commands::{gateway, market, memepump, portfolio, signal, swap, token};

// ── Token ──────────────────────────────────────────────────────────────
#[derive(Deserialize, JsonSchema)]
struct TokenSearchParams {
    /// Token name, symbol, or contract address (e.g. "ETH", "USDC", "0x...")
    query: String,
    /// Comma-separated chain names, e.g. "ethereum,solana" (optional, searches all)
    chains: Option<String>,
}

#[derive(Deserialize, JsonSchema)]
struct TokenAddressParams {
    /// Token contract address
    address: String,
    /// Chain name, e.g. "ethereum", "solana" (optional, defaults to ethereum)
    chain: Option<String>,
}

#[derive(Deserialize, JsonSchema)]
struct TokenTrendingParams {
    /// Comma-separated chain names, e.g. "ethereum,solana" (optional)
    chains: Option<String>,
    /// Sort by: 2=price change, 5=volume (default), 6=market cap
    sort_by: Option<String>,
    /// Time frame: 1=5min, 2=1h, 3=4h, 4=24h (default)
    time_frame: Option<String>,
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

#[derive(Clone)]
pub struct McpServer {
    tool_router: ToolRouter<Self>,
    client: ApiClient,
}

impl McpServer {
    pub fn new(base_url_override: Option<&str>) -> Result<Self> {
        Ok(Self {
            tool_router: Self::tool_router(),
            client: ApiClient::new(base_url_override)?,
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
    Ok(serde_json::to_string_pretty(&data)
        .unwrap_or_else(|e| format!("failed to serialize response: {e}")))
}

fn err(e: anyhow::Error) -> Result<String, String> {
    Err(format!("{e:#}"))
}

#[tool_router]
impl McpServer {
    #[tool(
        name = "token_search",
        description = "Search tokens by name/symbol/address across chains"
    )]
    async fn token_search(
        &self,
        Parameters(p): Parameters<TokenSearchParams>,
    ) -> Result<String, String> {
        let chains = p.chains.as_deref().unwrap_or("1,501");
        match token::fetch_search(&self.client, &p.query, chains).await {
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
            .unwrap_or_else(|| "1".to_string());
        match token::fetch_info(&self.client, &p.address, &chain_index).await {
            Ok(data) => ok(data),
            Err(e) => err(e),
        }
    }

    #[tool(
        name = "token_holders",
        description = "Get token holder distribution (top 20)"
    )]
    async fn token_holders(
        &self,
        Parameters(p): Parameters<TokenTagAddressParams>,
    ) -> Result<String, String> {
        let chain_index = p
            .chain
            .as_deref()
            .map(crate::chains::resolve_chain)
            .unwrap_or_else(|| "1".to_string());
        match token::fetch_holders(&self.client, &p.address, &chain_index, p.tag_filter).await {
            Ok(data) => ok(data),
            Err(e) => err(e),
        }
    }

    #[tool(name = "token_trending", description = "Get trending token rankings")]
    async fn token_trending(
        &self,
        Parameters(p): Parameters<TokenTrendingParams>,
    ) -> Result<String, String> {
        let chains = p.chains.as_deref().unwrap_or("1,501");
        let sort_by = p.sort_by.as_deref().unwrap_or("5");
        let time_frame = p.time_frame.as_deref().unwrap_or("4");
        match token::fetch_trending(&self.client, chains, sort_by, time_frame).await {
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
            .unwrap_or_else(|| "1".to_string());
        match token::fetch_price_info(&self.client, &p.address, &chain_index).await {
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
            .unwrap_or_else(|| "1".to_string());
        match market::fetch_price(&self.client, &p.address, &chain_index).await {
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
            .unwrap_or_else(|| "1".to_string());
        match market::fetch_prices(&self.client, &p.tokens, &default_chain).await {
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
            .unwrap_or_else(|| "1".to_string());
        let bar = p.bar.as_deref().unwrap_or("1H");
        let limit = p.limit.unwrap_or(100);
        match market::fetch_kline(&self.client, &p.address, &chain_index, bar, limit).await {
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
            .unwrap_or_else(|| "1".to_string());
        let limit = p.limit.unwrap_or(100);
        match token::fetch_token_trades(
            &self.client,
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
            .unwrap_or_else(|| "1".to_string());
        match market::fetch_index(&self.client, &p.address, &chain_index).await {
            Ok(data) => ok(data),
            Err(e) => err(e),
        }
    }

    #[tool(
        name = "signal_chains",
        description = "Get chains supported for smart money / KOL / whale signals"
    )]
    async fn signal_chains(&self) -> Result<String, String> {
        match signal::fetch_chains(&self.client).await {
            Ok(data) => ok(data),
            Err(e) => err(e),
        }
    }

    #[tool(
        name = "signal_list",
        description = "Get smart money / KOL / whale signal list for a chain"
    )]
    async fn signal_list(
        &self,
        Parameters(p): Parameters<MarketSignalListParams>,
    ) -> Result<String, String> {
        let chain_index = crate::chains::resolve_chain(&p.chain);
        match signal::fetch_list(
            &self.client,
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
        match memepump::fetch_chains(&self.client).await {
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
        match memepump::fetch_token_list(&self.client, p).await {
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
            &self.client,
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
            &self.client,
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
            &self.client,
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
            &self.client,
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
            &self.client,
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
        match swap::fetch_chains(&self.client).await {
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
            &self.client,
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
            &self.client,
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
        match swap::fetch_approve(&self.client, &chain_index, &p.token, &p.amount).await {
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
        match swap::fetch_liquidity(&self.client, &chain_index).await {
            Ok(data) => ok(data),
            Err(e) => err(e),
        }
    }

    #[tool(
        name = "portfolio_chains",
        description = "Get supported chains for wallet balance queries"
    )]
    async fn portfolio_chains(&self) -> Result<String, String> {
        match portfolio::fetch_chains(&self.client).await {
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
            &self.client,
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
            &self.client,
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
            &self.client,
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
        match gateway::fetch_chains(&self.client).await {
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
        match gateway::fetch_gas(&self.client, &chain_index).await {
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
            &self.client,
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
        match gateway::fetch_simulate(&self.client, &chain_index, &p.from, &p.to, amount, &p.data)
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
            &self.client,
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
        match gateway::fetch_orders(&self.client, &chain_index, &p.address, oid).await {
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
            .unwrap_or_else(|| "1".to_string());
        match token::fetch_liquidity(&self.client, &p.address, &chain_index).await {
            Ok(data) => ok(data),
            Err(e) => err(e),
        }
    }

    #[tool(
        name = "token_hot_tokens",
        description = "Get hot token list ranked by trending score or X mentions, with extensive filtering"
    )]
    async fn token_hot_tokens(
        &self,
        Parameters(p): Parameters<token::HotTokensParams>,
    ) -> Result<String, String> {
        match token::fetch_hot_tokens(&self.client, p).await {
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
            .unwrap_or_else(|| "1".to_string());
        match token::fetch_advanced_info(&self.client, &p.address, &chain_index).await {
            Ok(data) => ok(data),
            Err(e) => err(e),
        }
    }

    #[tool(
        name = "token_top_trader",
        description = "Get top traders (profit addresses) for a token"
    )]
    async fn token_top_trader(
        &self,
        Parameters(p): Parameters<TokenTagAddressParams>,
    ) -> Result<String, String> {
        let chain_index = p
            .chain
            .as_deref()
            .map(crate::chains::resolve_chain)
            .unwrap_or_else(|| "1".to_string());
        match token::fetch_top_trader(&self.client, &p.address, &chain_index, p.tag_filter).await {
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
        match market::fetch_portfolio_supported_chains(&self.client).await {
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
        match market::fetch_portfolio_overview(&self.client, &chain_index, &p.address, time_frame)
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
            &self.client,
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
            &self.client,
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
        match market::fetch_portfolio_token_pnl(&self.client, &chain_index, &p.address, &p.token)
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
