use std::sync::Arc;

use anyhow::{bail, Context as _, Result};
use clap::Subcommand;
use serde_json::{json, Value};
use tokio::task::JoinSet;

use super::Context;
use crate::{chains, output};

/// Max tokens per token-scan API request.
const BATCH_SIZE: usize = 50;

#[derive(Subcommand)]
pub enum SecurityCommand {
    /// Batch token security scan — detect honeypots, high tax, mint risks.
    /// Three input modes:
    ///   1. --tokens "chainId:addr,..."   — explicit token list (up to 50)
    ///   2. --address <wallet_addr>       — scan all tokens held at a wallet address
    ///   3. (no flags)                    — scan tokens of the logged-in agentic wallet
    TokenScan {
        /// Explicit token list: chainId:contractAddress, comma-separated (up to 10).
        /// Mutually exclusive with --address.
        #[arg(long, conflicts_with = "address")]
        tokens: Option<String>,

        /// Wallet address whose token holdings will be scanned.
        /// Mutually exclusive with --tokens.
        #[arg(long, conflicts_with = "tokens")]
        address: Option<String>,

        /// Chain name or ID filter used with --address or logged-in wallet mode
        /// (e.g. ethereum, solana, xlayer, 1, 501, 196).
        #[arg(long)]
        chain: Option<String>,
    },

    /// DApp / URL security scan — detect phishing sites, blacklisted domains
    DappScan {
        /// Full URL or domain to check
        #[arg(long)]
        domain: String,
    },

    /// Transaction pre-execution security scan (EVM & Solana)
    TxScan {
        /// Sender address (0x hex for EVM, Base58 for Solana)
        #[arg(long)]
        from: String,

        /// Target address (EVM only)
        #[arg(long)]
        to: Option<String>,

        /// Chain name or ID (e.g. ethereum, 1, solana, 501)
        #[arg(long)]
        chain: String,

        /// Transaction calldata, hex-encoded (EVM only)
        #[arg(long)]
        data: Option<String>,

        /// Transaction value in wei, hex string (EVM only)
        #[arg(long)]
        value: Option<String>,

        /// Gas limit (EVM only)
        #[arg(long)]
        gas: Option<u64>,

        /// Gas price (EVM only)
        #[arg(long)]
        gas_price: Option<u64>,

        /// Encoding format: base58 or base64 (Solana only)
        #[arg(long)]
        encoding: Option<String>,

        /// Transaction payloads, comma-separated (Solana only)
        #[arg(long)]
        transactions: Option<String>,
    },

    /// Query token approval / permit2 authorizations
    Approvals {
        /// Wallet address to query
        #[arg(long)]
        address: String,
        /// Comma-separated chain names or indexes (e.g. "ethereum,base" or "1,8453")
        #[arg(long)]
        chain: Option<String>,
        /// Number of results per page (default: 20)
        #[arg(long, default_value = "20")]
        limit: u32,
        /// Pagination cursor from previous response
        #[arg(long)]
        cursor: Option<u64>,
    },

    /// Message signature security scan — detect phishing signatures (EIP-712, personal_sign)
    SigScan {
        /// Signer address (0x hex)
        #[arg(long)]
        from: String,

        /// Chain name or ID (e.g. ethereum, 1, bsc, 56)
        #[arg(long)]
        chain: String,

        /// Signing method: personal_sign or eth_signTypedData_v4
        #[arg(long)]
        sig_method: String,

        /// Message content or EIP-712 typed data JSON string
        #[arg(long)]
        message: String,
    },
}

