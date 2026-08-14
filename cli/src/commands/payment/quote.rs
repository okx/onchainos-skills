//! `payment quote` — probe an HTTP 402 / A2MCP endpoint, parse the payment
//! challenge, run a wallet/balance preflight, rank candidates, and persist a
//! `paymentId` for a later `payment pay --payment-id`. Never signs.
//!
//! The heavy mechanical work the agent used to do by hand (curl, base64 decode,
//! `accepts` parse, amount conversion, balance filter, recommendation ranking)
//! all lives here so the agent collapses to a 2-round playbook.

use std::collections::{BTreeSet, HashMap};
use std::time::Duration;

use alloy_primitives::U256;
use anyhow::{anyhow, Result};
use serde::Serialize;
use serde_json::{Map, Value};

use super::payment_flow::{self, extract_amount};
use super::state::{
    self, AcceptEntry, Candidate, DecodedChallenge, ParamCarrier, ParamSpec, PaymentState,
};
use crate::mcp_client::{self, McpClient, McpTool, ToolCallOutcome};
use crate::output;

/// Machine tokens (leading word of `output::error`).
pub const TOKEN_ENDPOINT_UNREACHABLE: &str = "endpoint_unreachable";
pub const TOKEN_UNSUPPORTED: &str = "unsupported";
pub const TOKEN_INVALID_INPUT: &str = "invalid_input";
/// Merchant rejected the probe for an auth reason (401/403) — the agent should
/// prompt the user to authenticate rather than blindly retry.
pub const TOKEN_AUTH_REQUIRED: &str = "auth_required";
/// Merchant returned a 5xx — transient server-side; the agent may retry.
pub const TOKEN_ENDPOINT_SERVER_ERROR: &str = "endpoint_server_error";

/// Merchant-probe timeout — merchant hosts are arbitrary, so bound tightly.
const PROBE_TIMEOUT_SECS: u64 = 10;

/// `payment quote` `data` shape (stability contract — see `cli_command_spec.md`).
#[derive(Serialize)]
struct QuoteData {
    #[serde(rename = "paymentId", skip_serializing_if = "String::is_empty")]
    payment_id: String,
    #[serde(rename = "needsConfirm")]
    needs_confirm: bool,
    summary: String,
    #[serde(rename = "nextStep")]
    next_step: String,
    accepts: Vec<AcceptEntry>,
    #[serde(rename = "knownParams")]
    known_params: Map<String, Value>,
    #[serde(rename = "merchantBody")]
    merchant_body: String,
    #[serde(rename = "missingParams")]
    missing_params: Vec<String>,
    #[serde(rename = "paramPlan")]
    param_plan: Vec<ParamSpec>,
    candidates: Vec<Candidate>,
    alternatives: Vec<Candidate>,
    #[serde(rename = "decodedChallenge")]
    decoded_challenge: DecodedChallenge,
    #[serde(rename = "walletError", skip_serializing_if = "Option::is_none")]
    wallet_error: Option<String>,
    /// MCP discovery catalog (`cli_command_spec.md` Output A). Populated only in
    /// MCP discovery mode; omitted (empty) for REST and paid MCP quotes so the
    /// REST output stays byte-identical.
    #[serde(rename = "mcpTools", default, skip_serializing_if = "Vec::is_empty")]
    mcp_tools: Vec<McpTool>,
    /// Free / first-N-free MCP `tools/call` result (Output C). Populated only
    /// when an unpaid tool returns a result; omitted otherwise.
    #[serde(rename = "result", skip_serializing_if = "Option::is_none")]
    mcp_result: Option<Value>,
}

/// CLI handler: run the quote and print the always-on envelope. Classified
/// probe/parse failures propagate as `Err` so `main.rs` renders `output::error`
/// (exit 1); `walletError` / all-zero-balance are `Ok` data (exit 0).
pub async fn run(url: &str, param: &[String], method: &str, tool: Option<&str>) -> Result<()> {
    let data = fetch_quote(url, param, method, tool).await?;
    output::success(data);
    Ok(())
}

/// Data path shared by the CLI handler and the `payment_quote` MCP tool.
/// Returns the `data` payload (`QuoteData` serialized to `Value`).
///
/// Detection order (arch §3) — the MCP-transport branch is entered when **any**
/// fires: (1) `--tool` supplied (forces MCP); (2) the URL path ends `/mcp`|`/sse`;
/// (3) the bare probe signals MCP (`text/event-stream` or a JSON-RPC body). When
/// none fire, the existing REST path is unchanged.
pub async fn fetch_quote(
    url: &str,
    param: &[String],
    method: &str,
    tool: Option<&str>,
) -> Result<Value> {
    let known_params = parse_params(param)?;

    // (1) explicit --tool or (2) URL looks like MCP → force the MCP branch.
    if tool.is_some() || mcp_client::url_looks_like_mcp(url) {
        return fetch_quote_mcp(url, &known_params, tool).await;
    }

    let outcome = probe_endpoint(url, &known_params, method).await?;
    let (challenge_header, merchant_body) = match outcome {
        ProbeOutcome::NoCharge { body } => {
            // 200 → nothing to pay. Emit a read-only "free" quote (no paymentId
            // written, nothing signed).
            let data = QuoteData {
                payment_id: String::new(),
                needs_confirm: false,
                summary: "Endpoint returned 200 — no payment required".to_string(),
                next_step: String::new(),
                accepts: vec![],
                known_params,
                merchant_body: body,
                missing_params: vec![],
                param_plan: vec![],
                candidates: vec![],
                alternatives: vec![],
                decoded_challenge: free_challenge(),
                wallet_error: None,
                mcp_tools: vec![],
                mcp_result: None,
            };
            return serde_json::to_value(data).map_err(Into::into);
        }
        // (3) bare probe signaled MCP transport → hand off to the MCP branch.
        ProbeOutcome::MaybeMcp => {
            return fetch_quote_mcp(url, &known_params, tool).await;
        }
        ProbeOutcome::Challenge { header, body } => (header, body),
    };

    build_quote_from_challenge(
        url,
        known_params,
        method,
        &challenge_header,
        merchant_body,
        None,
    )
    .await
}

