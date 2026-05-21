//! x402 payment flow.
//!
//! After the user accepts, x402 is invoked to complete payment. Subprocess invocation
//! of `onchainos payment x402-pay` reuses the signing logic in `agentic_wallet`.
//! This module is responsible for:
//! - requesting the Provider endpoint → decoding HTTP 402;
//! - invoking the CLI signer;
//! - assembling the payment header → replaying the request.

use anyhow::{anyhow, bail, Context, Result};
use base64::{engine::general_purpose::STANDARD as B64, Engine};
use serde::{Deserialize, Serialize};
use serde_json::json;

// ─── Public types ───────────────────────────────────────────────────────

/// Decoded 402 response.
#[derive(Debug, Clone)]
pub struct X402Payload {
    pub x402_version: i64,
    pub accepts: Vec<serde_json::Value>,
    pub resource: Option<serde_json::Value>,
    pub raw: serde_json::Value,
}

/// CLI signing output.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct X402PaymentProof {
    pub signature: String,
    pub authorization: serde_json::Value,
    #[serde(default, rename = "sessionCert")]
    pub session_cert: Option<String>,
}

// ─── x402 validation & pricing ─────────────────────────────────────────

/// Raw pricing info extracted from the `accepts` array.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct X402Pricing {
    /// Amount in minimal units (e.g. "1000000" = 1 USDC).
    pub amount_minimal: String,
    /// ERC-20 contract address.
    pub asset: String,
    /// Recipient address.
    pub pay_to: String,
    /// CAIP-2 network identifier (e.g. "eip155:196").
    pub network: String,
    /// x402 scheme (`exact` / `aggr_deferred`).
    pub scheme: Option<String>,
    /// EIP-3009 timeout in seconds.
    pub max_timeout_seconds: u64,
    /// Decimals extracted directly from the accepts entry (preferred over a token-info lookup).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub extra_decimals: Option<u8>,
}

/// x402 endpoint validation result.
#[derive(Debug, Clone, Serialize)]
pub struct X402EndpointCheck {
    /// Whether this is a valid x402 endpoint.
    pub valid: bool,
    /// HTTP status code.
    pub status_code: u16,
    /// Pricing info (present when `valid=true`).
    pub pricing: Option<X402Pricing>,
    /// Raw `accepts` JSON (present when `valid=true`; passed downstream to `task-402-pay --accepts`).
    pub accepts_json: Option<String>,
    /// x402 protocol version.
    pub x402_version: Option<i64>,
}

/// Full pricing with human-readable info (token already resolved).
#[derive(Debug, Clone, Serialize)]
pub struct X402PricingResolved {
    pub amount_minimal: String,
    pub amount_human: f64,
    pub token_symbol: String,
    pub decimals: u8,
    pub asset: String,
    pub pay_to: String,
    pub network: String,
    pub scheme: Option<String>,
    pub max_timeout_seconds: u64,
}

// ─── 402 decoding ───────────────────────────────────────────────────────

/// Decode the HTTP 402 response and extract the `accepts` array.
///
/// - v2: `PAYMENT-REQUIRED` header (base64 JSON).
/// - v1: response body (raw JSON).
pub fn decode_402_response(
    headers: &reqwest::header::HeaderMap,
    body: &str,
) -> Result<X402Payload> {
    // v2: PAYMENT-REQUIRED header
    if let Some(header_val) = headers.get("PAYMENT-REQUIRED") {
        let header_str = header_val
            .to_str()
            .context("PAYMENT-REQUIRED header is not valid UTF-8")?;
        let decoded_bytes = B64
            .decode(header_str)
            .context("PAYMENT-REQUIRED header is not valid base64")?;
        let parsed: serde_json::Value = serde_json::from_slice(&decoded_bytes)
            .context("PAYMENT-REQUIRED decoded content is not valid JSON")?;
        let version = parsed["x402Version"].as_i64().unwrap_or(2);
        let accepts = parsed["accepts"].as_array().cloned().unwrap_or_default();
        let resource = parsed.get("resource").cloned();
        return Ok(X402Payload {
            x402_version: version,
            accepts,
            resource,
            raw: parsed,
        });
    }

    // v1: response body
    let parsed: serde_json::Value =
        serde_json::from_str(body).context("402 response body is not valid JSON")?;
    let version = parsed["x402Version"].as_i64().unwrap_or(1);
    let accepts = parsed["accepts"].as_array().cloned().unwrap_or_default();
    Ok(X402Payload {
        x402_version: version,
        accepts,
        resource: None,
        raw: parsed,
    })
}

