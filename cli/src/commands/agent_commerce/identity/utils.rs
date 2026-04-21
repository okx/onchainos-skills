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

pub(super) fn wallet_client(_ctx: &Context) -> Result<WalletApiClient> {
    WalletApiClient::new()
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
        bail!("missing required field in --service: ServiceName");
    }
    if service.service_description.is_empty() {
        bail!("missing required field in --service: ServiceDescription");
    }
    match service.service_type.as_str() {
        "A2A" => {
            // Product spec: A2A services do not have an Endpoint field.
            service.endpoint = None;
        }
        "A2MCP" => {
            if service.fee.is_empty() {
                bail!("missing required field in --service for A2MCP: Fee");
            }
            if service.endpoint.is_none() {
                bail!("missing required field in --service for A2MCP: Endpoint");
            }
        }
        other => bail!("invalid ServiceType in --service: {other}"),
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

pub(super) fn resolve_agent_id<'a>(
    agent_id: &'a Option<String>,
    agent_id_flag: &'a Option<String>,
) -> Result<&'a str> {
    if let Some(agent_id) = agent_id.as_deref().filter(|value| !value.trim().is_empty()) {
        return Ok(agent_id.trim());
    }
    if let Some(agent_id) = agent_id_flag
        .as_deref()
        .filter(|value| !value.trim().is_empty())
    {
        return Ok(agent_id.trim());
    }
    bail!("missing required parameter: agentId")
}

pub(super) fn require_non_empty<'a>(value: Option<&'a str>, flag: &str) -> Result<&'a str> {
    match value.map(str::trim).filter(|value| !value.is_empty()) {
        Some(value) => Ok(value),
        None => bail!("missing required parameter: {flag}"),
    }
}

pub(super) fn trim_or_empty(value: Option<&str>) -> String {
    value.unwrap_or("").trim().to_string()
}

pub(super) fn resolve_update_string(
    new_value: Option<&str>,
    current_value: Option<&str>,
    flag: &str,
) -> Result<String> {
    if let Some(value) = new_value {
        let trimmed = value.trim();
        if trimmed.is_empty() {
            bail!("missing required parameter: {flag}");
        }
        return Ok(trimmed.to_string());
    }
    if let Some(value) = current_value {
        return Ok(value.to_string());
    }
    bail!("missing required parameter: {flag}")
}

pub(super) fn resolve_optional_update_string(
    new_value: Option<&str>,
    current_value: Option<&str>,
) -> String {
    if let Some(value) = new_value {
        value.trim().to_string()
    } else {
        current_value.unwrap_or("").to_string()
    }
}

pub(super) fn ensure_provider_has_service(card: &AgentCard) -> Result<()> {
    if card.role == "provider" && card.services.is_empty() {
        bail!("provider agents require at least one service; provide --service");
    }
    Ok(())
}

pub(super) fn resolve_update_services(
    new_value: Option<&str>,
    current_value: Option<&Vec<AgentService>>,
) -> Result<Vec<AgentService>> {
    if new_value.is_some() {
        return parse_services(new_value);
    }
    Ok(current_value.cloned().unwrap_or_default())
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