/// MCP-transport quote (arch §3/§5). Assumes the caller already decided this is
/// MCP mode. Handshakes via `McpClient`, then:
/// - `tool = None` → **discovery**: `tools/list` → Output A. Returns the unified
///   `QuoteData` envelope (needsConfirm:false + summary + nextStep) carrying the
///   tool catalog in `mcpTools[]`, isomorphic with a REST quote.
/// - `tool = Some(name)` → validate against the catalog, coerce `--param` from
///   the tool's `inputSchema`, and `tools/call`: a 402 reuses the REST
///   challenge→candidate→state path (Output B, byte-identical) with the
///   `mcp_tool` marker + coerced arguments persisted; a non-402 result is a free
///   tool (Output C) returned as a `QuoteData` envelope (needsConfirm:false +
///   summary) carrying the tool result in `result`.
async fn fetch_quote_mcp(
    url: &str,
    known_params: &Map<String, Value>,
    tool: Option<&str>,
) -> Result<Value> {
    let mut client = McpClient::new(url)?;
    client.initialize().await?;
    let tools = client.list_tools().await?;

    let Some(name) = tool else {
        // Discovery (Output A): the unified QuoteData envelope carrying the tool
        // catalog plus machine-readable next-step guidance. No paymentId
        // (discovery is free, persists nothing) so `jq '.data.paymentId'` is
        // null (AC-2); `mcpTools[]` carries the catalog.
        let names: Vec<&str> = tools.iter().map(|t| t.name.as_str()).collect();
        let summary = format!(
            "MCP server exposes {} tool(s): {}",
            tools.len(),
            names.join(", ")
        );
        let next_step = format!(
            "onchainos payment quote {url} --tool <name> [--param k=v] — pick a tool to trigger its 402"
        );
        let data = QuoteData {
            payment_id: String::new(),
            needs_confirm: false,
            summary,
            next_step,
            accepts: vec![],
            known_params: known_params.clone(),
            merchant_body: String::new(),
            missing_params: vec![],
            param_plan: vec![],
            candidates: vec![],
            alternatives: vec![],
            decoded_challenge: free_challenge(),
            wallet_error: None,
            mcp_tools: tools,
            mcp_result: None,
        };
        return serde_json::to_value(data).map_err(Into::into);
    };

    // Validate the requested tool against the discovered catalog.
    let selected = tools.iter().find(|t| t.name == name).ok_or_else(|| {
        let available: Vec<&str> = tools.iter().map(|t| t.name.as_str()).collect();
        anyhow!(
            "{TOKEN_INVALID_INPUT}: tool '{name}' not found; available tools: [{}]",
            available.join(", ")
        )
    })?;

    // Coerce --param values per the tool's inputSchema, then invoke tools/call.
    let arguments = mcp_client::coerce_arguments(known_params, selected.input_schema.as_ref());
    match client.call_tool(name, &arguments).await? {
        ToolCallOutcome::Paid { header, body } => {
            // 402 → reuse the REST challenge pipeline, persisting the coerced
            // arguments as known_params and the mcp_tool marker so `pay` replays
            // the SAME tools/call. Output B (byte-identical to a REST quote).
            let coerced_params = arguments.as_object().cloned().unwrap_or_default();
            build_quote_from_challenge(url, coerced_params, "POST", &header, body, Some(name)).await
        }
        ToolCallOutcome::Free(result) => {
            // Non-402 result → a free / first-N-free tool (Output C). Unified
            // QuoteData envelope (needsConfirm:false + summary explaining no
            // payment is required) carrying the tool result in `result`.
            let summary = format!("MCP tool '{name}' returned a result — no payment required");
            let data = QuoteData {
                payment_id: String::new(),
                needs_confirm: false,
                summary,
                next_step: String::new(),
                accepts: vec![],
                known_params: known_params.clone(),
                merchant_body: String::new(),
                missing_params: vec![],
                param_plan: vec![],
                candidates: vec![],
                alternatives: vec![],
                decoded_challenge: free_challenge(),
                wallet_error: None,
                mcp_tools: vec![],
                mcp_result: Some(result),
            };
            serde_json::to_value(data).map_err(Into::into)
        }
    }
}

/// Decode a 402 challenge (REST or MCP `tools/call`), rank candidates, run the
/// wallet preflight, and persist `PaymentState`. Shared by the REST path and the
/// MCP paid path — the only MCP-specific input is `mcp_tool` (the tool name to
/// persist, flipping `pay` into the `replay_mcp` branch).
async fn build_quote_from_challenge(
    url: &str,
    known_params: Map<String, Value>,
    method: &str,
    challenge_header: &str,
    merchant_body: String,
    mcp_tool: Option<&str>,
) -> Result<Value> {
    // Decode the challenge blob (reuses the shared base64 / WWW-Authenticate
    // decoder) and pull the accepts[] array.
    let decoded = super::dispatcher::decode_payment_blob(challenge_header)
        .map_err(|e| anyhow!("{TOKEN_UNSUPPORTED}: could not decode 402 challenge: {e}"))?;
    let accepts_val = decoded
        .get("accepts")
        .and_then(|v| v.as_array())
        .cloned()
        .ok_or_else(|| anyhow!("{TOKEN_UNSUPPORTED}: 402 challenge has no accepts[] array"))?;
    if accepts_val.is_empty() {
        return Err(anyhow!(
            "{TOKEN_UNSUPPORTED}: 402 challenge accepts[] is empty"
        ));
    }

    let accepts = build_accepts(&accepts_val)?;
    let mut resolver = DecimalResolver::new();
    let decoded_challenge = build_decoded_challenge(&accepts_val, &mut resolver).await?;
    if !decoded_challenge.supported {
        let reason = decoded_challenge
            .unsupported_reason
            .clone()
            .unwrap_or_else(|| "no supported scheme".to_string());
        return Err(anyhow!("{TOKEN_UNSUPPORTED}: {reason}"));
    }

    // Parse the Bazaar `outputSchema` (Source 1): per-param carrier/required/type
    // and the paid-call HTTP method. Falls back to the probe method when the
    // schema does not pin one.
    let output_schema = find_output_schema(&decoded, &merchant_body);
    let param_plan = output_schema
        .as_ref()
        .and_then(|s| s.get("input"))
        .map(parse_param_plan)
        .unwrap_or_default();
    let paid_method = output_schema
        .as_ref()
        .and_then(|s| s.get("method"))
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
        .unwrap_or_else(|| method.to_string());

    // Build candidates, then run the wallet/balance preflight (best-effort;
    // login_required / balance_unavailable never abort the read-only quote).
    let mut candidates = build_candidates(&accepts_val, &accepts, &mut resolver).await?;
    let wallet_error = preflight_balances(&mut candidates, &accepts).await;

    let (candidates, alternatives) = payment_flow::rank_candidates(candidates);

    // Persisted state keeps the full ranked set (winner + alternatives) keyed by
    // `acceptsIndex`, not just the winner, so `payment pay`'s confirming preview
    // can render whichever `--selected-index` (an accepts[] index) the user pins.
    let mut state_candidates = candidates.clone();
    state_candidates.extend(alternatives.clone());

    // Persist state for `pay` (no key, no signed blob — see state.rs).
    let created_at = now_unix();
    let owner = state::current_owner_id().unwrap_or_default();
    let payment_id = new_payment_id(url, created_at);
    let expires_at = state::compute_expires_at(decoded_challenge.expires, created_at);
    let missing_params = missing_params(&merchant_body, &known_params, &param_plan);

    let st = PaymentState {
        payment_id: payment_id.clone(),
        owner_wallet: owner,
        created_at,
        expires_at,
        accepts: accepts.clone(),
        decoded_challenge: decoded_challenge.clone(),
        candidates: state_candidates,
        known_params: known_params.clone(),
        merchant_body: merchant_body.clone(),
        endpoint_url: url.to_string(),
        raw_accepts: accepts_val.clone(),
        resource: decoded.get("resource").cloned(),
        method: paid_method,
        param_plan: param_plan.clone(),
        mcp_tool: mcp_tool.map(|s| s.to_string()),
    };
    st.write()?;

    let summary = build_summary(&candidates, &alternatives, &decoded_challenge);
    let next_step =
        format!("onchainos payment pay --payment-id {payment_id} --selected-index <n> --yes");

    let data = QuoteData {
        payment_id,
        needs_confirm: true,
        summary,
        next_step,
        accepts,
        known_params,
        merchant_body,
        missing_params,
        param_plan,
        candidates,
        alternatives,
        decoded_challenge,
        wallet_error,
        mcp_tools: vec![],
        mcp_result: None,
    };
    serde_json::to_value(data).map_err(Into::into)
}

// ── Param parsing ──────────────────────────────────────────────────────