// ─── Scheme selection ────────────────────────────────────────────────────

/// Pick the best scheme entry from the `accepts` array.
/// Priority: `exact` > `aggr_deferred` > first.
fn select_best_scheme(accepts: &[serde_json::Value]) -> Result<(serde_json::Value, Option<String>)> {
    if accepts.is_empty() {
        bail!("accepts array is empty");
    }
    if let Some(entry) = accepts.iter().find(|a| a["scheme"].as_str() == Some("exact")) {
        return Ok((entry.clone(), Some("exact".to_string())));
    }
    if let Some(entry) = accepts.iter().find(|a| a["scheme"].as_str() == Some("aggr_deferred")) {
        return Ok((entry.clone(), Some("aggr_deferred".to_string())));
    }
    Ok((accepts[0].clone(), accepts[0]["scheme"].as_str().map(|s| s.to_string())))
}

// ─── x402 pricing extraction ────────────────────────────────────────────

/// Extract pricing info from the `accepts` array (picking the best scheme).
pub fn extract_x402_pricing(accepts: &[serde_json::Value]) -> Result<X402Pricing> {
    let (entry, scheme) = select_best_scheme(accepts)?;

    let amount_minimal = crate::commands::payment::payment_flow::extract_amount(&entry)?;

    let asset = entry["asset"].as_str()
        .ok_or_else(|| anyhow!("accepts entry is missing `asset`"))?
        .to_string();
    let pay_to = entry["payTo"].as_str()
        .ok_or_else(|| anyhow!("accepts entry is missing `payTo`"))?
        .to_string();
    let network = entry["network"].as_str()
        .ok_or_else(|| anyhow!("accepts entry is missing `network`"))?
        .to_string();
    let max_timeout = entry["maxTimeoutSeconds"].as_u64().unwrap_or(300);

    // Prefer extra.decimals; fall back to entry.decimals.
    let extra_decimals = entry["extra"]["decimals"].as_u64()
        .or_else(|| entry["decimals"].as_u64())
        .or_else(|| entry["extra"]["decimals"].as_str().and_then(|s| s.parse().ok()))
        .or_else(|| entry["decimals"].as_str().and_then(|s| s.parse().ok()))
        .and_then(|n| u8::try_from(n).ok());

    Ok(X402Pricing {
        amount_minimal,
        asset,
        pay_to,
        network,
        scheme,
        max_timeout_seconds: max_timeout,
        extra_decimals,
    })
}

// ─── x402 endpoint validation ──────────────────────────────────────────

/// Decide whether a URL is a valid x402 endpoint.
///
/// GET endpoint → 402? → decode `accepts` → extract pricing.
pub async fn check_x402_endpoint(endpoint: &str) -> Result<X402EndpointCheck> {
    let http = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(15))
        .build()
        .context("failed to build HTTP client")?;
    let resp = http.get(endpoint).send().await
        .map_err(|e| anyhow!("endpoint request failed: {e}"))?;

    let status = resp.status().as_u16();
    if status != 402 {
        return Ok(X402EndpointCheck {
            valid: false,
            status_code: status,
            pricing: None,
            accepts_json: None,
            x402_version: None,
        });
    }

    let headers = resp.headers().clone();
    let body_text = resp.text().await
        .map_err(|e| anyhow!("failed to read 402 response body: {e}"))?;

    let payload = decode_402_response(&headers, &body_text)?;
    if payload.accepts.is_empty() {
        return Ok(X402EndpointCheck {
            valid: false,
            status_code: 402,
            pricing: None,
            accepts_json: None,
            x402_version: Some(payload.x402_version),
        });
    }

    let pricing = extract_x402_pricing(&payload.accepts)?;
    let accepts_json = serde_json::to_string(&payload.accepts)?;

    Ok(X402EndpointCheck {
        valid: true,
        status_code: 402,
        pricing: Some(pricing),
        accepts_json: Some(accepts_json),
        x402_version: Some(payload.x402_version),
    })
}

