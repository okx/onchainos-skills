use anyhow::{bail, Result};
use clap::Subcommand;
use serde_json::{json, Value};

use super::Context;
use crate::client::ApiClient;
use crate::output;

/// Inner API base path for cross-chain bridge suite.
const BRIDGE_API_PREFIX: &str = "/priapi/v1/dx/trade/bridge/suite";

#[derive(Subcommand)]
pub enum CrossChainCommand {
    /// Get supported chain pairs for cross-chain
    Chains,

    /// Get available bridge protocols
    Bridge,

    /// Get cross-chain quote (read-only)
    Quote {
        /// Source token contract address or alias
        #[arg(long)]
        from: String,
        /// Destination token contract address or alias
        #[arg(long)]
        to: String,
        /// Source chain (e.g. ethereum, arbitrum)
        #[arg(long)]
        from_chain: String,
        /// Destination chain (e.g. optimism, base)
        #[arg(long)]
        to_chain: String,
        /// Human-readable amount (e.g. "10" for 10 USDC, decimal format, do NOT multiply by decimals)
        #[arg(long)]
        readable_amount: String,
        /// Receive address on destination chain (defaults to wallet address)
        #[arg(long)]
        receive_address: Option<String>,
        /// Sort preference: 0=optimal(default), 1=fastest, 2=max output
        #[arg(long, default_value = "0")]
        sort: String,
    },

    /// Execute cross-chain: three modes (default / --confirm-approve / --skip-approve)
    Execute {
        /// Source token contract address or alias
        #[arg(long)]
        from: String,
        /// Destination token contract address or alias
        #[arg(long)]
        to: String,
        /// Source chain
        #[arg(long)]
        from_chain: String,
        /// Destination chain
        #[arg(long)]
        to_chain: String,
        /// Human-readable amount (decimal format)
        #[arg(long)]
        readable_amount: String,
        /// User wallet address
        #[arg(long)]
        wallet: String,
        /// Receive address on destination chain
        #[arg(long)]
        receive_address: Option<String>,
        /// Route index from quote result (default: 0 = recommended)
        #[arg(long, default_value_t = 0)]
        route_index: usize,
        /// Gas priority: slow, average, fast
        #[arg(long, default_value = "average")]
        gas_level: String,
        /// Enable MEV protection (EVM chains)
        #[arg(long, default_value_t = false)]
        mev_protection: bool,
        /// Jito tips in SOL for Solana MEV protection
        #[arg(long)]
        tips: Option<String>,
        /// Confirm and execute token approval (after user confirms)
        #[arg(long, default_value_t = false, conflicts_with = "skip_approve")]
        confirm_approve: bool,
        /// Skip allowance check, execute trade directly (after approval confirmed)
        #[arg(long, default_value_t = false, conflicts_with = "confirm_approve")]
        skip_approve: bool,
    },

    /// Get calldata only — does NOT sign or broadcast (for manual use)
    Calldata {
        /// Source token contract address or alias
        #[arg(long)]
        from: String,
        /// Destination token contract address or alias
        #[arg(long)]
        to: String,
        /// Source chain
        #[arg(long)]
        from_chain: String,
        /// Destination chain
        #[arg(long)]
        to_chain: String,
        /// Human-readable amount (decimal format)
        #[arg(long)]
        readable_amount: String,
        /// User wallet address
        #[arg(long)]
        wallet: String,
        /// Receive address on destination chain
        #[arg(long)]
        receive_address: Option<String>,
        /// Route index from quote result (default: 0 = recommended)
        #[arg(long, default_value_t = 0)]
        route_index: usize,
    },

    /// Query cross-chain order status
    Status {
        /// Order ID returned by execute
        #[arg(long)]
        order_id: String,
    },

    /// Probe which common tokens (USDC/USDT/native) can be bridged between two chains
    Probe {
        /// Source chain (e.g. ethereum, arbitrum)
        #[arg(long)]
        from_chain: String,
        /// Destination chain (e.g. solana, base)
        #[arg(long)]
        to_chain: String,
        /// Human-readable amount for estimation (default: 100)
        #[arg(long, default_value = "100")]
        readable_amount: String,
    },
}

// ── Local validation helpers ───────────────────────────────────────

/// Validate readable-amount is a positive number.
fn validate_amount(readable_amount: &str) -> Result<()> {
    let amt: f64 = readable_amount
        .parse()
        .map_err(|_| anyhow::anyhow!("invalid readable-amount: \"{}\"", readable_amount))?;
    if amt <= 0.0 {
        bail!("readable-amount must be greater than 0");
    }
    Ok(())
}

/// Validate order-id is a non-empty numeric string.
fn validate_order_id(order_id: &str) -> Result<()> {
    if order_id.is_empty() || !order_id.chars().all(|c| c.is_ascii_digit()) {
        bail!(
            "invalid order-id: \"{}\". Order ID must be a numeric string.",
            order_id
        );
    }
    Ok(())
}