/// Parse repeatable `--param key=value` into a JSON object. Malformed entries
/// (no `=`, empty key) → `invalid_input`.
fn parse_params(param: &[String]) -> Result<Map<String, Value>> {
    let mut map = Map::new();
    for raw in param {
        let (k, v) = raw.split_once('=').ok_or_else(|| {
            anyhow!("{TOKEN_INVALID_INPUT}: --param must be key=value, got '{raw}'")
        })?;
        let k = k.trim();
        if k.is_empty() {
            return Err(anyhow!(
                "{TOKEN_INVALID_INPUT}: --param key must not be empty"
            ));
        }
        map.insert(k.to_string(), Value::String(v.to_string()));
    }
    Ok(map)
}

// ── Endpoint probe ─────────────────────────────────────────────────────

enum ProbeOutcome {
    /// HTTP 200 — endpoint served content without a payment challenge.
    NoCharge { body: String },
    /// HTTP 402 — a payment challenge header + the merchant response body.
    Challenge { header: String, body: String },
    /// The bare probe signaled MCP transport (`Content-Type: text/event-stream`
    /// or a JSON-RPC body). The MCP handshake re-runs from scratch, so no probe
    /// state needs to be carried forward.
    MaybeMcp,
}

/// Probe the merchant endpoint with a freshly-built `reqwest::Client`
/// (`ApiClient` is host-locked to web3.okx.com and cannot be reused here).
/// The request is assembled per `method` (GET by default; POST/PUT/PATCH send
/// known params as a JSON body) via [`http_carrier::build_request`], so a
/// POST/body A2MCP endpoint can be probed rather than always GET+query. The
/// per-param carrier plan is not yet known at probe time (it comes from the
/// challenge/outputSchema), so probe uses the method-based carrier defaults.
/// Non-402/non-200 or transport failure → `endpoint_unreachable`.
async fn probe_endpoint(
    url: &str,
    known_params: &Map<String, Value>,
    method: &str,
) -> Result<ProbeOutcome> {
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(PROBE_TIMEOUT_SECS))
        .build()
        .map_err(|e| anyhow!("{TOKEN_ENDPOINT_UNREACHABLE}: {e}"))?;

    let params: Vec<(String, String)> = known_params
        .iter()
        .filter_map(|(k, v)| v.as_str().map(|s| (k.clone(), s.to_string())))
        .collect();

    let resp = super::http_carrier::build_request(&client, method, url, &params, &[])
        .send()
        .await
        .map_err(|e| anyhow!("{TOKEN_ENDPOINT_UNREACHABLE}: {e}"))?;

    let status = resp.status();
    // Grab the payment challenge header before consuming the body.
    let header = resp
        .headers()
        .get("PAYMENT-REQUIRED")
        .or_else(|| resp.headers().get("WWW-Authenticate"))
        .and_then(|v| v.to_str().ok())
        .map(|s| s.to_string());
    let content_type = resp
        .headers()
        .get(reqwest::header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_string();
    let body = resp.text().await.unwrap_or_default();

    if status.as_u16() == 402 {
        // Some servers put the challenge in the body rather than a header.
        let header = header.unwrap_or_else(|| body.clone());
        Ok(ProbeOutcome::Challenge { header, body })
    } else if mcp_client::probe_signals_mcp(&content_type, &body) {
        // Detection order (3): a bare probe that advertises SSE or replies with a
        // JSON-RPC body is an MCP-transport endpoint — hand off to the MCP branch.
        Ok(ProbeOutcome::MaybeMcp)
    } else if status.is_success() {
        Ok(ProbeOutcome::NoCharge { body })
    } else {
        let code = status.as_u16();
        let token = classify_probe_error(code);
        if code == 405 {
            // A POST-only REST A2MCP (or an MCP endpoint that rejects the GET
            // probe) — advise the two ways forward rather than a dead end.
            Err(anyhow!(
                "{token}: endpoint returned HTTP 405 to the {method} probe — if this is an \
                 A2MCP endpoint, retry with --tool <name> (MCP transport) or --method POST (REST)"
            ))
        } else {
            Err(anyhow!(
                "{token}: unexpected HTTP {code} (expected 402 or 200)"
            ))
        }
    }
}

/// Map a non-402/non-2xx probe status to a machine token so the agent can pick
/// the right branch (auth prompt vs retry vs give up) instead of treating every
/// failure as `endpoint_unreachable`:
/// - 401/403 → `auth_required` (authenticate, do not blind-retry);
/// - 5xx     → `endpoint_server_error` (transient — retry is reasonable);
/// - other   → `endpoint_unreachable` (as before; transport errors keep this
///   token too, classified at the `send()` call site).
fn classify_probe_error(status: u16) -> &'static str {
    match status {
        401 | 403 => TOKEN_AUTH_REQUIRED,
        500..=599 => TOKEN_ENDPOINT_SERVER_ERROR,
        _ => TOKEN_ENDPOINT_UNREACHABLE,
    }
}

// ── Accepts / challenge shaping ─────────────────────────────────────────

fn build_accepts(accepts_val: &[Value]) -> Result<Vec<AcceptEntry>> {
    accepts_val
        .iter()
        .enumerate()
        .map(|(i, e)| {
            Ok(AcceptEntry {
                index: i,
                scheme: e
                    .get("scheme")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string(),
                amount: extract_amount(e).unwrap_or_default(),
                asset: e
                    .get("asset")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string(),
                network: e
                    .get("network")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string(),
            })
        })
        .collect()
}

/// Last-resort token decimals when neither the accepts entry nor the okx-dex
/// token metadata yields a value. Only applied after both sources are exhausted
/// (never silently default when token metadata is available).
const DEFAULT_DECIMALS: u32 = 6;

/// Read the decimals an accepts entry declares inline (`extra.decimals` or a
/// top-level `decimals`), accepting both numeric and string encodings.
fn declared_decimals(entry: &Value) -> Option<u32> {
    let v = entry
        .get("extra")
        .and_then(|x| x.get("decimals"))
        .or_else(|| entry.get("decimals"))?;
    v.as_u64()
        .map(|n| n as u32)
        .or_else(|| v.as_str().and_then(|s| s.parse::<u32>().ok()))
}

/// Extract `(chainIndex, tokenContractAddress)` from an accepts entry for an
/// okx-dex metadata lookup. Returns `None` when the asset address is absent.
fn entry_asset_and_chain(entry: &Value) -> Option<(String, String)> {
    let asset = entry
        .get("asset")
        .and_then(|v| v.as_str())
        .filter(|s| !s.is_empty())?;
    let network = entry.get("network").and_then(|v| v.as_str()).unwrap_or("");
    let chain_id = network.strip_prefix("eip155:").unwrap_or(network);
    Some((chain_id.to_string(), asset.to_string()))
}

/// Token metadata resolved from okx-dex basic-info: decimals (for amount
/// rendering) and the real ticker symbol (for display + symbol-fallback balance
/// matching). Each field is `None` when the lookup was attempted but yielded
/// nothing — the whole struct doubles as the negative-cache marker.
#[derive(Clone, Default, Debug, PartialEq, Eq)]
struct TokenMeta {
    decimals: Option<u32>,
    symbol: Option<String>,
}