// ─── Token resolution & amount conversion ──────────────────────────────

/// Look up token info via the task system's `tokenDetail` API.
/// Iterate the supported tokens (USDT, USDG); on contract-address match, return `(symbol, decimals)`.
pub async fn resolve_token_by_asset(
    client: &mut crate::commands::agent_commerce::task::common::network::task_api_client::TaskApiClient,
    asset: &str,
    agent_id: &str,
) -> Result<(String, u8)> {
    use crate::commands::agent_commerce::task::common::util::fetch_token_detail;

    let asset_lower = asset.to_lowercase();

    for symbol in &["USDT", "USDG"] {
        eprintln!("[resolve_token_by_asset] fetch_token_detail({symbol})");
        match fetch_token_detail(client, symbol, agent_id).await {
            Ok((addr, decimals)) => {
                eprintln!("[resolve_token_by_asset] {symbol} → addr={addr}, decimals={decimals}");
                if addr.to_lowercase() == asset_lower {
                    return Ok((symbol.to_string(), decimals as u8));
                }
            }
            Err(e) => {
                eprintln!("[resolve_token_by_asset] {symbol} lookup failed: {e}");
                continue;
            }
        }
    }

    bail!("asset {asset} is not in the task system's supported token list (checked: USDT, USDG)")
}

/// Convert a minimal-unit amount to a human-readable amount (pure string with decimal-point insertion, then `parse::<f64>` for display).
pub fn minimal_to_human(amount_minimal: &str, decimals: u8) -> Result<f64> {
    let d = decimals as usize;
    let s = amount_minimal.trim_start_matches('0');
    let s = if s.is_empty() { "0" } else { s };
    let human_str = if d == 0 {
        s.to_string()
    } else if s.len() <= d {
        format!("0.{:0>width$}", s, width = d)
    } else {
        let split = s.len() - d;
        format!("{}.{}", &s[..split], &s[split..])
    };
    human_str.parse::<f64>().context("minimal → f64 conversion failed")
}

/// Convert a human-readable amount string to a minimal-unit string (reuses swap's pure-string impl; no precision loss).
pub fn human_to_minimal(amount_human: &str, decimals: u8) -> Result<String> {
    crate::commands::swap::readable_to_minimal_str(amount_human, decimals as u32)
}

/// Enrich pricing info: resolve token symbol, decimals, and human-readable amount.
///
/// `decimals` priority: inline from accepts entry > token-info lookup.
pub async fn enrich_pricing(
    client: &mut crate::commands::agent_commerce::task::common::network::task_api_client::TaskApiClient,
    pricing: &X402Pricing,
    agent_id: &str,
) -> Result<X402PricingResolved> {
    eprintln!(
        "[enrich_pricing] asset={}, network={}, amount_minimal={}, extra_decimals={:?}",
        pricing.asset, pricing.network, pricing.amount_minimal, pricing.extra_decimals
    );
    let token_result = resolve_token_by_asset(client, &pricing.asset, agent_id).await;
    let (symbol, decimals) = match (&token_result, pricing.extra_decimals) {
        (Ok((sym, dec)), Some(extra_dec)) => {
            if *dec != extra_dec {
                eprintln!(
                    "⚠ decimals mismatch: accepts entry={extra_dec}, token info={dec}; using the accepts-entry value"
                );
            }
            (sym.clone(), extra_dec)
        }
        (Ok((sym, dec)), None) => (sym.clone(), *dec),
        (Err(e), Some(extra_dec)) => {
            eprintln!("⚠ token-info lookup failed: {e}; using accepts entry decimals={extra_dec}");
            ("UNKNOWN".to_string(), extra_dec)
        }
        (Err(e), None) => {
            bail!("cannot determine token decimals: token-info lookup failed ({e}) and the accepts entry does not provide a `decimals` field");
        }
    };
    eprintln!(
        "[enrich_pricing] result: symbol={symbol}, decimals={decimals}, extra_decimals={:?}, token_info={:?}",
        pricing.extra_decimals,
        token_result.as_ref().map(|(_, d)| *d).ok()
    );
    let amount_human = minimal_to_human(&pricing.amount_minimal, decimals)?;

    Ok(X402PricingResolved {
        amount_minimal: pricing.amount_minimal.clone(),
        amount_human,
        token_symbol: symbol,
        decimals,
        asset: pricing.asset.clone(),
        pay_to: pricing.pay_to.clone(),
        network: pricing.network.clone(),
        scheme: pricing.scheme.clone(),
        max_timeout_seconds: pricing.max_timeout_seconds,
    })
}

