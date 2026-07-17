//! `payment quote` — probe an HTTP 402 / A2MCP endpoint, parse the payment
//! challenge, run a wallet/balance preflight, rank candidates, and persist a
//! `paymentId` for a later `payment pay --payment-id`. Never signs.
//!
//! The heavy mechanical work the agent used to do by hand (curl, base64 decode,
//! `accepts` parse, amount conversion, balance filter, recommendation ranking)
//! all lives here so the agent collapses to a 2-round playbook.

use std::collections::{BTreeSet, HashMap};
use std::time::Duration;

use anyhow::{anyhow, Result};
use serde::Serialize;
use serde_json::{Map, Value};

use super::payment_flow::{self, extract_amount};
use super::state::{
    self, AcceptEntry, Candidate, DecodedChallenge, ParamCarrier, ParamSpec, PaymentState,
};
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
    #[serde(rename = "paymentId")]
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
}

/// CLI handler: run the quote and print the always-on envelope. Classified
/// probe/parse failures propagate as `Err` so `main.rs` renders `output::error`
/// (exit 1); `walletError` / all-zero-balance are `Ok` data (exit 0).
pub async fn run(url: &str, param: &[String], method: &str) -> Result<()> {
    let data = fetch_quote(url, param, method).await?;
    output::success(data);
    Ok(())
}

