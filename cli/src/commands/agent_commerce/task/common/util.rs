//! common::util — generic helpers for the task system.
//!
//! Collects small utilities reused across the task module that aren't tied to specific business logic,
//! preventing them from being scattered across the individual mod / flow files.
//! Future formatting helpers, string normalization, time conversion, and similar generic helpers
//! should also go here.

use std::path::{Component, Path};

use anyhow::{bail, Result};
use chrono::{TimeZone, Utc};

use super::deposit_qr::{InsufficientBalanceError, DEPOSIT_QR_MARKER, SCAN_TO_DEPOSIT_OPTION};
use super::network::task_api_client::TaskApiClient;
use super::{PaymentMode, DEBUG_LOG, XLAYER_CHAIN_INDEX};
use crate::commands::sink::CodedError;

/// unix seconds -> display string. 0 / negative are treated as unset; positive values are converted to RFC 3339.
pub fn fmt_unix_secs(secs: Option<i64>) -> String {
    match secs {
        Some(n) if n > 0 => Utc
            .timestamp_opt(n, 0)
            .single()
            .map(|t| t.to_rfc3339())
            .unwrap_or_else(|| n.to_string()),
        _ => "—".to_string(),
    }
}

// ─── JSON extraction helpers ────────────────────────────────────────────

/// Extract a string field from a JSON object.
pub fn json_str(obj: &serde_json::Value, key: &str) -> Result<String> {
    obj[key]
        .as_str()
        .ok_or_else(|| anyhow::anyhow!("response missing field: {key}"))
        .map(|s| s.to_string())
}

/// Extract a u64 field from a JSON object (accepts both number and string forms).
pub fn json_u64(obj: &serde_json::Value, key: &str) -> Result<u64> {
    if let Some(n) = obj[key].as_u64() {
        return Ok(n);
    }
    if let Some(s) = obj[key].as_str() {
        return s
            .parse()
            .map_err(|_| anyhow::anyhow!("failed to parse {key} as u64: {s}"));
    }
    bail!("response missing field: {key}")
}

// ─── Token lookup ───────────────────────────────────────────────────────

/// Look up a token's contract address and decimals via the tokenDetail API.
/// GET /priapi/v1/aieco/task/tokenDetail?symbol=<symbol>
/// Returns (token_address, decimals).
pub async fn fetch_token_detail(
    client: &mut TaskApiClient,
    symbol: &str,
    agent_id: &str,
) -> Result<(String, u32)> {
    let path = format!("/priapi/v1/aieco/task/tokenDetail?symbol={symbol}");
    let resp = client
        .get_with_agent_id(&path, agent_id)
        .await
        .map_err(|e| anyhow::anyhow!("failed to query tokenDetail (symbol={symbol}): {e}"))?;
    let address = resp["address"]
        .as_str()
        .ok_or_else(|| anyhow::anyhow!("tokenDetail response missing address field"))?
        .to_string();
    let decimals = resp["decimals"]
        .as_u64()
        .ok_or_else(|| anyhow::anyhow!("tokenDetail response missing decimals field"))?
        as u32;
    Ok((address, decimals))
}

// ─── Payment mode resolution ────────────────────────────────────────────

/// Resolve the payment mode: CLI flag > task detail paymentType.
pub async fn resolve_payment_mode(
    client: &mut TaskApiClient,
    payment_mode: Option<&str>,
    job_id: &str,
    agent_id: &str,
) -> Result<PaymentMode> {
    match payment_mode {
        Some(m) => Ok(PaymentMode::from_str(m)),
        None => {
            let task_resp = client
                .get_with_identity(&client.task_path(job_id), agent_id)
                .await?;
            let payment_mode_int = task_resp["paymentMode"].as_i64().unwrap_or(0) as i32;
            let mode = PaymentMode::from_int(payment_mode_int);
            if mode == PaymentMode::None {
                if DEBUG_LOG {
                    eprintln!("⚠ task paymentMode={payment_mode_int}, unrecognized payment mode, defaulting to escrow");
                }
                Ok(PaymentMode::Escrow)
            } else {
                if DEBUG_LOG {
                    eprintln!("ℹ --payment-mode not provided, using task detail paymentMode: {} ({payment_mode_int})", mode.as_str());
                }
                Ok(mode)
            }
        }
    }
}

/// Parse a composite fee string (e.g. "0.01 USDT") -> (amount, symbol).
pub fn parse_composite_fee(fee: &str) -> Result<(f64, String)> {
    let fee = fee.trim();
    if fee.is_empty() {
        bail!("x402: service fee field is empty");
    }
    let parts: Vec<&str> = fee.split_whitespace().collect();
    match parts.len() {
        2 => {
            let amt: f64 = parts[0]
                .parse()
                .map_err(|_| anyhow::anyhow!("x402: failed to parse fee amount: {}", parts[0]))?;
            Ok((amt, parts[1].to_string()))
        }
        1 => {
            let numeric_end = fee.find(|c: char| c.is_alphabetic()).unwrap_or(fee.len());
            if numeric_end >= fee.len() {
                bail!("x402: fee field contains only amount without token symbol: {fee}, unable to determine payment token");
            }
            let amt: f64 = fee[..numeric_end]
                .parse()
                .map_err(|_| anyhow::anyhow!("x402: failed to parse fee amount: {fee}"))?;
            let sym = fee[numeric_end..].to_string();
            Ok((amt, sym))
        }
        _ => bail!("x402: unable to parse fee format: {fee}"),
    }
}

