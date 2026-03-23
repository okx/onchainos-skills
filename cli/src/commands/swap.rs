use std::collections::HashMap;
use std::sync::LazyLock;

use anyhow::Result;
use clap::Subcommand;
use serde_json::Value;

use super::Context;
use crate::client::ApiClient;
use crate::output;

/// All aggregator endpoints are GET requests.
#[derive(Subcommand)]
pub enum SwapCommand {
    /// Get swap quote (read-only price estimate)
    Quote {
        /// Source token contract address
        #[arg(long)]
        from: String,
        /// Destination token contract address
        #[arg(long)]
        to: String,
        /// Amount in minimal units (wei/lamports)
        #[arg(long)]
        amount: String,
        /// Chain (e.g. ethereum, solana, xlayer)
        #[arg(long)]
        chain: String,
        /// Swap mode: exactIn or exactOut
        #[arg(long, default_value = "exactIn")]
        swap_mode: String,
    },
    /// Get swap transaction data (quote → sign → broadcast)
    Swap {
        /// Source token contract address
        #[arg(long)]
        from: String,
        /// Destination token contract address
        #[arg(long)]
        to: String,
        /// Amount in minimal units
        #[arg(long)]
        amount: String,
        /// Chain
        #[arg(long)]
        chain: String,
        /// Slippage tolerance in percent (e.g. "1" for 1%). Omit to use autoSlippage.
        #[arg(long)]
        slippage: Option<String>,
        /// User wallet address
        #[arg(long)]
        wallet: String,
        /// Gas priority: slow, average, fast (default: average)
        #[arg(long, default_value = "average")]
        gas_level: String,
        /// Swap mode: exactIn or exactOut
        #[arg(long, default_value = "exactIn")]
        swap_mode: String,
        /// Jito tips in SOL for Solana MEV protection (range: 0.0000000001–2). Response includes signatureData for jitoCalldata.
        #[arg(long)]
        tips: Option<String>,
        /// Max auto slippage percent cap when autoSlippage is enabled (e.g. "0.5" for 0.5%)
        #[arg(long)]
        max_auto_slippage: Option<String>,
    },
    /// Get ERC-20 approval transaction data
    Approve {
        /// Token contract address to approve
        #[arg(long)]
        token: String,
        /// Approval amount in minimal units
        #[arg(long)]
        amount: String,
        /// Chain
        #[arg(long)]
        chain: String,
    },
    /// Get supported chains for DEX aggregator
    Chains,
    /// Get available liquidity sources on a chain
    Liquidity {
        /// Chain
        #[arg(long)]
        chain: String,
    },
}