pub async fn execute(ctx: &Context, cmd: SecurityCommand) -> Result<()> {
    match cmd {
        SecurityCommand::TokenScan {
            tokens,
            address,
            chain,
        } => token_scan(ctx, tokens.as_deref(), address.as_deref(), chain.as_deref()).await,
        SecurityCommand::Approvals {
            address,
            chain,
            limit,
            cursor,
        } => approvals(ctx, &address, chain.as_deref(), limit, cursor).await,
        SecurityCommand::DappScan { domain } => dapp_scan(ctx, &domain).await,
        SecurityCommand::TxScan {
            from,
            to,
            chain,
            data,
            value,
            gas,
            gas_price,
            encoding,
            transactions,
        } => {
            tx_scan(
                ctx,
                &from,
                to.as_deref(),
                &chain,
                data.as_deref(),
                value.as_deref(),
                gas,
                gas_price,
                encoding.as_deref(),
                transactions.as_deref(),
            )
            .await
        }
        SecurityCommand::SigScan {
            from,
            chain,
            sig_method,
            message,
        } => sig_scan(ctx, &from, &chain, &sig_method, &message).await,
    }
}

/// Dispatcher — three paths matching the product decision tree:
///
/// Path 1  (no flags)           — Has Agentic Wallet
///   Uses authenticated wallet balance API (accountId) to fetch token holdings.
///   Supports wallet / chain / single-token dimension via --chain.
///
/// Path 2  (--address)          — No Agentic Wallet, user provides a public address
///   Uses public portfolio API to fetch token holdings for the given address.
///   Supports chain dimension via --chain.
///
/// Path 3  (--tokens)           — User provides explicit chainId:contractAddress
///   Directly scans the specified token(s). No asset lookup needed.
async fn token_scan(
    ctx: &Context,
    tokens: Option<&str>,
    address: Option<&str>,
    chain: Option<&str>,
) -> Result<()> {
    match (tokens, address) {
        // Path 3 (2.2.2): explicit chainId:contractAddress — direct scan
        (Some(t), _) => token_scan_explicit(ctx, t).await,

        // Path 2 (2.2.1): public address — query portfolio API then scan
        (None, Some(addr)) => {
            let pairs = fetch_tokens_by_address(ctx, addr, chain).await?;
            run_batch_scan(ctx, pairs).await
        }

        // Path 1: logged-in Agentic Wallet — query wallet balance API then scan
        (None, None) => {
            let wallets = crate::wallet_store::load_wallets()?.ok_or_else(|| {
                anyhow::anyhow!(
                    "Not logged in and no --address provided.\n\
                     Provide --address <wallet_addr> or login with `onchainos wallet login`."
                )
            })?;
            let account_id = super::agentic_wallet::account::resolve_active_account_id(&wallets)?;
            let access_token = crate::keyring_store::get("access_token").map_err(|e| {
                anyhow::anyhow!(
                    "Session expired or not logged in ({}). Run `onchainos wallet login`.",
                    e
                )
            })?;
            let pairs = fetch_tokens_from_wallet(&access_token, &account_id, chain).await?;
            run_batch_scan(ctx, pairs).await
        }
    }
}

/// Path 3: scan an explicit comma-separated list of chainId:contractAddress pairs (max 50).
async fn token_scan_explicit(ctx: &Context, tokens: &str) -> Result<()> {
    let client = ctx.client()?;

    let token_list: Vec<Value> = tokens
        .split(',')
        .map(|item| {
            let item = item.trim();
            let parts: Vec<&str> = item.splitn(2, ':').collect();
            if parts.len() != 2 {
                bail!(
                    "Invalid token format '{}'. Expected chainId:contractAddress (e.g. 1:0xdAC1...)",
                    item
                );
            }
            let chain_index = chains::resolve_chain(parts[0].trim());
            Ok(json!({
                "chainId": chain_index,
                "contractAddress": parts[1].trim()
            }))
        })
        .collect::<Result<Vec<_>>>()?;

    if token_list.is_empty() {
        bail!("--tokens must contain at least one chainId:contractAddress pair");
    }
    if token_list.len() > BATCH_SIZE {
        bail!("--tokens supports at most {} items per request", BATCH_SIZE);
    }

    let body = json!({ "source": "onchain_os_cli", "tokenList": token_list });
    let result = client.post("/api/v6/security/token-scan", &body).await?;
    output::success(result);
    Ok(())
}