/// Check that receive-address matches the destination chain's address family.
/// Returns an error if the address format clearly mismatches the chain.
fn validate_receive_address(receive_address: &str, to_chain_index: &str) -> Result<()> {
    let to_family = crate::chains::chain_family(to_chain_index);
    let addr_looks_evm = receive_address.starts_with("0x") && receive_address.len() == 42;
    let addr_looks_solana = !receive_address.starts_with("0x")
        && receive_address.len() >= 32
        && receive_address.len() <= 44
        && receive_address.chars().all(|c| c.is_alphanumeric());

    match to_family {
        "solana" if addr_looks_evm => {
            bail!(
                "receive-address looks like an EVM address, but destination chain is Solana. \
                 Please provide a Solana address."
            );
        }
        "evm" if addr_looks_solana && !addr_looks_evm => {
            bail!(
                "receive-address looks like a Solana address, but destination chain is EVM. \
                 Please provide an EVM address (0x...)."
            );
        }
        _ => Ok(()),
    }
}

// ── Public entry point ──────────────────────────────────────────────

pub async fn execute(ctx: &Context, cmd: CrossChainCommand) -> Result<()> {
    let mut client = ctx.client_async().await?;
    match cmd {
        CrossChainCommand::Chains => {
            output::success(fetch_chain_pairs(&mut client).await?);
        }
        CrossChainCommand::Bridge => {
            output::success(fetch_bridges(&mut client).await?);
        }
        CrossChainCommand::Quote {
            from,
            to,
            from_chain,
            to_chain,
            readable_amount,
            receive_address,
            sort,
        } => {
            validate_amount(&readable_amount)?;
            let from_chain_index = crate::chains::resolve_chain(&from_chain);
            let to_chain_index = crate::chains::resolve_chain(&to_chain);
            if let Some(ref addr) = receive_address {
                validate_receive_address(addr, &to_chain_index)?;
            }
            let from_token =
                crate::commands::swap::resolve_token_address(&from_chain_index, &from);
            let to_token = crate::commands::swap::resolve_token_address(&to_chain_index, &to);
            output::success(
                fetch_quote(
                    &mut client,
                    &from_chain_index,
                    &to_chain_index,
                    &from_token,
                    &to_token,
                    &readable_amount,
                    receive_address.as_deref(),
                    &sort,
                )
                .await?,
            );
        }
        CrossChainCommand::Execute {
            from,
            to,
            from_chain,
            to_chain,
            readable_amount,
            wallet,
            receive_address,
            route_index,
            gas_level,
            mev_protection,
            tips,
            confirm_approve,
            skip_approve,
        } => {
            cmd_execute(
                &mut client,
                &from,
                &to,
                &from_chain,
                &to_chain,
                &readable_amount,
                &wallet,
                receive_address.as_deref(),
                route_index,
                &gas_level,
                mev_protection,
                tips.as_deref(),
                confirm_approve,
                skip_approve,
            )
            .await?;
        }
        CrossChainCommand::Calldata {
            from,
            to,
            from_chain,
            to_chain,
            readable_amount,
            wallet,
            receive_address,
            route_index,
        } => {
            cmd_calldata(
                &mut client,
                &from,
                &to,
                &from_chain,
                &to_chain,
                &readable_amount,
                &wallet,
                receive_address.as_deref(),
                route_index,
            )
            .await?;
        }
        CrossChainCommand::Status { order_id } => {
            validate_order_id(&order_id)?;
            output::success(fetch_order_details(&mut client, &order_id).await?);
        }
        CrossChainCommand::Probe {
            from_chain,
            to_chain,
            readable_amount,
        } => {
            cmd_probe(&mut client, &from_chain, &to_chain, &readable_amount).await?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── validate_amount ────────────────────────────────────────────

    #[test]
    fn amount_positive_ok() {
        assert!(validate_amount("10").is_ok());
        assert!(validate_amount("0.001").is_ok());
    }

    #[test]
    fn amount_zero_rejected() {
        assert!(validate_amount("0").is_err());
    }

    #[test]
    fn amount_negative_rejected() {
        assert!(validate_amount("-1").is_err());
    }

    #[test]
    fn amount_non_numeric_rejected() {
        assert!(validate_amount("abc").is_err());
        assert!(validate_amount("").is_err());
    }

    // ── validate_order_id ────────────────────────────────────────

    #[test]
    fn order_id_numeric_ok() {
        assert!(validate_order_id("17109522093792128").is_ok());
    }

    #[test]
    fn order_id_non_numeric_rejected() {
        assert!(validate_order_id("abc-invalid").is_err());
        assert!(validate_order_id("123abc").is_err());
        assert!(validate_order_id("").is_err());
    }

    // ── validate_receive_address ─────────────────────────────────

    #[test]
    fn evm_addr_to_evm_chain_ok() {
        assert!(validate_receive_address(
            "0x896f4edd6601eda7d12f077a35e1cdf2898282ce",
            "42161" // Arbitrum
        )
        .is_ok());
    }

    #[test]
    fn evm_addr_to_solana_rejected() {
        assert!(validate_receive_address(
            "0x896f4edd6601eda7d12f077a35e1cdf2898282ce",
            "501" // Solana
        )
        .is_err());
    }

    #[test]
    fn solana_addr_to_evm_rejected() {
        assert!(validate_receive_address(
            "5EDUCQDeVmaGohSAJYQ8mwe4hZMXgDzS4X2Si3Zh3cL5",
            "8453" // Base
        )
        .is_err());
    }

    #[test]
    fn solana_addr_to_solana_ok() {
        assert!(validate_receive_address(
            "5EDUCQDeVmaGohSAJYQ8mwe4hZMXgDzS4X2Si3Zh3cL5",
            "501"
        )
        .is_ok());
    }

    // ── extract_bridge_ids ───────────────────────────────────────

    #[test]
    fn extract_ids_from_chain_pairs() {
        let pairs = json!({
            "1": {
                "10": [{"bridgeId": 52}, {"bridgeId": 39}],
                "42161": [{"bridgeId": 52}]
            },
            "8453": {
                "42161": [{"bridgeId": 100}, {"bridgeId": 52}]
            }
        });
        let ids = extract_bridge_ids(&pairs);
        assert_eq!(ids.len(), 3);
        assert!(ids.contains(&52));
        assert!(ids.contains(&39));
        assert!(ids.contains(&100));
    }

    #[test]
    fn extract_ids_empty_input() {
        let ids = extract_bridge_ids(&json!({}));
        assert!(ids.is_empty());
    }

    // ── Integration tests (manual) ──────────────────────────────

    #[tokio::test]
    #[ignore = "manual integration test against pre-production order/update"]
    async fn forged_order_update_returns_error() {
        let mut client = crate::client::ApiClient::new_async(None)
            .await
            .expect("create client");

        let result = fetch_order_update(
            &mut client,
            "99999999999999999",
            "0x1111111111111111111111111111111111111111111111111111111111111111",
        )
        .await;

        match result {
            Ok(value) => panic!("unexpected success: {value}"),
            Err(err) => {
                let msg = err.to_string();
                println!("forged order/update error: {msg}");
                assert!(
                    !msg.is_empty(),
                    "expected forged order/update to return a non-empty error"
                );
            }
        }
    }

    #[tokio::test]
    #[ignore = "manual integration test against pre-production order/save + forged order/update"]
    async fn order_save_then_forged_update_returns_error() {
        let mut client = crate::client::ApiClient::new_async(None)
            .await
            .expect("create client");

        let quote_param = json!({
            "chainId": "42161",
            "toChainId": "10",
            "fromTokenAddress": "0xaf88d065e77c8cc2239327c5edb3a432268e5831",
            "toTokenAddress": "0x0b2c639c533813f4aa9d7837caf62653d097ff85",
            "amount": "1",
            "slippage": "0",
            "gasDropType": 0,
            "slippageType": 0,
            "userWalletAddress": "0xf290d284ef50d6ad0b97dd67221e59c05c97dfbc",
        });

        let quote_result = client
            .post(
                &format!("{}/quote", BRIDGE_API_PREFIX),
                &json!({ "dexQuoteParam": quote_param, "isOnlineQuote": true }),
            )
            .await
            .expect("quote");

        let selected = quote_result["pathSelectionRouterList"][0].clone();
        let mut quote_for_order = quote_result.clone();
        quote_for_order["pathSelectionRouterList"] = json!([selected]);

        let order_id_val = fetch_order_save(
            &mut client,
            &quote_param,
            &quote_for_order,
            "0xf290d284ef50d6ad0b97dd67221e59c05c97dfbc",
            "0xf290d284ef50d6ad0b97dd67221e59c05c97dfbc",
        )
        .await
        .expect("order save");

        let order_id = match &order_id_val {
            Value::Number(n) => n.to_string(),
            Value::String(s) => s.clone(),
            _ => panic!("unexpected orderId format: {order_id_val:?}"),
        };

        let result = fetch_order_update(
            &mut client,
            &order_id,
            "0x2222222222222222222222222222222222222222222222222222222222222222",
        )
        .await;

        match result {
            Ok(value) => panic!("unexpected success for saved order {order_id}: {value}"),
            Err(err) => {
                let msg = err.to_string();
                println!("saved order {order_id} forged update error: {msg}");
                assert!(
                    !msg.is_empty(),
                    "expected forged update after order/save to return a non-empty error"
                );
            }
        }
    }
}


// ── API call functions ──────────────────────────────────────────────

/// POST /chainPair/list — Get supported chain pairs
pub async fn fetch_chain_pairs(client: &mut ApiClient) -> Result<Value> {
    client
        .post(&format!("{}/chainPair/list", BRIDGE_API_PREFIX), &json!({}))
        .await
}

/// Extract unique bridgeIds from chainPair/list response.
/// Structure: Map<fromChainId, Map<toChainId, List<{bridgeId, ...}>>>
fn extract_bridge_ids(pairs: &Value) -> std::collections::HashSet<i64> {
    let mut ids = std::collections::HashSet::new();
    let entries = pairs
        .as_object()
        .into_iter()
        .flat_map(|m| m.values())
        .filter_map(|v| v.as_object())
        .flat_map(|m| m.values())
        .filter_map(|v| v.as_array())
        .flatten();
    for entry in entries {
        if let Some(id) = entry["bridgeId"].as_i64() {
            ids.insert(id);
        }
    }
    ids
}

/// Get active bridge protocols.
/// Two-step: fetch chainPair/list to collect active bridgeIds, then
/// fetch dict/get and filter to only return bridges with active pairs.
pub async fn fetch_bridges(client: &mut ApiClient) -> Result<Value> {
    // 1. Get all chain pairs and extract unique bridgeIds
    let pairs = client
        .post(&format!("{}/chainPair/list", BRIDGE_API_PREFIX), &json!({}))
        .await?;
    let active_ids = extract_bridge_ids(&pairs);

    // 2. Get full bridge dict and filter
    let all_bridges = client
        .get(
            &format!("{}/dict/get", BRIDGE_API_PREFIX),
            &[("className", "BridgeSwapEnum")],
        )
        .await?;
    if let Some(arr) = all_bridges.as_array() {
        let filtered: Vec<&Value> = arr
            .iter()
            .filter(|b| {
                b["code"]
                    .as_i64()
                    .map(|c| active_ids.contains(&c))
                    .unwrap_or(false)
            })
            .collect();
        Ok(serde_json::json!(filtered))
    } else {
        Ok(all_bridges)
    }
}

/// POST /quote — Get cross-chain quote
#[allow(clippy::too_many_arguments)]
pub async fn fetch_quote(
    client: &mut ApiClient,
    from_chain_index: &str,
    to_chain_index: &str,
    from_token: &str,
    to_token: &str,
    amount: &str,
    receive_address: Option<&str>,
    sort: &str,
) -> Result<Value> {
    let mut dex_quote_param = json!({
        "chainId": from_chain_index,
        "toChainId": to_chain_index,
        "fromTokenAddress": from_token,
        "toTokenAddress": to_token,
        "amount": amount,
        "slippage": "0",
        "gasDropType": 0,
        "slippageType": 0,
    });
    if let Some(addr) = receive_address {
        dex_quote_param["userWalletAddress"] = json!(addr);
        dex_quote_param["receiveWalletAddress"] = json!(addr);
    }
    // sort is used at Skill layer to pick route, not sent to API
    let _ = sort;

    client
        .post(
            &format!("{}/quote", BRIDGE_API_PREFIX),
            &json!({ "dexQuoteParam": dex_quote_param, "isOnlineQuote": true }),
        )
        .await
}

/// POST /callData — Get unsigned transaction data
async fn fetch_call_data(
    client: &mut ApiClient,
    quote_param: &Value,
    quote_result: &Value,
    user_wallet: &str,
    receive_wallet: &str,
) -> Result<Value> {
    client
        .post(
            &format!("{}/callData", BRIDGE_API_PREFIX),
            &json!({
                "quoteParam": quote_param,
                "quoteResult": quote_result,
                "userWalletAddress": user_wallet,
                "receiveWalletAddress": receive_wallet,
            }),
        )
        .await
}

/// POST /contract — Get approve/router contract addresses (fallback when dynamicApproveAddress is null)
async fn fetch_contract(
    client: &mut ApiClient,
    bridge_id: i64,
    from_chain_id: i64,
    from_token: &str,
    to_chain_id: i64,
    to_token: &str,
) -> Result<Value> {
    client
        .post(
            &format!("{}/contract", BRIDGE_API_PREFIX),
            &json!({
                "bridgeId": bridge_id,
                "fromChainId": from_chain_id,
                "fromTokenAddress": from_token,
                "toChainId": to_chain_id,
                "toTokenAddress": to_token,
            }),
        )
        .await
}

/// POST /order/save — Save order, returns orderId
async fn fetch_order_save(
    client: &mut ApiClient,
    quote_param: &Value,
    quote_result: &Value,
    user_wallet: &str,
    receive_wallet: &str,
) -> Result<Value> {
    client
        .post(
            &format!("{}/order/save", BRIDGE_API_PREFIX),
            &json!({
                "quoteParam": quote_param,
                "quoteResult": quote_result,
                "userWalletAddress": user_wallet,
                "receiveWalletAddress": receive_wallet,
            }),
        )
        .await
}

/// POST /order/update — Update order with txHash
async fn fetch_order_update(client: &mut ApiClient, order_id: &str, tx_hash: &str) -> Result<Value> {
    client
        .post(
            &format!("{}/order/update", BRIDGE_API_PREFIX),
            &json!({
                "orderId": order_id.parse::<i64>().unwrap_or(0),
                "txHash": tx_hash,
            }),
        )
        .await
}

/// GET /order/details — Query order status
pub async fn fetch_order_details(client: &mut ApiClient, order_id: &str) -> Result<Value> {
    client
        .get(
            &format!("{}/order/details", BRIDGE_API_PREFIX),
            &[("orderId", order_id)],
        )
        .await
}

// ── Helper: resolve spender address for approve ─────────────────────

/// Get spender address: prefer dynamicApproveAddress from quote, fallback to /contract
async fn resolve_spender(
    client: &mut ApiClient,
    route: &Value,
    from_chain_index: &str,
    to_chain_index: &str,
    from_token: &str,
    to_token: &str,
) -> Result<String> {
    let bridge = &route["bridge"];

    // Try dynamicApproveAddress first
    if let Some(addr) = bridge["callDataMap"]["dynamicApproveAddress"].as_str() {
        if !addr.is_empty() {
            return Ok(addr.to_string());
        }
    }

    // Fallback: call /contract
    let bridge_id = bridge["bridgeId"]
        .as_str()
        .and_then(|s| s.parse::<i64>().ok())
        .or_else(|| bridge["bridgeId"].as_i64())
        .ok_or_else(|| anyhow::anyhow!("missing bridgeId in route"))?;
    let from_chain_id = from_chain_index
        .parse::<i64>()
        .map_err(|_| anyhow::anyhow!("invalid from_chain_index"))?;
    let to_chain_id = to_chain_index
        .parse::<i64>()
        .map_err(|_| anyhow::anyhow!("invalid to_chain_index"))?;

    let contract_data =
        fetch_contract(client, bridge_id, from_chain_id, from_token, to_chain_id, to_token)
            .await?;
    let approve_addr = contract_data["approve"]
        .as_str()
        .ok_or_else(|| anyhow::anyhow!("missing approve field in /contract response"))?;
    Ok(approve_addr.to_string())
}

// ── Helper: build approve calldata ──────────────────────────────────

/// Construct ERC20 approve(spender, amount) calldata hex
fn build_approve_calldata(spender: &str, amount_raw: &str) -> String {
    let spender_clean = spender.trim_start_matches("0x").to_lowercase();
    let amount_hex = format!("{:0>64x}", amount_raw.parse::<u128>().unwrap_or(0));
    format!(
        "0x095ea7b3{:0>64}{}",
        spender_clean, amount_hex
    )
}

// ── Helper: wallet contract-call wrapper ────────────────────────────

#[allow(clippy::too_many_arguments)]
async fn wallet_contract_call(
    to: &str,
    chain: &str,
    amt: &str,
    input_data: Option<&str>,
    unsigned_tx: Option<&str>,
    mev_protection: bool,
    jito_unsigned_tx: Option<&str>,
    aa_dex_token_addr: Option<&str>,
) -> Result<Value> {
    let tx_hash = crate::commands::agentic_wallet::transfer::execute_contract_call(
        to,
        chain,
        amt,
        input_data,
        unsigned_tx,
        None, // gas_limit
        None, // from
        aa_dex_token_addr,
        None,      // aa_dex_token_amount
        mev_protection,
        jito_unsigned_tx,
        false,     // force
        Some("3"), // tx_source: cross-chain bridge
    )
    .await?;
    Ok(json!({ "txHash": tx_hash }))
}

fn extract_tx_hash(data: &Value) -> Result<String> {
    data["txHash"]
        .as_str()
        .map(|s| s.to_string())
        .ok_or_else(|| anyhow::anyhow!("missing txHash in contract-call output"))
}

// ── cmd_calldata: returns calldata only, no sign/broadcast ──────────

#[allow(clippy::too_many_arguments)]
async fn cmd_calldata(
    client: &mut ApiClient,
    from: &str,
    to: &str,
    from_chain: &str,
    to_chain: &str,
    readable_amount: &str,
    wallet: &str,
    receive_address: Option<&str>,
    route_index: usize,
) -> Result<()> {
    validate_amount(readable_amount)?;
    let from_chain_index = crate::chains::resolve_chain(from_chain);
    let to_chain_index = crate::chains::resolve_chain(to_chain);
    let from_token = crate::commands::swap::resolve_token_address(&from_chain_index, from);
    let to_token = crate::commands::swap::resolve_token_address(&to_chain_index, to);
    let receive_wallet = receive_address.unwrap_or(wallet);
    validate_receive_address(receive_wallet, &to_chain_index)?;

    // 1. Quote
    let quote_param = json!({
        "chainId": from_chain_index,
        "toChainId": to_chain_index,
        "fromTokenAddress": from_token,
        "toTokenAddress": to_token,
        "amount": readable_amount,
        "slippage": "0",
        "gasDropType": 0,
        "slippageType": 0,
        "userWalletAddress": wallet,
    });
    let quote_result = client
        .post(
            &format!("{}/quote", BRIDGE_API_PREFIX),
            &json!({ "dexQuoteParam": quote_param, "isOnlineQuote": true }),
        )
        .await?;

    // 2. Select route
    let routes = quote_result["pathSelectionRouterList"]
        .as_array()
        .ok_or_else(|| anyhow::anyhow!("no routes returned from /quote"))?;
    if route_index >= routes.len() {
        bail!(
            "route_index {} out of range, only {} routes available",
            route_index,
            routes.len()
        );
    }
    let selected = &routes[route_index];
    let mut quote_for_calldata = quote_result.clone();
    quote_for_calldata["pathSelectionRouterList"] = json!([selected]);

    // 3. CallData
    let calldata_result = fetch_call_data(
        client,
        &quote_param,
        &quote_for_calldata,
        wallet,
        receive_wallet,
    )
    .await?;

    output::success(calldata_result);
    Ok(())
}

// ── cmd_execute: three-mode cross-chain flow ─────────────────────────
//
// Mode 1 (default):        check allowance → approve-required or execute
// Mode 2 (--confirm-approve): execute approval → approved
// Mode 3 (--skip-approve):    skip allowance check → execute trade

#[allow(clippy::too_many_arguments)]
async fn cmd_execute(
    client: &mut ApiClient,
    from: &str,
    to: &str,
    from_chain: &str,
    to_chain: &str,
    readable_amount: &str,
    wallet: &str,
    receive_address: Option<&str>,
    route_index: usize,
    gas_level: &str,
    mev_protection: bool,
    _tips: Option<&str>,
    confirm_approve: bool,
    skip_approve: bool,
) -> Result<()> {
    let from_chain_index = crate::chains::resolve_chain(from_chain);
    let to_chain_index = crate::chains::resolve_chain(to_chain);
    let from_token = crate::commands::swap::resolve_token_address(&from_chain_index, from);
    let to_token = crate::commands::swap::resolve_token_address(&to_chain_index, to);
    let receive_wallet = receive_address.unwrap_or(wallet);
    let _ = gas_level;

    // ── 0. Local validation ─────────────────────────────────────────
    validate_amount(readable_amount)?;
    validate_receive_address(receive_wallet, &to_chain_index)?;

    // Balance pre-check
    check_balance(client, wallet, &from_chain_index, &from_token, from, readable_amount).await?;

    // ── 1. Quote ────────────────────────────────────────────────────
    let quote_param = json!({
        "chainId": from_chain_index,
        "toChainId": to_chain_index,
        "fromTokenAddress": from_token,
        "toTokenAddress": to_token,
        "amount": readable_amount,
        "slippage": "0",
        "gasDropType": 0,
        "slippageType": 0,
        "userWalletAddress": wallet,
    });
    let quote_result = client
        .post(
            &format!("{}/quote", BRIDGE_API_PREFIX),
            &json!({ "dexQuoteParam": quote_param, "isOnlineQuote": true }),
        )
        .await?;

    // Select route
    let routes = quote_result["pathSelectionRouterList"]
        .as_array()
        .ok_or_else(|| anyhow::anyhow!("no routes returned from /quote"))?;
    if route_index >= routes.len() {
        bail!(
            "route_index {} out of range, only {} routes available",
            route_index,
            routes.len()
        );
    }
    let selected_route = &routes[route_index];
    let bridge_name = selected_route["bridge"]["bridgeName"]
        .as_str()
        .unwrap_or("unknown");
    let estimated_time = selected_route["estimatedTime"]
        .as_str()
        .unwrap_or("unknown");
    let receive_amount = selected_route["receiveAmount"]
        .as_str()
        .unwrap_or("unknown");
    let minimum_received = selected_route["minimumReceived"]
        .as_str()
        .unwrap_or("unknown");
    let total_fee = selected_route["totalFee"]
        .as_str()
        .unwrap_or("unknown");
    let token_symbol = quote_result["commonDexInfo"]["fromToken"]["tokenSymbol"]
        .as_str()
        .unwrap_or("token");

    // Compute required raw amount
    let decimal: u32 = quote_result["commonDexInfo"]["fromToken"]["decimals"]
        .as_str()
        .or_else(|| quote_result["commonDexInfo"]["fromToken"]["decimal"].as_str())
        .and_then(|s| s.parse().ok())
        .or_else(|| {
            quote_result["commonDexInfo"]["fromToken"]["decimals"]
                .as_u64()
                .map(|v| v as u32)
        })
        .or_else(|| {
            quote_result["commonDexInfo"]["fromToken"]["decimal"]
                .as_u64()
                .map(|v| v as u32)
        })
        .unwrap_or(18);
    let raw_amount = crate::commands::swap::readable_to_minimal_str(readable_amount, decimal)?;

    // ── Mode 2: --confirm-approve (execute approval only) ──────────
    if confirm_approve {
        let spender = resolve_spender(
            client,
            selected_route,
            &from_chain_index,
            &to_chain_index,
            &from_token,
            &to_token,
        )
        .await?;
        let token_address = selected_route["bridge"]["dexMultiTokenAllowanceOut"]
            ["tokenContractAddress"]
            .as_str()
            .unwrap_or(&from_token);

        // USDT pattern: revoke first
        let need_cancel = selected_route["bridge"]["dexMultiTokenAllowanceOut"]
            ["needCancelApproveToken"]
            .as_bool()
            .unwrap_or(false);
        if need_cancel {
            let revoke_calldata = build_approve_calldata(&spender, "0");
            let revoke_result = wallet_contract_call(
                token_address,
                &from_chain_index,
                "0",
                Some(&revoke_calldata),
                None,
                false,
                None,
                None,
            )
            .await?;
            extract_tx_hash(&revoke_result)?;
        }

        // Approve with exact transaction amount
        let approve_calldata = build_approve_calldata(&spender, &raw_amount);
        let approve_result = wallet_contract_call(
            token_address,
            &from_chain_index,
            "0",
            Some(&approve_calldata),
            None,
            false,
            None,
            None,
        )
        .await?;
        let approve_tx_hash = extract_tx_hash(&approve_result)?;

        output::success(json!({
            "action": "approved",
            "approveTxHash": approve_tx_hash,
            "spender": spender,
            "tokenAddress": token_address,
            "tokenSymbol": token_symbol,
            "approveAmount": raw_amount,
            "readableAmount": readable_amount,
            "bridgeName": bridge_name,
        }));
        return Ok(());
    }

    // ── Mode 1 (default): check allowance ──────────────────────────
    if !skip_approve {
        let allowance_amount = selected_route["bridge"]["dexMultiTokenAllowanceOut"]["amount"]
            .as_str()
            .unwrap_or("0");
        let allowance_insufficient =
            crate::commands::swap::is_allowance_insufficient(allowance_amount, &raw_amount);

        if allowance_insufficient {
            let spender = resolve_spender(
                client,
                selected_route,
                &from_chain_index,
                &to_chain_index,
                &from_token,
                &to_token,
            )
            .await?;
            let token_address = selected_route["bridge"]["dexMultiTokenAllowanceOut"]
                ["tokenContractAddress"]
                .as_str()
                .unwrap_or(&from_token);
            let need_cancel = selected_route["bridge"]["dexMultiTokenAllowanceOut"]
                ["needCancelApproveToken"]
                .as_bool()
                .unwrap_or(false);

            output::success(json!({
                "action": "approve-required",
                "spender": spender,
                "tokenAddress": token_address,
                "tokenSymbol": token_symbol,
                "approveAmount": raw_amount,
                "readableAmount": readable_amount,
                "currentAllowance": allowance_amount,
                "bridgeName": bridge_name,
                "needCancelApprove": need_cancel,
            }));
            return Ok(());
        }
        // Allowance sufficient → fall through to trade
    }

    // ── Mode 3 / default (sufficient): execute trade ───────────────

    // CallData
    let mut quote_for_calldata = quote_result.clone();
    quote_for_calldata["pathSelectionRouterList"] = json!([selected_route]);

    let calldata_result = fetch_call_data(
        client,
        &quote_param,
        &quote_for_calldata,
        wallet,
        receive_wallet,
    )
    .await?;

    let calldata_type = calldata_result["calldataType"]
        .as_i64()
        .unwrap_or(100);
    let call_data = &calldata_result["callData"];
    let enable_mev = calldata_result["mevConfig"]["enableMev"]
        .as_bool()
        .unwrap_or(false);
    let effective_mev = mev_protection || enable_mev;

    // Order Save
    let order_id_val = fetch_order_save(
        client,
        &quote_param,
        &quote_for_calldata,
        wallet,
        receive_wallet,
    )
    .await?;
    let order_id = match &order_id_val {
        Value::Number(n) => n.to_string(),
        Value::String(s) => s.clone(),
        _ => bail!("unexpected orderId format from /order/save: {:?}", order_id_val),
    };

    // Sign & Broadcast by calldataType
    //
    // Backend guarantees callData.to is always the correct on-chain target:
    //   100 (contract call): to = router/bridge contract, data = calldata
    //   101 (native transfer): to = bridge receiver, value = amount, no data
    //   110 (ERC20 transfer): to = ERC20 contract, data = transfer calldata, value = 0
    let crosschain_tx_hash = match calldata_type {
        100 | 110 => {
            // Contract call or ERC20 transfer — both use to + data + value
            let to_addr = call_data["to"]
                .as_str()
                .ok_or_else(|| anyhow::anyhow!("missing callData.to"))?;
            let input_data = call_data["data"]
                .as_str()
                .ok_or_else(|| anyhow::anyhow!("missing callData.data"))?;
            let value_str = if let Some(v) = call_data["value"].as_i64() {
                v.to_string()
            } else {
                call_data["value"].as_str().unwrap_or("0").to_string()
            };
            let result = wallet_contract_call(
                to_addr,
                &from_chain_index,
                &value_str,
                Some(input_data),
                None,
                effective_mev,
                None,
                None,
            )
            .await?;
            extract_tx_hash(&result)?
        }
        101 => {
            // Native transfer — to + value, no data
            let to_addr = call_data["to"]
                .as_str()
                .ok_or_else(|| anyhow::anyhow!("missing callData.to for transfer"))?;
            let value_str = if let Some(v) = call_data["value"].as_i64() {
                v.to_string()
            } else {
                call_data["value"].as_str().unwrap_or("0").to_string()
            };
            let result = wallet_contract_call(
                to_addr,
                &from_chain_index,
                &value_str,
                None,
                None,
                effective_mev,
                None,
                None,
            )
            .await?;
            extract_tx_hash(&result)?
        }
        _ => bail!("unsupported calldataType: {}", calldata_type),
    };

    // Order Update
    if crosschain_tx_hash != "pending" {
        let _ = fetch_order_update(client, &order_id, &crosschain_tx_hash).await;
    }

    // Output action=execute
    output::success(json!({
        "action": "execute",
        "orderId": order_id,
        "crosschainTxHash": crosschain_tx_hash,
        "selectedRoute": bridge_name,
        "fromAmount": readable_amount,
        "estimatedReceiveAmount": receive_amount,
        "minimumReceived": minimum_received,
        "totalFee": total_fee,
        "estimatedTime": estimated_time,
        "calldataType": calldata_type,
    }));

    Ok(())
}

// ── Probe: try common tokens for bridgeability ────────────────────

async fn cmd_probe(
    client: &mut ApiClient,
    from_chain: &str,
    to_chain: &str,
    readable_amount: &str,
) -> Result<()> {
    let from_chain_index = crate::chains::resolve_chain(from_chain);
    let to_chain_index = crate::chains::resolve_chain(to_chain);

    // Candidate aliases to try — resolve_token_address handles per-chain mapping
    let candidates = ["usdc", "usdt", "native"];
    let mut results = Vec::new();
    let mut seen_pairs: Vec<(String, String)> = Vec::new();

    for candidate in &candidates {
        let from_token =
            crate::commands::swap::resolve_token_address(&from_chain_index, candidate);
        let to_token =
            crate::commands::swap::resolve_token_address(&to_chain_index, candidate);

        // Skip if token not mapped on either chain (resolve returns original string unchanged)
        if from_token == *candidate || to_token == *candidate {
            continue;
        }

        // Skip duplicate address pairs (e.g. native on both EVM chains = same 0xeee…)
        let pair = (from_token.clone(), to_token.clone());
        if seen_pairs.contains(&pair) {
            continue;
        }
        seen_pairs.push(pair);

        // Try quote — silently skip failures
        match fetch_quote(
            client,
            &from_chain_index,
            &to_chain_index,
            &from_token,
            &to_token,
            readable_amount,
            None,
            "0",
        )
        .await
        {
            Ok(quote) => {
                if let Some(routes) = quote["pathSelectionRouterList"].as_array() {
                    if !routes.is_empty() {
                        let best = &routes[0];
                        results.push(json!({
                            "token": candidate,
                            "fromTokenAddress": from_token,
                            "toTokenAddress": to_token,
                            "fromTokenSymbol": quote["commonDexInfo"]["fromToken"]["tokenSymbol"],
                            "toTokenSymbol": quote["commonDexInfo"]["toToken"]["tokenSymbol"],
                            "receiveAmount": best["receiveAmount"],
                            "minimumReceived": best["minimumReceived"],
                            "totalFee": best["totalFee"],
                            "estimatedTime": best["estimatedTime"],
                            "bridgeName": best["bridge"]["bridgeName"],
                            "routeCount": routes.len(),
                        }));
                    }
                }
            }
            Err(_) => continue,
        }
    }

    output::success(json!({
        "fromChain": from_chain_index,
        "toChain": to_chain_index,
        "readableAmount": readable_amount,
        "bridgeableTokens": results,
    }));
    Ok(())
}

// ── Balance pre-check ──────────────────────────────────────────────

/// Check that the wallet has sufficient balance of the source token on the source chain.
/// Returns Ok(()) if sufficient, or bails with a clear error message.
async fn check_balance(
    client: &mut ApiClient,
    wallet: &str,
    chain_index: &str,
    token_address: &str,
    token_alias: &str,
    readable_amount: &str,
) -> Result<()> {
    let is_native = token_address.is_empty()
        || token_address == "0xeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee";

    // Query balance via public API
    let balance_result = crate::commands::portfolio::fetch_token_balances(
        client,
        wallet,
        &format!("{}:{}", chain_index, if is_native { "" } else { token_address }),
        None,
    )
    .await;

    let balance_data = match balance_result {
        Ok(data) => data,
        Err(_) => return Ok(()), // If balance query fails, skip check and let downstream handle it
    };

    // Parse balance from response: data[].tokenAssets[].{ tokenContractAddress, balance, symbol }
    let mut found_balance = "0".to_string();
    let mut found_symbol = token_alias.to_uppercase();
    if let Some(groups) = balance_data.as_array() {
        'outer: for group in groups {
            if let Some(assets) = group["tokenAssets"].as_array() {
                for token in assets {
                    let token_addr = token["tokenContractAddress"]
                        .as_str()
                        .unwrap_or("");
                    let matches = if is_native {
                        token_addr.is_empty()
                            || token_addr == "0xeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee"
                    } else {
                        token_addr.eq_ignore_ascii_case(token_address)
                    };
                    if matches {
                        found_balance = token["balance"]
                            .as_str()
                            .unwrap_or("0")
                            .to_string();
                        if let Some(s) = token["symbol"].as_str() {
                            found_symbol = s.to_string();
                        }
                        break 'outer;
                    }
                }
            }
        }
    }
    let (available_balance, symbol) = (found_balance, found_symbol);

    // Compare as f64
    let available: f64 = available_balance.parse().unwrap_or(0.0);
    let required: f64 = readable_amount.parse().unwrap_or(0.0);

    if available < required {
        bail!(
            "insufficient {} balance on chain {}: available {}, required {}",
            symbol,
            chain_index,
            available_balance,
            readable_amount
        );
    }

    Ok(())
}
