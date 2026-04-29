//! Cross-chain swap command surface.
//!
//! Flow:
//!   1. /quote (with checkApprove + userWalletAddress) → routerList[]
//!   2. (if needApprove) /approve-tx → wallet contract-call
//!      ├─ if needCancelApprove (USDT pattern) → approve 0 first, then full
//!   3. /swap → wallet contract-call broadcast → fromTxHash
//!   4. /status by hash → SUCCESS / PENDING / NOT_FOUND

use anyhow::{bail, Result};
use clap::Subcommand;
use serde_json::{json, Value};

use super::Context;
use crate::client::ApiClient;
use crate::output;

// ── /api/v6/dex/cross-chain HTTP layer ─────────────────────────────────────
//
// GET-only endpoints. Each helper builds query params, calls `client.get()`,
// and returns the `data` payload (the array body returned by
// `ApiClient::handle_response`).

const V6_PREFIX: &str = "/api/v6/dex/cross-chain";

/// Fetch bridgeable token catalog.
///
/// Server contract (per the cross-chain OpenAPI doc, §4.1):
/// - Both omitted → full catalog (every from-token across every chain).
/// - `from_chain_index` only → all bridgeable from-tokens on that source chain.
/// - `to_chain_index` only → all from-tokens that can reach that destination.
/// - Both supplied → only from-tokens that route from `fromChain` → `toChain`.
pub async fn fetch_supported_tokens(
    client: &mut ApiClient,
    from_chain_index: Option<&str>,
    to_chain_index: Option<&str>,
) -> Result<Value> {
    let mut params: Vec<(&str, &str)> = Vec::new();
    if let Some(f) = from_chain_index {
        params.push(("fromChainIndex", f));
    }
    if let Some(t) = to_chain_index {
        params.push(("toChainIndex", t));
    }
    client
        .get(&format!("{V6_PREFIX}/supported/tokens"), &params)
        .await
}

/// Fetch bridge support set.
///
/// Server contract (per the cross-chain OpenAPI doc, §4.2):
/// - Both omitted → full catalog of every bridge.
/// - `from_chain_index` only → bridges on that source chain.
/// - `to_chain_index` only → bridges able to reach that destination.
/// - Both supplied → bridges that connect that specific chain pair.
pub async fn fetch_supported_bridges(
    client: &mut ApiClient,
    from_chain_index: Option<&str>,
    to_chain_index: Option<&str>,
) -> Result<Value> {
    let mut params: Vec<(&str, &str)> = Vec::new();
    if let Some(f) = from_chain_index {
        params.push(("fromChainIndex", f));
    }
    if let Some(t) = to_chain_index {
        params.push(("toChainIndex", t));
    }
    client
        .get(&format!("{V6_PREFIX}/supported/bridges"), &params)
        .await
}

#[allow(clippy::too_many_arguments)]
pub async fn fetch_quote(
    client: &mut ApiClient,
    from_chain: &str,
    to_chain: &str,
    from_token: &str,
    to_token: &str,
    raw_amount: &str,
    slippage: &str,
    wallet: Option<&str>,
    check_approve: bool,
    bridge_id: Option<&str>,
    sort: Option<&str>,
    allow_bridges: Option<&str>,
    deny_bridges: Option<&str>,
) -> Result<Value> {
    let mut params: Vec<(&str, &str)> = vec![
        ("fromChainIndex", from_chain),
        ("toChainIndex", to_chain),
        ("fromTokenAddress", from_token),
        ("toTokenAddress", to_token),
        ("amount", raw_amount),
        ("slippage", slippage),
    ];
    if let Some(w) = wallet {
        params.push(("userWalletAddress", w));
    }
    if check_approve {
        params.push(("checkApprove", "true"));
    }
    if let Some(b) = bridge_id {
        params.push(("bridgeId", b));
    }
    if let Some(s) = sort {
        params.push(("sort", s));
    }
    // allowBridge / denyBridge are Integer[] per spec — server expects repeated
    // query params, not comma-separated. Split and push each id separately.
    let allow_ids: Vec<&str> = allow_bridges
        .map(|s| s.split(',').map(str::trim).filter(|s| !s.is_empty()).collect())
        .unwrap_or_default();
    for id in &allow_ids {
        params.push(("allowBridge", id));
    }
    let deny_ids: Vec<&str> = deny_bridges
        .map(|s| s.split(',').map(str::trim).filter(|s| !s.is_empty()).collect())
        .unwrap_or_default();
    for id in &deny_ids {
        params.push(("denyBridge", id));
    }
    client.get(&format!("{V6_PREFIX}/quote"), &params).await
}

/// `approve_amount` is the raw on-chain amount (smallest token unit, e.g.
/// "300000" for 0.3 USDC at decimals=6). "0" revokes the existing allowance
/// (USDT pattern).
///
/// Note: an earlier doc claim that this endpoint accepts human-readable amounts
/// was wrong — server returns 51000 Params error for non-integer values.
pub async fn fetch_approve_tx(
    client: &mut ApiClient,
    chain_index: &str,
    token: &str,
    wallet: &str,
    bridge_id: &str,
    approve_amount: &str,
    check_allowance: bool,
) -> Result<Value> {
    let mut params: Vec<(&str, &str)> = vec![
        ("chainIndex", chain_index),
        ("tokenContractAddress", token),
        ("userWalletAddress", wallet),
        ("bridgeId", bridge_id),
        ("approveAmount", approve_amount),
    ];
    if check_allowance {
        params.push(("checkAllowance", "true"));
    }
    client.get(&format!("{V6_PREFIX}/approve-tx"), &params).await
}

