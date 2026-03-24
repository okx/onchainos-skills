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
        /// Amount in minimal units (wei/lamports)
        #[arg(long)]
        amount: String,
        /// Chain (e.g. ethereum, solana, xlayer)
        #[arg(long)]
        chain: String,
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
            chain,
            swap_mode,
        } => {
            let chain_index = crate::chains::resolve_chain(&chain);
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
                fetch_check_approvals(
                    &client,
                    &chain_index,
                    &address,
                    &token,
                    spender.as_deref(),
                )
                .await?,
            );
        }
        SwapCommand::Chains => {
            output::success(fetch_chains(&client).await?);
        }
        SwapCommand::Liquidity { chain } => {
            let chain_index = crate::chains::resolve_chain(&chain);
            output::success(fetch_liquidity(&client, &chain_index).await?);
        }
        SwapCommand::Execute {
            from,
            to,
            amount,
            chain,
            slippage,
            gas_level,
            swap_mode,
            tips,
            max_auto_slippage,
            mev_protection,
        } => {
            cmd_execute(
                &client,
                &from,
                &to,
                &amount,
                &chain,
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
    let params = vec![
        ("chainIndex", chain_index),
        ("fromTokenAddress", from),
        ("toTokenAddress", to),
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
    let mut params = vec![
        ("chainIndex", chain_index),
        ("fromTokenAddress", from),
        ("toTokenAddress", to),
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
    client
        .get(
            "/api/v6/dex/aggregator/approve-transaction",
            &[
                ("chainIndex", chain_index),
                ("tokenContractAddress", token),
                ("approveAmount", amount),
            ],
        )
        .await
}

/// POST /api/v6/dex/pre-transaction/check-approvals
pub async fn fetch_check_approvals(
    client: &ApiClient,
    chain_index: &str,
    address: &str,
    token: &str,
    spender: Option<&str>,
) -> Result<Value> {
    let mut body = json!({
        "chainIndex": chain_index,
        "address": address,
        "tokens": [{ "tokenContractAddress": token }],
    });
    if let Some(s) = spender {
        body["spender"] = json!(s);
    }
    client
        .post("/api/v6/dex/pre-transaction/check-approvals", &body)
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

// ── Execute orchestration ────────────────────────────────────────────

/// Run an onchainos subcommand as a subprocess and return the `data` field from
/// the `{ "ok": true, "data": ... }` output envelope.
/// This keeps swap independent of wallet internals.
async fn run_onchainos_cmd(args: &[&str]) -> Result<Value> {
    let exe = std::env::current_exe().unwrap_or_else(|_| "onchainos".into());
    let output = tokio::process::Command::new(&exe)
        .args(args)
        .output()
        .await
        .map_err(|e| anyhow::anyhow!("failed to spawn onchainos {}: {e}", args.first().unwrap_or(&"")))?;

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
    let is_from_native = from_token.eq_ignore_ascii_case(native_addr);

    // ── 1. Resolve wallet address ────────────────────────────────────
    let wallet_address = resolve_wallet_address(&chain_index).await?;

    // ── 2. Quote ─────────────────────────────────────────────────────
    let quote_data =
        fetch_quote(client, &chain_index, from_token, to_token, amount, swap_mode).await?;

    let quote = unwrap_api_array(&quote_data);
    if quote.is_null() {
        bail!("no quote available for this token pair");
    }

    // Safety checks
    if quote["toToken"]["isHoneyPot"].as_bool() == Some(true) {
        bail!("blocked: destination token is flagged as honeypot");
    }

    let price_impact = quote["priceImpactPercent"]
        .as_str()
        .and_then(|s| s.parse::<f64>().ok())
        .unwrap_or(0.0);
    if price_impact > 10.0 {
        bail!(
            "blocked: price impact is {:.2}% (>10%). Reduce amount or split into smaller trades",
            price_impact
        );
    }

    // Tax rate check
    for side in &["fromToken", "toToken"] {
        let tax_rate = quote[side]["taxRate"]
            .as_str()
            .and_then(|s| s.parse::<f64>().ok())
            .unwrap_or(0.0);
        if tax_rate > 0.1 {
            bail!(
                "blocked: {} has a {:.1}% tax rate (>10%)",
                quote[side]["tokenSymbol"].as_str().unwrap_or(side),
                tax_rate * 100.0
            );
        } else if tax_rate > 0.0 {
            eprintln!(
                "[swap execute] warning: {} has a {:.1}% tax rate",
                quote[side]["tokenSymbol"].as_str().unwrap_or(side),
                tax_rate * 100.0
            );
        }
    }

    // ── 3. Approve (EVM + non-native only) ───────────────────────────
    let mut approve_tx_hash: Option<String> = None;

    if family == "evm" && !is_from_native {
        let approvals =
            fetch_check_approvals(client, &chain_index, &wallet_address, from_token, None).await?;

        let spendable = approvals["results"]
            .as_array()
            .and_then(|arr| arr.first())
            .and_then(|r| r["spendable"].as_str())
            .unwrap_or("0");

        if is_allowance_insufficient(spendable, amount) {
            // USDT pattern: non-zero but insufficient → revoke first
            let spendable_nonzero = spendable != "0" && !spendable.is_empty();
            if spendable_nonzero {
                eprintln!("[swap execute] revoking stale approval (USDT pattern)...");
                let revoke_data = fetch_approve(client, &chain_index, from_token, "0").await?;
                let revoke_calldata = extract_approve_calldata(&revoke_data)?;

                let result = wallet_contract_call(&[
                    "--to", from_token,
                    "--chain", &chain_index,
                    "--input-data", &revoke_calldata,
                ]).await?;
                // We don't need the revoke txHash in output, just ensure it succeeded
                extract_tx_hash(&result)?;
            }

            eprintln!("[swap execute] approving token...");
            let approve_data = fetch_approve(client, &chain_index, from_token, amount).await?;
            let approve_calldata = extract_approve_calldata(&approve_data)?;

            let result = wallet_contract_call(&[
                "--to", from_token,
                "--chain", &chain_index,
                "--input-data", &approve_calldata,
            ]).await?;
            approve_tx_hash = Some(extract_tx_hash(&result)?);
        }
    }

    // ── 4. Swap ──────────────────────────────────────────────────────
    eprintln!("[swap execute] executing swap...");
    let swap_data = fetch_swap(
        client,
        &chain_index,
        from_token,
        to_token,
        amount,
        slippage,
        &wallet_address,
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

    let tx = &swap_result["tx"];

    // ── 5. Sign & broadcast swap tx via wallet contract-call ─────────
    let swap_tx_hash = if family == "solana" {
        let unsigned_tx = tx["data"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("missing tx.data (unsigned tx) in swap response"))?;
        let to_addr = tx["to"].as_str().unwrap_or("");

        let mut args = vec![
            "--to", to_addr,
            "--chain", &chain_index,
            "--unsigned-tx", unsigned_tx,
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
        let value_ui = wei_to_ui(tx_value_wei, 18);

        let mut args = vec![
            "--to", to_addr,
            "--chain", &chain_index,
            "--value", &value_ui,
            "--input-data", input_data,
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
                "--aa-dex-token-addr", from_token,
                "--aa-dex-token-amount", &from_token_amount,
            ]);
        }

        if mev_protection {
            args.push("--mev-protection");
        }

        let result = wallet_contract_call(&args).await?;
        extract_tx_hash(&result)?
    };

    // ── 6. Output ────────────────────────────────────────────────────
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

/// Resolve wallet address by calling `onchainos wallet addresses --chain <chainIndex>`.
///
/// Output structure (after envelope unwrap):
/// ```json
/// { "accountId": "...", "xlayer": [...], "evm": [...], "solana": [...] }
/// ```
/// Each array entry: `{ "address": "0x...", "chainIndex": "1", "chainName": "eth" }`
async fn resolve_wallet_address(chain_index: &str) -> Result<String> {
    let data = run_onchainos_cmd(&["wallet", "addresses", "--chain", chain_index]).await?;

    // Determine which group to look into based on chain_index
    let group_key = match chain_index {
        "196" => "xlayer",
        "501" => "solana",
        _ => "evm",
    };

    let address = data[group_key]
        .as_array()
        .and_then(|arr| arr.first())
        .and_then(|entry| entry["address"].as_str())
        .ok_or_else(|| {
            anyhow::anyhow!(
                "no wallet address found for chain {} — run `onchainos wallet login` first",
                chain_index
            )
        })?;

    Ok(address.to_string())
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

/// Convert minimal units (wei) to UI units string.
fn wei_to_ui(wei: &str, decimals: u32) -> String {
    if wei == "0" || wei.is_empty() {
        return "0".to_string();
    }
    let wei_val: u128 = match wei.parse() {
        Ok(v) => v,
        Err(_) => return wei.to_string(),
    };
    if decimals == 0 {
        return wei_val.to_string();
    }
    let divisor = 10u128.pow(decimals);
    let whole = wei_val / divisor;
    let frac = wei_val % divisor;
    if frac == 0 {
        whole.to_string()
    } else {
        let frac_str = format!("{:0>width$}", frac, width = decimals as usize);
        let trimmed = frac_str.trim_end_matches('0');
        format!("{}.{}", whole, trimmed)
    }
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
        let uint256_max = "115792089237316195423570985008687907853269984665640564039457584007913129639935";
        assert!(!is_allowance_insufficient(uint256_max, "1000000"));
    }

    #[test]
    fn test_wei_to_ui() {
        assert_eq!(wei_to_ui("0", 18), "0");
        assert_eq!(wei_to_ui("", 18), "0");
        assert_eq!(wei_to_ui("1000000000000000000", 18), "1");
        assert_eq!(wei_to_ui("10000000000000000", 18), "0.01");
        assert_eq!(wei_to_ui("1500000000000000000", 18), "1.5");
        assert_eq!(wei_to_ui("1000000", 6), "1");
        assert_eq!(wei_to_ui("1500000", 6), "1.5");
        assert_eq!(wei_to_ui("100", 0), "100");
    }
}