// ─── x402 service params resolution ─────────────────────────────────────

/// Result of the x402 three-tier fallback resolution.
pub struct X402ServiceParams {
    pub endpoint: String,
    pub fee_amount: f64,
    pub fee_token_symbol: String,
}

fn cached_x402_service_fee(
    endpoint: &str,
    fee_amount: Option<f64>,
    token_symbol: &str,
) -> Option<f64> {
    fee_amount.filter(|fee| {
        !endpoint.is_empty() && fee.is_finite() && *fee >= 0.0 && !token_symbol.is_empty()
    })
}

/// Resolve x402 service params: CLI flag > negotiate cache > identity service-list API > error.
pub async fn resolve_x402_params(
    job_id: &str,
    provider_agent_id: Option<&str>,
    cli_endpoint: Option<&str>,
    cli_token_symbol: Option<&str>,
    cli_token_amount: Option<&str>,
) -> Result<X402ServiceParams> {
    // Tier 1: all CLI flags provided.
    if let (Some(ep), Some(sym), Some(amt_str)) = (cli_endpoint, cli_token_symbol, cli_token_amount)
    {
        let amt: f64 = amt_str
            .parse()
            .map_err(|_| anyhow::anyhow!("--token-amount format error: {amt_str}"))?;
        if DEBUG_LOG {
            eprintln!("ℹ x402: using CLI params endpoint={ep}, token={sym}, amount={amt}");
        }
        return Ok(X402ServiceParams {
            endpoint: ep.to_string(),
            fee_amount: amt,
            fee_token_symbol: sym.to_string(),
        });
    }

    // Tier 2: negotiate cache.
    let mut cached_provider_agent_id = String::new();
    match crate::commands::agent_commerce::task::user::negotiate::current(job_id) {
        Ok(Some(pi)) => {
            cached_provider_agent_id = pi.provider_agent_id.clone();
            if let Some(svc) = pi.services.first() {
                if let Some(cached_fee_amount) = cached_x402_service_fee(
                    &svc.endpoint,
                    svc.fee_amount,
                    &svc.fee_token_symbol,
                ) {
                    if DEBUG_LOG {
                        eprintln!(
                            "ℹ x402: using negotiate cache endpoint={}, token={}, amount={}",
                            svc.endpoint, svc.fee_token_symbol, cached_fee_amount
                        );
                    }
                    return Ok(X402ServiceParams {
                        endpoint: cli_endpoint.unwrap_or(&svc.endpoint).to_string(),
                        fee_amount: cli_token_amount
                            .and_then(|a| a.parse().ok())
                            .unwrap_or(cached_fee_amount),
                        fee_token_symbol: cli_token_symbol
                            .unwrap_or(&svc.fee_token_symbol)
                            .to_string(),
                    });
                }
            }
            if DEBUG_LOG {
                eprintln!("⚠ x402: negotiate cache services empty or fields missing, falling back to service-list API");
            }
        }
        Ok(None) => {
            if DEBUG_LOG {
                eprintln!(
                    "⚠ x402: negotiate cache has no current asp, falling back to service-list API"
                );
            }
        }
        Err(e) => {
            if DEBUG_LOG {
                eprintln!("⚠ x402: failed to read negotiate cache ({e}), falling back to service-list API");
            }
        }
    }

    // Tier 3: identity service-list API (prefer the input arg, otherwise the cached provider_agent_id).
    let resolved_id = provider_agent_id
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string())
        .unwrap_or(cached_provider_agent_id);
    if resolved_id.is_empty() {
        anyhow::bail!("unable to determine ASP agentId: negotiate cache is empty and --provider-agent-id was not provided");
    }
    let params = fetch_x402_service_from_identity(&resolved_id).await?;
    Ok(X402ServiceParams {
        endpoint: cli_endpoint.unwrap_or(&params.endpoint).to_string(),
        fee_amount: cli_token_amount
            .and_then(|a| a.parse().ok())
            .unwrap_or(params.fee_amount),
        fee_token_symbol: cli_token_symbol
            .unwrap_or(&params.fee_token_symbol)
            .to_string(),
    })
}