#[allow(clippy::too_many_arguments)]
pub async fn fetch_swap(
    client: &mut ApiClient,
    from_chain: &str,
    to_chain: &str,
    from_token: &str,
    to_token: &str,
    raw_amount: &str,
    slippage: &str,
    wallet: &str,
    receive_address: Option<&str>,
    bridge_id: Option<&str>,
    sort: Option<&str>,
    allow_bridges: Option<&str>,
    deny_bridges: Option<&str>,
) -> Result<Value> {
    let mut params: Vec<(&str, &str)> = vec![
        ("fromChainIndex", from_chain),
        ("toChainIndex", to_chain),
        ("fromTokenAddress", from_token),
        ("toTokenAddress", to_token),
        ("amount", raw_amount),
        ("slippage", slippage),
        ("userWalletAddress", wallet),
    ];
    if let Some(r) = receive_address {
        params.push(("receiveAddress", r));
    }
    if let Some(b) = bridge_id {
        params.push(("bridgeId", b));
    }
    if let Some(s) = sort {
        params.push(("sort", s));
    }
    let allow_ids: Vec<&str> = allow_bridges
        .map(|s| s.split(',').map(str::trim).filter(|s| !s.is_empty()).collect())
        .unwrap_or_default();
    for id in &allow_ids {
        params.push(("allowBridge", id));
    }
    let deny_ids: Vec<&str> = deny_bridges
        .map(|s| s.split(',').map(str::trim).filter(|s| !s.is_empty()).collect())
        .unwrap_or_default();
    for id in &deny_ids {
        params.push(("denyBridge", id));
    }
    client.get(&format!("{V6_PREFIX}/swap"), &params).await
}

pub async fn fetch_status(
    client: &mut ApiClient,
    tx_hash: &str,
    chain_index: Option<&str>,
    bridge_id: Option<&str>,
) -> Result<Value> {
    let mut params: Vec<(&str, &str)> = vec![("hash", tx_hash)];
    if let Some(c) = chain_index {
        params.push(("chainIndex", c));
    }
    if let Some(b) = bridge_id {
        params.push(("bridgeId", b));
    }
    let resp = client.get(&format!("{V6_PREFIX}/status"), &params).await?;
    Ok(annotate_bridge_id_mismatch(resp, bridge_id))
}

/// Server-side `/status` has been observed to echo a different `bridgeId` than
/// the one requested (internal-id vs openApiCode mapping inconsistency on the
/// backend). When that happens, attach a `_warning` to each row so callers
/// know not to trust the echoed `bridgeId` — they should use the bridge name
/// from their own `quote` / `execute` record instead.
pub(crate) fn annotate_bridge_id_mismatch(mut resp: Value, requested: Option<&str>) -> Value {
    let Some(req) = requested else { return resp };
    let Some(arr) = resp.as_array_mut() else { return resp };
    for item in arr {
        let echoed = item
            .get("bridgeId")
            .and_then(|v| v.as_str().map(String::from).or_else(|| v.as_i64().map(|n| n.to_string())));
        if let Some(e) = echoed {
            if e != req {
                if let Some(obj) = item.as_object_mut() {
                    obj.insert(
                        "_warning".to_string(),
                        json!(format!(
                            "server-side bridgeId mismatch: requested {req}, response echoed {e}. Trust the bridgeName from your own quote/execute record."
                        )),
                    );
                }
            }
        }
    }
    resp
}

/// Resolve an `orderId` (e.g. `swapOrderId` / `approveOrderId` returned by
/// `cross-chain execute`) to the underlying source-chain `txHash` via the
/// authenticated wallet `/order/detail` endpoint. Login is required.
pub(crate) async fn resolve_order_id_to_tx_hash(
    order_id: &str,
    chain_index: &str,
) -> Result<String> {
    let access_token = super::agentic_wallet::auth::ensure_tokens_refreshed().await?;
    let wallets = crate::wallet_store::load_wallets()?
        .ok_or_else(|| anyhow::anyhow!(super::agentic_wallet::common::ERR_NOT_LOGGED_IN))?;
    if wallets.selected_account_id.is_empty() {
        bail!(super::agentic_wallet::common::ERR_NOT_LOGGED_IN);
    }
    let mut client = crate::wallet_api::WalletApiClient::new()?;
    let query: Vec<(&str, &str)> = vec![
        ("accountId", &wallets.selected_account_id),
        ("chainIndex", chain_index),
        ("orderId", order_id),
    ];
    let data = client
        .get_authed(
            "/priapi/v5/wallet/agentic/order/detail",
            &access_token,
            &query,
        )
        .await?;
    let tx_hash = data
        .as_array()
        .and_then(|a| a.first())
        .and_then(|item| item.get("txHash"))
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
        .ok_or_else(|| {
            anyhow::anyhow!(
                "order-id {} not found on chain {} (no txHash in /order/detail)",
                order_id,
                chain_index
            )
        })?
        .to_string();
    Ok(tx_hash)
}

// ── Public command surface ─────────────────────────────────────────────────