/// Query the okx-dex token basic-info endpoint for a token's decimals + symbol
/// by (chainIndex, contractAddress). Best-effort — returns an empty
/// [`TokenMeta`] on any transport or shape failure so the caller can fall back
/// without aborting the quote.
async fn fetch_token_meta_from_okx_dex(
    client: &mut crate::client::ApiClient,
    chain_id: &str,
    address: &str,
) -> TokenMeta {
    let Ok(resp) = crate::commands::token::fetch_info(client, address, chain_id).await else {
        return TokenMeta::default();
    };
    let Some(item) = resp.as_array().and_then(|a| a.first()) else {
        return TokenMeta::default();
    };
    // basic-info carries decimals in the `decimal` (string) field; accept a
    // numeric `decimals` too for forward-compatibility.
    let decimals = item
        .get("decimal")
        .and_then(|d| d.as_str())
        .and_then(|s| s.parse::<u32>().ok())
        .or_else(|| {
            item.get("decimals")
                .and_then(|d| d.as_u64())
                .map(|n| n as u32)
        });
    // The real ticker (`symbol`, e.g. "USDT") — used to display a human ticker
    // instead of a contract address / EIP-712 domain, and to make the
    // symbol-fallback balance match reliable.
    let symbol = item
        .get("symbol")
        .or_else(|| item.get("tokenSymbol"))
        .and_then(|s| s.as_str())
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string());
    TokenMeta { decimals, symbol }
}

/// Resolves token metadata (decimals + symbol) for accepts entries. Decimals
/// prefer the entry's declared value, then an okx-dex metadata lookup, and only
/// then [`DEFAULT_DECIMALS`] (never silently default when metadata is
/// reachable). Memoizes okx-dex lookups by (chainIndex, address) — including
/// best-effort misses — so a multi-scheme challenge for one token queries
/// okx-dex at most once, even when the lookup fails and the caller falls back.
struct DecimalResolver {
    client: Option<crate::client::ApiClient>,
    /// (chainIndex, address) → resolved [`TokenMeta`]. A default (all-`None`)
    /// entry is the negative cache that prevents a second same-token candidate
    /// from re-hitting basic-info after the first attempt failed.
    memo: HashMap<(String, String), TokenMeta>,
}

impl DecimalResolver {
    fn new() -> Self {
        Self {
            client: crate::client::ApiClient::new(None).ok(),
            memo: HashMap::new(),
        }
    }

    /// Fetch (once) and memoize the okx-dex metadata for an entry's token.
    /// Returns `None` when the entry carries no asset address to look up.
    async fn ensure_meta(&mut self, entry: &Value) -> Option<&TokenMeta> {
        let (chain_id, address) = entry_asset_and_chain(entry)?;
        let key = (chain_id.clone(), address.clone());
        if !self.memo.contains_key(&key) {
            let resolved = match self.client.as_mut() {
                Some(client) => fetch_token_meta_from_okx_dex(client, &chain_id, &address).await,
                None => TokenMeta::default(),
            };
            self.memo.insert(key.clone(), resolved);
        }
        self.memo.get(&key)
    }

    async fn resolve(&mut self, entry: &Value) -> u32 {
        if let Some(d) = declared_decimals(entry) {
            return d;
        }
        if let Some(meta) = self.ensure_meta(entry).await {
            if let Some(d) = meta.decimals {
                return d;
            }
        }
        DEFAULT_DECIMALS
    }

    /// Resolve the token's real ticker symbol via okx-dex basic-info. `None`
    /// when the entry has no asset address or the lookup yielded no symbol —
    /// the caller then falls back to the challenge's own `extra.name` / asset.
    async fn resolve_symbol(&mut self, entry: &Value) -> Option<String> {
        self.ensure_meta(entry).await.and_then(|m| m.symbol.clone())
    }
}

/// Build the `decodedChallenge` from the best entry. Supported iff at least one
/// entry uses a known EVM scheme.
async fn build_decoded_challenge(
    accepts_val: &[Value],
    resolver: &mut DecimalResolver,
) -> Result<DecodedChallenge> {
    let (entry, _scheme) = payment_flow::select_accept_with_preference(accepts_val, None)
        .map_err(|e| anyhow!("{TOKEN_UNSUPPORTED}: {e}"))?;
    let amount = extract_amount(&entry).unwrap_or_default();
    let decimals = resolver.resolve(&entry).await;
    let recipient = entry
        .get("payTo")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    let expires = accepts_val
        .iter()
        .find_map(|e| e.get("expires").and_then(|v| v.as_u64()))
        .unwrap_or(0);

    let known_scheme =
        |s: &str| matches!(s, "exact" | "aggr_deferred" | "charge" | "upto" | "period");
    let supported = accepts_val
        .iter()
        .filter_map(|e| e.get("scheme").and_then(|v| v.as_str()))
        .any(known_scheme);
    let unsupported_reason = if supported {
        None
    } else {
        Some("no supported payment scheme in accepts[]".to_string())
    };

    Ok(DecodedChallenge {
        amount: amount.clone(),
        amount_human: human_amount(&amount, decimals),
        decimals,
        recipient,
        expires,
        supported,
        unsupported_reason,
    })
}

async fn build_candidates(
    accepts_val: &[Value],
    accepts: &[AcceptEntry],
    resolver: &mut DecimalResolver,
) -> Result<Vec<Candidate>> {
    let mut out = Vec::with_capacity(accepts.len());
    for a in accepts {
        let entry = &accepts_val[a.index];
        let chain_id = a
            .network
            .strip_prefix("eip155:")
            .unwrap_or(&a.network)
            .to_string();
        let is_mainnet = payment_flow::is_mainnet_chain(&chain_id);
        let chain_name = crate::chains::chain_display_name(&chain_id).to_string();
        // Prefer the real okx-dex ticker (e.g. "USDT") so display is meaningful
        // and the symbol-fallback balance match stays reliable; only then the
        // challenge's own `extra.name` (which for some tokens is an EIP-712
        // domain name), and finally the raw contract address as a last resort.
        let token_symbol = resolver
            .resolve_symbol(entry)
            .await
            .or_else(|| {
                entry
                    .get("extra")
                    .and_then(|x| x.get("name"))
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string())
            })
            .unwrap_or_else(|| a.asset.clone());
        let decimals = resolver.resolve(entry).await;
        out.push(Candidate {
            scheme: a.scheme.clone(),
            accepts_index: a.index,
            chain_id,
            chain_name,
            is_mainnet,
            token_symbol,
            amount: a.amount.clone(),
            amount_human: human_amount(&a.amount, decimals),
            decimals,
            has_balance: false,
            balance_status: "unavailable".to_string(),
            available_amount: String::new(),
            required_amount: human_amount(&a.amount, decimals),
            shortfall: String::new(),
            deposit_address: String::new(),
            recommended: None,
        });
    }
    Ok(out)
}

// ── Wallet / balance preflight ──────────────────────────────────────────