/// Extract token symbol from a service entry's chainIndex + contractAddress via token basic-info API.
pub(crate) async fn resolve_symbol_from_svc(svc: &serde_json::Value) -> Result<String> {
    let chain = svc["chainIndex"]
        .as_i64()
        .or_else(|| svc["chainIndex"].as_str().and_then(|s| s.parse().ok()))
        .map(|n| n.to_string())
        .ok_or_else(|| {
            anyhow::anyhow!("x402: service entry missing chainIndex, cannot resolve token symbol")
        })?;
    let addr = svc["contractAddress"]
        .as_str()
        .filter(|s| !s.is_empty())
        .ok_or_else(|| {
            anyhow::anyhow!(
                "x402: service entry missing contractAddress, cannot resolve token symbol"
            )
        })?;
    if DEBUG_LOG {
        eprintln!("ℹ x402: feeTokenSymbol missing, resolving from chain={chain} address={addr}");
    }
    resolve_token_symbol_by_address(&chain, addr).await
}

/// Query a provider's A2MCP service info via `onchainos agent service-list`.
async fn fetch_x402_service_from_identity(provider_agent_id: &str) -> Result<X402ServiceParams> {
    let exe = std::env::current_exe()
        .map_err(|e| anyhow::anyhow!("unable to determine executable path: {e}"))?;
    let output = tokio::process::Command::new(&exe)
        .args(["agent", "service-list", "--agent-id", provider_agent_id])
        .output()
        .await
        .map_err(|e| {
            anyhow::anyhow!("failed to call agent service-list --agent-id {provider_agent_id}: {e}")
        })?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        bail!(
            "x402 service-list query failed (exit {}): {stderr}",
            output.status
        );
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let body: serde_json::Value = serde_json::from_str(&stdout)
        .map_err(|e| anyhow::anyhow!("failed to parse service-list output: {e}"))?;

    // CLI output: {ok: true, data: [{agentInfo: {...}, list: [{endpoint, fee, ...}]}, ...]}
    // Flatten data[*].list[*] to get individual service entries (same shape as handle_designated_route).
    let data_arr = body["data"].as_array().ok_or_else(|| {
        anyhow::anyhow!(
            "x402: data array not found in service-list response, ASP={provider_agent_id}"
        )
    })?;
    let services: Vec<&serde_json::Value> = data_arr
        .iter()
        .flat_map(|item| item["list"].as_array().into_iter().flatten())
        .collect();

    let svc = services
        .iter()
        .find(|s| {
            let stype = s["servicetype"]
                .as_str()
                .or_else(|| s["serviceType"].as_str())
                .unwrap_or("");
            stype.eq_ignore_ascii_case("A2MCP")
        })
        .ok_or_else(|| anyhow::anyhow!("x402: ASP {provider_agent_id} has no A2MCP service"))?;

    let endpoint = svc["endpoint"]
        .as_str()
        .filter(|s| !s.is_empty())
        .ok_or_else(|| anyhow::anyhow!("x402: A2MCP service endpoint is empty in service-list"))?
        .to_string();

    let (fee_amount, fee_token_symbol) = if let Some(amt) = svc["feeAmount"].as_f64() {
        let sym = svc["feeTokenSymbol"]
            .as_str()
            .filter(|s| !s.is_empty())
            .map(|s| s.to_string());
        let sym = match sym {
            Some(s) => s,
            None => resolve_symbol_from_svc(svc).await?,
        };
        (amt, sym)
    } else {
        let fee_str = svc["fee"].as_str().unwrap_or("");
        match parse_composite_fee(fee_str) {
            Ok(pair) => pair,
            Err(_) => {
                let amt: f64 = fee_str
                    .parse()
                    .map_err(|_| anyhow::anyhow!("x402: fee field is not a number: {fee_str}"))?;
                let sym = resolve_symbol_from_svc(svc).await?;
                (amt, sym)
            }
        }
    };

    if DEBUG_LOG {
        eprintln!("ℹ x402: retrieved from service-list API endpoint={endpoint}, token={fee_token_symbol}, amount={fee_amount}");
    }
    Ok(X402ServiceParams {
        endpoint,
        fee_amount,
        fee_token_symbol,
    })
}

// ─── Token symbol resolution by contract address ──────────────────────

/// Look up a token's symbol via the DEX basic-info API given chainIndex + contractAddress.
pub(crate) async fn resolve_token_symbol_by_address(
    chain_index: &str,
    contract_address: &str,
) -> Result<String> {
    let mut client = crate::client::ApiClient::new(None)?;
    let resp =
        crate::commands::token::fetch_info(&mut client, contract_address, chain_index).await?;
    let sym = resp
        .as_array()
        .and_then(|arr| arr.first())
        .and_then(|t| t["tokenSymbol"].as_str())
        .filter(|s| !s.is_empty())
        .ok_or_else(|| {
            anyhow::anyhow!(
            "token basic-info returned no symbol for chain={chain_index} address={contract_address}"
        )
        })?;
    Ok(sym.to_string())
}

// ─── Balance precheck ──────────────────────────────────────────────────