#[derive(Subcommand)]
pub enum CrossChainCommand {
    /// List bridge protocols. Either flag is independently optional:
    ///   - both omitted → full catalog
    ///   - --from-chain only → bridges on that source chain
    ///   - --to-chain only → bridges able to reach that destination
    ///   - both → bridges that connect this specific chain pair
    Bridges {
        #[arg(long)]
        from_chain: Option<String>,
        #[arg(long)]
        to_chain: Option<String>,
    },

    /// List bridgeable tokens. Either flag is independently optional:
    ///   - both omitted → full catalog
    ///   - --from-chain only → all bridgeable from-tokens on that chain
    ///   - --to-chain only → from-tokens that can reach that destination
    ///   - both → from-tokens that route from --from-chain to --to-chain
    Tokens {
        #[arg(long)]
        from_chain: Option<String>,
        #[arg(long)]
        to_chain: Option<String>,
    },

    /// Get cross-chain quote (read-only). routerList may contain multiple bridges.
    Quote {
        /// Source token address or alias
        #[arg(long)]
        from: String,
        /// Destination token address or alias
        #[arg(long)]
        to: String,
        /// Source chain (e.g. ethereum, arbitrum)
        #[arg(long)]
        from_chain: String,
        /// Destination chain (e.g. optimism, base)
        #[arg(long)]
        to_chain: String,
        /// Human-readable amount (e.g. "1.5"). CLI fetches token decimals.
        #[arg(long, conflicts_with = "amount")]
        readable_amount: Option<String>,
        /// Raw amount in minimal units. Mutually exclusive with --readable-amount.
        #[arg(long, conflicts_with = "readable_amount")]
        amount: Option<String>,
        /// Slippage tolerance (decimal, 0.002–0.5). Default 0.01 (1%).
        #[arg(long, default_value = "0.01")]
        slippage: String,
        /// User wallet address. Required when --check-approve is set.
        #[arg(long)]
        wallet: Option<String>,
        /// Have server compare on-chain allowance and fill routerList[].needApprove
        #[arg(long, default_value_t = false)]
        check_approve: bool,
        /// Pin a specific bridge id (openApiCode from quote / supported-bridges)
        #[arg(long)]
        bridge_id: Option<String>,
        /// Sort preference: 0=optimal (default), 1=fastest, 2=max output
        #[arg(long)]
        sort: Option<String>,
        /// Allowed bridges (comma-separated bridge ids)
        #[arg(long)]
        allow_bridges: Option<String>,
        /// Denied bridges (comma-separated bridge ids)
        #[arg(long)]
        deny_bridges: Option<String>,
        /// Optional destination receive address. The v6 quote API does not consume
        /// this field — the CLI accepts it for symmetry with `execute` and
        /// validates its family matches `--to-chain` so heterogeneous-pair
        /// mismatches (e.g. EVM addr → Solana) fail early rather than at execute.
        #[arg(long)]
        receive_address: Option<String>,
    },

    /// Build ERC-20 approve transaction for a bridge router (manual use)
    Approve {
        #[arg(long)]
        chain: String,
        #[arg(long)]
        token: String,
        #[arg(long)]
        wallet: String,
        #[arg(long)]
        bridge_id: String,
        /// Approve amount human-readable (e.g. "100" for 100 USDC, "0" to revoke).
        /// /approve-tx accepts human-readable values, not raw.
        #[arg(long)]
        readable_amount: String,
        /// Skip server allowance check (default: skip; pass --check-allowance to enable)
        #[arg(long, default_value_t = false)]
        check_allowance: bool,
    },

    /// Get unsigned cross-chain swap tx (calldata only, does NOT broadcast)
    Swap {
        #[arg(long)]
        from: String,
        #[arg(long)]
        to: String,
        #[arg(long)]
        from_chain: String,
        #[arg(long)]
        to_chain: String,
        #[arg(long, conflicts_with = "amount")]
        readable_amount: Option<String>,
        #[arg(long, conflicts_with = "readable_amount")]
        amount: Option<String>,
        #[arg(long, default_value = "0.01")]
        slippage: String,
        #[arg(long)]
        wallet: String,
        /// Receive address on destination chain (required for heterogeneous chain pairs)
        #[arg(long)]
        receive_address: Option<String>,
        /// Pin a specific bridge id
        #[arg(long)]
        bridge_id: Option<String>,
        /// Sort preference: 0=optimal (default), 1=fastest, 2=max output
        #[arg(long)]
        sort: Option<String>,
        /// Allowed bridges (comma-separated bridge ids)
        #[arg(long)]
        allow_bridges: Option<String>,
        /// Denied bridges (comma-separated bridge ids)
        #[arg(long)]
        deny_bridges: Option<String>,
    },