/// Check whether the x402 amount (minimal units) matches a human-readable amount (string).
///
/// Allows a tolerance of 1 minimal unit.
pub fn amounts_match(x402_amount_minimal: &str, fee_amount_human: &str, decimals: u8) -> bool {
    let Ok(x402_raw) = x402_amount_minimal.parse::<u128>() else { return false };
    let Ok(expected_str) = human_to_minimal(fee_amount_human, decimals) else { return false };
    let Ok(expected_raw) = expected_str.parse::<u128>() else { return false };
    x402_raw.abs_diff(expected_raw) <= 1
}

// ─── Header assembly ────────────────────────────────────────────────────

/// Assemble the payment header from the signing result and the 402 payload.
///
/// Returns `(header_name, header_value)`:
/// - v2 → `("PAYMENT-SIGNATURE", base64(...))`
/// - v1 → `("X-PAYMENT", base64(...))`
pub fn assemble_payment_header(
    proof: &X402PaymentProof,
    payload: &X402Payload,
) -> Result<(String, String)> {
    let payment_payload = if payload.x402_version >= 2 {
        // v2: pick the accepted entry for the matching scheme.
        // Since the CLI auto-selects the scheme (exact > aggr_deferred > first),
        // here we try to infer the scheme from session_cert.
        let scheme = if proof.session_cert.is_some() {
            "aggr_deferred"
        } else {
            "exact"
        };

        let mut accepted = payload
            .accepts
            .iter()
            .find(|a| a["scheme"].as_str() == Some(scheme))
            .cloned()
            .or_else(|| payload.accepts.first().cloned())
            .ok_or_else(|| anyhow!("accepts array is empty"))?;

        // aggr_deferred: merge sessionCert into extra
        if let Some(ref cert) = proof.session_cert {
            if let Some(extra) = accepted.get_mut("extra") {
                extra["sessionCert"] = json!(cert);
            } else {
                accepted["extra"] = json!({ "sessionCert": cert });
            }
        }

        json!({
            "x402Version": payload.x402_version,
            "resource": payload.resource,
            "accepted": accepted,
            "payload": {
                "signature": proof.signature,
                "authorization": proof.authorization,
            }
        })
    } else {
        // v1
        let network = payload
            .accepts
            .first()
            .and_then(|a| a["network"].as_str())
            .unwrap_or("");
        let scheme = if proof.session_cert.is_some() {
            "aggr_deferred"
        } else {
            "exact"
        };
        json!({
            "x402Version": 1,
            "scheme": scheme,
            "network": network,
            "payload": {
                "signature": proof.signature,
                "authorization": proof.authorization,
            }
        })
    };

    let header_value = B64.encode(serde_json::to_string(&payment_payload)?);
    let header_name = if payload.x402_version >= 2 {
        "PAYMENT-SIGNATURE"
    } else {
        "X-PAYMENT"
    };

    Ok((header_name.to_string(), header_value))
}