/// Path 1: fetch token holdings from the authenticated Agentic Wallet balance API.
/// Uses accountId + JWT, returns tokens across all chains (or filtered by chain).
async fn fetch_tokens_from_wallet(
    access_token: &str,
    account_id: &str,
    chain: Option<&str>,
) -> Result<Vec<(String, String)>> {
    let wallet_client = crate::wallet_api::WalletApiClient::new()?;
    let chain_index = chain.map(chains::resolve_chain).unwrap_or_default();

    let mut query: Vec<(&str, &str)> = vec![("accountId", account_id)];
    if !chain_index.is_empty() {
        query.push(("chains", &chain_index));
    }

    let data = wallet_client
        .get_authed(
            "/priapi/v5/wallet/agentic/asset/wallet-all-token-balances",
            access_token,
            &query,
        )
        .await?;

    extract_token_pairs(&data)
}

/// Path 2: fetch token holdings from the public portfolio API by wallet address.
/// No authentication required. Optionally filtered by chain.
async fn fetch_tokens_by_address(
    ctx: &Context,
    address: &str,
    chain: Option<&str>,
) -> Result<Vec<(String, String)>> {
    let client = ctx.client()?;
    let chains_param = chain.map(chains::resolve_chain).unwrap_or_default();

    let mut query: Vec<(&str, &str)> = vec![
        ("address", address),
        ("filter", "1"), // return all tokens including risk tokens for security scanning
    ];
    if !chains_param.is_empty() {
        query.push(("chains", &chains_param));
    }

    let data = client
        .get("/api/v6/dex/balance/all-token-balances-by-address", &query)
        .await?;

    extract_token_pairs(&data)
}

/// Shared: dispatch token pairs to token-scan API in concurrent batches of 50.
/// Merges all batch results into a single array output.
async fn run_batch_scan(ctx: &Context, token_pairs: Vec<(String, String)>) -> Result<()> {
    if token_pairs.is_empty() {
        output::success(Value::Array(vec![]));
        return Ok(());
    }

    let client = Arc::new(ctx.client()?);
    let mut set: JoinSet<Result<Value>> = JoinSet::new();

    for batch in token_pairs.chunks(BATCH_SIZE) {
        let c = Arc::clone(&client);
        let token_list: Vec<Value> = batch
            .iter()
            .map(|(ci, addr)| json!({ "chainId": ci, "contractAddress": addr }))
            .collect();
        set.spawn(async move {
            let body = json!({ "source": "onchain_os_cli", "tokenList": token_list });
            c.post("/api/v6/security/token-scan", &body).await
        });
    }

    let mut all_results: Vec<Value> = Vec::new();
    while let Some(join_res) = set.join_next().await {
        match join_res? {
            Ok(Value::Array(arr)) => all_results.extend(arr),
            Ok(other) => all_results.push(other),
            Err(e) => return Err(e),
        }
    }

    output::success(Value::Array(all_results));
    Ok(())
}

/// Parse portfolio balance response into (chainIndex, tokenContractAddress) pairs.
///
/// Distinguishes real "no tokens" from unexpected/error responses:
/// - `null` or `[]` or `{tokenAssets:[]}` → Ok(empty)  — address has no tokens
/// - unexpected structure              → Err(...)       — API format mismatch
///
/// Skips native tokens (empty contractAddress).
fn extract_token_pairs(data: &Value) -> Result<Vec<(String, String)>> {
    // data: null → API returned null for data field; treat as no tokens
    if data.is_null() {
        return Ok(vec![]);
    }

    // Locate the token array: direct array or wrapped in { tokenAssets: [...] }
    let items = match data.as_array() {
        Some(arr) => arr,
        None => match data["tokenAssets"].as_array() {
            Some(arr) => arr,
            None => {
                let preview: String = data.to_string().chars().take(200).collect();
                bail!(
                    "Unexpected portfolio response format — expected array or \
                     {{tokenAssets:[...]}} but got: {}{}",
                    preview,
                    if data.to_string().len() > 200 {
                        "…"
                    } else {
                        ""
                    }
                );
            }
        },
    };

    let pairs = items
        .iter()
        .filter_map(|item| {
            let chain_index = item["chainIndex"].as_str()?;
            let contract_addr = item["tokenContractAddress"].as_str()?;
            if contract_addr.is_empty() {
                return None; // skip native tokens (ETH/SOL/OKB)
            }
            Some((chain_index.to_string(), contract_addr.to_string()))
        })
        .collect();

    Ok(pairs)
}

