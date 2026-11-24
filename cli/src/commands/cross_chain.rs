//! Cross-chain swap command surface.
//!
//! Flow:
//!   1. /quote (with checkApprove + userWalletAddress, + receiveAddress for
//!      heterogeneous EVM↔non-EVM pairs) → routerList[]
//!   2. (if needApprove) /approve-tx → wallet contract-call
//!      ├─ if needCancelApprove (USDT pattern) → approve 0 first, then full
//!   3. /swap → wallet contract-call broadcast → fromTxHash
//!   4. /status by hash → SUCCESS / PENDING / NOT_FOUND

use std::collections::HashSet;

use anyhow::{bail, Result};
use clap::Subcommand;
use serde_json::{json, Value};

use super::Context;
use crate::client::ApiClient;
use crate::commands::common::wait_tx_onchain;
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
    receive_address: Option<&str>,
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
    if let Some(r) = receive_address {
        params.push(("receiveAddress", r));
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

// ── No-direct-route transit fallback ───────────────────────────────────────
//
// When `/quote` reports no route (bails 82000/82104, or returns an empty
// routerList), the CLI silently probes common transit assets — intersected
// with the server's bridgeable set for the pair — and returns a structured
// `fallback` object so the agent never runs the discovery loop itself.
// `outcome` is one of: transit_available | no_path | env_unavailable.

/// Transit assets probed in preference order (resolved on the source chain,
/// then intersected with the bridgeable set). Native is appended separately.
const TRANSIT_PREFERENCE: &[&str] = &["usdc", "usdt", "dai"];

#[derive(Clone)]
struct TransitProbe {
    /// Display label (e.g. "USDC", "NATIVE").
    symbol: String,
    /// Call-ready source-chain address (as resolved; EVM already lowercase).
    /// Used as leg-2's `fromTokenAddress` and for the bridgeable intersection.
    address: String,
    /// Call-ready destination-chain address for the SAME asset. Stablecoins
    /// have different contracts per chain, so leg-2's `toTokenAddress` MUST use
    /// this — not `address`, which only resolves on the source chain.
    dest_address: String,
}

/// Parse an `API error (code=NNNNN): msg` envelope into `(code, msg)`.
/// Format is produced by `client.rs::unwrap_envelope`; keep in sync.
fn parse_api_error(s: &str) -> Option<(String, String)> {
    let code_at = s.find("code=")? + "code=".len();
    let rest = &s[code_at..];
    let close = rest.find(')')?;
    let code = rest[..close].trim().to_string();
    let msg = rest[close + 1..].trim_start_matches(':').trim().to_string();
    Some((code, msg))
}

/// Best human message from an API error (envelope `msg`, else the raw text).
fn api_error_msg(e: &anyhow::Error) -> String {
    let s = e.to_string();
    parse_api_error(&s).map(|(_, m)| m).unwrap_or(s)
}

/// True when a quote Result means "no available route" (vs a hard error that
/// should propagate): empty routerList (code=0), or a bail with 82000 / 82104.
fn is_no_route(res: &Result<Value>) -> bool {
    match res {
        Ok(data) => unwrap_data_array(data)["routerList"]
            .as_array()
            .map(|a| a.is_empty())
            .unwrap_or(true),
        Err(e) => parse_api_error(&e.to_string())
            .map(|(c, _)| c == "82000" || c == "82104")
            .unwrap_or(false),
    }
}

/// Lowercased source-chain token addresses from a `supported/tokens` payload.
fn bridgeable_source_addresses(tokens: &Value, from_idx: &str) -> HashSet<String> {
    tokens
        .as_array()
        .map(|arr| {
            arr.iter()
                .filter(|t| t["chainIndex"].as_str() == Some(from_idx))
                .filter_map(|t| t["tokenContractAddress"].as_str())
                .map(|a| a.to_lowercase())
                .collect()
        })
        .unwrap_or_default()
}

/// Build the transit candidate set: preference stables + native, deduped, and
/// (when known) intersected with `bridgeable`. Each candidate is resolved on
/// BOTH the source (`from_idx`) and destination (`to_idx`) chains; a stable is
/// kept only when it resolves on both, since leg-2 needs the dest-chain address
/// for `toTokenAddress` (stablecoin contracts differ per chain).
fn build_transit_candidates(
    from_idx: &str,
    to_idx: &str,
    bridgeable: &HashSet<String>,
) -> Vec<TransitProbe> {
    let mut raw: Vec<TransitProbe> = TRANSIT_PREFERENCE
        .iter()
        .filter_map(|sym| {
            let src = crate::token_alias::resolve_and_validate(from_idx, sym, "transit").ok()?;
            let dest = crate::token_alias::resolve_and_validate(to_idx, sym, "transit").ok()?;
            Some(TransitProbe {
                symbol: sym.to_uppercase(),
                address: src,
                dest_address: dest,
            })
        })
        .collect();
    raw.push(TransitProbe {
        symbol: "NATIVE".to_string(),
        address: crate::chains::native_token_address(from_idx).to_string(),
        dest_address: crate::chains::native_token_address(to_idx).to_string(),
    });

    let mut seen: HashSet<String> = HashSet::new();
    raw.into_iter()
        .filter(|c| seen.insert(c.address.to_lowercase()))
        .filter(|c| bridgeable.is_empty() || bridgeable.contains(&c.address.to_lowercase()))
        .collect()
}

/// Build one display-ready transit option from a successful transit bridge
/// quote (amounts stay raw + carry decimals so the agent formats them exactly
/// like the main quote table).
fn build_transit_option(symbol: &str, bridge_quote: &Value) -> Option<Value> {
    let obj = unwrap_data_array(bridge_quote);
    let route = obj["routerList"].as_array()?.first()?;
    Some(json!({
        "transitToken": symbol,
        "bridgeName": route["bridgeName"],
        "bridgeId": route["bridgeId"],
        "toTokenAmount": route["toTokenAmount"],
        "minimumReceived": route["minimumReceived"],
        "crossChainFee": route["crossChainFee"],
        "crossChainFeeTokenAddress": route["crossChainFeeTokenAddress"],
        "otherNativeFee": route["otherNativeFee"],
        "estimateTime": route["estimateTime"],
        "toTokenDecimals": obj["toToken"]["decimals"],
    }))
}

/// relay / mayan / butterswap bundle a from-swap leg into the bridge tx, which
/// is MEV-exposed — these always require MEV protection regardless of trade
/// size. Other bridges fall back to the caller's flag / size threshold.
fn bridge_forces_mev(bridge_name: &str) -> bool {
    let n = bridge_name.to_lowercase();
    n.contains("relay") || n.contains("mayan") || n.contains("butterswap")
}

/// Raw (base-unit) balance for `address` from an `all-token-balances`
/// `tokenAssets[]` payload — case-insensitive on the contract address; "0"
/// when the token is absent.
fn raw_balance_for<'a>(assets: &'a Value, address: &str) -> &'a str {
    assets
        .as_array()
        .into_iter()
        .flatten()
        .find(|a| {
            a["tokenContractAddress"]
                .as_str()
                .map(|s| s.eq_ignore_ascii_case(address))
                .unwrap_or(false)
        })
        .and_then(|a| a["rawBalance"].as_str())
        .unwrap_or("0")
}

