use std::collections::HashMap;
use std::sync::LazyLock;

use anyhow::{bail, Result};
use clap::Subcommand;
use serde_json::{json, Value};

use super::Context;
use crate::client::ApiClient;
use crate::output;
use crate::token_alias::{resolve_token_address, validate_address_for_chain};

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
        /// Jito tips in SOL for Solana MEV protection (range: 0.0000000001–2)
        #[arg(long)]
        tips: Option<String>,
        /// Max auto slippage percent cap
        #[arg(long)]
        max_auto_slippage: Option<String>,
        /// Enable MEV protection
        #[arg(long, default_value_t = false)]
        mev_protection: bool,
        /// Gas token contract address for Gas Station payment (from tokenList).
        /// Applied to both approve and swap transactions when set.
        #[arg(long)]
        gas_token_address: Option<String>,
        /// Relayer ID for Gas Station (from tokenList). Must be paired with --gas-token-address.
        #[arg(long)]
        relayer_id: Option<String>,
        /// Enable Gas Station first-time activation or re-enable. Pins --gas-token-address as default.
        #[arg(long, default_value_t = false)]
        enable_gas_station: bool,
        /// Force execution: skip backend risk warning 81362 (skipWarning=true on broadcast).
        /// Use only after explicit user confirmation.
        #[arg(long, default_value_t = false)]
        force: bool,
    },
}