/// Best-effort per-candidate balance check. Returns `Some(walletError)`:
/// `login_required` when no wallet is logged in, `balance_unavailable` when the
/// balance fetch fails. Never aborts the (read-only) quote.
///
/// A short-TTL balance snapshot cache is intentionally NOT implemented here.
/// The two-phase architecture collapses the old multi-step flow into a single
/// `payment quote`, which queries each (account, chainId) balance exactly once —
/// so that cache's original motive (reusing a snapshot across agent steps within one
/// operation) is already covered by the architecture. A *cross-quote* on-disk
/// cache would save a query only across separate `payment quote` invocations,
/// but `has_balance` is a fund-adjacent recommendation hint: a stale cached
/// "true" could auto-recommend a candidate the user can no longer afford. The
/// staleness risk on a fund path outweighs the marginal token/latency saving on
/// a hint that `pay`'s confirming gate + on-chain settle re-validate anyway, so
/// the snapshot cache is treated as covered-by-architecture rather than adding a
/// stale-prone balance cache.
async fn preflight_balances(
    candidates: &mut [Candidate],
    accepts: &[AcceptEntry],
) -> Option<String> {
    let wallets = match crate::wallet_store::load_wallets() {
        Ok(Some(w)) if !w.selected_account_id.is_empty() => w,
        _ => return Some("login_required".to_string()),
    };
    let Some(account) = wallets.accounts_map.get(&wallets.selected_account_id) else {
        return Some("login_required".to_string());
    };

    let mut client = match crate::client::ApiClient::new(None) {
        Ok(c) => c,
        Err(_) => return Some("balance_unavailable".to_string()),
    };

    let mut any_error = false;
    let chain_ids: BTreeSet<String> = candidates.iter().map(|c| c.chain_id.clone()).collect();
    for chain_id in chain_ids {
        let Some(addr) = account
            .address_list
            .iter()
            .find(|a| a.chain_index == chain_id)
            .map(|a| a.address.clone())
        else {
            any_error = true;
            continue;
        };
        for c in candidates.iter_mut().filter(|c| c.chain_id == chain_id) {
            c.deposit_address = addr.clone();
        }
        match crate::commands::portfolio::fetch_all_balances(
            &mut client,
            &addr,
            &chain_id,
            None,
            None,
        )
        .await
        {
            Ok(bal) => {
                for c in candidates.iter_mut().filter(|c| c.chain_id == chain_id) {
                    // Match wallet balance by the candidate's token contract
                    // address (`asset`) as the preferred key; the token symbol
                    // is only a fallback (see `json_has_positive_balance`).
                    let asset = accepts
                        .iter()
                        .find(|a| a.index == c.accepts_index)
                        .map(|a| a.asset.as_str())
                        .unwrap_or("");
                    match candidate_balance_atomic(&bal, &c.token_symbol, asset, c.decimals) {
                        Some(available) => {
                            let Ok(required) = U256::from_str_radix(&c.amount, 10) else {
                                any_error = true;
                                c.balance_status = "unavailable".to_string();
                                continue;
                            };
                            c.has_balance = available > U256::ZERO;
                            c.available_amount = human_amount(&available.to_string(), c.decimals);
                            c.required_amount = c.amount_human.clone();
                            if available >= required {
                                c.balance_status = "sufficient".to_string();
                                c.shortfall = "0".to_string();
                            } else {
                                c.balance_status = "insufficient".to_string();
                                c.shortfall =
                                    human_amount(&(required - available).to_string(), c.decimals);
                            }
                        }
                        None => {
                            any_error = true;
                            c.balance_status = "unavailable".to_string();
                        }
                    }
                }
            }
            Err(_) => any_error = true,
        }
    }
    if any_error {
        Some("balance_unavailable".to_string())
    } else {
        None
    }
}

/// Exact candidate balance in atomic units. Contract address is authoritative
/// whenever the response exposes one; symbol matching is a compatibility
/// fallback for older balance responses without token addresses.
fn candidate_balance_atomic(
    balances: &Value,
    symbol: &str,
    asset: &str,
    decimals: u32,
) -> Option<U256> {
    let by_address = !asset.is_empty() && balance_has_contract_addr_field(balances);
    let entry = find_balance_entry(balances, |o| {
        if by_address {
            balance_entry_addr(o).is_some_and(|a| a.eq_ignore_ascii_case(asset))
        } else {
            o.get("symbol")
                .and_then(Value::as_str)
                .is_some_and(|s| s.eq_ignore_ascii_case(symbol))
        }
    });
    match entry {
        Some(o) => balance_entry_atomic(o, decimals),
        None => Some(U256::ZERO),
    }
}

fn find_balance_entry<'a, F>(value: &'a Value, matches: F) -> Option<&'a Map<String, Value>>
where
    F: Fn(&Map<String, Value>) -> bool + Copy,
{
    match value {
        Value::Array(items) => items.iter().find_map(|v| find_balance_entry(v, matches)),
        Value::Object(object) => {
            if matches(object) {
                Some(object)
            } else {
                object.values().find_map(|v| find_balance_entry(v, matches))
            }
        }
        _ => None,
    }
}

fn balance_entry_atomic(entry: &Map<String, Value>, decimals: u32) -> Option<U256> {
    if let Some(raw) = entry
        .get("rawBalance")
        .or_else(|| entry.get("balanceRawAmount"))
        .and_then(Value::as_str)
    {
        if let Ok(value) = U256::from_str_radix(raw, 10) {
            return Some(value);
        }
    }
    entry
        .get("balance")
        .and_then(Value::as_str)
        .and_then(|human| human_to_atomic(human, decimals))
}

fn human_to_atomic(human: &str, decimals: u32) -> Option<U256> {
    let raw = human.trim();
    if raw.is_empty() || raw.starts_with('-') || raw.contains(['e', 'E']) {
        return None;
    }
    let (whole, fraction) = raw.split_once('.').unwrap_or((raw, ""));
    if !whole.chars().all(|c| c.is_ascii_digit())
        || !fraction.chars().all(|c| c.is_ascii_digit())
        || fraction.len() > decimals as usize
    {
        return None;
    }
    let mut digits = whole.to_string();
    digits.push_str(fraction);
    digits.extend(std::iter::repeat('0').take(decimals as usize - fraction.len()));
    let normalized = digits.trim_start_matches('0');
    U256::from_str_radix(
        if normalized.is_empty() {
            "0"
        } else {
            normalized
        },
        10,
    )
    .ok()
}

/// Is `s` a positive numeric balance string?
fn balance_is_positive(s: &str) -> bool {
    s.chars().any(|c| c.is_ascii_digit() && c != '0') || s.parse::<f64>().is_ok_and(|f| f > 0.0)
}

/// The token contract address a balance entry declares — the OKX field name
/// (`tokenContractAddress`) plus tolerant aliases.
fn balance_entry_addr(o: &Map<String, Value>) -> Option<&str> {
    o.get("tokenContractAddress")
        .or_else(|| o.get("tokenAddress"))
        .or_else(|| o.get("contractAddress"))
        .and_then(|v| v.as_str())
}

/// Does a balance entry object carry a positive `balance` amount?
fn balance_entry_positive_amount(o: &Map<String, Value>) -> bool {
    o.get("balance")
        .or_else(|| o.get("balanceRawAmount"))
        .or_else(|| o.get("rawBalance"))
        .and_then(|v| v.as_str())
        .is_some_and(balance_is_positive)
}

/// Does any object in the tree expose a token contract-address field? When the
/// response carries addresses, address matching is authoritative; when it does
/// not, we fall back to symbol matching.
fn balance_has_contract_addr_field(v: &Value) -> bool {
    match v {
        Value::Array(a) => a.iter().any(balance_has_contract_addr_field),
        Value::Object(o) => {
            balance_entry_addr(o).is_some() || o.values().any(balance_has_contract_addr_field)
        }
        _ => false,
    }
}

/// Positive balance for an entry whose contract address == `asset`
/// (case-insensitive). Authoritative match key.
fn balance_addr_positive(v: &Value, asset: &str) -> bool {
    match v {
        Value::Array(a) => a.iter().any(|e| balance_addr_positive(e, asset)),
        Value::Object(o) => {
            let hit = balance_entry_addr(o).is_some_and(|a| a.eq_ignore_ascii_case(asset))
                && balance_entry_positive_amount(o);
            hit || o.values().any(|e| balance_addr_positive(e, asset))
        }
        _ => false,
    }
}

/// Positive balance for an entry whose `symbol` == `symbol` (case-insensitive).
/// Fallback match key used only when the response has no contract addresses.
fn balance_symbol_positive(v: &Value, symbol: &str) -> bool {
    match v {
        Value::Array(a) => a.iter().any(|e| balance_symbol_positive(e, symbol)),
        Value::Object(o) => {
            let hit = o
                .get("symbol")
                .and_then(|v| v.as_str())
                .is_some_and(|s| s.eq_ignore_ascii_case(symbol))
                && balance_entry_positive_amount(o);
            hit || o.values().any(|e| balance_symbol_positive(e, symbol))
        }
        _ => false,
    }
}