/// Pre-quote balance gate for `execute`: halts before any quote / broadcast
/// when the wallet can't cover the trade (or has no gas). Degrades gracefully —
/// a failed or unexpectedly-shaped balance lookup returns None (proceed; the
/// broadcast-time check still guards). Returns Some((block_code, message)) when
/// the trade must be halted.
async fn execute_balance_block(
    client: &mut ApiClient,
    from_idx: &str,
    wallet: &str,
    from_token: &str,
    native_addr: &str,
    raw_amount: &str,
    is_from_native: bool,
) -> Option<(&'static str, String)> {
    let balances =
        crate::commands::portfolio::fetch_all_balances(client, wallet, from_idx, None, None)
            .await
            .ok()?;
    let unwrapped = unwrap_data_array(&balances);
    if !unwrapped["tokenAssets"].is_array() {
        return None; // unexpected shape → don't block
    }
    let assets = unwrapped["tokenAssets"].clone();

    let token_raw = raw_balance_for(&assets, from_token);
    // Borrowed as a base-unit `balance < amount` comparison.
    if crate::commands::swap::is_allowance_insufficient(token_raw, raw_amount) {
        return Some((
            "insufficient_balance",
            "Source token balance is less than the amount you want to bridge.".to_string(),
        ));
    }

    if !is_from_native {
        let mut native_raw = raw_balance_for(&assets, native_addr);
        if native_raw == "0" {
            native_raw = raw_balance_for(&assets, "");
        }
        if native_raw == "0" || native_raw.is_empty() {
            return Some((
                "insufficient_gas",
                "Source-chain native (gas) balance is zero — deposit native token for gas before bridging."
                    .to_string(),
            ));
        }
    }
    None
}

/// Classify a dead end (no transit succeeded) from the collected error msgs:
/// all msgs empty/"unknown error" → env_unavailable; otherwise → no_path.
fn classify_dead_end(errors: &[String]) -> (&'static str, String) {
    let informative: Vec<&String> = errors
        .iter()
        .filter(|m| {
            let t = m.trim();
            !t.is_empty() && t != "unknown error"
        })
        .collect();
    if informative.is_empty() {
        (
            "env_unavailable",
            "Bridge service appears unavailable for this chain pair on this environment — \
the pair is in the routing config but quote returns no reason across the direct route and \
every transit token. Typically a server-side / adapter issue, not your token or amount. \
Retry later or escalate to OKX support."
                .to_string(),
        )
    } else {
        ("no_path", informative[0].clone())
    }
}

/// Probe one transit asset: leg-1 source→transit swap (skipped when the source
/// token already IS the transit), then leg-2 transit cross-chain bridge quote.
async fn probe_transit(
    client: &mut ApiClient,
    from_idx: &str,
    to_idx: &str,
    from_token: &str,
    cand: &TransitProbe,
    raw_amount: &str,
    slippage: &str,
) -> std::result::Result<Value, String> {
    let transit_amount = if from_token.eq_ignore_ascii_case(&cand.address) {
        raw_amount.to_string()
    } else {
        let swap_q = crate::commands::swap::fetch_quote(
            client, from_idx, from_token, &cand.address, raw_amount, "",
        )
        .await
        .map_err(|e| api_error_msg(&e))?;
        unwrap_data_array(&swap_q)["toTokenAmount"]
            .as_str()
            .map(String::from)
            .ok_or_else(|| "source→transit swap returned no amount".to_string())?
    };
    let bridge_q = fetch_quote(
        client, from_idx, to_idx, &cand.address, &cand.dest_address, &transit_amount, slippage,
        None, false, None, None, None, None, None,
    )
    .await
    .map_err(|e| api_error_msg(&e))?;
    build_transit_option(&cand.symbol, &bridge_q)
        .ok_or_else(|| "transit bridge quote returned no route".to_string())
}