pub async fn execute(ctx: &Context, cmd: SwapCommand) -> Result<()> {
    let mut client = ctx.client_async().await?;
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
            let raw_amount = resolve_amount_arg(
                &mut client,
                amount.as_deref(),
                readable_amount.as_deref(),
                &from,
                &chain_index,
            )
            .await?;
            output::success(
                fetch_quote(
                    &mut client,
                    &chain_index,
                    &from,
                    &to,
                    &raw_amount,
                    &swap_mode,
                )
                .await?,
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
            let raw_amount = resolve_amount_arg(
                &mut client,
                amount.as_deref(),
                readable_amount.as_deref(),
                &from,
                &chain_index,
            )
            .await?;
            output::success(
                fetch_swap(
                    &mut client,
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
            let chain_index = crate::chains::resolve_chain(&chain);
            crate::chains::ensure_supported_chain(&chain_index, &chain)?;
            output::success(fetch_approve(&mut client, &chain_index, &token, &amount).await?);
        }
        SwapCommand::CheckApprovals {
            chain,
            address,
            token,
            spender,
        } => {
            let chain_index = crate::chains::resolve_chain(&chain);
            output::success(
                fetch_check_approvals(
                    &mut client,
                    &chain_index,
                    &address,
                    &token,
                    spender.as_deref(),
                )
                .await?,
            );
        }
        SwapCommand::Chains => {
            output::success(fetch_chains(&mut client).await?);
        }
        SwapCommand::Liquidity { chain } => {
            let chain_index = crate::chains::resolve_chain(&chain);
            crate::chains::ensure_supported_chain(&chain_index, &chain)?;
            output::success(fetch_liquidity(&mut client, &chain_index).await?);
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
            gas_token_address,
            relayer_id,
            enable_gas_station,
            force,
        } => {
            let chain_index = crate::chains::resolve_chain(&chain);
            crate::chains::ensure_supported_chain(&chain_index, &chain)?;
            let raw_amount = resolve_amount_arg(
                &mut client,
                amount.as_deref(),
                readable_amount.as_deref(),
                &from,
                &chain_index,
            )
            .await?;
            cmd_execute(
                &mut client,
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
                gas_token_address.as_deref(),
                relayer_id.as_deref(),
                enable_gas_station,
                force,
            )
            .await?;
        }
    }
    Ok(())
}

// ── Pre-flight validation helpers ────────────────────────────────────

/// Validate that `amount` is a non-empty string of digits (no Infinity, NaN,
/// negative, zero-only, leading-zeros, or other non-numeric values).
pub(crate) fn validate_amount(amount: &str) -> Result<()> {
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

/// Validate that `slippage` is a number strictly greater than 0 and at most 100.
/// Accepts decimals like "0.5", "1", "99.9", "100". Rejects "0", negatives, >100, non-numeric.
fn validate_slippage(slippage: &str) -> Result<()> {
    let slippage = slippage.trim();
    let val: f64 = slippage.parse().map_err(|_| {
        anyhow::anyhow!(
            "--slippage must be a number between 0 (exclusive) and 100 (inclusive), got \"{}\"",
            slippage
        )
    })?;
    if val.is_nan() || val.is_infinite() {
        bail!(
            "--slippage must be a finite number between 0 (exclusive) and 100 (inclusive), got \"{}\"",
            slippage
        );
    }
    if val <= 0.0 || val > 100.0 {
        bail!(
            "--slippage must be greater than 0 and at most 100, got \"{}\"",
            slippage
        );
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
        bail!(
            "--readable-amount must be a positive number, got \"{}\"",
            amount
        );
    }
    if !frac.chars().all(|c| c.is_ascii_digit()) {
        bail!(
            "--readable-amount must be a positive number, got \"{}\"",
            amount
        );
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
pub(crate) async fn resolve_amount_arg(
    client: &mut ApiClient,
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
                resolved_from,
                chain_index
            )
        })?;
        let decimal: u32 = match &info_arr[0]["decimal"] {
            serde_json::Value::String(s) => s.parse().map_err(|_| {
                anyhow::anyhow!(
                    "Invalid decimal value \"{}\" for token {}",
                    s,
                    resolved_from
                )
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
    validate_address_for_chain(chain_index, from, "from")?;
    validate_address_for_chain(chain_index, to, "to")?;
    ensure_different_tokens(from, to)?;
    Ok(())
}

/// Validate that `swap_mode` is one of the accepted values: "exactIn" or "exactOut".
fn validate_swap_mode(swap_mode: &str) -> Result<()> {
    match swap_mode {
        "exactIn" | "exactOut" => Ok(()),
        _ => bail!(
            "--swap-mode must be \"exactIn\" or \"exactOut\", got \"{}\"",
            swap_mode
        ),
    }
}

/// Validate that `gas_level` is one of the accepted values: "slow", "average", or "fast".
fn validate_gas_level(gas_level: &str) -> Result<()> {
    match gas_level {
        "slow" | "average" | "fast" => Ok(()),
        _ => bail!(
            "--gas-level must be \"slow\", \"average\", or \"fast\", got \"{}\"",
            gas_level
        ),
    }
}

/// Validate that `tips` is a positive integer (greater than 0).
fn validate_tips(tips: &str) -> Result<()> {
    let tips = tips.trim();
    if tips.is_empty() {
        bail!("--tips must not be empty");
    }
    let val: f64 = tips
        .parse()
        .map_err(|_| anyhow::anyhow!("--tips must be a number in SOL, got \"{}\"", tips))?;
    if val < 1e-10 {
        bail!("--tips must be at least 0.0000000001 SOL, got \"{}\"", tips);
    }
    if val > 2.0 {
        bail!("--tips must be at most 2 SOL, got \"{}\"", tips);
    }
    Ok(())
}

/// Validate non-negative integer string (≥ 0). Used for gasLimit, aaDexTokenAmount, etc.
pub(crate) fn validate_non_negative_integer(value: &str, label: &str) -> Result<()> {
    let value = value.trim();
    if value.is_empty() {
        bail!("--{} must not be empty", label);
    }
    if !value.chars().all(|c| c.is_ascii_digit()) {
        bail!(
            "--{} must be a non-negative integer, got \"{}\"",
            label,
            value
        );
    }
    // Allow "0", but reject leading zeros like "007"
    if value.len() > 1 && value.starts_with('0') {
        bail!("--{} must not have leading zeros, got \"{}\"", label, value);
    }
    Ok(())
}

// ── Aggregator API functions ─────────────────────────────────────────

/// GET /api/v6/dex/aggregator/quote
pub async fn fetch_quote(
    client: &mut ApiClient,
    chain_index: &str,
    from: &str,
    to: &str,
    amount: &str,
    swap_mode: &str,
) -> Result<Value> {
    if !swap_mode.is_empty() {
        validate_swap_mode(swap_mode)?;
    }
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
    // Generate trace ID: resolved from address + timestamp (not cached; quote has its own independent tid)
    let timestamp = chrono::Utc::now().timestamp_millis().to_string();
    let tid = format!("{}{}", from, timestamp);

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
    client: &mut ApiClient,
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
    // ── Input validation ──
    if !swap_mode.is_empty() {
        validate_swap_mode(swap_mode)?;
    }
    if !gas_level.is_empty() {
        validate_gas_level(gas_level)?;
    }
    if let Some(s) = slippage {
        validate_slippage(s)?;
    }
    if let Some(t) = tips {
        validate_tips(t)?;
    }
    if let Some(m) = max_auto_slippage {
        validate_slippage(m)?;
    }
    validate_address_for_chain(chain_index, wallet, "wallet")?;
    validate_amount(amount)?;

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
    // Generate a new trace ID for the swap flow and save to cache
    let timestamp = chrono::Utc::now().timestamp_millis().to_string();
    let tid = format!("{}{}", from, timestamp);
    // Save to cache (best-effort) — downstream sign_and_broadcast reads it for contract calls
    let _ = crate::wallet_store::set_swap_trace_id(&tid);
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
    let result = client
        .get_with_headers("/api/v6/dex/aggregator/swap", &params, Some(&headers))
        .await;
    if cfg!(feature = "debug-log") {
        eprintln!("[DEBUG][fetch_swap] response: {:?}", result);
    }
    result
}

/// Validate that `amount` is a non-negative integer string (allows "0" for revoke).
fn validate_approve_amount(amount: &str) -> Result<()> {
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
    // Allow "0" for revoke, but reject leading zeros like "007"
    if amount.len() > 1 && amount.starts_with('0') {
        bail!("--amount must not have leading zeros, got \"{}\"", amount);
    }
    Ok(())
}

/// GET /api/v6/dex/aggregator/approve-transaction
pub async fn fetch_approve(
    client: &mut ApiClient,
    chain_index: &str,
    token: &str,
    amount: &str,
) -> Result<Value> {
    // ── Input validation ──
    validate_approve_amount(amount)?;
    let orig_token = token;
    let token = resolve_token_address(chain_index, orig_token);
    validate_address_for_chain(chain_index, &token, "token")?;
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
    client: &mut ApiClient,
    chain_index: &str,
    address: &str,
    token: &str,
    spender: Option<&str>,
) -> Result<Value> {
    // ── Input validation ──
    validate_address_for_chain(chain_index, address, "address")?;
    let token = resolve_token_address(chain_index, token);
    validate_address_for_chain(chain_index, &token, "token")?;
    if let Some(s) = spender {
        validate_address_for_chain(chain_index, s, "spender")?;
    }
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
pub async fn fetch_chains(client: &mut ApiClient) -> Result<Value> {
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
pub async fn fetch_liquidity(client: &mut ApiClient, chain_index: &str) -> Result<Value> {
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

/// Call `execute_contract_call` directly and return the txHash wrapped in a JSON value.
#[allow(clippy::too_many_arguments)]
async fn wallet_contract_call(
    to: &str,
    chain: &str,
    amt: &str,
    input_data: Option<&str>,
    unsigned_tx: Option<&str>,
    gas_limit: Option<&str>,
    aa_dex_token_addr: Option<&str>,
    aa_dex_token_amount: Option<&str>,
    mev_protection: bool,
    jito_unsigned_tx: Option<&str>,
    gas_token_address: Option<&str>,
    relayer_id: Option<&str>,
    enable_gas_station: bool,
    force: bool,
) -> Result<Value> {
    let resp = crate::commands::agentic_wallet::transfer::execute_contract_call(
        to,
        chain,
        amt,
        input_data,
        unsigned_tx,
        gas_limit,
        None, // from: use selected account
        aa_dex_token_addr,
        aa_dex_token_amount,
        mev_protection,
        jito_unsigned_tx,
        force,
        None, // tx_source: not cross-chain
        gas_token_address,
        relayer_id,
        enable_gas_station,
        Some("dex"), // agent_biz_type: swap flow
        None,        // agent_skill_name
    )
    .await?;
    Ok(json!({ "txHash": resp.tx_hash, "orderId": resp.order_id }))
}

/// Extract txHash from `wallet contract-call` output data.
fn extract_tx_hash(data: &Value) -> Result<String> {
    data["txHash"]
        .as_str()
        .map(|s| s.to_string())
        .ok_or_else(|| anyhow::anyhow!("missing txHash in contract-call output"))
}

/// Extract (txHash, orderId) from `wallet contract-call` output data. orderId
/// may be empty for non-Gas-Station broadcasts; only txHash is required.
fn extract_tx_hash_and_order_id(data: &Value) -> Result<(String, String)> {
    let tx_hash = data["txHash"]
        .as_str()
        .map(|s| s.to_string())
        .ok_or_else(|| anyhow::anyhow!("missing txHash in contract-call output"))?;
    let order_id = data["orderId"].as_str().unwrap_or("").to_string();
    Ok((tx_hash, order_id))
}

#[allow(clippy::too_many_arguments)]
async fn cmd_execute(
    client: &mut ApiClient,
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
    gas_token_address: Option<&str>,
    relayer_id: Option<&str>,
    enable_gas_station: bool,
    force: bool,
) -> Result<()> {
    use crate::chains;

    let chain_index = chains::resolve_chain(chain);
    let family = chains::chain_family(&chain_index);
    let native_addr = chains::native_token_address(&chain_index);
    let from_token = resolve_token_address(&chain_index, from_token);
    let to_token = resolve_token_address(&chain_index, to_token);
    validate_swap_params(&chain_index, &from_token, &to_token)?;
    let is_from_native = from_token.eq_ignore_ascii_case(native_addr);

    // Routing:
    //   non-native + no Gas Station params + chain in support list  → batch path
    //   any condition fails                                         → single-tx path
    let batch_supported = !is_from_native
        && !enable_gas_station
        && gas_token_address.is_none()
        && relayer_id.is_none()
        && is_chain_batch_supported(&chain_index).await;
    if batch_supported {
        return cmd_execute_batch(
            client,
            &from_token,
            &to_token,
            amount,
            &chain_index,
            wallet_address,
            slippage,
            gas_level,
            swap_mode,
            tips,
            max_auto_slippage,
            mev_protection,
            force,
        )
        .await;
    }

    // Gas Station `enableGasStation=true` must only fire on the FIRST gas-consuming
    // tx of this flow (revoke / approve / swap, whichever runs first). After that,
    // the account is activated — subsequent txs only need gasTokenAddress + relayerId.
    let mut gs_enable_remaining = enable_gas_station;

    if cfg!(feature = "debug-log") {
        eprintln!(
            "[DEBUG][cmd_execute] from_token={}, to_token={}, amount={}, chain={} (chain_index={}, family={}), wallet={}, slippage={:?}, gas_level={}, swap_mode={}, tips={:?}, max_auto_slippage={:?}, mev_protection={}",
            from_token, to_token, amount, chain, chain_index, family, wallet_address, slippage, gas_level, swap_mode, tips, max_auto_slippage, mev_protection
        );
    }

    // ── 1. Approve (EVM + non-native only) ──────────────────────────
    let mut approve_tx_hash: Option<String> = None;
    let mut approve_order_id: Option<String> = None;

    if family == "evm" && !is_from_native {
        // Fetch approve-transaction first to get dexContractAddress (spender) and calldata
        let approve_data = fetch_approve(client, &chain_index, &from_token, amount).await?;
        let approve_obj = unwrap_api_array(&approve_data);
        let approve_calldata = approve_obj["data"]
            .as_str()
            .map(|s| s.to_string())
            .ok_or_else(|| anyhow::anyhow!("missing 'data' field in approve response"))?;
        let dex_contract_address = approve_obj["dexContractAddress"]
            .as_str()
            .map(|s| s.to_string());
        if cfg!(feature = "debug-log") {
            eprintln!(
                "[DEBUG][cmd_execute] dexContractAddress={:?}",
                dex_contract_address
            );
        }

        let approvals = fetch_check_approvals(
            client,
            &chain_index,
            wallet_address,
            &from_token,
            dex_contract_address.as_deref(),
        )
        .await?;

        let spendable = approvals
            .as_array()
            .and_then(|arr| arr.first())
            .and_then(|r| r["tokens"].as_array())
            .and_then(|tokens| tokens.first())
            .and_then(|t| t["spendable"].as_str())
            .unwrap_or("0");

        let (needs_approve, needs_revoke) =
            classify_approve_action(&chain_index, &from_token, spendable, amount);

        if cfg!(feature = "debug-log") {
            eprintln!(
                "[DEBUG][cmd_execute] spendable={}, amount={}, needs_approve={}, needs_revoke={}",
                spendable, amount, needs_approve, needs_revoke
            );
        }

        if needs_approve {
            if needs_revoke {
                if cfg!(feature = "debug-log") {
                    eprintln!("[swap execute] revoking stale approval (USDT pattern)...");
                }
                let revoke_data = fetch_approve(client, &chain_index, &from_token, "0").await?;
                let revoke_calldata = extract_approve_calldata(&revoke_data)?;

                let gs_enable_this_call = std::mem::replace(&mut gs_enable_remaining, false);
                let result = wallet_contract_call(
                    &from_token,
                    &chain_index,
                    "0",
                    Some(&revoke_calldata),
                    None,
                    None,
                    None,
                    None,
                    false,
                    None,
                    gas_token_address,
                    relayer_id,
                    gs_enable_this_call,
                    force,
                )
                .await?;
                let revoke_tx_hash = extract_tx_hash(&result)?;
                // Approve must wait for revoke to confirm — sending approve
                // before the revoke is mined leaves the original allowance
                // in place and the swap will revert.
                wait_tx_onchain(client, &revoke_tx_hash, &chain_index).await?;
            }

            if cfg!(feature = "debug-log") {
                eprintln!("[swap execute] approving token...");
            }
            // Reuse the approve calldata already fetched above
            let gs_enable_this_call = std::mem::replace(&mut gs_enable_remaining, false);
            let result = wallet_contract_call(
                &from_token,
                &chain_index,
                "0",
                Some(&approve_calldata),
                None,
                None,
                None,
                None,
                false,
                None,
                gas_token_address,
                relayer_id,
                gs_enable_this_call,
                force,
            )
            .await?;
            let (tx_hash, order_id) = extract_tx_hash_and_order_id(&result)?;
            // Swap must see the approve on-chain before fetching the swap tx —
            // otherwise the router quote will reject the route.
            wait_tx_onchain(client, &tx_hash, &chain_index).await?;
            approve_tx_hash = Some(tx_hash);
            if !order_id.is_empty() {
                approve_order_id = Some(order_id);
            }
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
    let (swap_tx_hash, swap_order_id) = if family == "solana" {
        let unsigned_tx = tx["data"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("missing tx.data (unsigned tx) in swap response"))?;
        let to_addr = tx["to"].as_str().unwrap_or("");

        // Jito MEV protection: `/swap` returns jitoCalldata nested inside
        // tx.signatureData[0] as a JSON string, not at the top level.
        let jito_tx_owned: Option<String> = tx["signatureData"]
            .as_array()
            .and_then(|arr| arr.first())
            .and_then(|v| v.as_str())
            .and_then(|s| serde_json::from_str::<Value>(s).ok())
            .and_then(|v| v["jitoCalldata"].as_str().map(str::to_string));
        let jito_tx = jito_tx_owned.as_deref();
        let effective_mev = jito_tx.is_some() || mev_protection;

        let gs_enable_this_call = std::mem::replace(&mut gs_enable_remaining, false);
        let result = wallet_contract_call(
            to_addr,
            &chain_index,
            "0",
            None,
            Some(unsigned_tx),
            None,
            None,
            None,
            effective_mev,
            jito_tx,
            gas_token_address,
            relayer_id,
            gs_enable_this_call,
            force,
        )
        .await?;
        extract_tx_hash_and_order_id(&result)?
    } else {
        let to_addr = tx["to"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("missing tx.to in swap response"))?;
        let input_data = tx["data"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("missing tx.data in swap response"))?;
        let tx_value_wei = tx["value"].as_str().unwrap_or("0");

        // Gas limit from swap response
        let gas_limit_str = tx["gas"].as_str();

        // XLayer AA DEX params (mainnet 196 + testnet 1952)
        let from_token_amount;
        let (aa_addr, aa_amount) = if chain_index == "196" || chain_index == "1952" {
            from_token_amount = swap_result["routerResult"]["fromTokenAmount"]
                .as_str()
                .unwrap_or(amount)
                .to_string();
            (Some(from_token.as_str()), Some(from_token_amount.as_str()))
        } else {
            (None, None)
        };

        let gs_enable_this_call = std::mem::replace(&mut gs_enable_remaining, false);
        let result = wallet_contract_call(
            to_addr,
            &chain_index,
            tx_value_wei,
            Some(input_data),
            None,
            gas_limit_str,
            aa_addr,
            aa_amount,
            mev_protection,
            None,
            gas_token_address,
            relayer_id,
            gs_enable_this_call,
            force,
        )
        .await?;
        extract_tx_hash_and_order_id(&result)?
    };

    // ── 6. Output ────────────────────────────────────────────────────
    if cfg!(feature = "debug-log") {
        eprintln!(
            "[DEBUG][cmd_execute] swap_tx_hash={}, approve_tx_hash={:?}",
            swap_tx_hash, approve_tx_hash
        );
    }
    let router_result = &swap_result["routerResult"];
    let mut out = json!({
        "approveTxHash": approve_tx_hash,
        "swapTxHash": swap_tx_hash,
        "fromToken": router_result["fromToken"],
        "toToken": router_result["toToken"],
        "fromAmount": router_result["fromTokenAmount"],
        "toAmount": router_result["toTokenAmount"],
        "priceImpact": router_result["priceImpactPercent"],
        "gasUsed": router_result["estimateGasFee"],
    });
    // Only emit orderId fields when Gas Station was actually used (non-empty).
    // Normal (native-gas) swaps return empty orderId and shouldn't pollute output.
    if let Some(ref oid) = approve_order_id {
        out["approveOrderId"] = json!(oid);
    }
    if !swap_order_id.is_empty() {
        out["swapOrderId"] = json!(swap_order_id);
    }
    output::success(out);

    Ok(())
}

// ── Batch unsignedInfo + broadcast ───────────────────────────────────
// Build [revoke?, approve?, swap], one batch unsignedInfo call, sign
// locally, dispatch via batch broadcast (or single broadcast if backend
// merged to len=1 — XLayer EIP-5792).

/// Returns `false` on any failure so the caller falls through to single-tx.
async fn is_chain_batch_supported(chain_index: &str) -> bool {
    let access_token = match crate::commands::agentic_wallet::auth::ensure_tokens_refreshed().await
    {
        Ok(t) => t,
        Err(_) => return false,
    };
    let mut client = match crate::wallet_api::WalletApiClient::new() {
        Ok(c) => c,
        Err(_) => return false,
    };
    match client.batch_support_chain_index_list(&access_token).await {
        // chain_index in list → supports batch
        // not in list or request error → not supported (fall through to single-tx)
        Ok(list) => list.iter().any(|c| c == chain_index),
        Err(_) => false,
    }
}

#[allow(clippy::too_many_arguments)]
async fn cmd_execute_batch(
    client: &mut ApiClient,
    from_token: &str,
    to_token: &str,
    amount: &str,
    chain_index: &str,
    wallet_address: &str,
    slippage: Option<&str>,
    gas_level: &str,
    swap_mode: &str,
    tips: Option<&str>,
    max_auto_slippage: Option<&str>,
    mev_protection: bool,
    force: bool,
) -> Result<()> {
    use crate::commands::agentic_wallet::transfer::{batch_sign_and_broadcast, BatchTxParams};

    // 1. Approve check (gates revoke / approve / swap construction).
    let approve_data = fetch_approve(client, chain_index, from_token, amount).await?;
    let approve_obj = unwrap_api_array(&approve_data);
    let approve_calldata = approve_obj["data"]
        .as_str()
        .map(|s| s.to_string())
        .ok_or_else(|| anyhow::anyhow!("missing 'data' field in approve response"))?;
    let dex_contract_address = approve_obj["dexContractAddress"]
        .as_str()
        .map(|s| s.to_string());

    let approvals = fetch_check_approvals(
        client,
        chain_index,
        wallet_address,
        from_token,
        dex_contract_address.as_deref(),
    )
    .await?;
    let spendable = approvals
        .as_array()
        .and_then(|arr| arr.first())
        .and_then(|r| r["tokens"].as_array())
        .and_then(|tokens| tokens.first())
        .and_then(|t| t["spendable"].as_str())
        .unwrap_or("0")
        .to_string();

    let (needs_approve, needs_revoke) =
        classify_approve_action(chain_index, from_token, &spendable, amount);

    // 2. Fetch swap calldata.
    let swap_data = fetch_swap(
        client,
        chain_index,
        from_token,
        to_token,
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
    let swap_tx = &swap_result["tx"];
    let swap_to = swap_tx["to"]
        .as_str()
        .ok_or_else(|| anyhow::anyhow!("missing tx.to in swap response"))?
        .to_string();
    let swap_input_data = swap_tx["data"]
        .as_str()
        .ok_or_else(|| anyhow::anyhow!("missing tx.data in swap response"))?
        .to_string();
    let swap_value_wei = swap_tx["value"].as_str().unwrap_or("0").to_string();
    let swap_gas_limit = swap_tx["gas"].as_str().map(|s| s.to_string());

    // XLayer (196) AA DEX params, mirrored from the single-tx path.
    let (aa_addr, aa_amount) = if chain_index == "196" {
        let amt = swap_result["routerResult"]["fromTokenAmount"]
            .as_str()
            .unwrap_or(amount)
            .to_string();
        (Some(from_token.to_string()), Some(amt))
    } else {
        (None, None)
    };

    // Layer 4 fallback:
    //   allowance >= amount (no approve, no revoke needed) → single-tx broadcast (skip batch)
    //   allowance insufficient                             → continue to batch construction
    if !needs_approve && !needs_revoke {
        let result = wallet_contract_call(
            &swap_to,
            chain_index,
            &swap_value_wei,
            Some(&swap_input_data),
            None, // unsigned_tx (Solana only)
            swap_gas_limit.as_deref(),
            aa_addr.as_deref(),
            aa_amount.as_deref(),
            mev_protection,
            None,  // jito_unsigned_tx
            None,  // gas_token_address (no GS in batch)
            None,  // relayer_id
            false, // enable_gas_station
            force,
        )
        .await?;
        let swap_tx_hash = extract_tx_hash(&result)?;
        let router_result = &swap_result["routerResult"];
        let out = json!({
            "approveTxHash": Value::Null,
            "swapTxHash": swap_tx_hash,
            "fromToken": router_result["fromToken"],
            "toToken": router_result["toToken"],
            "fromAmount": router_result["fromTokenAmount"],
            "toAmount": router_result["toTokenAmount"],
            "priceImpact": router_result["priceImpactPercent"],
            "gasUsed": router_result["estimateGasFee"],
        });
        output::success(out);
        return Ok(());
    }

    // Build elements: [revoke?, approve, swap].
    let mut tx_params: Vec<BatchTxParams> = Vec::new();
    if needs_revoke {
        let revoke_data = fetch_approve(client, chain_index, from_token, "0").await?;
        let revoke_calldata = extract_approve_calldata(&revoke_data)?;
        tx_params.push(BatchTxParams {
            to_addr: from_token.to_string(),
            value: "0".to_string(),
            contract_addr: Some(from_token.to_string()),
            input_data: Some(revoke_calldata),
            ..Default::default()
        });
    }
    tx_params.push(BatchTxParams {
        to_addr: from_token.to_string(),
        value: "0".to_string(),
        contract_addr: Some(from_token.to_string()),
        input_data: Some(approve_calldata),
        ..Default::default()
    });
    tx_params.push(BatchTxParams {
        to_addr: swap_to.clone(),
        value: swap_value_wei,
        // Swap router must appear in unsignedInfo's contractAddr field
        // (same as single-tx execute_contract_call).
        contract_addr: Some(swap_to),
        input_data: Some(swap_input_data),
        gas_limit: swap_gas_limit,
        aa_dex_token_addr: aa_addr,
        aa_dex_token_amount: aa_amount,
    });

    let responses = batch_sign_and_broadcast(
        chain_index,
        Some(wallet_address),
        &tx_params,
        true,        // is_contract_call
        mev_protection,
        force,
        None,        // tx_source — backend coerces with parseInt; omit to match single-tx path
        Some("dex"), // agent_biz_type
        Some("okx-dex-swap-batch"),
    )
    .await?;

    // Response length contract:
    //   merging chain (X Layer 196/1952) → response.len ∈ {1, request_len}
    //   non-merging EVM                  → response.len == request_len
    //   anything else                    → bail
    let merging_chain = crate::chains::merges_batch_unsignedinfo(chain_index);
    let length_ok = if merging_chain {
        responses.len() == 1 || responses.len() == tx_params.len()
    } else {
        responses.len() == tx_params.len()
    };
    if !length_ok {
        bail!(
            "batch broadcast on chain {chain_index}: response length {} not in expected set \
             (request length {}, merging chain={merging_chain})",
            responses.len(),
            tx_params.len(),
        );
    }

    let hashes: Vec<String> = responses.iter().map(|r| r.tx_hash.clone()).collect();
    let (approve_tx_hash, swap_tx_hash) = extract_batch_hashes(&hashes, needs_approve, needs_revoke);
    if cfg!(feature = "debug-log") && needs_revoke && responses.len() >= 2 {
        eprintln!(
            "[DEBUG][cmd_execute_batch] revoke txHash={} (silent)",
            responses[0].tx_hash
        );
    }

    let router_result = &swap_result["routerResult"];
    let out = json!({
        "approveTxHash": approve_tx_hash,
        "swapTxHash": swap_tx_hash,
        "fromToken": router_result["fromToken"],
        "toToken": router_result["toToken"],
        "fromAmount": router_result["fromTokenAmount"],
        "toAmount": router_result["toTokenAmount"],
        "priceImpact": router_result["priceImpactPercent"],
        "gasUsed": router_result["estimateGasFee"],
    });
    output::success(out);
    Ok(())
}

// ── Helpers ──────────────────────────────────────────────────────────

/// Map batch hashes (input order `[revoke?, approve, swap]`) to
/// `(approveTxHash, swapTxHash)`. `len==1` = X Layer merge (only swap hash
/// survives). Caller must pre-validate `hashes.len()`.
pub(crate) fn extract_batch_hashes(
    hashes: &[String],
    needs_approve: bool,
    needs_revoke: bool,
) -> (Option<String>, String) {
    debug_assert!(!hashes.is_empty(), "responses must be non-empty");
    // len == 1 → X Layer merge: approve folded into swap, only swap hash exists.
    if hashes.len() == 1 {
        return (None, hashes[0].clone());
    }
    // len > 1 → no merge: swap is always last; approve sits at idx 1 (with revoke
    // at idx 0) or idx 0 (no revoke); surfaced only when needs_approve.
    let swap = hashes.last().expect("non-empty").clone();
    let approve = if needs_approve {
        let idx = if needs_revoke { 1 } else { 0 };
        Some(hashes[idx].clone())
    } else {
        None
    };
    (approve, swap)
}

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

/// Tokens that require revoke-to-zero before re-approval (USDT-pattern
/// `approve` race-condition guard). Keyed by `chain_index`; values are
/// lowercase token addresses.
static REVOKE_REQUIRED_TOKENS: LazyLock<HashMap<&str, &[&str]>> = LazyLock::new(|| {
    HashMap::from([(
        // Ethereum
        "1",
        &[
            "0xdac17f958d2ee523a2206206994597c13d831ec7",
            "0x5a98fcbea516cf06857215779fd812ca3bef1b32",
            "0x1776e1f26f98b1a5df9cd347953a26dd3cb46671",
            "0xd3e4ba569045546d09cf021ecc5dfe42b1d7f6e4",
        ][..],
    )])
});

/// True if `(chain_index, token)` is a known USDT-pattern token and the caller
/// must revoke before re-approving.
fn token_requires_revoke(chain_index: &str, token: &str) -> bool {
    let token_lc = token.to_lowercase();
    REVOKE_REQUIRED_TOKENS
        .get(chain_index)
        .is_some_and(|addrs| addrs.iter().any(|a| a.to_lowercase() == token_lc))
}

/// Per-chain confirmation timeout for [`wait_tx_onchain`]. Picked to cover a
/// typical block time with a small buffer; falls back to a generous default
/// for unknown chains so the poller still bounds.
fn tx_confirmation_timeout(chain_index: &str) -> std::time::Duration {
    use std::time::Duration;
    match chain_index {
        // ETH, Linea
        "1" | "59144" => Duration::from_secs(20),
        _ => Duration::from_secs(10),
    }
}

/// Poll the public DEX tx-history endpoint until the tx confirms on-chain
/// (`txStatus == "success"`) or the per-chain timeout elapses.
///
/// GET `/api/v6/dex/post-transaction/transaction-detail-by-txhash`
async fn wait_tx_onchain(client: &mut ApiClient, tx_hash: &str, chain_index: &str) -> Result<()> {
    use std::time::{Duration, Instant};

    let timeout = tx_confirmation_timeout(chain_index);
    let poll_interval = Duration::from_secs(1);
    let deadline = Instant::now() + timeout;

    loop {
        let result = client
            .get(
                "/api/v6/dex/post-transaction/transaction-detail-by-txhash",
                &[("chainIndex", chain_index), ("txHash", tx_hash)],
            )
            .await;
        if cfg!(feature = "debug-log") {
            eprintln!(
                "[DEBUG][wait_tx_onchain] tx={} chain={} response={:?}",
                tx_hash, chain_index, result
            );
        }

        if let Ok(data) = result {
            let detail = unwrap_api_array(&data);
            let status = detail["txStatus"].as_str().unwrap_or("");
            if status.eq_ignore_ascii_case("success") {
                return Ok(());
            }
            if status.eq_ignore_ascii_case("fail") {
                bail!("tx {} failed on-chain (chain={})", tx_hash, chain_index);
            }
        }

        if Instant::now() >= deadline {
            bail!(
                "tx {} not confirmed on-chain within {}s (chain={})",
                tx_hash,
                timeout.as_secs(),
                chain_index
            );
        }
        tokio::time::sleep(poll_interval).await;
    }
}

/// Compare allowance (spendable) against required amount.
/// Single source of truth for approve / revoke gating. Used by both single-tx
/// and batch swap paths to guarantee identical decisions for identical inputs.
///
/// Returns `(needs_approve, needs_revoke)`:
///   spendable >= amount                                      → (false, false)
///   spendable == 0                                           → (true,  false)
///   0 < spendable < amount, non-USDT-pattern token           → (true,  false)
///   0 < spendable < amount, USDT-pattern token (whitelist)   → (true,  true)
pub(crate) fn classify_approve_action(
    chain_index: &str,
    token: &str,
    spendable: &str,
    amount: &str,
) -> (bool, bool) {
    let needs_approve = is_allowance_insufficient(spendable, amount);
    let spendable_nonzero = spendable != "0" && !spendable.is_empty();
    let needs_revoke =
        needs_approve && spendable_nonzero && token_requires_revoke(chain_index, token);
    (needs_approve, needs_revoke)
}

/// Both are decimal strings in minimal units. Returns true if allowance < amount.
pub(crate) fn is_allowance_insufficient(spendable: &str, amount: &str) -> bool {
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
    fn classify_approve_action_truth_table() {
        const USDT_ETH: &str = "0xdac17f958d2ee523a2206206994597c13d831ec7";
        const USDC_ETH: &str = "0xA0b86991c6218b36c1D19D4a2e9Eb0cE3606eB48";

        // spendable >= amount → no approve / no revoke (Layer 4 case)
        assert_eq!(classify_approve_action("1", USDT_ETH, "1000000", "500000"), (false, false));
        // spendable == 0 → approve only (no revoke even for USDT)
        assert_eq!(classify_approve_action("1", USDT_ETH, "0", "1000000"), (true, false));
        // 0 < spendable < amount, USDT-pattern token → revoke + approve
        assert_eq!(classify_approve_action("1", USDT_ETH, "100", "1000000"), (true, true));
        // 0 < spendable < amount, non-USDT-pattern token (USDC) → approve only
        assert_eq!(classify_approve_action("1", USDC_ETH, "100", "1000000"), (true, false));
        // 0 < spendable < amount, USDT but on chain without entry → approve only
        assert_eq!(classify_approve_action("56", USDT_ETH, "100", "1000000"), (true, false));
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
        assert_eq!(
            readable_to_minimal_str("0.1", 18).unwrap(),
            "100000000000000000"
        );
        assert_eq!(
            readable_to_minimal_str("1", 18).unwrap(),
            "1000000000000000000"
        );
        // SOL: 9 decimals
        assert_eq!(readable_to_minimal_str("1", 9).unwrap(), "1000000000");
        // Excess fractional digits with non-zero content → error
        assert!(readable_to_minimal_str("0.1234567", 6).is_err());
        assert!(readable_to_minimal_str("1.00000002", 2).is_err());
        // Excess fractional digits that are all zero → ok
        assert_eq!(readable_to_minimal_str("1.000", 2).unwrap(), "100");
        assert_eq!(readable_to_minimal_str("0.1230000", 6).unwrap(), "123000");
    }

    // ── slippage validation ────────────────────────────────────────

    #[test]
    fn test_validate_slippage_valid() {
        assert!(validate_slippage("0.5").is_ok());
        assert!(validate_slippage("1").is_ok());
        assert!(validate_slippage("50").is_ok());
        assert!(validate_slippage("99.9").is_ok());
        assert!(validate_slippage("100").is_ok()); // upper bound inclusive
        assert!(validate_slippage("100.0").is_ok());
        assert!(validate_slippage("0.001").is_ok());
        assert!(validate_slippage("0.01").is_ok());
        assert!(validate_slippage("  1  ").is_ok()); // trimmed
    }

    #[test]
    fn test_validate_slippage_boundary_reject() {
        // 0 is exclusive
        assert!(validate_slippage("0").is_err());
        assert!(validate_slippage("0.0").is_err());
        // >100 rejected
        assert!(validate_slippage("100.1").is_err());
    }

    #[test]
    fn test_validate_slippage_out_of_range() {
        assert!(validate_slippage("-1").is_err());
        assert!(validate_slippage("-0.5").is_err());
        assert!(validate_slippage("100.1").is_err());
        assert!(validate_slippage("200").is_err());
    }

    #[test]
    fn test_validate_slippage_non_numeric() {
        assert!(validate_slippage("abc").is_err());
        assert!(validate_slippage("").is_err());
        assert!(validate_slippage("   ").is_err());
        assert!(validate_slippage("NaN").is_err());
        assert!(validate_slippage("inf").is_err());
        assert!(validate_slippage("infinity").is_err());
        assert!(validate_slippage("-inf").is_err());
    }

    // ── amount validation (swap: positive integer) ─────────────────

    #[test]
    fn test_validate_amount_valid() {
        assert!(validate_amount("1").is_ok());
        assert!(validate_amount("1000000").is_ok());
        assert!(validate_amount("999999999999999999").is_ok());
    }

    #[test]
    fn test_validate_amount_reject_decimal() {
        assert!(validate_amount("1.5").is_err());
        assert!(validate_amount("0.1").is_err());
        assert!(validate_amount("100.0").is_err());
    }

    #[test]
    fn test_validate_amount_reject_zero() {
        assert!(validate_amount("0").is_err());
        assert!(validate_amount("000").is_err());
    }

    #[test]
    fn test_validate_amount_reject_negative_and_non_numeric() {
        assert!(validate_amount("-1").is_err());
        assert!(validate_amount("-100").is_err());
        assert!(validate_amount("abc").is_err());
        assert!(validate_amount("12abc").is_err());
        assert!(validate_amount("").is_err());
        assert!(validate_amount("  ").is_err());
    }

    #[test]
    fn test_validate_amount_reject_leading_zeros() {
        assert!(validate_amount("007").is_err());
        assert!(validate_amount("01").is_err());
    }

    // ── approve amount validation (allows 0 for revoke) ────────────

    #[test]
    fn test_validate_approve_amount_valid() {
        assert!(validate_approve_amount("0").is_ok()); // revoke
        assert!(validate_approve_amount("1").is_ok());
        assert!(validate_approve_amount("1000000").is_ok());
    }

    #[test]
    fn test_validate_approve_amount_reject_decimal() {
        assert!(validate_approve_amount("1.5").is_err());
        assert!(validate_approve_amount("0.1").is_err());
    }

    #[test]
    fn test_validate_approve_amount_reject_leading_zeros() {
        assert!(validate_approve_amount("007").is_err());
        assert!(validate_approve_amount("00").is_err());
    }

    #[test]
    fn test_validate_approve_amount_reject_negative_and_non_numeric() {
        assert!(validate_approve_amount("-1").is_err());
        assert!(validate_approve_amount("abc").is_err());
        assert!(validate_approve_amount("").is_err());
    }

    // ── swapMode validation ───────────────────────────────────────────

    #[test]
    fn test_validate_swap_mode_valid() {
        assert!(validate_swap_mode("exactIn").is_ok());
        assert!(validate_swap_mode("exactOut").is_ok());
    }

    #[test]
    fn test_validate_swap_mode_invalid() {
        assert!(validate_swap_mode("exactin").is_err());
        assert!(validate_swap_mode("EXACTIN").is_err());
        assert!(validate_swap_mode("ExactIn").is_err());
        assert!(validate_swap_mode("").is_err());
        assert!(validate_swap_mode("foobar").is_err());
        assert!(validate_swap_mode("exact_in").is_err());
    }

    #[test]
    fn test_validate_swap_mode_error_message() {
        let err = validate_swap_mode("bad").unwrap_err();
        assert!(err.to_string().contains("exactIn"));
        assert!(err.to_string().contains("exactOut"));
    }

    // ── gasLevel validation ───────────────────────────────────────────

    #[test]
    fn test_validate_gas_level_valid() {
        assert!(validate_gas_level("slow").is_ok());
        assert!(validate_gas_level("average").is_ok());
        assert!(validate_gas_level("fast").is_ok());
    }

    #[test]
    fn test_validate_gas_level_invalid() {
        assert!(validate_gas_level("").is_err());
        assert!(validate_gas_level("Slow").is_err());
        assert!(validate_gas_level("FAST").is_err());
        assert!(validate_gas_level("medium").is_err());
        assert!(validate_gas_level("turbo").is_err());
        assert!(validate_gas_level("instant").is_err());
    }

    #[test]
    fn test_validate_gas_level_error_message() {
        let err = validate_gas_level("medium").unwrap_err();
        assert!(err.to_string().contains("slow"));
        assert!(err.to_string().contains("average"));
        assert!(err.to_string().contains("fast"));
    }

    // ── tips validation ───────────────────────────────────────────────

    #[test]
    fn test_validate_tips_valid() {
        assert!(validate_tips("0.0000000001").is_ok());
        assert!(validate_tips("0.001").is_ok());
        assert!(validate_tips("1").is_ok());
        assert!(validate_tips("2").is_ok());
    }

    #[test]
    fn test_validate_tips_rejects_out_of_range() {
        assert!(validate_tips("0").is_err());
        assert!(validate_tips("0.00000000001").is_err()); // below minimum
        assert!(validate_tips("2.0000000001").is_err()); // above maximum
        assert!(validate_tips("3").is_err());
    }

    #[test]
    fn test_validate_tips_rejects_non_numeric() {
        assert!(validate_tips("abc").is_err());
        assert!(validate_tips("-1").is_err());
        assert!(validate_tips("").is_err());
        assert!(validate_tips("  ").is_err());
    }

    #[test]
    fn test_validate_tips_trims_whitespace() {
        assert!(validate_tips("  1  ").is_ok());
    }

    // ── non-negative integer validation ───────────────────────────────

    #[test]
    fn test_validate_non_negative_integer_valid() {
        assert!(validate_non_negative_integer("0", "gas-limit").is_ok());
        assert!(validate_non_negative_integer("1", "gas-limit").is_ok());
        assert!(validate_non_negative_integer("21000", "gas-limit").is_ok());
        assert!(validate_non_negative_integer("999999999", "aa-dex-token-amount").is_ok());
    }

    #[test]
    fn test_validate_non_negative_integer_rejects_non_numeric() {
        assert!(validate_non_negative_integer("abc", "gas-limit").is_err());
        assert!(validate_non_negative_integer("-1", "gas-limit").is_err());
        assert!(validate_non_negative_integer("1.5", "gas-limit").is_err());
        assert!(validate_non_negative_integer("", "gas-limit").is_err());
        assert!(validate_non_negative_integer("  ", "gas-limit").is_err());
    }

    #[test]
    fn test_validate_non_negative_integer_rejects_leading_zeros() {
        assert!(validate_non_negative_integer("007", "gas-limit").is_err());
        assert!(validate_non_negative_integer("00", "gas-limit").is_err());
        assert!(validate_non_negative_integer("01", "aa-dex-token-amount").is_err());
    }

    #[test]
    fn test_validate_non_negative_integer_allows_zero() {
        assert!(validate_non_negative_integer("0", "gas-limit").is_ok());
    }

    #[test]
    fn test_validate_non_negative_integer_error_contains_label() {
        let err = validate_non_negative_integer("abc", "gas-limit").unwrap_err();
        assert!(err.to_string().contains("--gas-limit"));
        let err2 = validate_non_negative_integer("-1", "aa-dex-token-amount").unwrap_err();
        assert!(err2.to_string().contains("--aa-dex-token-amount"));
    }

    // ── extract_tx_hash_and_order_id (Gas Station orderId propagation) ──

    #[test]
    fn extract_tx_hash_and_order_id_both_present() {
        let data = json!({ "txHash": "0xabc", "orderId": "ord_123" });
        let (tx_hash, order_id) = extract_tx_hash_and_order_id(&data).unwrap();
        assert_eq!(tx_hash, "0xabc");
        assert_eq!(order_id, "ord_123");
    }

    #[test]
    fn extract_tx_hash_and_order_id_order_id_optional() {
        // Non-Gas-Station path: orderId is absent → defaults to empty string, no error.
        let data = json!({ "txHash": "0xabc" });
        let (tx_hash, order_id) = extract_tx_hash_and_order_id(&data).unwrap();
        assert_eq!(tx_hash, "0xabc");
        assert_eq!(order_id, "");
    }

    #[test]
    fn extract_tx_hash_and_order_id_order_id_empty_string() {
        // Backend may return empty orderId explicitly for non-GS broadcasts.
        let data = json!({ "txHash": "0xabc", "orderId": "" });
        let (tx_hash, order_id) = extract_tx_hash_and_order_id(&data).unwrap();
        assert_eq!(tx_hash, "0xabc");
        assert_eq!(order_id, "");
    }

    #[test]
    fn extract_tx_hash_and_order_id_empty_tx_hash_still_ok() {
        // GS async: txHash is empty string but present; orderId carries the state.
        let data = json!({ "txHash": "", "orderId": "ord_async" });
        let (tx_hash, order_id) = extract_tx_hash_and_order_id(&data).unwrap();
        assert_eq!(tx_hash, "");
        assert_eq!(order_id, "ord_async");
    }

    #[test]
    fn extract_tx_hash_and_order_id_errors_when_tx_hash_missing() {
        let data = json!({ "orderId": "ord_123" });
        assert!(extract_tx_hash_and_order_id(&data).is_err());
    }

    // ── extract_batch_hashes (input order: [revoke?, approve, swap]) ──

    #[test]
    fn batch_hashes_full_merge_xlayer_5792() {
        // XLayer / smart-account: backend collapses [approve, swap] into one tx.
        let hashes = vec!["0xmerged".to_string()];
        let (approve, swap) = extract_batch_hashes(&hashes, true, false);
        assert_eq!(approve, None);
        assert_eq!(swap, "0xmerged");
    }

    #[test]
    fn batch_hashes_full_merge_with_revoke() {
        // [revoke, approve, swap] collapsed into one tx.
        let hashes = vec!["0xmerged".to_string()];
        let (approve, swap) = extract_batch_hashes(&hashes, true, true);
        assert_eq!(approve, None);
        assert_eq!(swap, "0xmerged");
    }

    #[test]
    fn batch_hashes_no_merge_approve_swap() {
        // EOA chain (Optimism etc.): [approve, swap] kept separate.
        let hashes = vec!["0xapprove".to_string(), "0xswap".to_string()];
        let (approve, swap) = extract_batch_hashes(&hashes, true, false);
        assert_eq!(approve.as_deref(), Some("0xapprove"));
        assert_eq!(swap, "0xswap");
    }

    #[test]
    fn batch_hashes_no_merge_revoke_approve_swap() {
        let hashes = vec![
            "0xrevoke".to_string(),
            "0xapprove".to_string(),
            "0xswap".to_string(),
        ];
        let (approve, swap) = extract_batch_hashes(&hashes, true, true);
        // revoke (idx 0) is intentionally not surfaced — see fn doc.
        assert_eq!(approve.as_deref(), Some("0xapprove"));
        assert_eq!(swap, "0xswap");
    }

    #[test]
    fn batch_hashes_no_approve_needed() {
        // Allowance sufficient case should short-circuit upstream, but if we
        // ever reach this fn with needs_approve=false, swap-only output is
        // the safe behavior.
        let hashes = vec!["0xswap".to_string()];
        let (approve, swap) = extract_batch_hashes(&hashes, false, false);
        assert_eq!(approve, None);
        assert_eq!(swap, "0xswap");
    }
}