    /// Execute cross-chain. Three modes: default / --confirm-approve / --skip-approve.
    Execute {
        #[arg(long)]
        from: String,
        #[arg(long)]
        to: String,
        #[arg(long)]
        from_chain: String,
        #[arg(long)]
        to_chain: String,
        #[arg(long, conflicts_with = "amount")]
        readable_amount: Option<String>,
        #[arg(long, conflicts_with = "readable_amount")]
        amount: Option<String>,
        #[arg(long, default_value = "0.01")]
        slippage: String,
        #[arg(long)]
        wallet: String,
        #[arg(long)]
        receive_address: Option<String>,
        /// Pin a specific bridge id (from quote.routerList[].bridgeId).
        /// Mutually exclusive with --route-index.
        #[arg(long, conflicts_with = "route_index")]
        bridge_id: Option<String>,
        /// Pick a route by its zero-based index in quote.routerList[].
        /// Mutually exclusive with --bridge-id.
        #[arg(long, conflicts_with = "bridge_id")]
        route_index: Option<usize>,
        /// Sort preference: 0=optimal (default), 1=fastest, 2=max output
        #[arg(long)]
        sort: Option<String>,
        /// Allowed bridges (comma-separated bridge ids)
        #[arg(long)]
        allow_bridges: Option<String>,
        /// Denied bridges (comma-separated bridge ids)
        #[arg(long)]
        deny_bridges: Option<String>,
        /// Enable MEV protection on the swap broadcast (EVM)
        #[arg(long, default_value_t = false)]
        mev_protection: bool,
        /// Confirm and execute approve transaction (after user reviews quote)
        #[arg(long, default_value_t = false, conflicts_with = "skip_approve")]
        confirm_approve: bool,
        /// Skip allowance check, go straight to swap (use after approve confirmed)
        #[arg(long, default_value_t = false, conflicts_with = "confirm_approve")]
        skip_approve: bool,
        /// Force execution: skip backend risk warning 81362. Only after user confirms.
        #[arg(long, default_value_t = false)]
        force: bool,
    },

    /// Query cross-chain status. Provide either --tx-hash OR --order-id.
    Status {
        /// Source chain transaction hash. Use this OR --order-id (mutually exclusive).
        #[arg(
            long,
            required_unless_present = "order_id",
            conflicts_with = "order_id"
        )]
        tx_hash: Option<String>,
        /// Order id from a prior `cross-chain execute` (e.g. `swapOrderId`,
        /// `approveOrderId`). The CLI resolves it to the underlying tx hash via
        /// `wallet /order/detail` (login required). Use --tx-hash for anonymous
        /// queries.
        #[arg(
            long,
            required_unless_present = "tx_hash",
            conflicts_with = "tx_hash"
        )]
        order_id: Option<String>,
        /// Bridge id used for the swap. Required — server returns 50014 when absent.
        /// Get it from `bridgeId` of the prior `execute` / `quote.routerList[]` /
        /// `bridges` response.
        #[arg(long)]
        bridge_id: String,
        /// Source chain. Required — server returns 50014 (chainIndex) when absent.
        /// Accepts chain name or chainIndex.
        #[arg(long)]
        from_chain: String,
    },
}

// ── Dispatcher ─────────────────────────────────────────────────────────────