/// Discover an indirect path when no direct route exists. Never errors —
/// failures collapse to a no_path / env_unavailable outcome.
async fn discover_transit_fallback(
    client: &mut ApiClient,
    from_idx: &str,
    to_idx: &str,
    from_token: &str,
    raw_amount: &str,
    slippage: &str,
) -> Value {
    let bridgeable = fetch_supported_tokens(client, Some(from_idx), Some(to_idx))
        .await
        .ok()
        .map(|v| bridgeable_source_addresses(&v, from_idx))
        .unwrap_or_default();

    let candidates = build_transit_candidates(from_idx, to_idx, &bridgeable);
    if candidates.is_empty() {
        return json!({
            "outcome": "no_path",
            "transitOptions": [],
            "message": "No common transit token (USDC / USDT / DAI / native) is bridgeable from this source chain.",
        });
    }

    // Probe candidates SEQUENTIALLY (mirrors the original transit-discovery
    // loop). The backend rate-limits the quote endpoint, so a concurrent burst
    // risks throttling; this is a rare fallback path where the extra latency is
    // an acceptable trade for staying within limits.
    let mut options = Vec::new();
    let mut errors = Vec::new();
    for cand in candidates {
        match probe_transit(client, from_idx, to_idx, from_token, &cand, raw_amount, slippage).await
        {
            Ok(opt) => options.push(opt),
            Err(msg) => errors.push(msg),
        }
    }

    if options.is_empty() {
        let (outcome, message) = classify_dead_end(&errors);
        json!({ "outcome": outcome, "transitOptions": [], "message": message })
    } else {
        json!({ "outcome": "transit_available", "transitOptions": options })
    }
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
        /// Source chain filter (name or chainIndex). Optional — see the combinations above.
        #[arg(long)]
        from_chain: Option<String>,
        /// Destination chain filter (name or chainIndex). Optional — see the combinations above.
        #[arg(long)]
        to_chain: Option<String>,
    },

    /// List bridgeable tokens. Either flag is independently optional:
    ///   - both omitted → full catalog
    ///   - --from-chain only → all bridgeable from-tokens on that chain
    ///   - --to-chain only → from-tokens that can reach that destination
    ///   - both → from-tokens that route from --from-chain to --to-chain
    Tokens {
        /// Source chain filter (name or chainIndex). Optional — see the combinations above.
        #[arg(long)]
        from_chain: Option<String>,
        /// Destination chain filter (name or chainIndex). Optional — see the combinations above.
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
        /// Slippage tolerance as **decimal**, range (0, 1] (e.g. 0.01 = 1%, 0.5 = 50%). Default "0.01".
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
        /// Sort preference: 0=optimal, 1=fastest, 2=max output. Omit to let BE pick (BE-default = 0).
        #[arg(long, value_parser = clap::builder::PossibleValuesParser::new(["0", "1", "2"]))]
        sort: Option<String>,
        /// Allowed bridges (comma-separated bridge ids)
        #[arg(long)]
        allow_bridges: Option<String>,
        /// Denied bridges (comma-separated bridge ids)
        #[arg(long)]
        deny_bridges: Option<String>,
        /// Destination receive address. Optional from the CLI; the server
        /// requires it for heterogeneous (EVM ⇌ non-EVM) bridges and returns
        /// 82202 when missing. When supplied, family must match `--to-chain`.
        #[arg(long)]
        receive_address: Option<String>,
    },

    /// Build ERC-20 approve transaction for a bridge router (manual use).
    ///
    /// Exactly one of `--amount` (raw integer, smallest token unit) or
    /// `--readable-amount` (human-readable decimal, CLI fetches token
    /// decimals and converts) is required. They are mutually exclusive.
    Approve {
        /// Source chain (name or chainIndex).
        #[arg(long)]
        chain: String,
        /// Token contract address to approve.
        #[arg(long)]
        token: String,
        /// User wallet address (token owner granting the allowance).
        #[arg(long)]
        wallet: String,
        /// Bridge id (openApiCode) from `bridges` or `quote.routerList[]` — the router that receives the allowance.
        #[arg(long)]
        bridge_id: String,
        /// Approve amount in **smallest token unit** (raw integer, e.g. "500000"
        /// for 0.5 USDC at 6 decimals). Pass "0" to revoke (USDT pattern).
        /// Mutually exclusive with --readable-amount.
        #[arg(long, conflicts_with = "readable_amount")]
        amount: Option<String>,
        /// Approve amount in **human-readable form** (e.g. "0.5" for 0.5 USDC).
        /// CLI fetches token decimals via token-info and converts to raw
        /// minimal units before broadcast. Pass "0" / "0.0" to revoke
        /// (equivalent to `--amount 0`). Mutually exclusive with --amount.
        #[arg(long, conflicts_with = "amount")]
        readable_amount: Option<String>,
        /// Skip server allowance check (default: skip; pass --check-allowance to enable)
        #[arg(long, default_value_t = false)]
        check_allowance: bool,
    },

    /// Get unsigned cross-chain swap tx (calldata only, does NOT broadcast)
    Swap {
        /// Source token address or alias.
        #[arg(long)]
        from: String,
        /// Destination token address or alias.
        #[arg(long)]
        to: String,
        /// Source chain (e.g. ethereum, arbitrum).
        #[arg(long)]
        from_chain: String,
        /// Destination chain (e.g. optimism, base).
        #[arg(long)]
        to_chain: String,
        /// Human-readable amount (e.g. "1.5"). CLI fetches token decimals. Mutually exclusive with --amount.
        #[arg(long, conflicts_with = "amount")]
        readable_amount: Option<String>,
        /// Raw amount in minimal units. Mutually exclusive with --readable-amount.
        #[arg(long, conflicts_with = "readable_amount")]
        amount: Option<String>,
        /// Slippage tolerance as **decimal**, range (0, 1] (e.g. 0.01 = 1%, 0.5 = 50%). Default "0.01".
        #[arg(long, default_value = "0.01")]
        slippage: String,
        /// User wallet address (sender).
        #[arg(long)]
        wallet: String,
        /// Receive address on destination chain (required for heterogeneous chain pairs)
        #[arg(long)]
        receive_address: Option<String>,
        /// Pin a specific bridge id
        #[arg(long)]
        bridge_id: Option<String>,
        /// Sort preference: 0=optimal, 1=fastest, 2=max output. Omit to let BE pick (BE-default = 0).
        #[arg(long, value_parser = clap::builder::PossibleValuesParser::new(["0", "1", "2"]))]
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
        /// Source token address or alias.
        #[arg(long)]
        from: String,
        /// Destination token address or alias.
        #[arg(long)]
        to: String,
        /// Source chain (e.g. ethereum, arbitrum).
        #[arg(long)]
        from_chain: String,
        /// Destination chain (e.g. optimism, base).
        #[arg(long)]
        to_chain: String,
        /// Human-readable amount (e.g. "1.5"). CLI fetches token decimals. Mutually exclusive with --amount.
        #[arg(long, conflicts_with = "amount")]
        readable_amount: Option<String>,
        /// Raw amount in minimal units. Mutually exclusive with --readable-amount.
        #[arg(long, conflicts_with = "readable_amount")]
        amount: Option<String>,
        /// Slippage tolerance as **decimal**, range (0, 1] (e.g. 0.01 = 1%, 0.5 = 50%). Default "0.01".
        #[arg(long, default_value = "0.01")]
        slippage: String,
        /// User wallet address (sender + signer).
        #[arg(long)]
        wallet: String,
        /// Destination receive address. Server requires it for heterogeneous (EVM ⇌ non-EVM) bridges (else 82202); family must match --to-chain. A non-sender address needs explicit user confirmation.
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
        /// Sort preference: 0=optimal, 1=fastest, 2=max output. Omit to let BE pick (BE-default = 0).
        #[arg(long, value_parser = clap::builder::PossibleValuesParser::new(["0", "1", "2"]))]
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
            if let Some(addr) = receive_address.as_deref() {
                validate_receive_address(addr, &to_idx)?;
            }
            let from_token = crate::token_alias::resolve_and_validate(&from_idx, &from, "from")?;
            let to_token = crate::token_alias::resolve_and_validate(&to_idx, &to, "to")?;
            crate::validators::validate_slippage_zero_to_one(&slippage)?;
            let raw_amount = crate::commands::swap::resolve_amount_arg(
                &mut client,
                amount.as_deref(),
                readable_amount.as_deref(),
                &from,
                &from_idx,
            )
            .await?;
            let quote_res = fetch_quote(
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
                receive_address.as_deref(),
            )
            .await;
            if is_no_route(&quote_res) {
                let fallback = discover_transit_fallback(
                    &mut client,
                    &from_idx,
                    &to_idx,
                    &from_token,
                    &raw_amount,
                    &slippage,
                )
                .await;
                // Wrap in an array to match the happy-path `data` shape
                // (`[{ routerList, ... }]`) so `data[0].routerList` stays valid
                // (empty) for scripts instead of becoming an object.
                output::success(json!([{ "routerList": [], "fallback": fallback }]));
            } else {
                output::success(quote_res?);
            }
        }

        CrossChainCommand::Approve {
            chain,
            token,
            wallet,
            bridge_id,
            amount,
            readable_amount,
            check_allowance,
        } => {
            let chain_idx = crate::chains::resolve_chain(&chain).to_string();
            crate::chains::ensure_supported_chain(&chain_idx, &chain)?;
            let resolved_token =
                crate::token_alias::resolve_and_validate(&chain_idx, &token, "token")?;
            let raw_amount = resolve_approve_amount(
                &mut client,
                amount.as_deref(),
                readable_amount.as_deref(),
                &resolved_token,
                &chain_idx,
            )
            .await?;
            output::success(
                fetch_approve_tx(
                    &mut client,
                    &chain_idx,
                    &resolved_token,
                    &wallet,
                    &bridge_id,
                    &raw_amount,
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
            let from_token = crate::token_alias::resolve_and_validate(&from_idx, &from, "from")?;
            let to_token = crate::token_alias::resolve_and_validate(&to_idx, &to, "to")?;
            crate::token_alias::validate_address_for_chain(&from_idx, &wallet, "wallet")?;
            if let Some(ref addr) = receive_address {
                validate_receive_address(addr, &to_idx)?;
            }
            crate::validators::validate_slippage_zero_to_one(&slippage)?;
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
/// `--receive-address` itself is optional from the CLI; per the v6 OpenAPI spec
/// (§4.3 / §4.5) the server requires it for heterogeneous (EVM ⇌ non-EVM)
/// bridges and returns 82202 when missing — the CLI does not duplicate that
/// check, so direct callers can omit and rely on the server response.
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


/// Canonical zero: `"0"` or `"0.0…"` (only). Rejects `"-0"` / `"00"` / `"0e10"` / `".0"` / `"0."`.
/// Examples: `"0"` ✓, `"0.000"` ✓, `"-0"` ✗, `"001"` ✗.
fn is_canonical_zero_str(s: &str) -> bool {
    if s == "0" {
        return true;
    }
    s.starts_with("0.")
        && s.len() > 2
        && s[2..].chars().all(|c| c == '0')
}

/// Approve amount: raw `--amount` (allows `"0"` for revoke) or `--readable-amount`
/// (human decimal, fetches token decimals + converts).
/// Examples: `("500000", None)` → `"500000"`, `(None, "0.5")` → `"500000"` (USDC),
/// `(None, "0")` → `"0"` (revoke).
async fn resolve_approve_amount(
    client: &mut ApiClient,
    amount: Option<&str>,
    readable_amount: Option<&str>,
    token: &str,
    chain_index: &str,
) -> Result<String> {
    if let Some(raw) = amount {
        let raw = raw.trim();
        crate::validators::validate_non_negative_integer(raw, "amount")?;
        return Ok(raw.to_string());
    }
    if let Some(readable) = readable_amount {
        let readable = readable.trim();
        if readable.is_empty() {
            bail!("--readable-amount must not be empty");
        }
        // Canonical zero → revoke; non-canonical forms fall through to readable_to_minimal_str.
        if is_canonical_zero_str(readable) {
            return Ok("0".to_string());
        }
        let info = crate::commands::token::fetch_info(client, token, chain_index)
            .await
            .map_err(|e| {
                anyhow::anyhow!(
                    "Failed to fetch token decimals for {token}: {e}. Use --amount with raw units instead."
                )
            })?;
        let info_arr = info.as_array().filter(|a| !a.is_empty()).ok_or_else(|| {
            anyhow::anyhow!(
                "Token not found for address {token} on chain {chain_index}. \
                 Verify the address is correct. Use --amount with raw units instead."
            )
        })?;
        let decimal: u32 = match &info_arr[0]["decimal"] {
            serde_json::Value::String(s) => s.parse().map_err(|_| {
                anyhow::anyhow!("Invalid decimal value \"{s}\" for token {token}")
            })?,
            serde_json::Value::Number(n) => n
                .as_u64()
                .ok_or_else(|| anyhow::anyhow!("Invalid decimal value for token {token}"))?
                as u32,
            _ => bail!(
                "Token decimal not found for {token}. Use --amount with raw units instead."
            ),
        };
        return crate::validators::readable_to_minimal_str(readable, decimal);
    }
    bail!("either --amount or --readable-amount is required")
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

    let from_token = crate::token_alias::resolve_and_validate(&from_idx, from, "from")?;
    let to_token = crate::token_alias::resolve_and_validate(&to_idx, to, "to")?;
    crate::token_alias::validate_address_for_chain(&from_idx, wallet, "wallet")?;
    if let Some(addr) = receive_address {
        validate_receive_address(addr, &to_idx)?;
    }
    crate::validators::validate_slippage_zero_to_one(slippage)?;

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

    // ── Step 0: Balance gate ───────────────────────────────────────────────
    // Halt before quoting/broadcasting when the wallet can't cover the trade
    // (or gas). Degrades to proceed on lookup failure.
    if let Some((block, message)) = execute_balance_block(
        client,
        &from_idx,
        wallet,
        &from_token,
        native_addr,
        &raw_amount,
        is_from_native,
    )
    .await
    {
        output::success(json!({ "action": "blocked", "block": block, "message": message }));
        return Ok(());
    }

    // ── Step 1: Quote ──────────────────────────────────────────────────────
    let quote_res = fetch_quote(
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
        receive_address,
    )
    .await;
    // No direct route → surface the same transit-fallback structure instead of
    // broadcasting (an indirect path needs separate per-step user consent).
    if is_no_route(&quote_res) {
        let fallback =
            discover_transit_fallback(client, &from_idx, &to_idx, &from_token, &raw_amount, slippage)
                .await;
        output::success(json!({ "action": "fallback", "routerList": [], "fallback": fallback }));
        return Ok(());
    }
    let quote_data = quote_res?;
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
        // Default mode (NEW, spec §1.3 / Appendix A): one-shot approve+wait.
        // Runs the revoke (if `needCancelApprove`) and approve legs inline,
        // each followed by `wait_tx_onchain`, then falls through to the swap
        // step below. The new output schema (spec §2.2 — emitted in Step 4)
        // is uniquely identified by the presence of `nextSteps`.

        // 3a. Revoke if needed (USDT pattern). Mirrors the --confirm-approve
        // branch's `fetch_approve_tx(amount=0)` → broadcast pattern, then
        // additionally waits for the revoke tx to confirm before approving.
        // Lenient on `tx` shape (parity with --confirm-approve revoke leg):
        // if the server returns a non-object `tx` we silently skip the revoke
        // broadcast; strictness only kicks in for the approve leg.
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
            if let Some(revoke_tx_obj) = revoke_obj["tx"].as_object() {
                let revoke_calldata = revoke_tx_obj
                    .get("data")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| anyhow::anyhow!("missing tx.data in revoke approve-tx"))?;
                let revoke_gas_limit = revoke_tx_obj.get("gasLimit").and_then(|v| v.as_str());
                let revoke_result = wallet_contract_call(
                    &from_token,
                    &from_idx,
                    "0",
                    Some(revoke_calldata),
                    revoke_gas_limit,
                    false,
                    force,
                )
                .await?;
                let revoke_hash = extract_tx_hash(&revoke_result)?;
                wait_tx_onchain(client, &revoke_hash, &from_idx).await?;
            }
        }

        // 3b. Approve. Server returns the ready-built tx; broadcast via
        // wallet_contract_call, then wait for it to confirm before swapping.
        let approve_data = fetch_approve_tx(
            client,
            &from_idx,
            &from_token,
            wallet,
            &resolved_bridge_id,
            &raw_amount,
            false,
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
        let (a_tx_hash, a_order_id) = extract_tx_hash_and_order_id(&result)?;
        wait_tx_onchain(client, &a_tx_hash, &from_idx).await?;
        approve_tx_hash = Some(a_tx_hash);
        if !a_order_id.is_empty() {
            approve_order_id = Some(a_order_id);
        }
        // Fall through to Step 3 (swap) below — do NOT return here.
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

    // relay / mayan / butterswap embed a from-swap leg → MEV-mandatory.
    // Otherwise honor the caller's flag (size-threshold judgment is agent-side).
    let mev_protection = mev_protection || bridge_forces_mev(route["bridgeName"].as_str().unwrap_or(""));

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
    // Two shapes deliberately diverge (spec §2.3 M1):
    //   - Default (one-shot) path: NEW schema with `nextSteps` /
    //     `fromChainIndex` / `bridgeName`. Built via `build_execute_data`.
    //   - `--skip-approve` path: existing schema (untouched per PRD).
    if !skip_approve {
        let out = build_execute_data(
            route,
            &resolved_bridge_id,
            &from_idx,
            &swap_tx_hash,
            &swap_order_id,
            approve_tx_hash.as_deref(),
            approve_order_id.as_deref(),
        );
        output::success(out);
        return Ok(());
    }

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
    // Cross-chain `/swap` returns `tx.data` in the source chain's encoding —
    // EVM gets ABI-encoded contract input, Solana gets a base58-serialized
    // unsigned transaction. The Agentic Wallet `unsignedInfo` API is encoding-
    // aware: EVM goes through `inputData`, Solana through `unsignedTx`. Routing
    // the wrong one through `inputData` (the historical default) makes backend
    // return code=50001 "Service temporarily unavailable" because the request
    // structure does not match the chain family. Dispatch by family here.
    let (effective_input_data, effective_unsigned_tx, effective_amt, effective_gas_limit) =
        if crate::chains::chain_family(chain) == "solana" {
            (None, input_data, "0", None)
        } else {
            (input_data, None, amt, gas_limit)
        };
    let resp = crate::commands::agentic_wallet::transfer::execute_contract_call(
        to,
        chain,
        effective_amt,
        effective_input_data,
        effective_unsigned_tx,
        effective_gas_limit,
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

/// Build ready-to-paste `onchainos cross-chain status` command so callers can
/// poll bridge status without string surgery. Mirrors `swap::next_steps_for_swap`.
fn next_steps_for_bridge(bridge_id: &str, from_chain_index: &str, from_tx_hash: &str) -> Value {
    let mut steps = serde_json::Map::new();
    steps.insert(
        "checkBridgeStatus".to_string(),
        json!(format!(
            "onchainos cross-chain status --tx-hash {} --bridge-id {} --from-chain {}",
            from_tx_hash, bridge_id, from_chain_index
        )),
    );
    Value::Object(steps)
}

/// Assemble the default-path `data` payload (spec §2.2) emitted by the
/// one-shot `cross-chain execute` flow.
///
/// Pure: takes the route from `/quote` and the final swap tx info plus the
/// optional approval-leg artifacts; returns the `Value` ready to hand to
/// `output::success`. Factored out so the schema (`action`/`nextSteps`/
/// approval-conditional fields) is unit-testable without standing up the
/// full async network stack.
fn build_execute_data(
    route: &Value,
    resolved_bridge_id: &str,
    from_chain_index: &str,
    from_tx_hash: &str,
    swap_order_id: &str,
    approve_tx_hash: Option<&str>,
    approve_order_id: Option<&str>,
) -> Value {
    let next_steps = next_steps_for_bridge(resolved_bridge_id, from_chain_index, from_tx_hash);
    let mut out = json!({
        "action": "execute",
        "fromTxHash": from_tx_hash,
        "bridgeId": resolved_bridge_id,
        "bridgeName": route["bridgeName"],
        "fromChainIndex": from_chain_index,
        "minimumReceived": route["minimumReceived"],
        "toTokenAmount": route["toTokenAmount"],
        "crossChainFee": route["crossChainFee"],
        "estimateTime": route["estimateTime"],
        "nextSteps": next_steps,
    });
    if !swap_order_id.is_empty() {
        out["swapOrderId"] = json!(swap_order_id);
    }
    if let Some(hash) = approve_tx_hash {
        out["approveTxHash"] = json!(hash);
    }
    if let Some(oid) = approve_order_id {
        out["approveOrderId"] = json!(oid);
    }
    out
}

// ── Tests ──────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    // ── transit fallback ───────────────────────────────────────────────

    #[test]
    fn parse_api_error_extracts_code_and_msg() {
        assert_eq!(
            parse_api_error("API error (code=82000): Insufficient liquidity"),
            Some(("82000".to_string(), "Insufficient liquidity".to_string()))
        );
        // empty msg → empty string (classifier treats it as non-informative)
        assert_eq!(
            parse_api_error("API error (code=82000): "),
            Some(("82000".to_string(), String::new()))
        );
        assert_eq!(parse_api_error("some other error"), None);
    }

    #[test]
    fn is_no_route_true_for_82000_82104_and_empty_router_list() {
        let err82000: Result<Value> = Err(anyhow::anyhow!("API error (code=82000): no liquidity"));
        let err82104: Result<Value> = Err(anyhow::anyhow!("API error (code=82104): token"));
        assert!(is_no_route(&err82000));
        assert!(is_no_route(&err82104));
        // empty routerList (code=0 success body)
        let empty: Result<Value> = Ok(json!([{ "routerList": [] }]));
        assert!(is_no_route(&empty));
    }

    #[test]
    fn is_no_route_false_for_routes_and_unrelated_errors() {
        let with_route: Result<Value> = Ok(json!([{ "routerList": [{ "bridgeId": 1 }] }]));
        assert!(!is_no_route(&with_route));
        // a hard error that should propagate, not trigger fallback
        let auth_err: Result<Value> = Err(anyhow::anyhow!("API error (code=50114): not logged in"));
        assert!(!is_no_route(&auth_err));
    }

    #[test]
    fn bridgeable_source_addresses_filters_by_chain_and_lowercases() {
        let tokens = json!([
            { "chainIndex": "1", "tokenContractAddress": "0xAAA" },
            { "chainIndex": "1", "tokenContractAddress": "0xbbb" },
            { "chainIndex": "1088", "tokenContractAddress": "0xCCC" }
        ]);
        let set = bridgeable_source_addresses(&tokens, "1");
        assert!(set.contains("0xaaa"));
        assert!(set.contains("0xbbb"));
        assert!(!set.contains("0xccc")); // wrong chain dropped
    }

    #[test]
    fn build_transit_candidates_intersects_bridgeable_when_known() {
        // empty bridgeable set → no intersection filter (probe-all fallback)
        let all = build_transit_candidates("1", "42161", &HashSet::new());
        assert!(!all.is_empty());
        // restrict to a single known-bridgeable address → only matching candidate kept
        let only = all[0].address.to_lowercase();
        let set: HashSet<String> = [only.clone()].into_iter().collect();
        let filtered = build_transit_candidates("1", "42161", &set);
        assert!(filtered.iter().all(|c| c.address.to_lowercase() == only));
    }

    #[test]
    fn build_transit_candidates_resolves_dest_address_per_chain() {
        // Regression for the transit leg-2 toTokenAddress bug: a stablecoin's
        // dest address MUST be resolved on `to_idx`, not reused from the source
        // chain. USDC on Ethereum (1) and Arbitrum (42161) have different CAs.
        let cands = build_transit_candidates("1", "42161", &HashSet::new());
        let usdc = cands
            .iter()
            .find(|c| c.symbol == "USDC")
            .expect("USDC candidate");
        assert_eq!(usdc.address, "0xa0b86991c6218b36c1d19d4a2e9eb0ce3606eb48"); // ETH USDC
        assert_eq!(usdc.dest_address, "0xaf88d065e77c8cc2239327c5edb3a432268e5831"); // ARB USDC
        assert_ne!(usdc.address, usdc.dest_address);
    }

    #[test]
    fn build_transit_candidates_drops_stable_absent_on_dest() {
        // DAI exists on Ethereum but not in the Arbitrum alias map, so it cannot
        // serve as transit into Arbitrum and must be dropped (no wrong-address
        // probe). NATIVE always survives (shared sentinel address).
        let cands = build_transit_candidates("1", "42161", &HashSet::new());
        assert!(cands.iter().all(|c| c.symbol != "DAI"));
        assert!(cands.iter().any(|c| c.symbol == "NATIVE"));
    }

    #[test]
    fn build_transit_candidates_resolves_usdt_dest_address() {
        // Not just USDC: every stable must get its dest-chain address. USDT on
        // Ethereum (0xdac1…) and Arbitrum (0xfd08…) differ.
        let cands = build_transit_candidates("1", "42161", &HashSet::new());
        let usdt = cands
            .iter()
            .find(|c| c.symbol == "USDT")
            .expect("USDT candidate");
        assert_eq!(usdt.address, "0xdac17f958d2ee523a2206206994597c13d831ec7"); // ETH USDT
        assert_eq!(usdt.dest_address, "0xfd086bc7cd5c481dcc9c85ebe478a1c0b69fcbb9"); // ARB USDT
        assert_ne!(usdt.address, usdt.dest_address);
    }

    #[test]
    fn build_transit_candidates_resolves_dest_across_vm_families() {
        // Solana source → EVM dest: the trickiest case. Source addresses are
        // base58, dest addresses are 0x — leg-2's toTokenAddress must be the
        // EVM-side address, never the base58 source one.
        let cands = build_transit_candidates("501", "1", &HashSet::new());
        let usdc = cands
            .iter()
            .find(|c| c.symbol == "USDC")
            .expect("USDC candidate");
        assert_eq!(usdc.address, "EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v"); // SOL USDC
        assert_eq!(usdc.dest_address, "0xa0b86991c6218b36c1d19d4a2e9eb0ce3606eb48"); // ETH USDC
        // NATIVE too: SOL sentinel source, EVM sentinel dest.
        let native = cands
            .iter()
            .find(|c| c.symbol == "NATIVE")
            .expect("NATIVE candidate");
        assert_eq!(native.address, "11111111111111111111111111111111");
        assert_eq!(native.dest_address, "0xeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee");
    }

    #[test]
    fn build_transit_candidates_intersection_keys_on_source_address() {
        // Regression guard: the bridgeable intersection must filter on the
        // SOURCE address, not the dest. A set holding only the dest USDC address
        // (Arbitrum) must NOT keep the ETH→ARB USDC candidate (whose source is
        // the Ethereum USDC address).
        let dest_only: HashSet<String> =
            ["0xaf88d065e77c8cc2239327c5edb3a432268e5831".to_string()] // ARB USDC (dest)
                .into_iter()
                .collect();
        let cands = build_transit_candidates("1", "42161", &dest_only);
        assert!(
            cands.iter().all(|c| c.symbol != "USDC"),
            "USDC must be dropped — its SOURCE address is not in the bridgeable set"
        );
    }

    #[test]
    fn build_transit_option_maps_first_route() {
        let bridge_quote = json!([{
            "toToken": { "decimals": 6 },
            "routerList": [{
                "bridgeName": "ACROSS V3",
                "bridgeId": 636,
                "toTokenAmount": "999533",
                "minimumReceived": "999533",
                "crossChainFee": "466",
                "crossChainFeeTokenAddress": "0xaf88",
                "otherNativeFee": "0",
                "estimateTime": "43"
            }]
        }]);
        let opt = build_transit_option("USDC", &bridge_quote).expect("option");
        assert_eq!(opt["transitToken"], json!("USDC"));
        assert_eq!(opt["bridgeName"], json!("ACROSS V3"));
        assert_eq!(opt["toTokenAmount"], json!("999533"));
        assert_eq!(opt["toTokenDecimals"], json!(6));
    }

    #[test]
    fn build_transit_option_none_when_no_route() {
        assert!(build_transit_option("USDC", &json!([{ "routerList": [] }])).is_none());
    }

    #[test]
    fn raw_balance_for_matches_case_insensitive_and_defaults_zero() {
        let assets = json!([
            { "tokenContractAddress": "0xAaA", "rawBalance": "100" },
            { "tokenContractAddress": "0xbbb", "rawBalance": "200" }
        ]);
        assert_eq!(raw_balance_for(&assets, "0xaaa"), "100");
        assert_eq!(raw_balance_for(&assets, "0xBBB"), "200");
        assert_eq!(raw_balance_for(&assets, "0xccc"), "0"); // absent → "0"
    }

    #[test]
    fn bridge_forces_mev_matches_from_swap_bridges() {
        assert!(bridge_forces_mev("RELAY"));
        assert!(bridge_forces_mev("Mayan Swift"));
        assert!(bridge_forces_mev("butterswap"));
        assert!(!bridge_forces_mev("ACROSS V3"));
        assert!(!bridge_forces_mev("STARGATE V2 TAXI MODE"));
    }

    #[test]
    fn classify_dead_end_distinguishes_no_path_from_env_unavailable() {
        let (outcome, msg) = classify_dead_end(&["Insufficient liquidity".to_string()]);
        assert_eq!(outcome, "no_path");
        assert_eq!(msg, "Insufficient liquidity");

        let (outcome, _) = classify_dead_end(&["".to_string(), "unknown error".to_string()]);
        assert_eq!(outcome, "env_unavailable");
    }

    // ── is_canonical_zero_str ──────────────────────────────────────────

    #[test]
    fn canonical_zero_accepts_zero_forms() {
        assert!(is_canonical_zero_str("0"));
        assert!(is_canonical_zero_str("0.0"));
        assert!(is_canonical_zero_str("0.00"));
        assert!(is_canonical_zero_str("0.000000"));
    }

    #[test]
    fn canonical_zero_rejects_signed_and_leading_zero_forms() {
        // Signed → typo-ish, fall through to a real error
        assert!(!is_canonical_zero_str("-0"));
        assert!(!is_canonical_zero_str("+0"));
        assert!(!is_canonical_zero_str("-0.0"));
        // Leading-zero — validate_amount rejects elsewhere, keep parity
        assert!(!is_canonical_zero_str("00"));
        assert!(!is_canonical_zero_str("001"));
        assert!(!is_canonical_zero_str("00.0"));
    }

    #[test]
    fn canonical_zero_rejects_other_non_canonical_forms() {
        assert!(!is_canonical_zero_str(""));
        assert!(!is_canonical_zero_str("0."));        // no trailing digits
        assert!(!is_canonical_zero_str(".0"));        // no leading "0"
        assert!(!is_canonical_zero_str("0e10"));      // scientific notation
        assert!(!is_canonical_zero_str("0.0.0"));     // not a valid number
        assert!(!is_canonical_zero_str("0.0a"));      // mixed
        assert!(!is_canonical_zero_str("1"));         // non-zero
        assert!(!is_canonical_zero_str("0.1"));       // non-zero fractional
    }

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

    // ── next_steps_for_bridge ───────────────────────────────────────────────

    #[test]
    fn next_steps_for_bridge_emits_check_bridge_status_command() {
        let steps = next_steps_for_bridge("199", "1", "0xabc");
        let obj = steps.as_object().expect("nextSteps must be a JSON object");
        let cmd = obj
            .get("checkBridgeStatus")
            .and_then(|v| v.as_str())
            .expect("checkBridgeStatus must be a string");
        assert_eq!(
            cmd,
            "onchainos cross-chain status --tx-hash 0xabc --bridge-id 199 --from-chain 1"
        );
    }

    // ── build_execute_data (default-path output schema, spec §2.2) ──────────

    fn sample_route() -> Value {
        json!({
            "bridgeId": 199,
            "bridgeName": "Across",
            "needApprove": true,
            "needCancelApprove": false,
            "estimateTime": "30",
            "minimumReceived": "0.99",
            "toTokenAmount": "1.00",
            "crossChainFee": "0.01",
            "fromToken": { "tokenSymbol": "USDC" },
        })
    }

    #[test]
    fn build_execute_data_default_path_carries_action_and_next_steps() {
        // Spec §2.2: default path emits action="execute" with nextSteps.checkBridgeStatus
        // and fromChainIndex/bridgeName — uniquely identifying the one-shot shape.
        let out = build_execute_data(
            &sample_route(),
            "199",
            "1",
            "0xfromhash",
            "swap-oid-42",
            None,
            None,
        );
        assert_eq!(out["action"], json!("execute"));
        assert_eq!(out["fromTxHash"], json!("0xfromhash"));
        assert_eq!(out["fromChainIndex"], json!("1"));
        assert_eq!(out["bridgeId"], json!("199"));
        assert_eq!(out["bridgeName"], json!("Across"));
        assert_eq!(out["swapOrderId"], json!("swap-oid-42"));
        assert_eq!(out["minimumReceived"], json!("0.99"));
        assert_eq!(out["toTokenAmount"], json!("1.00"));
        assert_eq!(out["crossChainFee"], json!("0.01"));
        assert_eq!(out["estimateTime"], json!("30"));
        let next = out
            .get("nextSteps")
            .and_then(|v| v.as_object())
            .expect("nextSteps must be a JSON object");
        let cmd = next
            .get("checkBridgeStatus")
            .and_then(|v| v.as_str())
            .expect("checkBridgeStatus must be a string");
        assert_eq!(
            cmd,
            "onchainos cross-chain status --tx-hash 0xfromhash --bridge-id 199 --from-chain 1"
        );
    }

    #[test]
    fn build_execute_data_omits_approve_fields_when_no_approval_performed() {
        // Spec §2.2: `approveTxHash` / `approveOrderId` MUST be absent when no
        // approval ran (e.g. allowance sufficient, Solana source chain, native token).
        let out = build_execute_data(
            &sample_route(),
            "199",
            "1",
            "0xfromhash",
            "swap-oid",
            None,
            None,
        );
        let obj = out.as_object().expect("data must be a JSON object");
        assert!(
            !obj.contains_key("approveTxHash"),
            "approveTxHash must be omitted when no approval performed, got: {out}"
        );
        assert!(
            !obj.contains_key("approveOrderId"),
            "approveOrderId must be omitted when no approval performed, got: {out}"
        );
    }

    #[test]
    fn build_execute_data_includes_approve_fields_when_approval_performed() {
        // Spec §2.2: `approveTxHash` / `approveOrderId` present iff approval ran
        // (and the broadcast returned a non-empty order id).
        let out = build_execute_data(
            &sample_route(),
            "199",
            "1",
            "0xfromhash",
            "swap-oid",
            Some("0xapprovehash"),
            Some("approve-oid-7"),
        );
        assert_eq!(out["approveTxHash"], json!("0xapprovehash"));
        assert_eq!(out["approveOrderId"], json!("approve-oid-7"));
    }

    #[test]
    fn build_execute_data_omits_swap_order_id_when_empty() {
        // Mirrors the pre-existing `--skip-approve` convention: do not pollute
        // output with an empty `swapOrderId` (only Gas Station paths set it).
        let out = build_execute_data(&sample_route(), "199", "1", "0xfromhash", "", None, None);
        assert!(
            !out.as_object().unwrap().contains_key("swapOrderId"),
            "swapOrderId must be omitted when empty, got: {out}"
        );
    }
}