/// Heuristic scan of an OKX `all-token-balances-by-address` response for a
/// positive balance of a given token.
///
/// Matching is **by contract address first** (`asset`, case-insensitive): OKX
/// balance entries carry a `tokenContractAddress`, and matching on it is the
/// only reliable key. Symbol matching (case-insensitive) is a fallback used
/// only when the response exposes no contract-address field at all — the base
/// MR!144 bug matched purely by symbol, which fails whenever the accepts'
/// derived `token_symbol` is an EIP-712 domain name / contract address that
/// never equals the wallet balance's `symbol` (e.g. X Layer `USD₮0` vs the
/// wallet's `USDT`). The safe fallback is `false` (→ the ranker asks the user
/// rather than auto-picking).
fn json_has_positive_balance(balances: &Value, symbol: &str, asset: &str) -> bool {
    if !asset.is_empty() && balance_has_contract_addr_field(balances) {
        return balance_addr_positive(balances, asset);
    }
    balance_symbol_positive(balances, symbol)
}

// ── Summary / missing params / id / amount helpers ──────────────────────

fn build_summary(
    candidates: &[Candidate],
    _alternatives: &[Candidate],
    challenge: &DecodedChallenge,
) -> String {
    if let Some(pick) = candidates
        .iter()
        .find(|c| c.recommended == Some(true))
        .or_else(|| candidates.first())
    {
        // `upto` authorizes a spend cap, not a fixed charge — say "up to" so
        // the buyer never reads the amount as a guaranteed deduction (the
        // WWW-Authenticate path already distinguishes this with "per request").
        let verb = if pick.scheme.eq_ignore_ascii_case("upto") {
            "Will pay up to"
        } else {
            "Will pay"
        };
        format!(
            "{} {} {} ({}, {})",
            verb, pick.amount_human, pick.token_symbol, pick.scheme, pick.chain_name
        )
    } else {
        format!("Will pay {}", challenge.amount_human)
    }
}

/// Params the merchant requires but the caller did not supply. Two sources:
/// - Source 1 — the parsed `outputSchema.input` plan: every `required` param
///   absent from `known_params`;
/// - Source 2 — the merchant body's flat `missingParams` / `required` array.
///
/// The two are unioned (plan first), de-duplicated, and filtered to what the
/// caller has not already provided.
fn missing_params(
    merchant_body: &str,
    known_params: &Map<String, Value>,
    plan: &[ParamSpec],
) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let push_unique = |k: &str, out: &mut Vec<String>| {
        if !known_params.contains_key(k) && !out.iter().any(|e| e == k) {
            out.push(k.to_string());
        }
    };

    // Source 1: outputSchema.input required params.
    for spec in plan.iter().filter(|s| s.required) {
        push_unique(&spec.name, &mut out);
    }

    // Source 2: flat missingParams / required list on the merchant body.
    if let Ok(v) = serde_json::from_str::<Value>(merchant_body) {
        if let Some(list) = v
            .get("missingParams")
            .or_else(|| v.get("required"))
            .and_then(|v| v.as_array())
        {
            for k in list.iter().filter_map(|e| e.as_str()) {
                push_unique(k, &mut out);
            }
        }
    }
    out
}

/// Locate the merchant's `outputSchema` (Source 1 param descriptor). Prefers the
/// decoded challenge, then the merchant response body.
fn find_output_schema(decoded: &Value, merchant_body: &str) -> Option<Value> {
    if let Some(s) = decoded.get("outputSchema") {
        if !s.is_null() {
            return Some(s.clone());
        }
    }
    serde_json::from_str::<Value>(merchant_body)
        .ok()
        .and_then(|v| v.get("outputSchema").cloned())
        .filter(|s| !s.is_null())
}

/// Map an `outputSchema.input` carrier string to [`ParamCarrier`]. Unknown /
/// absent carriers default to `query` (the pre-carrier behavior).
fn parse_carrier(s: &str) -> ParamCarrier {
    match s.to_ascii_lowercase().as_str() {
        "body" => ParamCarrier::Body,
        "header" => ParamCarrier::Header,
        "path" => ParamCarrier::Path,
        _ => ParamCarrier::Query,
    }
}

/// Build a [`ParamSpec`] from a single `outputSchema.input` entry.
fn param_spec_from(name: &str, spec: &Value) -> ParamSpec {
    ParamSpec {
        name: name.to_string(),
        carrier: spec
            .get("carrier")
            .and_then(|v| v.as_str())
            .map(parse_carrier)
            .unwrap_or_default(),
        required: spec
            .get("required")
            .and_then(|v| v.as_bool())
            .unwrap_or(false),
        type_: spec
            .get("type")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string(),
    }
}

/// Parse `outputSchema.input` into a per-param plan. Accepts either the object
/// map form (`{name: {carrier, required, type}}`) or an array of objects each
/// carrying a `name` field.
fn parse_param_plan(schema_input: &Value) -> Vec<ParamSpec> {
    match schema_input {
        Value::Object(map) => map
            .iter()
            .map(|(name, spec)| param_spec_from(name, spec))
            .collect(),
        Value::Array(arr) => arr
            .iter()
            .filter_map(|spec| {
                spec.get("name")
                    .and_then(|v| v.as_str())
                    .map(|name| param_spec_from(name, spec))
            })
            .collect(),
        _ => vec![],
    }
}

fn now_unix() -> u64 {
    chrono::Utc::now().timestamp().max(0) as u64
}

/// Derive a non-secret, opaque paymentId from the endpoint + a high-resolution
/// timestamp. Not a credential — just a state-file handle.
fn new_payment_id(url: &str, created_at: u64) -> String {
    use sha2::{Digest, Sha256};
    let nanos = chrono::Utc::now()
        .timestamp_nanos_opt()
        .unwrap_or(created_at as i64);
    let mut hasher = Sha256::new();
    hasher.update(url.as_bytes());
    hasher.update(created_at.to_le_bytes());
    hasher.update(nanos.to_le_bytes());
    let digest = hasher.finalize();
    let hex: String = digest.iter().take(12).map(|b| format!("{b:02x}")).collect();
    format!("pay_{hex}")
}

/// String-based atomic→human conversion (no float rounding, per NFR §2.14).
fn human_amount(atomic: &str, decimals: u32) -> String {
    let digits: String = atomic.chars().filter(|c| c.is_ascii_digit()).collect();
    if digits.is_empty() {
        return "0".to_string();
    }
    let d = decimals as usize;
    if d == 0 {
        let trimmed = digits.trim_start_matches('0');
        return if trimmed.is_empty() {
            "0".into()
        } else {
            trimmed.to_string()
        };
    }
    let padded = format!("{digits:0>width$}", width = d + 1);
    let split = padded.len() - d;
    let int_part = padded[..split].trim_start_matches('0');
    let int_part = if int_part.is_empty() { "0" } else { int_part };
    let frac = padded[split..].trim_end_matches('0');
    if frac.is_empty() {
        int_part.to_string()
    } else {
        format!("{int_part}.{frac}")
    }
}

