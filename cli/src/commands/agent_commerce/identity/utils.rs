//! Stateless helpers shared by queries / mutations / signing: HTTP client
//! factory, logging formatters, CLI arg validators, JSON normalization, and
//! agent service / role parsing. No network calls, no signing. Functions
//! here are deliberately small and dependency-light.

use anyhow::{anyhow, bail, Context as _, Result};
use serde_json::Value;

use crate::commands::Context;
use crate::wallet_api::{UnsignedInfoResponse, WalletApiClient};

use super::models::{AgentCard, AgentService};

// ─── HTTP client ──────────────────────────────────────────────────────────

/// Build the wallet HTTP client honoring `--base-url`. Forwards
/// `ctx.base_url_override` to `WalletApiClient::with_base_url` so the
/// override is actually applied (precedence inside `with_base_url`:
/// runtime `OKX_BASE_URL` > compile-time `OKX_BASE_URL` > override >
/// `DEFAULT_BASE_URL`).
pub(super) fn wallet_client(ctx: &Context) -> Result<WalletApiClient> {
    WalletApiClient::with_base_url(ctx.base_url_override.as_deref())
}

// ─── Logging helpers ──────────────────────────────────────────────────────

pub(super) fn redact_token_for_debug(token: &str) -> String {
    if token.len() <= 16 {
        return format!("{token}***");
    }
    format!("{}***{}", &token[..8], &token[token.len() - 6..])
}

// Log-only helpers. Precedence mirrors WalletApiClient::with_base_url:
// compile-time OKX_BASE_URL > ctx.base_url_override > DEFAULT_BASE_URL.
// Note: reconstruct_get_url_for_log does NOT percent-encode values, so the
// logged URL may diverge from the actual wire URL when values contain
// characters that wallet_api::build_query_string would escape.
fn resolve_base_url_for_log(ctx: &Context) -> String {
    option_env!("OKX_BASE_URL")
        .map(str::to_string)
        .or_else(|| ctx.base_url_override.clone())
        .unwrap_or_else(|| crate::client::DEFAULT_BASE_URL.to_string())
}

/// Production WS endpoint for the `wallet-agentic-identity` push channel.
/// Mirrors the `WS_URL_PROD` / `WS_URL_PRE` + `ONCHAINOS_WS_URL` env-
/// override pattern in `cli/src/watch/daemon.rs:18-19,134` (same WS
/// gateway host, different per-service path: `/ws/v5/private` here vs
/// `/ws/v6/dex` for the watch dex feed). Identity keeps its own
/// constant rather than importing from `watch/` so identity-side
/// changes never risk regressing the watch daemon's contract.
const WS_URL_PROD: &str = "wss://wsdex.okx.com/ws/v5/private";

/// Resolve the full WS URL for the `wallet-agentic-identity` push
/// channel. Precedence:
///   1. runtime `OKX_AGENTIC_WS_URL` — explicit override, full URL
///      including `/ws/v5/private` path (escape hatch for forked /
///      pre / debug envs; production leaves it unset).
///   2. `WS_URL_PROD` constant — production default.
///
/// Identity does not derive this URL from the HTTP base — the WS push
/// service runs on a separate host (`wsdex.okx.com`) from the HTTP API
/// (`web3.okx.com`), so scheme swap on the HTTP base would land WS on
/// the wrong host.
///
/// **Breaking change vs. earlier revisions**: prior to this refactor the
/// WS URL was derived from `--base-url` / runtime `OKX_BASE_URL` /
/// compile-time `OKX_BASE_URL` via scheme swap (`http→ws`, `https→wss`)
/// with `/ws/v5/private` appended. **That coupling is gone.** Setting
/// `--base-url` (or either `OKX_BASE_URL` flavor) now only affects HTTP
/// calls; the WS subscription always uses `WS_URL_PROD` unless
/// `OKX_AGENTIC_WS_URL` is also set. The failure mode is **silent
/// degradation**, not an error: if you point HTTP at a pre / forked env
/// without also pointing `OKX_AGENTIC_WS_URL` at the matching WS host,
/// `agent create` / `agent update` will still succeed (broadcast +
/// agentList come from HTTP), but the `agent` field in the response
/// envelope will be absent because the WS push never lands on the right
/// host. Migration: when switching HTTP targets, also set
/// `OKX_AGENTIC_WS_URL` to the corresponding WS endpoint.
pub(super) fn identity_ws_url() -> String {
    std::env::var("OKX_AGENTIC_WS_URL")
        .unwrap_or_else(|_| WS_URL_PROD.to_string())
}

pub(super) fn reconstruct_post_url_for_log(ctx: &Context, path: &str) -> String {
    format!("{}{}", resolve_base_url_for_log(ctx), path)
}

pub(super) fn reconstruct_get_url_for_log(
    ctx: &Context,
    path: &str,
    query: &[(&str, &str)],
) -> String {
    let base = resolve_base_url_for_log(ctx);
    let filtered: Vec<&(&str, &str)> = query.iter().filter(|(_, v)| !v.is_empty()).collect();
    if filtered.is_empty() {
        return format!("{base}{path}");
    }
    let pairs: Vec<String> = filtered
        .iter()
        .map(|(k, v)| format!("{k}={v}"))
        .collect();
    format!("{base}{path}?{}", pairs.join("&"))
}