pub async fn execute(ctx: &Context, cmd: CrossChainCommand) -> Result<()> {
    let mut client = ctx.client_async().await?;
    match cmd {
        CrossChainCommand::Bridges {
            from_chain,
            to_chain,
        } => {
            let from_idx = from_chain
                .as_deref()
                .map(|c| crate::chains::resolve_chain(c).to_string());
            let to_idx = to_chain
                .as_deref()
                .map(|c| crate::chains::resolve_chain(c).to_string());
            output::success(
                fetch_supported_bridges(
                    &mut client,
                    from_idx.as_deref(),
                    to_idx.as_deref(),
                )
                .await?,
            );
        }

        CrossChainCommand::Tokens {
            from_chain,
            to_chain,
        } => {
            let from_idx = from_chain
                .as_deref()
                .map(|c| crate::chains::resolve_chain(c).to_string());
            let to_idx = to_chain
                .as_deref()
                .map(|c| crate::chains::resolve_chain(c).to_string());
            output::success(
                fetch_supported_tokens(
                    &mut client,
                    from_idx.as_deref(),
                    to_idx.as_deref(),
                )
                .await?,
            );
        }

        CrossChainCommand::Quote {
            from,
            to,
            from_chain,
            to_chain,
            readable_amount,
            amount,
            slippage,
            wallet,
            check_approve,
            bridge_id,
            sort,
            allow_bridges,
            deny_bridges,
            receive_address,
        } => {
            let from_idx = crate::chains::resolve_chain(&from_chain).to_string();
            let to_idx = crate::chains::resolve_chain(&to_chain).to_string();
            crate::chains::ensure_supported_chain(&from_idx, &from_chain)?;
            crate::chains::ensure_supported_chain(&to_idx, &to_chain)?;
            // Validate (but do not forward) receive_address — v6 /quote does not
            // accept it. Family mismatch fails up front instead of at execute.
            if let Some(addr) = receive_address.as_deref() {
                validate_receive_address(addr, &to_idx)?;
            }
            let from_token = crate::commands::swap::resolve_token_address(&from_idx, &from);
            let to_token = crate::commands::swap::resolve_token_address(&to_idx, &to);
            let raw_amount = crate::commands::swap::resolve_amount_arg(
                &mut client,
                amount.as_deref(),
                readable_amount.as_deref(),
                &from,
                &from_idx,
            )
            .await?;
            output::success(
                fetch_quote(
                    &mut client,
                    &from_idx,
                    &to_idx,
                    &from_token,
                    &to_token,
                    &raw_amount,
                    &slippage,
                    wallet.as_deref(),
                    check_approve,
                    bridge_id.as_deref(),
                    sort.as_deref(),
                    allow_bridges.as_deref(),
                    deny_bridges.as_deref(),
                )
                .await?,
            );
        }

        CrossChainCommand::Approve {
            chain,
            token,
            wallet,
            bridge_id,
            readable_amount,
            check_allowance,
        } => {
            let chain_idx = crate::chains::resolve_chain(&chain).to_string();
            crate::chains::ensure_supported_chain(&chain_idx, &chain)?;
            let resolved_token = crate::commands::swap::resolve_token_address(&chain_idx, &token);
            output::success(
                fetch_approve_tx(
                    &mut client,
                    &chain_idx,
                    &resolved_token,
                    &wallet,
                    &bridge_id,
                    &readable_amount,
                    check_allowance,
                )
                .await?,
            );
        }

        CrossChainCommand::Swap {
            from,
            to,
            from_chain,
            to_chain,
            readable_amount,
            amount,
            slippage,
            wallet,
            receive_address,
            bridge_id,
            sort,
            allow_bridges,
            deny_bridges,
        } => {
            let from_idx = crate::chains::resolve_chain(&from_chain).to_string();
            let to_idx = crate::chains::resolve_chain(&to_chain).to_string();
            crate::chains::ensure_supported_chain(&from_idx, &from_chain)?;
            crate::chains::ensure_supported_chain(&to_idx, &to_chain)?;
            let from_token = crate::commands::swap::resolve_token_address(&from_idx, &from);
            let to_token = crate::commands::swap::resolve_token_address(&to_idx, &to);
            if let Some(ref addr) = receive_address {
                validate_receive_address(addr, &to_idx)?;
            }
            let raw_amount = crate::commands::swap::resolve_amount_arg(
                &mut client,
                amount.as_deref(),
                readable_amount.as_deref(),
                &from,
                &from_idx,
            )
            .await?;
            output::success(
                fetch_swap(
                    &mut client,
                    &from_idx,
                    &to_idx,
                    &from_token,
                    &to_token,
                    &raw_amount,
                    &slippage,
                    &wallet,
                    receive_address.as_deref(),
                    bridge_id.as_deref(),
                    sort.as_deref(),
                    allow_bridges.as_deref(),
                    deny_bridges.as_deref(),
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
            amount,
            slippage,
            wallet,
            receive_address,
            bridge_id,
            route_index,
            sort,
            allow_bridges,
            deny_bridges,
            mev_protection,
            confirm_approve,
            skip_approve,
            force,
        } => {
            cmd_execute(
                &mut client,
                &from,
                &to,
                &from_chain,
                &to_chain,
                amount.as_deref(),
                readable_amount.as_deref(),
                &slippage,
                &wallet,
                receive_address.as_deref(),
                bridge_id.as_deref(),
                route_index,
                sort.as_deref(),
                allow_bridges.as_deref(),
                deny_bridges.as_deref(),
                mev_protection,
                confirm_approve,
                skip_approve,
                force,
            )
            .await?;
        }

        CrossChainCommand::Status {
            tx_hash,
            order_id,
            bridge_id,
            from_chain,
        } => {
            let chain_idx = crate::chains::resolve_chain(&from_chain).to_string();
            let resolved_tx_hash = match (tx_hash, order_id) {
                (Some(h), _) => h,
                (None, Some(oid)) => resolve_order_id_to_tx_hash(&oid, &chain_idx).await?,
                (None, None) => unreachable!("clap requires one of tx_hash / order_id"),
            };
            output::success(
                fetch_status(
                    &mut client,
                    &resolved_tx_hash,
                    Some(&chain_idx),
                    Some(&bridge_id),
                )
                .await?,
            );
        }
    }
    Ok(())
}

// ── Validation ─────────────────────────────────────────────────────────────

/// Check that `receive_address` matches the destination chain's address family.
pub(crate) fn validate_receive_address(receive_address: &str, to_chain_index: &str) -> Result<()> {
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

// ── Execute orchestration (4-step flow) ────────────────────────────────────

#[allow(clippy::too_many_arguments)]
async fn cmd_execute(
    client: &mut ApiClient,
    from: &str,
    to: &str,
    from_chain: &str,
    to_chain: &str,
    amount: Option<&str>,
    readable_amount: Option<&str>,
    slippage: &str,
    wallet: &str,
    receive_address: Option<&str>,
    bridge_id: Option<&str>,
    route_index: Option<usize>,
    sort: Option<&str>,
    allow_bridges: Option<&str>,
    deny_bridges: Option<&str>,
    mev_protection: bool,
    confirm_approve: bool,
    skip_approve: bool,
    force: bool,
) -> Result<()> {
    let from_idx = crate::chains::resolve_chain(from_chain).to_string();
    let to_idx = crate::chains::resolve_chain(to_chain).to_string();
    crate::chains::ensure_supported_chain(&from_idx, from_chain)?;
    crate::chains::ensure_supported_chain(&to_idx, to_chain)?;

    let from_token = crate::commands::swap::resolve_token_address(&from_idx, from);
    let to_token = crate::commands::swap::resolve_token_address(&to_idx, to);
    if let Some(addr) = receive_address {
        validate_receive_address(addr, &to_idx)?;
    }

    let raw_amount = crate::commands::swap::resolve_amount_arg(
        client,
        amount,
        readable_amount,
        from,
        &from_idx,
    )
    .await?;

    let family = crate::chains::chain_family(&from_idx);
    let native_addr = crate::chains::native_token_address(&from_idx);
    let is_from_native = from_token.eq_ignore_ascii_case(native_addr);

    // ── Step 1: Quote ──────────────────────────────────────────────────────
    let quote_data = fetch_quote(
        client,
        &from_idx,
        &to_idx,
        &from_token,
        &to_token,
        &raw_amount,
        slippage,
        Some(wallet),
        true, // checkApprove=true so server fills needApprove from on-chain allowance
        bridge_id,
        sort,
        allow_bridges,
        deny_bridges,
    )
    .await?;
    let quote_obj = unwrap_data_array(&quote_data);
    let router_list = quote_obj["routerList"]
        .as_array()
        .filter(|a| !a.is_empty())
        .ok_or_else(|| anyhow::anyhow!("/quote returned empty routerList — no available route"))?;
    let picked_index = route_index.unwrap_or(0);
    if picked_index >= router_list.len() {
        bail!(
            "--route-index {} out of bounds: routerList has {} entries",
            picked_index,
            router_list.len()
        );
    }
    let route = &router_list[picked_index];
    let resolved_bridge_id = extract_bridge_id(route)?;
    let need_approve = route["needApprove"].as_bool().unwrap_or(false);
    // USDT-pattern revoke flag. Defaults false when backend has not yet emitted it.
    let need_cancel_approve = route["needCancelApprove"].as_bool().unwrap_or(false);

    // ── Step 2: Approve branch ─────────────────────────────────────────────
    let mut approve_tx_hash: Option<String> = None;
    let mut approve_order_id: Option<String> = None;

    let needs_approve_branch = family == "evm" && !is_from_native && need_approve && !skip_approve;

    if needs_approve_branch && !confirm_approve {
        // Default mode: surface action=approve-required and stop.
        output::success(json!({
            "action": "approve-required",
            "tokenAddress": from_token,
            "tokenSymbol": route["fromToken"]["tokenSymbol"],
            "approveAmount": raw_amount,
            "readableAmount": readable_amount.unwrap_or(""),
            "bridgeId": resolved_bridge_id,
            "bridgeName": route["bridgeName"],
            "needCancelApprove": need_cancel_approve,
            "estimateTime": route["estimateTime"],
            "minimumReceived": route["minimumReceived"],
            "toTokenAmount": route["toTokenAmount"],
            "crossChainFee": route["crossChainFee"],
        }));
        return Ok(());
    }

    if needs_approve_branch && confirm_approve {
        // USDT pattern: revoke (approve 0) before approving full amount
        if need_cancel_approve {
            let revoke_data = fetch_approve_tx(
                client,
                &from_idx,
                &from_token,
                wallet,
                &resolved_bridge_id,
                "0",
                false,
            )
            .await?;
            let revoke_obj = unwrap_data_array(&revoke_data);
            if let Some(tx) = revoke_obj["tx"].as_object() {
                let revoke_calldata = tx
                    .get("data")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| anyhow::anyhow!("missing tx.data in revoke approve-tx"))?;
                let gas_limit = tx.get("gasLimit").and_then(|v| v.as_str());
                let result = wallet_contract_call(
                    &from_token,
                    &from_idx,
                    "0",
                    Some(revoke_calldata),
                    gas_limit,
                    false,
                    force,
                )
                .await?;
                // Don't surface revoke txHash separately — only the final approve matters.
                let _ = extract_tx_hash(&result)?;
            }
        }

        // Full approve (server returns the ready-built tx).
        // Server expects raw on-chain amount (smallest unit) — see
        // fetch_approve_tx doc.
        let approve_data = fetch_approve_tx(
            client,
            &from_idx,
            &from_token,
            wallet,
            &resolved_bridge_id,
            &raw_amount,
            false, // we already know we need it from quote.needApprove
        )
        .await?;
        let approve_obj = unwrap_data_array(&approve_data);
        let tx_obj = approve_obj["tx"]
            .as_object()
            .ok_or_else(|| anyhow::anyhow!("/approve-tx returned null tx — sanity check failed"))?;
        let approve_calldata = tx_obj
            .get("data")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow::anyhow!("missing tx.data in approve-tx response"))?;
        let approve_gas_limit = tx_obj.get("gasLimit").and_then(|v| v.as_str());
        let result = wallet_contract_call(
            &from_token,
            &from_idx,
            "0",
            Some(approve_calldata),
            approve_gas_limit,
            false,
            force,
        )
        .await?;
        let (tx_hash, order_id) = extract_tx_hash_and_order_id(&result)?;
        approve_tx_hash = Some(tx_hash);
        if !order_id.is_empty() {
            approve_order_id = Some(order_id);
        }

        // Mode: --confirm-approve only. Return approveTxHash, do NOT swap.
        let mut out = json!({
            "action": "approved",
            "approveTxHash": approve_tx_hash,
            "tokenAddress": from_token,
            "tokenSymbol": route["fromToken"]["tokenSymbol"],
            "approveAmount": raw_amount,
            "readableAmount": readable_amount.unwrap_or(""),
            "bridgeId": resolved_bridge_id,
            "bridgeName": route["bridgeName"],
        });
        if let Some(oid) = approve_order_id {
            out["approveOrderId"] = json!(oid);
        }
        output::success(out);
        return Ok(());
    }

    // ── Step 3: Swap ──────────────────────────────────────────────────────
    let swap_data = fetch_swap(
        client,
        &from_idx,
        &to_idx,
        &from_token,
        &to_token,
        &raw_amount,
        slippage,
        wallet,
        receive_address,
        Some(&resolved_bridge_id),
        sort,
        allow_bridges,
        deny_bridges,
    )
    .await?;
    let swap_obj = unwrap_data_array(&swap_data);
    let tx = &swap_obj["tx"];
    let tx_to = tx["to"]
        .as_str()
        .ok_or_else(|| anyhow::anyhow!("missing tx.to in swap response"))?;
    let tx_data = tx["data"]
        .as_str()
        .ok_or_else(|| anyhow::anyhow!("missing tx.data in swap response"))?;
    let tx_value = tx["value"].as_str().unwrap_or("0");
    let tx_gas_limit = tx["gasLimit"].as_str();

    let result = wallet_contract_call(
        tx_to,
        &from_idx,
        tx_value,
        Some(tx_data),
        tx_gas_limit,
        mev_protection,
        force,
    )
    .await?;
    let (swap_tx_hash, swap_order_id) = extract_tx_hash_and_order_id(&result)?;

    // ── Step 4: Output ────────────────────────────────────────────────────
    let router = &swap_obj["router"];
    let mut out = json!({
        "action": "execute",
        "fromTxHash": swap_tx_hash,
        "approveTxHash": approve_tx_hash,
        "selectedRoute": router["bridgeName"],
        "bridgeId": router["bridgeId"],
        "fromAmount": swap_obj["fromTokenAmount"],
        "toAmount": swap_obj["toTokenAmount"],
        "minimumReceived": swap_obj["minimumReceived"],
        "estimateTime": router["estimateTime"],
        "crossChainFee": router["crossChainFee"],
    });
    if let Some(oid) = approve_order_id {
        out["approveOrderId"] = json!(oid);
    }
    if !swap_order_id.is_empty() {
        out["swapOrderId"] = json!(swap_order_id);
    }
    output::success(out);
    Ok(())
}

// ── Helpers ────────────────────────────────────────────────────────────────

/// If the API returns an array, take the first element; otherwise return as-is.
fn unwrap_data_array(data: &Value) -> Value {
    if data.is_array() {
        data.as_array()
            .and_then(|a| a.first())
            .cloned()
            .unwrap_or(Value::Null)
    } else {
        data.clone()
    }
}

/// `routerList[].bridgeId` is documented as Integer but tolerate String form too.
fn extract_bridge_id(route: &Value) -> Result<String> {
    if let Some(i) = route["bridgeId"].as_i64() {
        return Ok(i.to_string());
    }
    if let Some(s) = route["bridgeId"].as_str() {
        return Ok(s.to_string());
    }
    Err(anyhow::anyhow!(
        "quote.routerList[0].bridgeId missing or wrong type"
    ))
}

// ── Helper: build approve calldata ──────────────────────────────────

/// Construct ERC20 approve(spender, amount) calldata hex.
/// Handles uint256 range via string-based hex conversion (u128 overflows for MaxUint256).
fn build_approve_calldata(spender: &str, amount_raw: &str) -> String {
    let spender_clean = spender.trim_start_matches("0x").to_lowercase();
    let amount_hex = decimal_to_hex64(amount_raw);
    format!("0x095ea7b3{:0>64}{}", spender_clean, amount_hex)
}

/// Convert a decimal string to a zero-padded 64-char hex string.
/// Supports full uint256 range by iterating digit-by-digit.
fn decimal_to_hex64(decimal: &str) -> String {
    if decimal == "0" {
        return "0".repeat(64);
    }
    // Try u128 first (covers most cases)
    if let Ok(v) = decimal.parse::<u128>() {
        return format!("{:0>64x}", v);
    }
    // Fallback: manual base conversion for values > u128::MAX
    let mut bytes = [0u8; 32]; // 256 bits
    let mut dec_digits: Vec<u8> = decimal.bytes().map(|b| b - b'0').collect();
    let mut bit_pos = 0;
    while !dec_digits.is_empty() && bit_pos < 256 {
        let remainder = div_decimal_by_2(&mut dec_digits);
        if remainder == 1 {
            bytes[31 - bit_pos / 8] |= 1 << (bit_pos % 8);
        }
        bit_pos += 1;
        // Remove leading zeros
        while dec_digits.first() == Some(&0) && dec_digits.len() > 1 {
            dec_digits.remove(0);
        }
        if dec_digits == [0] {
            break;
        }
    }
    bytes.iter().map(|b| format!("{:02x}", b)).collect()
}

/// Divide a decimal digit array by 2, return remainder (0 or 1).
fn div_decimal_by_2(digits: &mut [u8]) -> u8 {
    let mut carry = 0u8;
    for d in digits.iter_mut() {
        let val = carry * 10 + *d;
        *d = val / 2;
        carry = val % 2;
    }
    carry
}

// ── Helper: wallet contract-call wrapper ────────────────────────────

#[allow(clippy::too_many_arguments)]
async fn wallet_contract_call(
    to: &str,
    chain: &str,
    amt: &str,
    input_data: Option<&str>,
    gas_limit: Option<&str>,
    mev_protection: bool,
    force: bool,
) -> Result<Value> {
    let resp = crate::commands::agentic_wallet::transfer::execute_contract_call(
        to,
        chain,
        amt,
        input_data,
        None, // unsigned_tx (Solana path; cross-chain currently EVM-source)
        gas_limit,
        None, // from: use selected account
        None, // aa_dex_token_addr
        None, // aa_dex_token_amount
        mev_protection,
        None, // jito_unsigned_tx
        force,
        Some("3"), // tx_source: cross-chain bridge
        None,      // gas_token_address
        None,      // relayer_id
        false,     // enable_gas_station
        Some("cross-chain"), // agent_biz_type
        None,      // agent_skill_name
    )
    .await?;
    Ok(json!({ "txHash": resp.tx_hash, "orderId": resp.order_id }))
}

fn extract_tx_hash(data: &Value) -> Result<String> {
    data["txHash"]
        .as_str()
        .map(|s| s.to_string())
        .ok_or_else(|| anyhow::anyhow!("missing txHash in contract-call output"))
}

fn extract_tx_hash_and_order_id(data: &Value) -> Result<(String, String)> {
    let tx_hash = extract_tx_hash(data)?;
    let order_id = data["orderId"].as_str().unwrap_or("").to_string();
    Ok((tx_hash, order_id))
}

// ── Tests ──────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    // ── annotate_bridge_id_mismatch ─────────────────────────────────────────

    #[test]
    fn bridge_id_mismatch_warning_added_when_echoed_differs() {
        let resp = json!([{
            "bridgeId": 52,
            "status": "PENDING",
            "txHash": "0xabc"
        }]);
        let out = annotate_bridge_id_mismatch(resp, Some("636"));
        let arr = out.as_array().unwrap();
        let warn = arr[0].get("_warning").and_then(|v| v.as_str()).unwrap();
        assert!(warn.contains("requested 636"));
        assert!(warn.contains("echoed 52"));
    }

    #[test]
    fn bridge_id_mismatch_no_warning_when_match() {
        let resp = json!([{
            "bridgeId": 636,
            "status": "PENDING",
            "txHash": "0xabc"
        }]);
        let out = annotate_bridge_id_mismatch(resp, Some("636"));
        assert!(out.as_array().unwrap()[0].get("_warning").is_none());
    }

    #[test]
    fn bridge_id_mismatch_match_string_form() {
        let resp = json!([{ "bridgeId": "636", "status": "PENDING" }]);
        let out = annotate_bridge_id_mismatch(resp, Some("636"));
        assert!(out.as_array().unwrap()[0].get("_warning").is_none());
    }

    #[test]
    fn bridge_id_mismatch_no_op_when_request_absent() {
        let resp = json!([{ "bridgeId": 52, "status": "PENDING" }]);
        let out = annotate_bridge_id_mismatch(resp.clone(), None);
        assert_eq!(out, resp);
    }

    #[test]
    fn bridge_id_mismatch_no_op_when_response_missing_bridge_id() {
        let resp = json!([{ "status": "NOT_FOUND" }]);
        let out = annotate_bridge_id_mismatch(resp.clone(), Some("636"));
        assert_eq!(out, resp);
    }

    #[test]
    fn evm_addr_to_solana_rejected() {
        assert!(validate_receive_address(
            "0x896f4edd6601eda7d12f077a35e1cdf2898282ce",
            "501"
        )
        .is_err());
    }

    #[test]
    fn solana_addr_to_evm_rejected() {
        assert!(validate_receive_address(
            "5EDUCQDeVmaGohSAJYQ8mwe4hZMXgDzS4X2Si3Zh3cL5",
            "8453"
        )
        .is_err());
    }

    #[test]
    fn solana_addr_to_solana_ok() {
        assert!(
            validate_receive_address("5EDUCQDeVmaGohSAJYQ8mwe4hZMXgDzS4X2Si3Zh3cL5", "501").is_ok()
        );
    }

    #[test]
    fn evm_addr_to_evm_ok() {
        assert!(validate_receive_address(
            "0x896f4edd6601eda7d12f077a35e1cdf2898282ce",
            "1"
        )
        .is_ok());
    }

    #[test]
    fn extract_bridge_id_integer() {
        let v = json!({"bridgeId": 636});
        assert_eq!(extract_bridge_id(&v).unwrap(), "636");
    }

    #[test]
    fn extract_bridge_id_string() {
        let v = json!({"bridgeId": "636"});
        assert_eq!(extract_bridge_id(&v).unwrap(), "636");
    }

    #[test]
    fn extract_bridge_id_missing() {
        let v = json!({});
        assert!(extract_bridge_id(&v).is_err());
    }

    #[test]
    fn unwrap_data_array_picks_first() {
        let v = json!([{"a": 1}, {"a": 2}]);
        assert_eq!(unwrap_data_array(&v), json!({"a": 1}));
    }

    #[test]
    fn unwrap_data_array_passthrough_object() {
        let v = json!({"a": 1});
        assert_eq!(unwrap_data_array(&v), json!({"a": 1}));
    }

    #[test]
    fn unwrap_data_array_empty_returns_null() {
        let v = json!([]);
        assert_eq!(unwrap_data_array(&v), Value::Null);
    }
}