async fn dapp_scan(ctx: &Context, domain: &str) -> Result<()> {
    let client = ctx.client()?;

    let body = json!({
        "source": "onchain_os_cli",
        "url": domain.trim(),
    });
    let result = client.post("/api/v6/security/dapp-scan", &body).await?;
    output::success(result);
    Ok(())
}

#[allow(clippy::too_many_arguments)]
async fn tx_scan(
    ctx: &Context,
    from: &str,
    to: Option<&str>,
    chain: &str,
    data: Option<&str>,
    value: Option<&str>,
    gas: Option<u64>,
    gas_price: Option<u64>,
    encoding: Option<&str>,
    transactions: Option<&str>,
) -> Result<()> {
    let chain_index = chains::resolve_chain(chain);
    let family = chains::chain_family(&chain_index);
    let client = ctx.client()?;

    match family {
        "solana" => {
            let encoding =
                encoding.ok_or_else(|| anyhow::anyhow!("--encoding is required for Solana"))?;
            let transactions = transactions
                .ok_or_else(|| anyhow::anyhow!("--transactions is required for Solana"))?;

            let tx_list: Vec<&str> = transactions.split(',').map(|s| s.trim()).collect();

            let body = json!({
                "source": "onchain_os_cli",
                "from": from,
                "chainId": "501",
                "encoding": encoding,
                "transactions": tx_list,
            });

            let result = client
                .post("/api/v6/security/transaction-scan/sol", &body)
                .await?;
            output::success(result);
        }
        "evm" => {
            let real_chain_id =
                super::agentic_wallet::chain::get_real_chain_index(&chain_index).await?;

            let data = data.ok_or_else(|| anyhow::anyhow!("--data is required for EVM tx-scan"))?;

            let mut body = json!({
                "source": "onchain_os_cli",
                "from": from,
                "chainId": real_chain_id,
                "data": data,
            });

            if let Some(to) = to {
                body["to"] = json!(to);
            }
            if let Some(value) = value {
                // API requires hex string; auto-convert decimal input to hex
                let hex_value = if value.starts_with("0x") || value.starts_with("0X") {
                    value.to_string()
                } else if let Ok(n) = value.parse::<u128>() {
                    format!("0x{:x}", n)
                } else {
                    value.to_string()
                };
                body["value"] = json!(hex_value);
            }
            if let Some(gas) = gas {
                body["gas"] = json!(gas);
            }
            if let Some(gas_price) = gas_price {
                body["gasPrice"] = json!(gas_price);
            }

            let result = client
                .post("/api/v6/security/transaction-scan/evm", &body)
                .await?;
            output::success(result);
        }
        _ => {
            bail!(
                "Chain '{}' (family: {}) is not supported for security tx-scan. Only EVM and Solana chains are supported.",
                chain, family
            );
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    // ── extract_token_pairs ───────────────────────────────────────────────────

    #[test]
    fn extract_null_returns_empty() {
        let result = extract_token_pairs(&Value::Null).unwrap();
        assert!(result.is_empty());
    }

    #[test]
    fn extract_empty_array_returns_empty() {
        let result = extract_token_pairs(&json!([])).unwrap();
        assert!(result.is_empty());
    }

    #[test]
    fn extract_direct_array() {
        let data = json!([
            {"chainIndex": "1", "tokenContractAddress": "0xabc"},
            {"chainIndex": "56", "tokenContractAddress": "0xdef"},
        ]);
        let pairs = extract_token_pairs(&data).unwrap();
        assert_eq!(pairs.len(), 2);
        assert_eq!(pairs[0], ("1".to_string(), "0xabc".to_string()));
        assert_eq!(pairs[1], ("56".to_string(), "0xdef".to_string()));
    }

    #[test]
    fn extract_token_assets_wrapper() {
        let data = json!({
            "tokenAssets": [
                {"chainIndex": "501", "tokenContractAddress": "So111"},
                {"chainIndex": "1",   "tokenContractAddress": "0xusdc"},
            ]
        });
        let pairs = extract_token_pairs(&data).unwrap();
        assert_eq!(pairs.len(), 2);
        assert_eq!(pairs[0].0, "501");
        assert_eq!(pairs[1].1, "0xusdc");
    }

    #[test]
    fn extract_skips_native_tokens_with_empty_address() {
        let data = json!([
            {"chainIndex": "1",  "tokenContractAddress": ""},      // native ETH — skip
            {"chainIndex": "1",  "tokenContractAddress": "0xabc"}, // ERC-20 — keep
            {"chainIndex": "501","tokenContractAddress": ""},      // native SOL — skip
        ]);
        let pairs = extract_token_pairs(&data).unwrap();
        assert_eq!(pairs.len(), 1);
        assert_eq!(pairs[0], ("1".to_string(), "0xabc".to_string()));
    }

    #[test]
    fn extract_skips_items_missing_fields() {
        let data = json!([
            {"chainIndex": "1"},                          // missing tokenContractAddress — skip
            {"tokenContractAddress": "0xabc"},            // missing chainIndex — skip
            {"chainIndex": "56", "tokenContractAddress": "0xdef"}, // complete — keep
        ]);
        let pairs = extract_token_pairs(&data).unwrap();
        assert_eq!(pairs.len(), 1);
        assert_eq!(pairs[0], ("56".to_string(), "0xdef".to_string()));
    }

    #[test]
    fn extract_unexpected_structure_returns_error() {
        let data = json!({"foo": "bar"}); // no array, no tokenAssets
        let result = extract_token_pairs(&data);
        assert!(result.is_err());
        let msg = result.unwrap_err().to_string();
        assert!(msg.contains("Unexpected portfolio response format"));
    }

    #[test]
    fn extract_error_message_is_truncated_for_large_response() {
        // Build a response with a long string value
        let long_val = "x".repeat(500);
        let data = json!({"unexpected": long_val});
        let result = extract_token_pairs(&data);
        assert!(result.is_err());
        let msg = result.unwrap_err().to_string();
        // Error message preview should not exceed ~250 chars (200 + overhead)
        assert!(
            msg.len() < 400,
            "error message too long: {} chars",
            msg.len()
        );
    }

    // ── BATCH_SIZE constant ───────────────────────────────────────────────────

    #[test]
    fn batch_size_is_50() {
        assert_eq!(BATCH_SIZE, 50);
    }

    #[test]
    fn default_base_url_is_beta() {
        assert_eq!(crate::client::DEFAULT_BASE_URL, "https://web3.okx.com");
    }

    // ── token format parsing (mirrors token_scan_explicit closure logic) ──────

    fn parse_token(item: &str) -> Result<(String, String)> {
        let item = item.trim();
        let parts: Vec<&str> = item.splitn(2, ':').collect();
        if parts.len() != 2 {
            bail!("Invalid token format '{}'", item);
        }
        let chain_index = chains::resolve_chain(parts[0].trim());
        Ok((chain_index, parts[1].trim().to_string()))
    }

    #[test]
    fn parse_token_numeric_chain_id() {
        let (ci, addr) = parse_token("1:0xdAC17F").unwrap();
        assert_eq!(ci, "1");
        assert_eq!(addr, "0xdAC17F");
    }

    #[test]
    fn parse_token_named_chain() {
        let (ci, addr) = parse_token("ethereum:0xabc").unwrap();
        assert_eq!(ci, "1");
        assert_eq!(addr, "0xabc");
    }

    #[test]
    fn parse_token_solana() {
        let (ci, addr) = parse_token("501:EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v").unwrap();
        assert_eq!(ci, "501");
        assert_eq!(addr, "EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v");
    }

    #[test]
    fn parse_token_solana_named() {
        let (ci, _) = parse_token("solana:So111").unwrap();
        assert_eq!(ci, "501");
    }

    #[test]
    fn parse_token_address_with_colon_preserved() {
        // contract address itself should not be split on ':'
        let (ci, addr) = parse_token("1:0xabc:extra").unwrap();
        assert_eq!(ci, "1");
        assert_eq!(addr, "0xabc:extra"); // splitn(2) preserves rest
    }

    #[test]
    fn parse_token_missing_colon_returns_error() {
        let result = parse_token("1_0xabc");
        assert!(result.is_err());
    }

    #[test]
    fn parse_token_empty_returns_error() {
        let result = parse_token("");
        assert!(result.is_err());
    }

    #[test]
    fn parse_token_outer_whitespace_trimmed() {
        // Both outer item and inner parts are trimmed.
        let (ci, addr) = parse_token("  56 : 0xdef  ").unwrap();
        assert_eq!(ci, "56");
        assert_eq!(addr, "0xdef");
    }
}

// --- approvals ---
async fn approvals(
    ctx: &Context,
    address: &str,
    chain: Option<&str>,
    limit: u32,
    cursor: Option<u64>,
) -> Result<()> {
    let mut body = json!({
        "nested": false,
        "limit": limit,
    });

    let address_list: Vec<Value> = match chain {
        Some(chains_str) => chains_str
            .split(',')
            .map(|c| c.trim())
            .filter(|c| !c.is_empty())
            .map(|c| {
                let ci = chains::resolve_chain(c);
                json!({
                    "chainIndex": ci,
                    "address": address,
                })
            })
            .collect(),
        None => {
            let all_chains = super::agentic_wallet::chain::get_all_chains()
                .await
                .context("Failed to load supported chain list")?;
            all_chains
                .iter()
                .filter_map(|c| {
                    let ci = c["chainIndex"]
                        .as_i64()
                        .or_else(|| c["chainIndex"].as_str().and_then(|s| s.parse().ok()))?;
                    let ci_str = ci.to_string();
                    if chains::chain_family(&ci_str) == "evm" {
                        Some(ci)
                    } else {
                        None
                    }
                })
                .map(|ci| json!({ "chainIndex": ci, "address": address }))
                .collect()
        }
    };
    if address_list.is_empty() {
        bail!("No supported chains found");
    }
    body["addressList"] = json!(address_list);

    if let Some(c) = cursor {
        body["cursor"] = json!(c);
    }

    let client = ctx.client()?;
    let data = client
        .post("/api/v6/security/approval-mng", &body)
        .await
        .context("Failed to fetch approvals")?;

    output::success(data);
    Ok(())
}

async fn sig_scan(
    ctx: &Context,
    from: &str,
    chain: &str,
    sig_method: &str,
    message: &str,
) -> Result<()> {
    let chain_index = chains::resolve_chain(chain);

    let real_chain_id = super::agentic_wallet::chain::get_real_chain_index(&chain_index).await?;

    let valid_methods = [
        "personal_sign",
        "eth_sign",
        "eth_signTypedData",
        "eth_signTypedData_v3",
        "eth_signTypedData_v4",
    ];
    if !valid_methods.contains(&sig_method) {
        bail!(
            "Invalid --sig-method '{}'. Must be one of: personal_sign, eth_sign, eth_signTypedData, eth_signTypedData_v3, eth_signTypedData_v4",
            sig_method
        );
    }

    let message_value: Value = serde_json::from_str(message).unwrap_or_else(|_| json!(message));

    let body = json!({
        "source": "onchain_os_cli",
        "from": from,
        "chainId": real_chain_id,
        "signType": sig_method,
        "message": message_value,
    });

    let client = ctx.client()?;
    let result = client
        .post("/api/v6/security/sign-message-check", &body)
        .await?;
    output::success(result);
    Ok(())
}