// ─── HTTP query building ──────────────────────────────────────────────────

pub(super) fn push_optional_query(
    query: &mut Vec<(String, String)>,
    key: &str,
    value: Option<&str>,
) {
    if let Some(value) = value.filter(|value| !value.trim().is_empty()) {
        query.push((key.to_string(), value.trim().to_string()));
    }
}

pub(super) fn push_multi_query(query: &mut Vec<(String, String)>, key: &str, values: &[String]) {
    for value in values {
        if !value.trim().is_empty() {
            query.push((key.to_string(), value.trim().to_string()));
        }
    }
}

// ─── Response shape helpers ───────────────────────────────────────────────

pub(super) fn normalize_singleton_object(data: Value) -> Value {
    match data {
        Value::Array(mut arr) if arr.len() == 1 && arr[0].is_object() => arr.remove(0),
        other => other,
    }
}

pub(super) fn parse_agent_unsigned(data: Value) -> Result<UnsignedInfoResponse> {
    let item = data
        .as_array()
        .and_then(|arr| arr.first())
        .cloned()
        .ok_or_else(|| anyhow!("pre-transaction response is empty"))?;
    serde_json::from_value(item).context("failed to parse pre-transaction response")
}

// ─── Service / Role parsing ───────────────────────────────────────────────

pub(super) fn parse_services(raw: Option<&str>) -> Result<Vec<AgentService>> {
    let Some(raw) = raw else {
        return Ok(Vec::new());
    };
    let services: Vec<AgentService> =
        serde_json::from_str(raw).context("failed to parse --service as JSON array")?;
    services
        .into_iter()
        .map(normalize_service)
        .collect::<Result<Vec<_>>>()
}

pub(super) fn normalize_service(mut service: AgentService) -> Result<AgentService> {
    if service
        .id
        .as_deref()
        .map(str::trim)
        .unwrap_or("")
        .is_empty()
    {
        service.id = None;
    } else {
        service.id = Some(service.id.unwrap().trim().to_string());
    }
    service.service_name = service.service_name.trim().to_string();
    service.service_description = service.service_description.trim().to_string();
    service.fee = service.fee.trim().to_string();
    service.service_type = service.service_type.trim().to_ascii_uppercase();
    service.endpoint = service
        .endpoint
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty());

    if service.service_name.is_empty() {
        bail!("missing required field in --service: name");
    }
    if service.service_description.is_empty() {
        bail!("missing required field in --service: servicedescription");
    }
    match service.service_type.as_str() {
        "A2A" => {
            // Product spec: A2A services do not have an endpoint field.
            service.endpoint = None;
        }
        "A2MCP" => {
            if service.fee.is_empty() {
                bail!("missing required field in --service for A2MCP: fee");
            }
            if service.endpoint.is_none() {
                bail!("missing required field in --service for A2MCP: endpoint");
            }
        }
        other => bail!("invalid servicetype in --service: {other}"),
    }

    Ok(service)
}

pub(super) fn normalize_role(role: &str) -> Result<String> {
    match role.trim().to_ascii_lowercase().as_str() {
        "1" | "buyer" | "requestor" | "requester" => Ok("requester".to_string()),
        "2" | "provider" => Ok("provider".to_string()),
        "3" | "evaluator" => Ok("evaluator".to_string()),
        other => bail!("invalid value for --role: {other}"),
    }
}

// ─── CLI arg helpers ──────────────────────────────────────────────────────

pub(super) fn require_non_empty<'a>(value: Option<&'a str>, flag: &str) -> Result<&'a str> {
    match value.map(str::trim).filter(|value| !value.is_empty()) {
        Some(value) => Ok(value),
        None => bail!("missing required parameter: {flag}"),
    }
}

pub(super) fn trim_or_empty(value: Option<&str>) -> String {
    value.unwrap_or("").trim().to_string()
}

pub(super) fn ensure_provider_has_service(card: &AgentCard) -> Result<()> {
    if card.role == "provider" && card.services.is_empty() {
        bail!("provider agents require at least one service; provide --service");
    }
    Ok(())
}

pub(super) fn parse_u32_arg(
    value: Option<&str>,
    flag: &str,
    default: u32,
    min: Option<u32>,
    max: Option<u32>,
    clamp_max: bool,
) -> Result<u32> {
    let Some(value) = value else {
        return Ok(default);
    };
    let parsed = value
        .trim()
        .parse::<u32>()
        .map_err(|_| anyhow!("invalid value for {flag}: expected integer"))?;
    if let Some(min) = min {
        if parsed < min {
            bail!("invalid value for {flag}: must be >= {min}");
        }
    }
    if let Some(max) = max {
        if parsed > max {
            if clamp_max {
                return Ok(max);
            }
            bail!("invalid value for {flag}: must be <= {max}");
        }
    }
    Ok(parsed)
}
