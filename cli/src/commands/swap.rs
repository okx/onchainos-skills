use std::collections::HashMap;
use std::sync::LazyLock;

use anyhow::{bail, Result};
use clap::Subcommand;
use serde_json::{json, Value};

use super::Context;
use crate::client::ApiClient;
use crate::output;

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
        /// Amount in minimal units (wei/lamports). Mutually exclusive with --readable-amount.
        #[arg(long, conflicts_with = "readable_amount")]
        amount: Option<String>,
        /// Human-readable amount (e.g. "1.5" for 1.5 USDC). CLI fetches token decimals and converts automatically.
        #[arg(long, conflicts_with = "amount")]
        readable_amount: Option<String>,
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
        /// Amount in minimal units. Mutually exclusive with --readable-amount.
        #[arg(long, conflicts_with = "readable_amount")]
        amount: Option<String>,
        /// Human-readable amount (e.g. "1.5" for 1.5 USDC). CLI fetches token decimals and converts automatically.
        #[arg(long, conflicts_with = "amount")]
        readable_amount: Option<String>,
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
    /// Check ERC-20 token approval allowance
    CheckApprovals {
        /// Chain (e.g. ethereum, xlayer)
        #[arg(long)]
        chain: String,
        /// Wallet address (owner)
        #[arg(long)]
        address: String,
        /// Token contract address to check
        #[arg(long)]
        token: String,
        /// Spender address (optional, defaults to OKX DEX router)
        #[arg(long)]
        spender: Option<String>,
    },
    /// Get supported chains for DEX aggregator
    Chains,
    /// Get available liquidity sources on a chain
    Liquidity {
        /// Chain
        #[arg(long)]
        chain: String,
    },
    /// One-shot swap: quote → approve (if needed) → swap → sign & broadcast → txHash
    Execute {
        /// Source token contract address
        #[arg(long)]
        from: String,
        /// Destination token contract address
        #[arg(long)]
        to: String,
        /// Amount in minimal units (wei/lamports). Mutually exclusive with --readable-amount.
        #[arg(long, conflicts_with = "readable_amount")]
        amount: Option<String>,
        /// Human-readable amount (e.g. "1.5" for 1.5 USDC). CLI fetches token decimals and converts automatically.
        #[arg(long, conflicts_with = "amount")]
        readable_amount: Option<String>,
        /// Chain (e.g. ethereum, solana, xlayer)
        #[arg(long)]
        chain: String,
        /// User wallet address
        #[arg(long)]
        wallet: String,
        /// Slippage tolerance in percent. Omit to use autoSlippage.
        #[arg(long)]
        slippage: Option<String>,
        /// Gas priority: slow, average, fast
        #[arg(long, default_value = "average")]
        gas_level: String,
        /// Swap mode: exactIn or exactOut
        #[arg(long, default_value = "exactIn")]
        swap_mode: String,
        /// Jito tips in SOL for Solana MEV protection
        #[arg(long)]
        tips: Option<String>,
        /// Max auto slippage percent cap
        #[arg(long)]
        max_auto_slippage: Option<String>,
        /// Enable MEV protection
        #[arg(long, default_value_t = false)]
        mev_protection: bool,
    },
}