pub async fn execute(ctx: &Context, cmd: SwapCommand) -> Result<()> {
    let client = ctx.client_async().await?;
    match cmd {
        SwapCommand::Quote {
            from,
            to,
            amount,
            chain,
            swap_mode,
        } => {
            let chain_index = crate::chains::resolve_chain(&chain);
            crate::chains::ensure_supported_chain(&chain_index, &chain)?;
            output::success(
                fetch_quote(&client, &chain_index, &from, &to, &amount, &swap_mode).await?,
            );
        }
        SwapCommand::Swap {
            from,
            to,
            amount,
            chain,
            slippage,
            wallet,
            gas_level,
            swap_mode,
            tips,
            max_auto_slippage,
        } => {
            let chain_index = crate::chains::resolve_chain(&chain);
            crate::chains::ensure_supported_chain(&chain_index, &chain)?;
            output::success(
                fetch_swap(
                    &client,
                    &chain_index,
                    &from,
                    &to,
                    &amount,
                    slippage.as_deref(),
                    &wallet,
                    &swap_mode,
                    &gas_level,
                    tips.as_deref(),
                    max_auto_slippage.as_deref(),
                )
                .await?,
            );
        }
        SwapCommand::Approve {
            token,
            amount,
            chain,
        } => {
            let chain_index = crate::chains::resolve_chain(&chain);
            crate::chains::ensure_supported_chain(&chain_index, &chain)?;
            output::success(fetch_approve(&client, &chain_index, &token, &amount).await?);
        }
        SwapCommand::Chains => {
            output::success(fetch_chains(&client).await?);
        }
        SwapCommand::Liquidity { chain } => {
            let chain_index = crate::chains::resolve_chain(&chain);
            crate::chains::ensure_supported_chain(&chain_index, &chain)?;
            output::success(fetch_liquidity(&client, &chain_index).await?);
        }
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Token address mapping: chain_index -> { lowercase_key -> correct_address }
// Covers:
//   - Symbol → CA resolution (e.g. "USDC" → contract address)
//   - "native" keyword → native token address per chain
//   - Error CA auto-correction (e.g. wSOL SPL address → native SOL address)
// Matching is case-insensitive.
// ---------------------------------------------------------------------------

static TOKEN_MAP: LazyLock<HashMap<&str, HashMap<&str, &str>>> = LazyLock::new(|| {
    HashMap::from([
        // Solana (501)
        ("501", HashMap::from([
            ("sol", "11111111111111111111111111111111"),
            ("native", "11111111111111111111111111111111"),
            ("usdc", "EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v"),
            ("usdt", "Es9vMFrzaCERmJfrF4H2FYD4KCoNkY11McCe8BenwNYB"),
            // Error CA corrections: wSOL SPL token / typo
            ("so11111111111111111111111111111111111111112", "11111111111111111111111111111111"),
            ("so11111111111111111111111111111111111111111", "11111111111111111111111111111111"),
        ])),
        // Ethereum (1)
        ("1", HashMap::from([
            ("eth", "0xeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee"),
            ("native", "0xeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee"),
            ("usdc", "0xa0b86991c6218b36c1d19d4a2e9eb0ce3606eb48"),
            ("usdt", "0xdac17f958d2ee523a2206206994597c13d831ec7"),
            ("wbtc", "0x2260fac5e5542a773aa44fbcfedf7c193bc2c599"),
            ("dai", "0x6b175474e89094c44da98b954eedeac495271d0f"),
            ("weth", "0xc02aaa39b223fe8d0a0e5c4f27ead9083c756cc2"),
        ])),
        // Base (8453)
        ("8453", HashMap::from([
            ("eth", "0xeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee"),
            ("native", "0xeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee"),
            ("usdc", "0x833589fcd6edb6e08f4c7c32d4f71b54bda02913"),
            ("weth", "0x4200000000000000000000000000000000000006"),
            ("usdbc", "0xd9aaec86b65d86f6a7b5b1b0c42ffa531710b6ca"),
        ])),
        // BSC (56)
        ("56", HashMap::from([
            ("bnb", "0xeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee"),
            ("native", "0xeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee"),
            ("usdt", "0x55d398326f99059ff775485246999027b3197955"),
            ("usdc", "0x8ac76a51cc950d9822d68b83fe1ad97b32cd580d"),
            ("wbnb", "0xbb4cdb9cbd36b01bd1cbaebf2de08d9173bc095c"),
            ("weth", "0x2170ed0880ac9a755fd29b2688956bd959f933f8"),
            ("btcb", "0x7130d2a12b9bcbfae4f2634d864a1ee1ce3ead9c"),
        ])),
        // Arbitrum (42161)
        ("42161", HashMap::from([
            ("eth", "0xeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee"),
            ("native", "0xeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee"),
            ("usdc", "0xaf88d065e77c8cc2239327c5edb3a432268e5831"),
            ("usdt", "0xfd086bc7cd5c481dcc9c85ebe478a1c0b69fcbb9"),
            ("weth", "0x82af49447d8a07e3bd95bd0d56f35241523fbab1"),
        ])),
        // Polygon (137)
        ("137", HashMap::from([
            ("matic", "0xeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee"),
            ("pol", "0xeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee"),
            ("native", "0xeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee"),
            ("usdc", "0x3c499c542cef5e3811e1192ce70d8cc03d5c3359"),
            ("usdt0", "0xc2132d05d31c914a87c6611c10748aeb04b58e8f"),
            ("weth", "0x7ceb23fd6bc0add59e62ac25578270cff1b9f619"),
            ("wmatic", "0x0d500b1d8e8ef31e21c99d1db9a6444d3adf1270"),
            ("wpol", "0x0d500b1d8e8ef31e21c99d1db9a6444d3adf1270"),
        ])),
        // Optimism (10)
        ("10", HashMap::from([
            ("eth", "0xeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee"),
            ("native", "0xeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee"),
            ("usdc", "0x0b2c639c533813f4aa9d7837caf62653d097ff85"),
            ("usdt", "0x94b008aa00579c1307b0ef2c499ad98a8ce58e58"),
            ("weth", "0x4200000000000000000000000000000000000006"),
            ("op", "0x4200000000000000000000000000000000000042"),
        ])),
        // Avalanche (43114)
        ("43114", HashMap::from([
            ("avax", "0xeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee"),
            ("native", "0xeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee"),
            ("usdc", "0xb97ef9ef8734c71904d8002f8b6bc66dd9c48a6e"),
            ("usdt", "0x9702230a8ea53601f5cd2dc00fdbc13d4df4a8c7"),
            ("wavax", "0xb31f66aa3c1e785363f0875a1b74e27b85fd66c7"),
            ("weth.e", "0x49d5c2bdffac6ce2bfdb6640f4f80f226bc10bab"),
        ])),
        // XLayer (196)
        ("196", HashMap::from([
            ("okb", "0xeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee"),
            ("native", "0xeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee"),
            ("usdc", "0x74b7f16337b8972027f6196a17a631ac6de26d22"),
            ("xlayer_usdt", "0x1e4a5963abfd975d8c9021ce480b42188849d41d"),
            ("usdt0", "0x779ded0c9e1022225f8e0630b35a9b54be713736"),
            ("usdt", "0x779ded0c9e1022225f8e0630b35a9b54be713736"),
            ("weth", "0x5a77f1443d16ee5761d310e38b62f77f726bc71c"),
            ("wokb", "0xe538905cf8410324e03a5a23c1c177a474d59b2b"),
        ])),
        // Linea (59144)
        ("59144", HashMap::from([
            ("eth", "0xeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee"),
            ("native", "0xeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee"),
            ("usdc", "0x176211869ca2b568f2a7d4ee941e073a821ee1ff"),
            ("usdt", "0xa219439258ca9da29e9cc4ce5596924745e12b93"),
            ("weth", "0xe5d7c2a44ffddf6b295a15c148167daaaf5cf34f"),
        ])),
        // Scroll (534352)
        ("534352", HashMap::from([
            ("eth", "0xeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee"),
            ("native", "0xeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee"),
            ("usdc", "0x06efdbff2a14a7c8e15944d1f4a48f9f95f663a4"),
            ("usdt", "0xf55bec9cafdbe8730f096aa55dad6d22d44099df"),
            ("weth", "0x5300000000000000000000000000000000000004"),
        ])),
        // zkSync (324)
        ("324", HashMap::from([
            ("eth", "0xeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee"),
            ("native", "0xeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee"),
            ("weth", "0x5aea5775959fbc2557cc8789bc1bf90a239d9a91"),
            ("usdt", "0x493257fd37edb34451f62edf8d2a0c418852ba4c"),
        ])),
        // Fantom (250)
        ("250", HashMap::from([
            ("ftm", "0xeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee"),
            ("native", "0xeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee"),
            ("wftm", "0x21be370d5312f44cb42ce377bc9b8a0cef1a4c83"),
        ])),
        // Tron (195)
        ("195", HashMap::from([
            ("trx", "T9yD14Nj9j7xAB4dbGeiX9h8unkKHxuWwb"),
            ("native", "T9yD14Nj9j7xAB4dbGeiX9h8unkKHxuWwb"),
            ("usdt", "TR7NHqjeKQxGTCi8q8ZY4pL8otSzgjLj6t"),
            ("wtrx", "TNUC9Qb1rRpS5CbWLmNMxXBjyFoydXjWFR"),
            ("eth", "THb4CqiFdwNHsWsQCs4JhzwjMWys4aqCbF"),
        ])),
        // Sui (784)
        ("784", HashMap::from([
            ("sui", "0x2::sui::SUI"),
            ("native", "0x2::sui::SUI"),
            ("wusdc", "0x5d4b302506645c37ff133b98c4b50a5ae14841659738d6d733d59d0d217a93bf::coin::COIN"),
            ("wusdt", "0xc060006111016b8a020ad5b33834984a437aaa7d3c74c18e09a95d48aceab08c::coin::COIN"),
        ])),
    ])
});

/// Resolve a token address using the chain-specific mapping table.
/// Matching is case-insensitive. If no match is found, returns the original value unchanged.
fn resolve_token_address(chain_index: &str, token: &str) -> String {
    let key = token.to_ascii_lowercase();
    if let Some(chain_map) = TOKEN_MAP.get(chain_index) {
        if let Some(&resolved) = chain_map.get(key.as_str()) {
            return resolved.to_string();
        }
    }
    token.to_string()
}

/// GET /api/v6/dex/aggregator/quote
pub async fn fetch_quote(
    client: &ApiClient,
    chain_index: &str,
    from: &str,
    to: &str,
    amount: &str,
    swap_mode: &str,
) -> Result<Value> {
    let orig_from = from;
    let orig_to = to;
    let from = resolve_token_address(chain_index, orig_from);
    let to = resolve_token_address(chain_index, orig_to);
    if cfg!(feature = "debug-log") {
        eprintln!(
            "[DEBUG][fetch_quote] chain_index={}, from={}, to={}, amount={}, swap_mode={}",
            chain_index, orig_from, orig_to, amount, swap_mode
        );
        if orig_from != from.as_str() {
            eprintln!(
                "[DEBUG][fetch_quote] from resolved: {} → {}",
                orig_from, from
            );
        }
        if orig_to != to.as_str() {
            eprintln!(
                "[DEBUG][fetch_quote] to resolved: {} → {}",
                orig_to, to
            );
        }
    }
    let params = vec![
        ("chainIndex", chain_index),
        ("fromTokenAddress", from.as_str()),
        ("toTokenAddress", to.as_str()),
        ("amount", amount),
        ("swapMode", swap_mode),
    ];
    client.get("/api/v6/dex/aggregator/quote", &params).await
}

/// GET /api/v6/dex/aggregator/swap
#[allow(clippy::too_many_arguments)]
pub async fn fetch_swap(
    client: &ApiClient,
    chain_index: &str,
    from: &str,
    to: &str,
    amount: &str,
    slippage: Option<&str>,
    wallet: &str,
    swap_mode: &str,
    gas_level: &str,
    tips: Option<&str>,
    max_auto_slippage: Option<&str>,
) -> Result<Value> {
    let orig_from = from;
    let orig_to = to;
    let from = resolve_token_address(chain_index, orig_from);
    let to = resolve_token_address(chain_index, orig_to);
    if cfg!(feature = "debug-log") {
        eprintln!(
            "[DEBUG][fetch_swap] chain_index={}, from={}, to={}, amount={}, wallet={}, swap_mode={}, gas_level={}, slippage={:?}, tips={:?}, max_auto_slippage={:?}",
            chain_index, orig_from, orig_to, amount, wallet, swap_mode, gas_level, slippage, tips, max_auto_slippage
        );
        if orig_from != from.as_str() {
            eprintln!(
                "[DEBUG][fetch_swap] from resolved: {} → {}",
                orig_from, from
            );
        }
        if orig_to != to.as_str() {
            eprintln!(
                "[DEBUG][fetch_swap] to resolved: {} → {}",
                orig_to, to
            );
        }
    }
    let mut params = vec![
        ("chainIndex", chain_index),
        ("fromTokenAddress", from.as_str()),
        ("toTokenAddress", to.as_str()),
        ("amount", amount),
        ("userWalletAddress", wallet),
        ("swapMode", swap_mode),
        ("gasLevel", gas_level),
    ];
    if let Some(s) = slippage {
        params.push(("slippagePercent", s));
    } else {
        params.push(("autoSlippage", "true"));
        params.push(("slippagePercent", "0.5"));
    }
    if let Some(t) = tips {
        params.push(("tips", t));
        // Jito tips and computeUnitPrice are mutually exclusive
        params.push(("computeUnitPrice", "0"));
    }
    if let Some(m) = max_auto_slippage {
        params.push(("maxAutoSlippagePercent", m));
    }
    client.get("/api/v6/dex/aggregator/swap", &params).await
}

/// GET /api/v6/dex/aggregator/approve-transaction
pub async fn fetch_approve(
    client: &ApiClient,
    chain_index: &str,
    token: &str,
    amount: &str,
) -> Result<Value> {
    let orig_token = token;
    let token = resolve_token_address(chain_index, orig_token);
    if cfg!(feature = "debug-log") {
        eprintln!(
            "[DEBUG][fetch_approve] chain_index={}, token={}, amount={}",
            chain_index, orig_token, amount
        );
        if orig_token != token.as_str() {
            eprintln!(
                "[DEBUG][fetch_approve] token resolved: {} → {}",
                orig_token, token
            );
        }
    }
    client
        .get(
            "/api/v6/dex/aggregator/approve-transaction",
            &[
                ("chainIndex", chain_index),
                ("tokenContractAddress", token.as_str()),
                ("approveAmount", amount),
            ],
        )
        .await
}

/// GET /api/v6/dex/aggregator/supported/chain
pub async fn fetch_chains(client: &ApiClient) -> Result<Value> {
    client
        .get("/api/v6/dex/aggregator/supported/chain", &[])
        .await
}

/// GET /api/v6/dex/aggregator/get-liquidity
pub async fn fetch_liquidity(client: &ApiClient, chain_index: &str) -> Result<Value> {
    client
        .get(
            "/api/v6/dex/aggregator/get-liquidity",
            &[("chainIndex", chain_index)],
        )
        .await
}