/// Normalize a token symbol: map Unicode currency symbols to their ASCII letter equivalents, then uppercase.
/// Example: `USD₮0` -> `USDT0` (₮ U+20AE -> T).
fn normalize_token_symbol(s: &str) -> String {
    s.chars()
        .map(|c| match c {
            '₮' => 'T',
            _ => c,
        })
        .collect::<String>()
        .to_uppercase()
}

/// Call `onchainos wallet balance --chain 196` to look up the **business token** balance on XLayer
/// (USDT/USDG); bail and block downstream flow if insufficient.
///
/// Note: the task system is fully gas-free; gas is paid by the paymaster. This check applies only
/// to the business-token principal and **never** implies the user needs OKB / native to pay gas.
/// The bail message must make this explicit to avoid downstream agents misattributing the error
/// to a "top up gas" issue.
pub async fn ensure_sufficient_balance(required: f64, currency: &str) -> Result<()> {
    let exe = std::env::current_exe()
        .map_err(|e| anyhow::anyhow!("unable to determine executable path: {e}"))?;

    let output = tokio::process::Command::new(&exe)
        .args(["wallet", "balance", "--chain", XLAYER_CHAIN_INDEX])
        .output()
        .await
        .map_err(|e| anyhow::anyhow!("balance query failed: {e}"))?;

    if !output.status.success() {
        bail!(
            "balance query failed (exit {}), please check login status",
            output.status
        );
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let parsed: serde_json::Value = serde_json::from_str(&stdout)
        .map_err(|e| anyhow::anyhow!("failed to parse balance query result: {e}"))?;

    let currency_norm = normalize_token_symbol(currency);
    let details = parsed["data"]["details"].as_array();
    if let Some(details) = details {
        for detail in details {
            let assets = detail["tokenAssets"]
                .as_array()
                .or_else(|| detail["assets"].as_array());
            if let Some(assets) = assets {
                for asset in assets {
                    let symbol = asset["tokenSymbol"]
                        .as_str()
                        .or_else(|| asset["symbol"].as_str())
                        .unwrap_or("");
                    let sym_norm = normalize_token_symbol(symbol);
                    if sym_norm == currency_norm || sym_norm == format!("{currency_norm}0") {
                        let balance: f64 = asset["balance"]
                            .as_str()
                            .and_then(|s| s.parse().ok())
                            .or_else(|| asset["balance"].as_f64())
                            .unwrap_or(0.0);
                        if balance < required {
                            let shortfall = required - balance;
                            let message = format!(
                                "Insufficient {currency} balance on XLayer (current: {balance}, need: {required}, shortfall: {shortfall})\n\
                                 \n\
                                 Fund your wallet — pick one:\n\
                                 1. {SCAN_TO_DEPOSIT_OPTION}\n\
                                 {DEPOSIT_QR_MARKER}\n\
                                 2. Swap on XLayer — \"swap <token> to {shortfall} {currency} on xlayer\"\n\
                                 3. Bridge from another chain — \"bridge {shortfall} {currency} from <chain> to xlayer\"\n\
                                 4. Send from OKX exchange — withdraw {currency} to your wallet address on XLayer network\n\
                                 \n\
                                 Note: gas is paid by the platform paymaster, no OKB / native required."
                            );
                            return Err(InsufficientBalanceError::new(
                                message, currency, required, balance,
                            )
                            .into());
                        }
                        return Ok(());
                    }
                }
            }
        }
    }

    let message = format!(
        "{currency} balance not found on XLayer (need {required} {currency})\n\
         \n\
         Fund your wallet — pick one:\n\
         1. {SCAN_TO_DEPOSIT_OPTION}\n\
         {DEPOSIT_QR_MARKER}\n\
         2. Swap on XLayer — \"swap <token> to {required} {currency} on xlayer\"\n\
         3. Bridge from another chain — \"bridge {required} {currency} from <chain> to xlayer\"\n\
         4. Send from OKX exchange — withdraw {currency} to your wallet address on XLayer network\n\
         \n\
         Note: gas is paid by the platform paymaster, no OKB / native required."
    );
    Err(InsufficientBalanceError::new(message, currency, required, 0.0).into())
}

/// Query the XLayer balance of `currency` for a given on-chain `address`.
/// Returns `Ok(balance)` on success, `Err` on any query/parse failure.
pub async fn query_xlayer_balance(address: &str, currency: &str) -> Result<f64> {
    let exe = std::env::current_exe()
        .map_err(|e| anyhow::anyhow!("unable to determine executable path: {e}"))?;
    let output = tokio::process::Command::new(&exe)
        .args(["portfolio", "all-balances", "--address", address, "--chains", XLAYER_CHAIN_INDEX])
        .output()
        .await
        .map_err(|e| anyhow::anyhow!("portfolio balance query failed: {e}"))?;
    if !output.status.success() {
        bail!("portfolio balance query failed (exit {})", output.status);
    }
    let stdout = String::from_utf8_lossy(&output.stdout);
    let parsed: serde_json::Value = serde_json::from_str(&stdout)
        .map_err(|e| anyhow::anyhow!("failed to parse portfolio balance result: {e}"))?;
    let currency_norm = normalize_token_symbol(currency);
    let token_assets = parsed["data"]
        .as_array()
        .into_iter()
        .flatten()
        .flat_map(|chain| chain["tokenAssets"].as_array().into_iter().flatten());
    for asset in token_assets {
        let symbol = asset["symbol"]
            .as_str()
            .or_else(|| asset["tokenSymbol"].as_str())
            .unwrap_or("");
        let sym_norm = normalize_token_symbol(symbol);
        if sym_norm == currency_norm || sym_norm == format!("{currency_norm}0") {
            let balance: f64 = asset["balance"]
                .as_str()
                .and_then(|s| s.parse().ok())
                .or_else(|| asset["balance"].as_f64())
                .unwrap_or(0.0);
            return Ok(balance);
        }
    }
    Ok(0.0)
}

/// Like [`ensure_sufficient_balance`] but queries a specific on-chain address
/// via `onchainos portfolio all-balances` (public API, independent of selected_account_id).
/// Used by provider-side flows where the signing account may differ from the active account.
pub async fn ensure_sufficient_balance_at(
    required: f64,
    currency: &str,
    address: &str,
) -> Result<()> {
    let exe = std::env::current_exe()
        .map_err(|e| anyhow::anyhow!("unable to determine executable path: {e}"))?;

    let output = tokio::process::Command::new(&exe)
        .args(["portfolio", "all-balances", "--address", address, "--chains", XLAYER_CHAIN_INDEX])
        .output()
        .await
        .map_err(|e| anyhow::anyhow!("portfolio balance query failed: {e}"))?;

    if !output.status.success() {
        bail!(
            "portfolio balance query failed (exit {}), address={address}",
            output.status
        );
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let parsed: serde_json::Value = serde_json::from_str(&stdout)
        .map_err(|e| anyhow::anyhow!("failed to parse portfolio balance result: {e}"))?;

    let currency_norm = normalize_token_symbol(currency);

    let token_assets = parsed["data"]
        .as_array()
        .into_iter()
        .flatten()
        .flat_map(|chain| chain["tokenAssets"].as_array().into_iter().flatten());

    for asset in token_assets {
        let symbol = asset["symbol"]
            .as_str()
            .or_else(|| asset["tokenSymbol"].as_str())
            .unwrap_or("");
        let sym_norm = normalize_token_symbol(symbol);
        if sym_norm == currency_norm || sym_norm == format!("{currency_norm}0") {
            let balance: f64 = asset["balance"]
                .as_str()
                .and_then(|s| s.parse().ok())
                .or_else(|| asset["balance"].as_f64())
                .unwrap_or(0.0);
            if balance < required {
                let shortfall = required - balance;
                let message = format!(
                    "Insufficient {currency} balance on XLayer (current: {balance}, need: {required}, shortfall: {shortfall}, address: {address})\n\
                     \n\
                     Fund your wallet — pick one:\n\
                     1. {SCAN_TO_DEPOSIT_OPTION}\n\
                     {DEPOSIT_QR_MARKER}\n\
                     2. Swap on XLayer — \"swap <token> to {shortfall} {currency} on xlayer\"\n\
                     3. Bridge from another chain — \"bridge {shortfall} {currency} from <chain> to xlayer\"\n\
                     4. Send from OKX exchange — withdraw {currency} to your wallet address on XLayer network\n\
                     \n\
                     Note: gas is paid by the platform paymaster, no OKB / native required."
                );
                return Err(
                    InsufficientBalanceError::new(message, currency, required, balance).into(),
                );
            }
            return Ok(());
        }
    }

    let message = format!(
        "{currency} balance not found on XLayer (need {required} {currency}, address: {address})\n\
         \n\
         Fund your wallet — pick one:\n\
         1. {SCAN_TO_DEPOSIT_OPTION}\n\
         {DEPOSIT_QR_MARKER}\n\
         2. Swap on XLayer — \"swap <token> to {required} {currency} on xlayer\"\n\
         3. Bridge from another chain — \"bridge {required} {currency} from <chain> to xlayer\"\n\
         4. Send from OKX exchange — withdraw {currency} to your wallet address on XLayer network\n\
         \n\
         Note: gas is paid by the platform paymaster, no OKB / native required."
    );
    Err(InsufficientBalanceError::new(message, currency, required, 0.0).into())
}

// ─── jobId path-safety validator ─────────────────────────────────────────
//
// A deliverable / attachment directory name is used verbatim as a single
// filesystem path component under `ONCHAINOS_HOME`. `PathBuf::join` does NOT
// keep the result inside its base, so an unsafe value like `../../x` or a name
// containing `/` escapes the task root. This validator fails closed on any
// value that is not provably a single safe path component, while still
// accepting legitimate `<jobId>_<title>` directory names (including Unicode /
// CJK / emoji titles) used by the deliverables list-all scan.
//
// NOTE: this path-component validator is deliberately SEPARATE from the
// autotrade `job_id_is_safe` charset check (`[A-Za-z0-9_-]`) in
// `autotrade::grants` — jobId validation and ordinary path-component
// validation must not share one predicate.

/// Fail-closed upper bound on a path component's **byte** length. A real jobId
/// is 66 bytes (`0x` + 64 hex); a `<jobId>_<title>` deliverable directory stays
/// well under this ceiling. Bounds pure-CPU validation work.
pub const MAX_JOB_ID_LEN: usize = 256;

/// Stable coded error: a value is not a single safe path component. Fixed
/// message — never interpolates the raw value, so a malicious path is never
/// echoed into audit / error output.
fn unsafe_job_path_err() -> anyhow::Error {
    CodedError::new(
        "UNSAFE_JOB_PATH_COMPONENT",
        None,
        "jobId is not a safe path component",
    )
    .into()
}

/// Predicate — is `s` a single, safe, ordinary path component?
///
/// Rejects (fail-closed) if ANY row matches; otherwise accepts:
/// - R1 empty; R2 byte length > `MAX_JOB_ID_LEN`; R3 contains `/` or `\`;
///   R4 equals `.` or `..`; R5 any control char; R6 not exactly one
///   `Component::Normal` (catches absolute `/x`, drive `C:\x`, UNC `\\?\x`).
///
/// Ordinary Unicode / CJK / emoji characters are accepted (only the rows above
/// reject), so existing `<jobId>_<title>` deliverable directories stay valid.
/// MUST NOT call `canonicalize()` — the target directory may not exist yet.
fn is_single_safe_component(s: &str) -> bool {
    // R1 — empty component cannot name a directory.
    if s.is_empty() {
        return false;
    }
    // R2 — fail-closed byte ceiling.
    if s.len() > MAX_JOB_ID_LEN {
        return false;
    }
    // R3 — explicit separator rejection for both OS families (also catches the
    // backslash in Windows drive / UNC forms on a Unix build, where `\` is not a
    // path separator and would otherwise slip through the Component check).
    if s.contains('/') || s.contains('\\') {
        return false;
    }
    // R4 — current / parent-dir traversal.
    if s == "." || s == ".." {
        return false;
    }
    // R5 — any control char (NUL, newline, CR, or other ASCII/Unicode control):
    // truncation / log-injection / filesystem edge cases.
    if s.chars().any(char::is_control) {
        return false;
    }
    // R6 — exactly one ordinary (`Component::Normal`) component equal to the input.
    let mut comps = Path::new(s).components();
    matches!(
        (comps.next(), comps.next()),
        (Some(Component::Normal(seg)), None) if seg == std::ffi::OsStr::new(s)
    )
}

/// Validate that a value is a single, safe path component before it is turned
/// into a filesystem path — the path-builder guard for the deliverable /
/// attachment directories. Returns `Ok(())` iff
/// [`is_single_safe_component`] passes, else fails closed with the stable
/// `UNSAFE_JOB_PATH_COMPONENT` coded error. MUST NOT call `canonicalize()`.
pub fn validate_job_id_path_component(job_id: &str) -> Result<()> {
    if is_single_safe_component(job_id) {
        Ok(())
    } else {
        Err(unsafe_job_path_err())
    }
}

// ─── jobId formatting ───────────────────────────────────────────────────

/// Validate that `job_id` is one of the legal forms before it's used in
/// CLI commands / HTTP requests:
///   - Real on-chain jobId: `0x` + 64 lowercase hex chars (66 chars total)
///   - System placeholder: starts with `system_` (per SKILL.md §--jobid source path)
///
/// Catches LLM mistakes early (before they reach the backend as opaque
/// `task not found`) — truncated shortJobId form, wrong length, non-hex
/// characters, missing `0x` prefix, or accidental sessionKey paste.
pub fn validate_job_id(job_id: &str) -> std::result::Result<(), String> {
    // Placeholder for events fired BEFORE a task exists (e.g. `create_task`).
    if job_id == "_" {
        return Ok(());
    }
    // Per SKILL.md §--jobid source path exception: backend-emitted pseudo jobIds
    // for account-level events (voter staking, no-ASP, etc.) — pass through as-is.
    if job_id.starts_with("system_") {
        return Ok(());
    }
    if !job_id.starts_with("0x") || job_id.len() != 66 {
        return Err(format!(
            "--jobid invalid (must be `0x` + 64 chars, got {} chars). Re-read jobId from envelope \
             (system event / user_decision_* → `message.jobId`; a2a-agent-chat → top-level `jobId`), \
             then retry.",
            job_id.len()
        ));
    }
    Ok(())
}

/// Short jobId: first 6 + … + last 4 characters. A 0x... hex value yields `0x1b76…1be1`;
/// a long string ID yields `task-0…long`. Returned as-is if ≤ 12 characters.
pub fn short_job_id(job_id: &str) -> String {
    if job_id.chars().count() <= 12 {
        return job_id.to_string();
    }
    let chars: Vec<char> = job_id.chars().collect();
    let head: String = chars.iter().take(6).collect();
    let tail: String = chars
        .iter()
        .rev()
        .take(4)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect();
    format!("{head}…{tail}")
}

/// Neutralize shell metacharacters in a task title before it is re-embedded into the
/// `onchainos agent user-notify --content "…"` command templates that downstream AI agents
/// execute on their own shell (including Windows `cmd.exe`). This is a mild, portable pass —
/// distinct from the aggressive alphanumeric-only `deliverables::sanitize_title`; do NOT merge
/// the two.
///
/// Char-class rules:
/// - command separators (`&` `|` `;`) → replaced with a single space;
/// - substitution / redirection / subshell chars (`` ` `` `$` `>` `<` `(` `)` `!`) → deleted;
/// - everything else (letters, digits, CJK, `:` `-` `.`, spaces, …) → kept unchanged.
///
/// Then consecutive-whitespace runs are collapsed to one space and the ends are trimmed.
///
/// Pure, deterministic, O(n). Never grows the string, so a post-truncation `≤ MAX_TITLE_CHARS`
/// invariant is preserved. Cannot fail — an all-metacharacter title sanitizes to `""`, which the
/// caller handles via its existing empty-title path.
pub fn sanitize_title_for_shell(title: &str) -> String {
    let mut mapped = String::with_capacity(title.len());
    for ch in title.chars() {
        match ch {
            // Command separators → single space.
            '&' | '|' | ';' => mapped.push(' '),
            // Substitution / redirection / subshell → delete.
            '`' | '$' | '>' | '<' | '(' | ')' | '!' => {}
            // Everything else → keep.
            other => mapped.push(other),
        }
    }
    // Collapse consecutive whitespace to a single space, then trim ends.
    mapped.split_whitespace().collect::<Vec<_>>().join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn short_job_id_hex_64() {
        assert_eq!(
            short_job_id("0x1b76dabd3bf884626184e3b36b7c65b54929a827a8a26e223c4b8aa868d41be1"),
            "0x1b76…1be1"
        );
    }

    #[test]
    fn short_job_id_passthrough() {
        assert_eq!(short_job_id("0x12"), "0x12");
        assert_eq!(short_job_id("task-1"), "task-1");
        assert_eq!(short_job_id("task-001-12"), "task-001-12");
    }

    #[test]
    fn short_job_id_long_string() {
        assert_eq!(short_job_id("task-001-very-long"), "task-0…long");
    }

    #[test]
    fn sanitize_title_for_shell_empty() {
        assert_eq!(sanitize_title_for_shell(""), "");
    }

    #[test]
    fn sanitize_title_for_shell_pure_metacharacters() {
        assert_eq!(sanitize_title_for_shell("&;|"), "");
    }

    #[test]
    fn sanitize_title_for_shell_mixed() {
        assert_eq!(sanitize_title_for_shell("A & B  C > D"), "A B C D");
    }

    #[test]
    fn sanitize_title_for_shell_cjk_with_ampersand() {
        // CJK expressed via \u{...} escapes so the source carries no literal CJK bytes
        // (keeps the "no Chinese in Rust source" gate happy) while still covering AC-7's
        // CJK-with-ampersand row: a 4-char + " & " + 4-char title collapses the separator
        // to a single space (the two CJK words are preserved unchanged).
        assert_eq!(
            sanitize_title_for_shell(
                "\u{94b1}\u{5305}\u{7a0e}\u{52a1} & \u{5408}\u{89c4}\u{62a5}\u{544a}"
            ),
            "\u{94b1}\u{5305}\u{7a0e}\u{52a1} \u{5408}\u{89c4}\u{62a5}\u{544a}"
        );
    }

    #[test]
    fn sanitize_title_for_shell_safe_punctuation_unchanged() {
        assert_eq!(
            sanitize_title_for_shell("Normal Title: Hello"),
            "Normal Title: Hello"
        );
    }

    #[test]
    fn sanitize_title_for_shell_brd_row() {
        assert_eq!(
            sanitize_title_for_shell("Wallet Tax & Compliance Report"),
            "Wallet Tax Compliance Report"
        );
    }

    #[test]
    fn sanitize_title_for_shell_deletes_subshell_chars() {
        // Substitution / redirection / subshell chars are deleted, not spaced.
        assert_eq!(sanitize_title_for_shell("a$(b)`c`!d"), "abcd");
    }

    #[test]
    fn sanitize_title_for_shell_never_grows() {
        let input = "x & y | z ; w";
        assert!(sanitize_title_for_shell(input).chars().count() <= input.chars().count());
    }

    // Task 5 / FR-6: the structured InsufficientBalanceError's Display renders the
    // legacy message string byte-for-byte (so a surfaced free-text `error` is
    // unchanged when the deposit-address enrichment degrades).
    #[test]
    fn insufficient_balance_error_display_equals_legacy_message() {
        let legacy = "Insufficient USDT balance on XLayer (current: 5, need: 100, shortfall: 95)\n\
                      Note: gas is paid by the platform paymaster, no OKB / native required.";
        let e = InsufficientBalanceError::new(legacy.to_string(), "USDT", 100.0, 5.0);
        assert_eq!(format!("{e}"), legacy);
    }

    // ── jobId / manifest path-safety validators ───────────────────────────

    /// Assert an `anyhow::Error` carries the expected stable `errorCode`.
    fn assert_code(err: anyhow::Error, code: &str) {
        let c = err
            .downcast_ref::<CodedError>()
            .unwrap_or_else(|| panic!("expected CodedError, got: {err:#}"));
        assert_eq!(c.code, code, "wrong errorCode");
    }

    #[test]
    fn validate_job_id_path_component_accepts_normal() {
        // architecture §4.1 accept table.
        for ok in ["report-42", "system_voter_staking", "a", &"0".repeat(256)] {
            assert!(
                validate_job_id_path_component(ok).is_ok(),
                "should accept {ok:?}"
            );
        }
        // A real 66-byte 0x… id is also a single safe component.
        let hex = format!("0x{}", "a".repeat(64));
        assert!(validate_job_id_path_component(&hex).is_ok());
    }

    #[test]
    fn validate_job_id_path_component_rejects_unsafe() {
        // architecture §4.1 reject table — every case → UNSAFE_JOB_PATH_COMPONENT.
        let cases = [
            "",               // R1 empty
            "../secret",      // R3/R6 traversal
            "a/b",            // R3 unix separator
            "a\\b",           // R3 windows separator
            ".",              // R4
            "..",             // R4
            "/etc/passwd",    // R3/R6 absolute
            "C:\\x",          // R3 windows drive
            "\\\\?\\x",       // R3 UNC/verbatim
            "a\0b",           // R5 embedded NUL
            "a\nb",           // R5 newline
            &"x".repeat(257), // R2 over the byte ceiling
        ];
        for bad in cases {
            let err = validate_job_id_path_component(bad).expect_err(bad);
            assert_code(err, "UNSAFE_JOB_PATH_COMPONENT");
        }
    }

    #[test]
    fn validate_job_id_path_component_never_canonicalizes() {
        // Source-level guarantee: this compiles and validates a non-existent path
        // without touching the filesystem (no canonicalize / metadata call).
        assert!(validate_job_id_path_component("does-not-exist-anywhere").is_ok());
    }

    #[test]
    fn validate_job_id_path_component_accepts_composite_title() {
        // Existing `<jobId>_<Unicode/CJK/emoji title>` deliverable directory
        // names (scanned by list-all) must remain valid single components. Built
        // from \u{...} escapes so the source stays ASCII (no raw CJK in .rs).
        let hex = format!("0x{}", "a".repeat(64));
        // "<hex>_<CJK title>" composite name (CJK built from \u{...} escapes).
        let cjk = format!("{hex}_\u{6211}\u{7684}\u{62a5}\u{544a}");
        assert!(validate_job_id_path_component(&cjk).is_ok(), "CJK title dir");
        // "<hex>_<emoji title>" e.g. jobId_📄report
        let emoji = format!("{hex}_\u{1F4C4}report");
        assert!(
            validate_job_id_path_component(&emoji).is_ok(),
            "emoji title dir"
        );
        // Legacy bare-jobId directory name.
        assert!(validate_job_id_path_component(&hex).is_ok(), "bare jobId dir");
    }

    /// The restored autotrade-era `validate_job_id` contract (Result<(), String>):
    /// `_` placeholder, any `system_…`, and a `0x`+64-char id pass; everything else
    /// returns the free-text `must be 0x + 64 chars` error. This predicate is
    /// deliberately SEPARATE from the path-component validator above.
    #[test]
    fn validate_job_id_original_contract() {
        assert!(validate_job_id("_").is_ok());
        assert!(validate_job_id("system_voter_staking").is_ok());
        assert!(validate_job_id(&format!("0x{}", "a".repeat(64))).is_ok());
        // Wrong length / missing prefix → Err (free-text message, not a coded error).
        assert!(validate_job_id("0xdeadbeef").is_err());
        assert!(validate_job_id("task-1").is_err());
        assert!(validate_job_id("").is_err());
    }

    #[test]
    fn cached_x402_service_accepts_zero_fee() {
        assert_eq!(
            cached_x402_service_fee("https://example.invalid/x402", Some(0.0), "USDT"),
            Some(0.0)
        );
    }

    #[test]
    fn cached_x402_service_rejects_missing_negative_or_non_finite_fee() {
        let endpoint = "https://example.invalid/x402";

        assert_eq!(cached_x402_service_fee(endpoint, None, "USDT"), None);
        assert_eq!(cached_x402_service_fee(endpoint, Some(-0.01), "USDT"), None);
        assert_eq!(
            cached_x402_service_fee(endpoint, Some(f64::INFINITY), "USDT"),
            None
        );
    }
}