fn free_challenge() -> DecodedChallenge {
    DecodedChallenge {
        amount: "0".into(),
        amount_human: "0".into(),
        decimals: 0,
        recipient: String::new(),
        expires: 0,
        supported: true,
        unsupported_reason: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_params_builds_object() {
        let out = parse_params(&["orderId=42".into(), "note=hi there".into()]).unwrap();
        assert_eq!(out.get("orderId").unwrap(), "42");
        assert_eq!(out.get("note").unwrap(), "hi there");
    }

    #[test]
    fn classify_probe_error_subdivides_status() {
        assert_eq!(classify_probe_error(401), TOKEN_AUTH_REQUIRED);
        assert_eq!(classify_probe_error(403), TOKEN_AUTH_REQUIRED);
        assert_eq!(classify_probe_error(500), TOKEN_ENDPOINT_SERVER_ERROR);
        assert_eq!(classify_probe_error(503), TOKEN_ENDPOINT_SERVER_ERROR);
        // Other 4xx and anything unclassified stay endpoint_unreachable.
        assert_eq!(classify_probe_error(404), TOKEN_ENDPOINT_UNREACHABLE);
        assert_eq!(classify_probe_error(418), TOKEN_ENDPOINT_UNREACHABLE);
    }

    #[test]
    fn parse_params_rejects_malformed() {
        let err = parse_params(&["noequals".into()]).unwrap_err();
        assert!(err.to_string().starts_with(TOKEN_INVALID_INPUT));
        let err2 = parse_params(&["=v".into()]).unwrap_err();
        assert!(err2.to_string().starts_with(TOKEN_INVALID_INPUT));
    }

    #[test]
    fn build_summary_marks_upto_as_a_cap() {
        let cand = |scheme: &str| Candidate {
            scheme: scheme.into(),
            accepts_index: 0,
            chain_id: "196".into(),
            chain_name: "X Layer".into(),
            is_mainnet: true,
            token_symbol: "USDC".into(),
            amount: "50000".into(),
            amount_human: "0.05".into(),
            decimals: 6,
            has_balance: true,
            balance_status: "sufficient".into(),
            available_amount: "1".into(),
            required_amount: "0.05".into(),
            shortfall: "0".into(),
            deposit_address: "0xwallet".into(),
            recommended: Some(true),
        };
        // `upto` is a spend cap → "up to" wording so 0.05 isn't read as fixed.
        assert_eq!(
            build_summary(&[cand("upto")], &[], &free_challenge()),
            "Will pay up to 0.05 USDC (upto, X Layer)"
        );
        // Fixed-charge schemes keep the plain wording.
        assert_eq!(
            build_summary(&[cand("exact")], &[], &free_challenge()),
            "Will pay 0.05 USDC (exact, X Layer)"
        );
    }

    #[test]
    fn human_amount_no_rounding() {
        assert_eq!(human_amount("10000", 6), "0.01");
        assert_eq!(human_amount("1000000", 6), "1");
        assert_eq!(human_amount("1234567", 6), "1.234567");
        assert_eq!(human_amount("0", 6), "0");
        assert_eq!(human_amount("500", 0), "500");
    }

    #[test]
    fn candidate_balance_compares_exact_atomic_amount() {
        let bal = serde_json::json!({
            "data": [{ "tokenAssets": [{
                "symbol": "USDT",
                "tokenContractAddress": "0xaaa",
                "balance": "0.009999",
                "rawBalance": "9999"
            }]}]
        });
        assert_eq!(
            candidate_balance_atomic(&bal, "USDT", "0xAAA", 6),
            Some(U256::from(9_999u64))
        );
        assert!(
            candidate_balance_atomic(&bal, "USDT", "0xAAA", 6).unwrap() < U256::from(10_000u64)
        );
    }

    #[test]
    fn candidate_balance_zero_when_matching_token_is_absent() {
        let bal = serde_json::json!({
            "data": [{ "tokenAssets": [{
                "symbol": "USDT",
                "tokenContractAddress": "0xaaa",
                "rawBalance": "50000"
            }]}]
        });
        assert_eq!(
            candidate_balance_atomic(&bal, "USDC", "0xbbb", 6),
            Some(U256::ZERO)
        );
    }

    #[test]
    fn human_balance_conversion_rejects_precision_loss() {
        assert_eq!(
            human_to_atomic("1.234567", 6),
            Some(U256::from(1_234_567u64))
        );
        assert_eq!(human_to_atomic("0.01", 6), Some(U256::from(10_000u64)));
        assert_eq!(human_to_atomic("0.0000001", 6), None);
        assert_eq!(human_to_atomic("1e-2", 6), None);
    }

    #[test]
    fn balance_scan_matches_symbol() {
        // No contract-address field in the response → symbol fallback path.
        let bal = serde_json::json!({
            "data": [{ "tokenAssets": [
                { "symbol": "USDC", "balance": "12.5" },
                { "symbol": "ETH", "balance": "0" }
            ]}]
        });
        assert!(json_has_positive_balance(&bal, "usdc", ""));
        assert!(!json_has_positive_balance(&bal, "eth", ""));
        assert!(!json_has_positive_balance(&bal, "dai", ""));
    }

    #[test]
    fn balance_scan_matches_by_contract_address_when_symbol_differs() {
        // The regression this fix targets: the accepts' derived token_symbol is
        // an EIP-712 domain / contract address (never equal to the wallet's
        // "USDT"), so the OLD symbol-only match returned false despite a real
        // balance. Address matching on `asset` recovers hasBalance:true.
        let bal = serde_json::json!({
            "data": [{ "tokenAssets": [
                { "symbol": "USDT", "tokenContractAddress": "0x779dEd0c9e1022225f8E0630b35a9b54bE713736", "balance": "5.0" },
                { "symbol": "OKB", "tokenContractAddress": "0xabc", "balance": "1.0" }
            ]}]
        });
        // token_symbol is the domain-y "USD₮0" — symbol match would miss — but
        // the asset address matches (case-insensitively).
        assert!(json_has_positive_balance(
            &bal,
            "USD₮0",
            "0x779ded0c9e1022225f8e0630b35a9b54be713736"
        ));
        // A token the wallet does not hold (address absent) → false, and does
        // NOT fall back to symbol (address is authoritative once present).
        assert!(!json_has_positive_balance(&bal, "USDT", "0xdeadbeef"));
    }

    #[test]
    fn balance_scan_address_match_is_authoritative_on_zero() {
        // Matching address with a zero balance → false, even if some other
        // entry shares the fallback symbol.
        let bal = serde_json::json!({
            "data": [{ "tokenAssets": [
                { "symbol": "USDT", "tokenContractAddress": "0xaaa", "balance": "0" },
                { "symbol": "USDT", "tokenContractAddress": "0xbbb", "balance": "9" }
            ]}]
        });
        assert!(!json_has_positive_balance(&bal, "USDT", "0xaaa"));
        assert!(json_has_positive_balance(&bal, "USDT", "0xbbb"));
    }

    #[test]
    fn missing_params_reads_explicit_list() {
        let body = r#"{"missingParams":["orderId","email"]}"#;
        let known = parse_params(&["email=a@b.c".into()]).unwrap();
        assert_eq!(
            missing_params(body, &known, &[]),
            vec!["orderId".to_string()]
        );
        assert!(missing_params("not json", &known, &[]).is_empty());
    }

    #[test]
    fn missing_params_unions_plan_required_and_flat_list() {
        // Plan requires orderId (missing) + email (supplied); flat list adds note.
        let plan = vec![
            ParamSpec {
                name: "orderId".into(),
                carrier: ParamCarrier::Query,
                required: true,
                type_: "string".into(),
            },
            ParamSpec {
                name: "email".into(),
                carrier: ParamCarrier::Body,
                required: true,
                type_: String::new(),
            },
        ];
        let body = r#"{"required":["note","orderId"]}"#;
        let known = parse_params(&["email=a@b.c".into()]).unwrap();
        // orderId from plan (missing), note from flat list; email supplied so
        // excluded; orderId not duplicated across the two sources.
        assert_eq!(
            missing_params(body, &known, &plan),
            vec!["orderId".to_string(), "note".to_string()]
        );
    }

    #[test]
    fn parse_param_plan_object_and_array_forms() {
        // Object-map form.
        let obj = serde_json::json!({
            "orderId": {"carrier": "path", "required": true, "type": "string"},
            "sig": {"carrier": "header"}
        });
        let plan = parse_param_plan(&obj);
        let order = plan.iter().find(|s| s.name == "orderId").unwrap();
        assert_eq!(order.carrier, ParamCarrier::Path);
        assert!(order.required);
        assert_eq!(order.type_, "string");
        let sig = plan.iter().find(|s| s.name == "sig").unwrap();
        assert_eq!(sig.carrier, ParamCarrier::Header);
        assert!(!sig.required);
        // Array-of-objects form.
        let arr = serde_json::json!([{"name": "q", "carrier": "query", "required": false}]);
        let plan2 = parse_param_plan(&arr);
        assert_eq!(plan2.len(), 1);
        assert_eq!(plan2[0].name, "q");
        assert_eq!(plan2[0].carrier, ParamCarrier::Query);
    }

    #[test]
    fn find_output_schema_prefers_challenge_then_body() {
        let decoded = serde_json::json!({"outputSchema": {"method": "POST"}});
        assert_eq!(
            find_output_schema(&decoded, "{}")
                .and_then(|s| s.get("method").and_then(|v| v.as_str()).map(str::to_string)),
            Some("POST".to_string())
        );
        // Falls back to the merchant body when the challenge lacks it.
        let body = r#"{"outputSchema":{"method":"PUT"}}"#;
        assert_eq!(
            find_output_schema(&serde_json::json!({}), body)
                .and_then(|s| s.get("method").and_then(|v| v.as_str()).map(str::to_string)),
            Some("PUT".to_string())
        );
        assert!(find_output_schema(&serde_json::json!({}), "no schema here").is_none());
    }

    #[test]
    fn payment_id_is_opaque_and_prefixed() {
        let id = new_payment_id("https://m.example/x", 1000);
        assert!(id.starts_with("pay_"));
        assert_eq!(id.len(), 4 + 24);
    }

    /// A helper mirroring how discovery/free build the unified envelope, used to
    /// assert the serialized MCP-output contract without a live endpoint.
    fn envelope(mcp_tools: Vec<McpTool>, mcp_result: Option<Value>, summary: &str) -> Value {
        let data = QuoteData {
            payment_id: String::new(),
            needs_confirm: false,
            summary: summary.to_string(),
            next_step: String::new(),
            accepts: vec![],
            known_params: Map::new(),
            merchant_body: String::new(),
            missing_params: vec![],
            param_plan: vec![],
            candidates: vec![],
            alternatives: vec![],
            decoded_challenge: free_challenge(),
            wallet_error: None,
            mcp_tools,
            mcp_result,
        };
        serde_json::to_value(data).unwrap()
    }

    #[test]
    fn discovery_envelope_is_isomorphic_and_omits_payment_id() {
        // Discovery: unified QuoteData with mcpTools[], needsConfirm:false, and
        // NO paymentId (null via skip-when-empty, preserving AC-2).
        let tools = vec![McpTool {
            name: "premium".into(),
            description: None,
            input_schema: None,
        }];
        let v = envelope(tools, None, "MCP server exposes 1 tool(s): premium");
        assert!(
            v.get("paymentId").is_none(),
            "discovery must not carry paymentId (jq .data.paymentId == null): {v}"
        );
        assert_eq!(v["needsConfirm"], serde_json::json!(false));
        assert_eq!(v["mcpTools"][0]["name"], serde_json::json!("premium"));
        // The free-only `result` field is omitted in discovery.
        assert!(v.get("result").is_none());
    }

    #[test]
    fn free_tool_envelope_carries_result_without_payment_id() {
        // Free tool: unified QuoteData with the tool result in `result`,
        // needsConfirm:false, and no paymentId / no mcpTools.
        let result = serde_json::json!({"content": [{"type": "text", "text": "ok"}]});
        let v = envelope(
            vec![],
            Some(result.clone()),
            "MCP tool 'free_tool' returned a result — no payment required",
        );
        assert!(v.get("paymentId").is_none());
        assert_eq!(v["needsConfirm"], serde_json::json!(false));
        assert_eq!(v["result"], result);
        // The discovery-only `mcpTools` field is omitted (empty) for a free tool.
        assert!(v.get("mcpTools").is_none());
    }

    #[test]
    fn declared_decimals_reads_numeric_and_string_forms() {
        // extra.decimals numeric.
        assert_eq!(
            declared_decimals(&serde_json::json!({"extra": {"decimals": 18}})),
            Some(18)
        );
        // top-level decimals as a string.
        assert_eq!(
            declared_decimals(&serde_json::json!({"decimals": "8"})),
            Some(8)
        );
        // extra wins over top-level.
        assert_eq!(
            declared_decimals(&serde_json::json!({"extra": {"decimals": 9}, "decimals": 6})),
            Some(9)
        );
        // absent → None (caller falls back to okx-dex, then DEFAULT_DECIMALS).
        assert_eq!(
            declared_decimals(&serde_json::json!({"asset": "0xabc"})),
            None
        );
    }

    #[test]
    fn entry_asset_and_chain_strips_eip155_prefix() {
        let entry = serde_json::json!({"asset": "0xUSDC", "network": "eip155:8453"});
        assert_eq!(
            entry_asset_and_chain(&entry),
            Some(("8453".to_string(), "0xUSDC".to_string()))
        );
        // No asset → None (cannot query okx-dex).
        assert_eq!(
            entry_asset_and_chain(&serde_json::json!({"network": "eip155:1"})),
            None
        );
    }

    #[tokio::test]
    async fn resolver_declared_decimals_never_touch_okx_dex_memo() {
        // An inline-declared entry must resolve from `extra.decimals` alone,
        // without recording a (chain,address) memo entry (no okx-dex lookup).
        let mut resolver = DecimalResolver {
            client: None,
            memo: HashMap::new(),
        };
        let entry = serde_json::json!({
            "asset": "0xUSDC", "network": "eip155:8453", "extra": {"decimals": 18}
        });
        assert_eq!(resolver.resolve(&entry).await, 18);
        assert!(
            resolver.memo.is_empty(),
            "declared decimals must not memoize"
        );
    }

    #[tokio::test]
    async fn resolver_negative_caches_missed_lookup_once() {
        // With no okx-dex client, a token that declares no inline decimals falls
        // back to DEFAULT_DECIMALS — and the (chain,address) miss is memoized as
        // `None` so a second candidate for the same token does not re-attempt
        // the lookup (the redundant-request guard).
        let mut resolver = DecimalResolver {
            client: None,
            memo: HashMap::new(),
        };
        let entry = serde_json::json!({"asset": "0xNODECIMALS", "network": "eip155:8453"});
        assert_eq!(resolver.resolve(&entry).await, DEFAULT_DECIMALS);
        assert_eq!(
            resolver
                .memo
                .get(&("8453".to_string(), "0xNODECIMALS".to_string())),
            Some(&TokenMeta::default()),
            "a missed lookup must be negatively cached"
        );
        // Second resolve for the same token still returns the default and leaves
        // exactly one memo entry — no duplicate (chain,address) key.
        assert_eq!(resolver.resolve(&entry).await, DEFAULT_DECIMALS);
        assert_eq!(resolver.memo.len(), 1);
    }
}