pub async fn execute(ctx: &Context, cmd: SwapCommand) -> Result<()> {
    let client = ctx.client_async().await?;
    match cmd {
        SwapCommand::Quote {
            from,
            to,
            amount,
            readable_amount,
            chain,
            swap_mode,
        } => {
            let chain_index = crate::chains::resolve_chain(&chain);
            crate::chains::ensure_supported_chain(&chain_index, &chain)?;
            let raw_amount = resolve_amount_arg(&client, amount.as_deref(), readable_amount.as_deref(), &from, &chain_index).await?;
            output::success(
                fetch_quote(&client, &chain_index, &from, &to, &raw_amount, &swap_mode).await?,
            );
        }
        SwapCommand::Swap {
            from,
            to,
            amount,
            readable_amount,
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
            let raw_amount = resolve_amount_arg(&client, amount.as_deref(), readable_amount.as_deref(), &from, &chain_index).await?;
            output::success(
                fetch_swap(
                    &client,
                    &chain_index,
                    &from,
                    &to,
                    &raw_amount,
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
            validate_amount(&amount)?;
            let chain_index = crate::chains::resolve_chain(&chain);
            crate::chains::ensure_supported_chain(&chain_index, &chain)?;
            let resolved_token = resolve_token_address(&chain_index, &token);
            validate_token_for_chain(&chain_index, &resolved_token, "token")?;
            output::success(fetch_approve(&client, &chain_index, &token, &amount).await?);
        }
        SwapCommand::CheckApprovals {
            chain,
            address,
            token,
            spender,
        } => {
            let chain_index = crate::chains::resolve_chain(&chain);
            output::success(
                fetch_check_approvals(&client, &chain_index, &address, &token, spender.as_deref())
                    .await?,
            );
        }
        SwapCommand::Chains => {
            output::success(fetch_chains(&client).await?);
        }
        SwapCommand::Liquidity { chain } => {
            let chain_index = crate::chains::resolve_chain(&chain);
            crate::chains::ensure_supported_chain(&chain_index, &chain)?;
            output::success(fetch_liquidity(&client, &chain_index).await?);
        }
        SwapCommand::Execute {
            from,
            to,
            amount,
            readable_amount,
            chain,
            wallet,
            slippage,
            gas_level,
            swap_mode,
            tips,
            max_auto_slippage,
            mev_protection,
        } => {
            let chain_index = crate::chains::resolve_chain(&chain);
            crate::chains::ensure_supported_chain(&chain_index, &chain)?;
            let raw_amount = resolve_amount_arg(&client, amount.as_deref(), readable_amount.as_deref(), &from, &chain_index).await?;
            cmd_execute(
                &client,
                &from,
                &to,
                &raw_amount,
                &chain,
                &wallet,
                slippage.as_deref(),
                &gas_level,
                &swap_mode,
                tips.as_deref(),
                max_auto_slippage.as_deref(),
                mev_protection,
            )
            .await?;
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

// ── Pre-flight validation helpers ────────────────────────────────────

/// Validate that `amount` is a non-empty string of digits (no Infinity, NaN,
/// negative, zero-only, leading-zeros, or other non-numeric values).
fn validate_amount(amount: &str) -> Result<()> {
    let amount = amount.trim();
    if amount.is_empty() {
        bail!("--amount must not be empty");
    }
    if amount.contains('.') {
        bail!("--amount must be a whole number in minimal units (no decimals)");
    }
    if !amount.chars().all(|c| c.is_ascii_digit()) {
        bail!(
            "--amount must be a whole number in minimal units, got \"{}\". \
             Infinity, NaN, negative numbers and non-numeric values are not accepted.",
            amount
        );
    }
    if amount.chars().all(|c| c == '0') {
        bail!("--amount must be greater than zero");
    }
    if amount.starts_with('0') {
        bail!("--amount must not have leading zeros, got \"{}\"", amount);
    }
    Ok(())
}

/// Convert a human-readable decimal string to minimal units (integer string).
/// Uses string arithmetic to avoid floating-point precision issues.
/// e.g. "0.1" with decimal=6 → "100000", "1.5" with decimal=18 → "1500000000000000000"
pub(crate) fn readable_to_minimal_str(amount: &str, decimal: u32) -> Result<String> {
    let (integer, frac) = if let Some(dot_pos) = amount.find('.') {
        (&amount[..dot_pos], &amount[dot_pos + 1..])
    } else {
        (amount, "")
    };
    if integer.is_empty() || !integer.chars().all(|c| c.is_ascii_digit()) {
        bail!("--readable-amount must be a positive number, got \"{}\"", amount);
    }
    if !frac.chars().all(|c| c.is_ascii_digit()) {
        bail!("--readable-amount must be a positive number, got \"{}\"", amount);
    }
    let precision = decimal as usize;
    let frac_padded = if frac.len() >= precision {
        if frac[precision..].chars().any(|c| c != '0') {
            bail!(
                "--readable-amount \"{}\" has more decimal places than this token supports ({} decimals)",
                amount, decimal
            );
        }
        frac[..precision].to_string()
    } else {
        format!("{:0<width$}", frac, width = precision)
    };
    let combined = format!("{}{}", integer, frac_padded);
    let stripped = combined.trim_start_matches('0');
    let result = if stripped.is_empty() { "0" } else { stripped };
    if result == "0" {
        bail!(
            "--readable-amount {} is too small for this token ({} decimals); results in zero minimal units",
            amount, decimal
        );
    }
    Ok(result.to_string())
}

/// Resolve the effective raw amount from either --amount (raw) or --readable-amount (human-readable).
/// If --readable-amount is given, fetches token decimals via token info and converts.
async fn resolve_amount_arg(
    client: &ApiClient,
    amount: Option<&str>,
    readable_amount: Option<&str>,
    from: &str,
    chain_index: &str,
) -> Result<String> {
    if let Some(amt) = amount {
        let amt = amt.trim();
        validate_amount(amt)?;
        return Ok(amt.to_string());
    }
    if let Some(readable) = readable_amount {
        let readable = readable.trim();
        if readable.is_empty() {
            bail!("--readable-amount must not be empty");
        }
        let resolved_from = resolve_token_address(chain_index, from);
        let info = crate::commands::token::fetch_info(client, &resolved_from, chain_index)
            .await
            .map_err(|e| {
                anyhow::anyhow!(
                    "Failed to fetch token decimals for {}: {}. Use --amount with raw units instead.",
                    resolved_from, e
                )
            })?;
        let info_arr = info.as_array().filter(|a| !a.is_empty()).ok_or_else(|| {
            anyhow::anyhow!(
                "Token not found for address {} on chain {}. Verify the address is correct. \
                 Use --amount with raw units instead.",
                resolved_from, chain_index
            )
        })?;
        let decimal: u32 = match &info_arr[0]["decimal"] {
            serde_json::Value::String(s) => s.parse().map_err(|_| {
                anyhow::anyhow!("Invalid decimal value \"{}\" for token {}", s, resolved_from)
            })?,
            serde_json::Value::Number(n) => n.as_u64().ok_or_else(|| {
                anyhow::anyhow!("Invalid decimal value for token {}", resolved_from)
            })? as u32,
            _ => anyhow::bail!(
                "Token decimal not found for {}. Use --amount with raw units instead.",
                resolved_from
            ),
        };
        return readable_to_minimal_str(readable, decimal);
    }
    bail!("Either --amount or --readable-amount is required")
}


/// Called after `resolve_token_address` so we inspect the actual address.
///
/// Note: chain_family() is a binary "solana" / "evm" function and classifies
/// Tron (195), TON (607), and Sui (784) as "evm" for historical reasons.
/// Those chains have their own address formats, so we skip format validation
/// for them and only check genuine Solana vs. EVM chains.
fn validate_token_for_chain(chain_index: &str, token: &str, label: &str) -> Result<()> {
    match chain_index {
        // Solana: must not be a 0x-prefixed EVM address.
        "501" => {
            if token.starts_with("0x") || token.starts_with("0X") {
                bail!(
                    "--{label} looks like an EVM address (0x…) but chain is Solana. \
                     Solana uses base58 addresses (e.g. EPjFWdd5...wyTDt1v). \
                     Did you mean to use a different chain?"
                );
            }
        }
        // Tron / TON / Sui — their native address formats differ from both EVM and Solana;
        // skip format validation and let the API handle address errors.
        "195" | "607" | "784" => {}
        // EVM chains: heuristic — 32-44 alphanumeric chars with uppercase → likely Solana base58.
        _ => {
            if !token.starts_with("0x")
                && !token.starts_with("0X")
                && token.len() >= 32
                && token.len() <= 44
                && token.chars().all(|c| c.is_ascii_alphanumeric())
                && token.chars().any(|c| c.is_ascii_uppercase())
            {
                bail!(
                    "--{label} looks like a Solana/base58 address but chain is EVM (chainIndex={chain_index}). \
                     EVM addresses start with 0x (e.g. 0xa0b869...606eb48). \
                     Did you mean to use --chain solana?"
                );
            }
        }
    }
    Ok(())
}

/// Reject swaps where fromToken and toToken are the same address.
fn ensure_different_tokens(from: &str, to: &str) -> Result<()> {
    if from.eq_ignore_ascii_case(to) {
        bail!(
            "fromToken and toToken are the same address ({}). Cannot swap a token to itself.",
            from
        );
    }
    Ok(())
}

/// Validate resolved token pair: format matches chain + tokens are different.
/// Call after `resolve_token_address`.
fn validate_swap_params(chain_index: &str, from: &str, to: &str) -> Result<()> {
    validate_token_for_chain(chain_index, from, "from")?;
    validate_token_for_chain(chain_index, to, "to")?;
    ensure_different_tokens(from, to)?;
    Ok(())
}

// ── Aggregator API functions ─────────────────────────────────────────

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
    validate_swap_params(chain_index, &from, &to)?;
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
            eprintln!("[DEBUG][fetch_quote] to resolved: {} → {}", orig_to, to);
        }
    }
    // Generate trace ID: resolved from address + timestamp
    let timestamp = chrono::Utc::now().timestamp_millis().to_string();
    let tid = format!("{}{}", from, timestamp);
    // Save to cache (best-effort, don't fail the request)
    let _ = crate::wallet_store::set_swap_trace_id(&tid);

    let params = vec![
        ("chainIndex", chain_index),
        ("fromTokenAddress", from.as_str()),
        ("toTokenAddress", to.as_str()),
        ("amount", amount),
        ("swapMode", swap_mode),
    ];
    let headers = [
        ("ok-client-tid", tid.as_str()),
        ("ok-client-timestamp", timestamp.as_str()),
    ];
    if cfg!(feature = "debug-log") {
        eprintln!(
            "[DEBUG][fetch_quote] trace headers: ok-client-tid={}, ok-client-timestamp={}",
            tid, timestamp
        );
    }
    let result = client
        .get_with_headers("/api/v6/dex/aggregator/quote", &params, Some(&headers))
        .await;
    if cfg!(feature = "debug-log") {
        eprintln!("[DEBUG][fetch_quote] response: {:?}", result);
    }
    result
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
    validate_swap_params(chain_index, &from, &to)?;
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
            eprintln!("[DEBUG][fetch_swap] to resolved: {} → {}", orig_to, to);
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
    // Read swap trace ID from cache; attach trace headers if present
    let cached_tid = crate::wallet_store::get_swap_trace_id().ok().flatten();
    let result = if let Some(ref tid) = cached_tid {
        let timestamp = chrono::Utc::now().timestamp_millis().to_string();
        if cfg!(feature = "debug-log") {
            eprintln!(
                "[DEBUG][fetch_swap] trace headers: ok-client-tid={}, ok-client-timestamp={}",
                tid, timestamp
            );
        }
        let headers = [
            ("ok-client-tid", tid.as_str()),
            ("ok-client-timestamp", timestamp.as_str()),
        ];
        client
            .get_with_headers("/api/v6/dex/aggregator/swap", &params, Some(&headers))
            .await
    } else {
        client.get("/api/v6/dex/aggregator/swap", &params).await
    };
    if cfg!(feature = "debug-log") {
        eprintln!("[DEBUG][fetch_swap] response: {:?}", result);
    }
    result
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
    let result = client
        .get(
            "/api/v6/dex/aggregator/approve-transaction",
            &[
                ("chainIndex", chain_index),
                ("tokenContractAddress", token.as_str()),
                ("approveAmount", amount),
            ],
        )
        .await;
    if cfg!(feature = "debug-log") {
        eprintln!("[DEBUG][fetch_approve] response: {:?}", result);
    }
    result
}

/// POST /api/v6/dex/pre-transaction/check-approvals
pub async fn fetch_check_approvals(
    client: &ApiClient,
    chain_index: &str,
    address: &str,
    token: &str,
    spender: Option<&str>,
) -> Result<Value> {
    if cfg!(feature = "debug-log") {
        eprintln!(
            "[DEBUG][fetch_check_approvals] chain_index={}, address={}, token={}, spender={:?}",
            chain_index, address, token, spender
        );
    }
    let mut body = json!({
        "chainIndex": chain_index,
        "address": address,
        "tokens": [{ "tokenContractAddress": token }],
    });
    if let Some(s) = spender {
        body["spender"] = json!(s);
    }
    let result = client
        .post("/api/v6/dex/pre-transaction/check-approvals", &body)
        .await;
    if cfg!(feature = "debug-log") {
        eprintln!("[DEBUG][fetch_check_approvals] response: {:?}", result);
    }
    result
}

/// GET /api/v6/dex/aggregator/supported/chain
pub async fn fetch_chains(client: &ApiClient) -> Result<Value> {
    if cfg!(feature = "debug-log") {
        eprintln!("[DEBUG][fetch_chains] fetching supported chains");
    }
    let result = client
        .get("/api/v6/dex/aggregator/supported/chain", &[])
        .await;
    if cfg!(feature = "debug-log") {
        eprintln!("[DEBUG][fetch_chains] response: {:?}", result);
    }
    result
}

/// GET /api/v6/dex/aggregator/get-liquidity
pub async fn fetch_liquidity(client: &ApiClient, chain_index: &str) -> Result<Value> {
    if cfg!(feature = "debug-log") {
        eprintln!("[DEBUG][fetch_liquidity] chain_index={}", chain_index);
    }
    let result = client
        .get(
            "/api/v6/dex/aggregator/get-liquidity",
            &[("chainIndex", chain_index)],
        )
        .await;
    if cfg!(feature = "debug-log") {
        eprintln!("[DEBUG][fetch_liquidity] response: {:?}", result);
    }
    result
}

// ── Execute orchestration ────────────────────────────────────────────

/// Run an onchainos subcommand as a subprocess and return the `data` field from
/// the `{ "ok": true, "data": ... }` output envelope.
/// This keeps swap independent of wallet internals.
async fn run_onchainos_cmd(args: &[&str]) -> Result<Value> {
    if cfg!(feature = "debug-log") {
        eprintln!("[DEBUG][run_onchainos_cmd] args: {:?}", args);
    }
    let exe = std::env::current_exe().unwrap_or_else(|_| "onchainos".into());
    let output = tokio::process::Command::new(&exe)
        .args(args)
        .output()
        .await
        .map_err(|e| {
            anyhow::anyhow!(
                "failed to spawn onchainos {}: {e}",
                args.first().unwrap_or(&"")
            )
        })?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let stdout = String::from_utf8_lossy(&output.stdout);
        // Try to extract error message from JSON output envelope
        if let Ok(parsed) = serde_json::from_str::<Value>(stdout.trim()) {
            if let Some(err_msg) = parsed["error"].as_str() {
                bail!("{}", err_msg);
            }
        }
        bail!(
            "onchainos {} failed (exit {}): {}",
            args.first().unwrap_or(&""),
            output.status.code().unwrap_or(-1),
            stderr.trim(),
        );
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let parsed: Value = serde_json::from_str(stdout.trim())
        .map_err(|e| anyhow::anyhow!("failed to parse onchainos output: {e}"))?;

    // Unwrap the { "ok": true, "data": ... } envelope
    if parsed["ok"].as_bool() != Some(true) {
        let err_msg = parsed["error"].as_str().unwrap_or("unknown error");
        bail!("onchainos command failed: {}", err_msg);
    }

    if cfg!(feature = "debug-log") {
        eprintln!("[DEBUG][run_onchainos_cmd] result: {}", parsed["data"]);
    }
    Ok(parsed["data"].clone())
}

/// Run `onchainos wallet contract-call` and return the `data` field.
async fn wallet_contract_call(args: &[&str]) -> Result<Value> {
    let mut full_args = vec!["wallet", "contract-call"];
    full_args.extend_from_slice(args);
    run_onchainos_cmd(&full_args).await
}

/// Extract txHash from `wallet contract-call` output data.
fn extract_tx_hash(data: &Value) -> Result<String> {
    data["txHash"]
        .as_str()
        .map(|s| s.to_string())
        .ok_or_else(|| anyhow::anyhow!("missing txHash in contract-call output"))
}

#[allow(clippy::too_many_arguments)]
async fn cmd_execute(
    client: &ApiClient,
    from_token: &str,
    to_token: &str,
    amount: &str,
    chain: &str,
    wallet_address: &str,
    slippage: Option<&str>,
    gas_level: &str,
    swap_mode: &str,
    tips: Option<&str>,
    max_auto_slippage: Option<&str>,
    mev_protection: bool,
) -> Result<()> {
    use crate::chains;

    let chain_index = chains::resolve_chain(chain);
    let family = chains::chain_family(&chain_index);
    let native_addr = chains::native_token_address(&chain_index);
    let from_token = resolve_token_address(&chain_index, from_token);
    let to_token = resolve_token_address(&chain_index, to_token);
    let is_from_native = from_token.eq_ignore_ascii_case(native_addr);

    if cfg!(feature = "debug-log") {
        eprintln!(
            "[DEBUG][cmd_execute] from_token={}, to_token={}, amount={}, chain={} (chain_index={}, family={}), wallet={}, slippage={:?}, gas_level={}, swap_mode={}, tips={:?}, max_auto_slippage={:?}, mev_protection={}",
            from_token, to_token, amount, chain, chain_index, family, wallet_address, slippage, gas_level, swap_mode, tips, max_auto_slippage, mev_protection
        );
    }

    // ── 1. Approve (EVM + non-native only) ──────────────────────────
    let mut approve_tx_hash: Option<String> = None;

    if family == "evm" && !is_from_native {
        let approvals =
            fetch_check_approvals(client, &chain_index, wallet_address, &from_token, None).await?;

        let spendable = approvals
            .as_array()
            .and_then(|arr| arr.first())
            .and_then(|r| r["tokens"].as_array())
            .and_then(|tokens| tokens.first())
            .and_then(|t| t["spendable"].as_str())
            .unwrap_or("0");

        if cfg!(feature = "debug-log") {
            eprintln!(
                "[DEBUG][cmd_execute] spendable={}, amount={}, needs_approve={}",
                spendable, amount, is_allowance_insufficient(spendable, amount)
            );
        }

        if is_allowance_insufficient(spendable, amount) {
            // USDT pattern: non-zero but insufficient → revoke first
            let spendable_nonzero = spendable != "0" && !spendable.is_empty();
            if spendable_nonzero {
                if cfg!(feature = "debug-log") {
                    eprintln!("[swap execute] revoking stale approval (USDT pattern)...");
                }
                let revoke_data = fetch_approve(client, &chain_index, &from_token, "0").await?;
                let revoke_calldata = extract_approve_calldata(&revoke_data)?;

                let result = wallet_contract_call(&[
                    "--to",
                    &from_token,
                    "--chain",
                    &chain_index,
                    "--input-data",
                    &revoke_calldata,
                ])
                .await?;
                // We don't need the revoke txHash in output, just ensure it succeeded
                extract_tx_hash(&result)?;
            }

            if cfg!(feature = "debug-log") {
                eprintln!("[swap execute] approving token...");
            }
            let approve_data = fetch_approve(client, &chain_index, &from_token, amount).await?;
            let approve_calldata = extract_approve_calldata(&approve_data)?;

            let result = wallet_contract_call(&[
                "--to",
                &from_token,
                "--chain",
                &chain_index,
                "--input-data",
                &approve_calldata,
            ])
            .await?;
            approve_tx_hash = Some(extract_tx_hash(&result)?);
        }
    }

    // ── 4. Swap ──────────────────────────────────────────────────────
    if cfg!(feature = "debug-log") {
        eprintln!("[swap execute] executing swap...");
    }
    let swap_data = fetch_swap(
        client,
        &chain_index,
        &from_token,
        &to_token,
        amount,
        slippage,
        wallet_address,
        swap_mode,
        gas_level,
        tips,
        max_auto_slippage,
    )
    .await?;

    let swap_result = unwrap_api_array(&swap_data);
    if swap_result.is_null() {
        bail!("swap API returned empty result");
    }
    if cfg!(feature = "debug-log") {
        eprintln!("[DEBUG][cmd_execute] swap_result: {}", swap_result);
    }

    let tx = &swap_result["tx"];

    // ── 5. Sign & broadcast swap tx via wallet contract-call ─────────
    let swap_tx_hash = if family == "solana" {
        let unsigned_tx = tx["data"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("missing tx.data (unsigned tx) in swap response"))?;
        let to_addr = tx["to"].as_str().unwrap_or("");

        let mut args = vec![
            "--to",
            to_addr,
            "--chain",
            &chain_index,
            "--unsigned-tx",
            unsigned_tx,
        ];

        // Jito MEV protection
        if let Some(jito_tx) = swap_result["jitoCalldata"].as_str() {
            args.extend_from_slice(&["--jito-unsigned-tx", jito_tx, "--mev-protection"]);
        } else if mev_protection {
            args.push("--mev-protection");
        }

        let result = wallet_contract_call(&args).await?;
        extract_tx_hash(&result)?
    } else {
        let to_addr = tx["to"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("missing tx.to in swap response"))?;
        let input_data = tx["data"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("missing tx.data in swap response"))?;
        let tx_value_wei = tx["value"].as_str().unwrap_or("0");

        let mut args = vec![
            "--to",
            to_addr,
            "--chain",
            &chain_index,
            "--amt",
            &tx_value_wei,
            "--input-data",
            input_data,
        ];

        // Gas limit from swap response
        let gas_limit_val;
        if let Some(g) = tx["gas"].as_str() {
            gas_limit_val = g.to_string();
            args.extend_from_slice(&["--gas-limit", &gas_limit_val]);
        }

        // XLayer AA DEX params
        let from_token_amount;
        if chain_index == "196" {
            from_token_amount = swap_result["routerResult"]["fromTokenAmount"]
                .as_str()
                .unwrap_or(amount)
                .to_string();
            args.extend_from_slice(&[
                "--aa-dex-token-addr",
                &from_token,
                "--aa-dex-token-amount",
                &from_token_amount,
            ]);
        }

        if mev_protection {
            args.push("--mev-protection");
        }

        let result = wallet_contract_call(&args).await?;
        extract_tx_hash(&result)?
    };

    // ── 6. Output ────────────────────────────────────────────────────
    if cfg!(feature = "debug-log") {
        eprintln!(
            "[DEBUG][cmd_execute] swap_tx_hash={}, approve_tx_hash={:?}",
            swap_tx_hash, approve_tx_hash
        );
    }
    let router_result = &swap_result["routerResult"];
    output::success(json!({
        "approveTxHash": approve_tx_hash,
        "swapTxHash": swap_tx_hash,
        "fromToken": router_result["fromToken"],
        "toToken": router_result["toToken"],
        "fromAmount": router_result["fromTokenAmount"],
        "toAmount": router_result["toTokenAmount"],
        "priceImpact": router_result["priceImpactPercent"],
        "gasUsed": router_result["estimateGasFee"],
    }));

    Ok(())
}

// ── Helpers ──────────────────────────────────────────────────────────

/// If the API returns an array, extract the first element; otherwise return as-is.
fn unwrap_api_array(data: &Value) -> Value {
    if data.is_array() {
        data.as_array()
            .and_then(|a| a.first())
            .cloned()
            .unwrap_or(Value::Null)
    } else {
        data.clone()
    }
}

/// Extract calldata from approve API response.
fn extract_approve_calldata(approve_data: &Value) -> Result<String> {
    let obj = unwrap_api_array(approve_data);
    obj["data"]
        .as_str()
        .map(|s| s.to_string())
        .ok_or_else(|| anyhow::anyhow!("missing 'data' field in approve response"))
}

/// Compare allowance (spendable) against required amount.
/// Both are decimal strings in minimal units. Returns true if allowance < amount.
fn is_allowance_insufficient(spendable: &str, amount: &str) -> bool {
    // If spendable is very long (uint256 max approval = 78 digits), treat as sufficient.
    // This avoids u128 overflow for unlimited approvals.
    if spendable.len() > 38 {
        return false;
    }
    let spendable_val = spendable.parse::<u128>().unwrap_or(0);
    let amount_val = amount.parse::<u128>().unwrap_or(u128::MAX);
    spendable_val < amount_val
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_is_allowance_insufficient() {
        assert!(is_allowance_insufficient("0", "1000000"));
        assert!(is_allowance_insufficient("999999", "1000000"));
        assert!(!is_allowance_insufficient("1000000", "1000000"));
        assert!(!is_allowance_insufficient("2000000", "1000000"));
        // Unparseable spendable defaults to 0 → insufficient
        assert!(is_allowance_insufficient("abc", "1000000"));
        // uint256 max approval (78 digits) → sufficient (not insufficient)
        let uint256_max =
            "115792089237316195423570985008687907853269984665640564039457584007913129639935";
        assert!(!is_allowance_insufficient(uint256_max, "1000000"));
    }

    #[test]
    fn test_readable_to_minimal_str() {
        // USDC: 6 decimals
        assert_eq!(readable_to_minimal_str("0.1", 6).unwrap(), "100000");
        assert_eq!(readable_to_minimal_str("1.5", 6).unwrap(), "1500000");
        assert_eq!(readable_to_minimal_str("100", 6).unwrap(), "100000000");
        assert_eq!(readable_to_minimal_str("1", 6).unwrap(), "1000000");
        assert_eq!(readable_to_minimal_str("0.000001", 6).unwrap(), "1");
        // ETH: 18 decimals
        assert_eq!(readable_to_minimal_str("0.1", 18).unwrap(), "100000000000000000");
        assert_eq!(readable_to_minimal_str("1", 18).unwrap(), "1000000000000000000");
        // SOL: 9 decimals
        assert_eq!(readable_to_minimal_str("1", 9).unwrap(), "1000000000");
        // 超出精度且非零 → error
        assert!(readable_to_minimal_str("0.1234567", 6).is_err());
        assert!(readable_to_minimal_str("1.00000002", 2).is_err());
        // 超出精度但全是零 → ok
        assert_eq!(readable_to_minimal_str("1.000", 2).unwrap(), "100");
        assert_eq!(readable_to_minimal_str("0.1230000", 6).unwrap(), "123000");
    }
}