/// Data path shared by the CLI handler and the `payment_quote` MCP tool.
/// Returns the `data` payload (`QuoteData` serialized to `Value`).
pub async fn fetch_quote(url: &str, param: &[String], method: &str) -> Result<Value> {
    let known_params = parse_params(param)?;

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
            };
            return serde_json::to_value(data).map_err(Into::into);
        }
        ProbeOutcome::Challenge { header, body } => (header, body),
    };

    // Decode the challenge blob (reuses the shared base64 / WWW-Authenticate
    // decoder) and pull the accepts[] array.
    let decoded = super::dispatcher::decode_payment_blob(&challenge_header)
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
    let wallet_error = preflight_balances(&mut candidates).await;

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
    let body = resp.text().await.unwrap_or_default();

    if status.as_u16() == 402 {
        // Some servers put the challenge in the body rather than a header.
        let header = header.unwrap_or_else(|| body.clone());
        Ok(ProbeOutcome::Challenge { header, body })
    } else if status.is_success() {
        Ok(ProbeOutcome::NoCharge { body })
    } else {
        let code = status.as_u16();
        Err(anyhow!(
            "{}: unexpected HTTP {code} (expected 402 or 200)",
            classify_probe_error(code)
        ))
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

/// Query the okx-dex token basic-info endpoint for a token's decimals by
/// (chainIndex, contractAddress). Best-effort — returns `None` on any transport
/// or shape failure so the caller can fall back without aborting the quote.
async fn fetch_decimals_from_okx_dex(
    client: &mut crate::client::ApiClient,
    chain_id: &str,
    address: &str,
) -> Option<u32> {
    let resp = crate::commands::token::fetch_info(client, address, chain_id)
        .await
        .ok()?;
    let item = resp.as_array().and_then(|a| a.first())?;
    // basic-info carries decimals in the `decimal` (string) field; accept a
    // numeric `decimals` too for forward-compatibility.
    item.get("decimal")
        .and_then(|d| d.as_str())
        .and_then(|s| s.parse::<u32>().ok())
        .or_else(|| {
            item.get("decimals")
                .and_then(|d| d.as_u64())
                .map(|n| n as u32)
        })
}

/// Resolves token decimals for accepts entries, preferring the entry's declared
/// value, then an okx-dex metadata lookup, and only then [`DEFAULT_DECIMALS`]
/// (never silently default when metadata is reachable). Memoizes
/// okx-dex lookups by (chainIndex, address) — including best-effort misses — so
/// a multi-scheme challenge for one token queries okx-dex at most once, even
/// when the lookup fails and the caller falls back to the default.
struct DecimalResolver {
    client: Option<crate::client::ApiClient>,
    /// (chainIndex, address) → resolved decimals, or `None` when the okx-dex
    /// lookup was already attempted and yielded nothing. Caching the miss
    /// (negative cache) is what prevents a second same-token candidate from
    /// re-hitting basic-info after the first attempt failed.
    memo: HashMap<(String, String), Option<u32>>,
}

impl DecimalResolver {
    fn new() -> Self {
        Self {
            client: crate::client::ApiClient::new(None).ok(),
            memo: HashMap::new(),
        }
    }

    async fn resolve(&mut self, entry: &Value) -> u32 {
        if let Some(d) = declared_decimals(entry) {
            return d;
        }
        if let Some((chain_id, address)) = entry_asset_and_chain(entry) {
            let key = (chain_id.clone(), address.clone());
            if !self.memo.contains_key(&key) {
                let resolved = match self.client.as_mut() {
                    Some(client) => fetch_decimals_from_okx_dex(client, &chain_id, &address).await,
                    None => None,
                };
                self.memo.insert(key.clone(), resolved);
            }
            if let Some(&Some(d)) = self.memo.get(&key) {
                return d;
            }
        }
        DEFAULT_DECIMALS
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
        let token_symbol = entry
            .get("extra")
            .and_then(|x| x.get("name"))
            .and_then(|v| v.as_str())
            .map(|s| s.to_string())
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
            has_balance: false,
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
async fn preflight_balances(candidates: &mut [Candidate]) -> Option<String> {
    let wallets = match crate::wallet_store::load_wallets() {
        Ok(Some(w)) if !w.selected_account_id.is_empty() => w,
        _ => return Some("login_required".to_string()),
    };
    let account = wallets.accounts_map.get(&wallets.selected_account_id)?;

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
            continue;
        };
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
                    c.has_balance = json_has_positive_balance(&bal, &c.token_symbol);
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

/// Heuristic scan of an OKX `all-token-balances-by-address` response for a
/// positive balance whose symbol matches `symbol` (case-insensitive). The safe
/// fallback is `false` (→ the ranker asks the user rather than auto-picking).
fn json_has_positive_balance(balances: &Value, symbol: &str) -> bool {
    fn positive(s: &str) -> bool {
        s.chars().any(|c| c.is_ascii_digit() && c != '0') || s.parse::<f64>().is_ok_and(|f| f > 0.0)
    }
    fn walk(v: &Value, symbol: &str) -> bool {
        match v {
            Value::Array(a) => a.iter().any(|e| walk(e, symbol)),
            Value::Object(o) => {
                let sym_match = o
                    .get("symbol")
                    .and_then(|v| v.as_str())
                    .is_some_and(|s| s.eq_ignore_ascii_case(symbol));
                if sym_match {
                    let bal = o
                        .get("balance")
                        .or_else(|| o.get("balanceRawAmount"))
                        .or_else(|| o.get("rawBalance"));
                    if let Some(b) = bal.and_then(|v| v.as_str()) {
                        if positive(b) {
                            return true;
                        }
                    }
                }
                o.values().any(|e| walk(e, symbol))
            }
            _ => false,
        }
    }
    walk(balances, symbol)
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
        format!(
            "Will pay {} {} ({}, {})",
            pick.amount_human, pick.token_symbol, pick.scheme, pick.chain_name
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
    fn human_amount_no_rounding() {
        assert_eq!(human_amount("10000", 6), "0.01");
        assert_eq!(human_amount("1000000", 6), "1");
        assert_eq!(human_amount("1234567", 6), "1.234567");
        assert_eq!(human_amount("0", 6), "0");
        assert_eq!(human_amount("500", 0), "500");
    }

    #[test]
    fn balance_scan_matches_symbol() {
        let bal = serde_json::json!({
            "data": [{ "tokenAssets": [
                { "symbol": "USDC", "balance": "12.5" },
                { "symbol": "ETH", "balance": "0" }
            ]}]
        });
        assert!(json_has_positive_balance(&bal, "usdc"));
        assert!(!json_has_positive_balance(&bal, "eth"));
        assert!(!json_has_positive_balance(&bal, "dai"));
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
            Some(&None),
            "a missed lookup must be negatively cached"
        );
        // Second resolve for the same token still returns the default and leaves
        // exactly one memo entry — no duplicate (chain,address) key.
        assert_eq!(resolver.resolve(&entry).await, DEFAULT_DECIMALS);
        assert_eq!(resolver.memo.len(), 1);
    }
}
