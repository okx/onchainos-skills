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
const WS_URL_PROD: &str = "wss://wsdex.okx.com:8443/ws/v5/private";

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

/// Normalize a user-supplied language tag into a canonically-cased BCP-47 tag,
/// then apply product-level default-region completion.
///
/// Casing (per RFC 5646 conventions): language subtag lowercased, script subtag
/// (4 alpha) title-cased, region subtag (2 alpha / 3 digit) uppercased,
/// everything else lowercased. So `zh_CN` / `ZH-cn` → `zh-CN`, `en_us` →
/// `en-US`, `zh-hant-tw` → `zh-Hant-TW`.
///
/// Default-region completion (product policy — see [`default_region_complete`]):
/// a *bare* primary-language tag is mapped to the product's canonical region
/// (`zh` → `zh-CN`, `en` → `en-US`, `ja` → `ja-JP`) so the backend localizes to
/// a concrete locale rather than its own default. This is deliberately NOT part
/// of BCP-47 canonicalization (`zh` and `zh-CN` are distinct tags); we add the
/// region on purpose. Tags that already carry a script/region/variant
/// (`zh-TW`, `zh-Hant`) are left untouched.
///
/// Returns `None` when the input is blank or the leading language subtag is not
/// a well-formed 2–8 alpha code — callers omit `preferredLanguage` in that case
/// so the backend falls back to its own default. Note this is a casing
/// normalizer, not a full BCP-47 validator: subtags after the language are
/// re-cased but not structurally validated, and grandfathered / deprecated tags
/// are not specially handled.
pub(super) fn normalize_bcp47(value: Option<&str>) -> Option<String> {
    let raw = value?.trim();
    if raw.is_empty() {
        return None;
    }
    let mut subtags = raw.split(['-', '_']).filter(|s| !s.is_empty());

    let language = subtags.next()?;
    if !(2..=8).contains(&language.len()) || !language.chars().all(|c| c.is_ascii_alphabetic()) {
        return None;
    }
    let mut out = language.to_ascii_lowercase();

    for subtag in subtags {
        out.push('-');
        let is_alpha = subtag.chars().all(|c| c.is_ascii_alphabetic());
        let is_digit = subtag.chars().all(|c| c.is_ascii_digit());
        if subtag.len() == 4 && is_alpha {
            // script: title-case (e.g. `Hant`)
            let mut chars = subtag.chars();
            out.push(chars.next().unwrap().to_ascii_uppercase());
            out.extend(chars.map(|c| c.to_ascii_lowercase()));
        } else if (subtag.len() == 2 && is_alpha) || (subtag.len() == 3 && is_digit) {
            // region: uppercase (e.g. `CN`, `001`)
            out.push_str(&subtag.to_ascii_uppercase());
        } else {
            out.push_str(&subtag.to_ascii_lowercase());
        }
    }
    Some(default_region_complete(out))
}

/// Product-level default-region completion for a *bare* primary-language tag.
///
/// When `tag` carries no further subtag (no `-`), map the supported languages
/// to their canonical product region so the backend resolves a concrete locale;
/// unmapped languages and any tag that already has a script/region/variant pass
/// through unchanged. Not BCP-47 canonicalization — see [`normalize_bcp47`].
fn default_region_complete(tag: String) -> String {
    if tag.contains('-') {
        return tag;
    }
    match tag.as_str() {
        "zh" => "zh-CN".to_string(),
        "en" => "en-US".to_string(),
        "ja" => "ja-JP".to_string(),
        _ => tag,
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

// ─── Rating: 0.00–5.00 stars (CLI surface) ↔ 0–100 score (backend wire) ───
//
// Single source of truth for the conversion. The CLI takes user input in
// stars with up to 2 decimal places (step 0.01) and renders 2-decimal
// stars in responses; the wire format with the backend remains 0–100
// integers. Skills no longer need to do the multiplication themselves —
// earlier revisions pushed that onto the skill, which was fragile because
// skills are prompt-driven; a forgetful prompt would send raw stars to
// the wire and corrupt the rating.
//
// All conversions use **round-half-up** at the displayed precision —
// consistent with the canonical rule pinned in
// `skills/okx-agent-identity/SKILL.md` §Amount Display Rules. Note that
// the wire (0..=100 integer) gives an effective storage grain of 0.05
// stars per wire unit, so distinct 2-decimal inputs whose ×20 product
// rounds to the same integer collapse on the wire (e.g. 3.30 / 3.31 /
// 3.32 all → wire 66). That is a wire limitation, not a parser bug.

/// Parse a `--score` CLI argument: 0.00–5.00 stars with up to 2 decimal
/// places, returning the 0–100 backend wire value (round-half-up). Pure
/// integer arithmetic to avoid float drift on inputs like 3.33.
pub(super) fn parse_stars_arg(value: &str, flag: &str) -> Result<u32> {
    let trimmed = value.trim();
    let error = || anyhow!("invalid value for {flag}: expected 0.00–5.00 (up to 2 decimal places)");
    let (int_str, frac_str) = match trimmed.split_once('.') {
        Some((int_str, frac_str)) => {
            if frac_str.is_empty() || frac_str.len() > 2 {
                return Err(error());
            }
            (int_str, frac_str)
        }
        None => (trimmed, ""),
    };
    if int_str.is_empty() || !int_str.bytes().all(|b| b.is_ascii_digit()) {
        return Err(error());
    }
    if !frac_str.bytes().all(|b| b.is_ascii_digit()) {
        return Err(error());
    }
    let int_val: u32 = int_str.parse().map_err(|_| error())?;
    let frac_val: u32 = match frac_str.len() {
        0 => 0,
        1 => frac_str.parse::<u32>().map_err(|_| error())? * 10,
        2 => frac_str.parse::<u32>().map_err(|_| error())?,
        _ => unreachable!(),
    };
    // stars in cents: 0..=500 corresponds to 0.00..=5.00.
    let stars_cents = int_val
        .checked_mul(100)
        .and_then(|v| v.checked_add(frac_val))
        .ok_or_else(error)?;
    if stars_cents > 500 {
        bail!("invalid value for {flag}: must be between 0.00 and 5.00");
    }
    // stars × 20 = stars_cents / 5. Round-half-up: (n + 2) / 5. The half
    // boundary (n % 5 == 2.5) is unreachable for integer n, so this is
    // exact, not an approximation.
    Ok((stars_cents + 2) / 5)
}

/// 0–100 backend score → 0.00–5.00 star rating (2 decimals, exact).
/// Used for both per-review entries and aggregate `average` since the
/// CLI now accepts 2-decimal input and the wire grain matches.
pub(super) fn score_to_stars(score: u64) -> f64 {
    (score.min(100) * 5) as f64 / 100.0
}

/// In-place convert score-like fields in a feedback-list response from
/// 0–100 backend ints to 0.00–5.00 star floats.
///
/// Conversions applied (each only when the field exists and is numeric):
///   - top-level `average` → 2-decimal stars
///   - `items[*].score`     → 2-decimal stars
///   - `list[*].score`      → 2-decimal stars (alternate field name;
///     backend is inconsistent across endpoints — see `agent-list` which
///     uses `list`, so accept either; only one will actually be present)
pub(super) fn convert_feedback_list_scores(v: &mut Value) {
    let convert = |score: u64| serde_json::Number::from_f64(score_to_stars(score));
    if let Value::Object(map) = v {
        if let Some(score) = map.get("average").and_then(Value::as_u64) {
            if let Some(num) = convert(score) {
                map.insert("average".to_string(), Value::Number(num));
            }
        }
        for key in ["items", "list"] {
            if let Some(Value::Array(arr)) = map.get_mut(key) {
                for entry in arr.iter_mut() {
                    if let Value::Object(entry_map) = entry {
                        if let Some(score) = entry_map.get("score").and_then(Value::as_u64) {
                            if let Some(num) = convert(score) {
                                entry_map.insert("score".to_string(), Value::Number(num));
                            }
                        }
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    // ─── parse_stars_arg: happy path ─────────────────────────────────────

    #[test]
    fn parse_stars_arg_accepts_integers() {
        assert_eq!(parse_stars_arg("0", "--score").unwrap(), 0);
        assert_eq!(parse_stars_arg("1", "--score").unwrap(), 20);
        assert_eq!(parse_stars_arg("5", "--score").unwrap(), 100);
    }

    #[test]
    fn parse_stars_arg_accepts_one_and_two_decimals() {
        assert_eq!(parse_stars_arg("4.5", "--score").unwrap(), 90);
        assert_eq!(parse_stars_arg("5.00", "--score").unwrap(), 100);
        assert_eq!(parse_stars_arg("0.01", "--score").unwrap(), 0); // 0.2 → round to 0
        assert_eq!(parse_stars_arg("0.03", "--score").unwrap(), 1); // 0.6 → round to 1
    }

    #[test]
    fn parse_stars_arg_round_half_up_at_wire_boundary() {
        // 3.30 / 3.31 / 3.32 all collapse to wire 66 (0.05-star grain).
        assert_eq!(parse_stars_arg("3.30", "--score").unwrap(), 66);
        assert_eq!(parse_stars_arg("3.31", "--score").unwrap(), 66);
        assert_eq!(parse_stars_arg("3.32", "--score").unwrap(), 66);
        // 3.33 rounds up to wire 67 (= 66.6 round-half-up).
        assert_eq!(parse_stars_arg("3.33", "--score").unwrap(), 67);
        // 3.35 is exact (no rounding needed).
        assert_eq!(parse_stars_arg("3.35", "--score").unwrap(), 67);
        // Upper-edge: 4.97 → 99.4 → 99; 4.98 / 4.99 → 100.
        assert_eq!(parse_stars_arg("4.97", "--score").unwrap(), 99);
        assert_eq!(parse_stars_arg("4.98", "--score").unwrap(), 100);
        assert_eq!(parse_stars_arg("4.99", "--score").unwrap(), 100);
    }

    #[test]
    fn parse_stars_arg_trims_whitespace() {
        assert_eq!(parse_stars_arg("  4.5  ", "--score").unwrap(), 90);
    }

    // ─── parse_stars_arg: rejected inputs ────────────────────────────────

    #[test]
    fn parse_stars_arg_rejects_more_than_two_decimals() {
        assert!(parse_stars_arg("3.333", "--score").is_err());
        assert!(parse_stars_arg("0.001", "--score").is_err());
    }

    #[test]
    fn parse_stars_arg_rejects_trailing_dot() {
        assert!(parse_stars_arg("3.", "--score").is_err());
    }

    #[test]
    fn parse_stars_arg_rejects_signs_and_exponent() {
        assert!(parse_stars_arg("-1", "--score").is_err());
        assert!(parse_stars_arg("+5", "--score").is_err());
        assert!(parse_stars_arg("5e0", "--score").is_err());
    }

    #[test]
    fn parse_stars_arg_rejects_out_of_range() {
        assert!(parse_stars_arg("6", "--score").is_err());
        assert!(parse_stars_arg("5.01", "--score").is_err());
    }

    #[test]
    fn parse_stars_arg_rejects_non_numeric() {
        assert!(parse_stars_arg("abc", "--score").is_err());
        assert!(parse_stars_arg("3.3.3", "--score").is_err());
        assert!(parse_stars_arg("", "--score").is_err());
        assert!(parse_stars_arg("   ", "--score").is_err());
    }

    // ─── score_to_stars: wire (0..=100) → stars (0.0..=5.0) ──────────────

    #[test]
    fn score_to_stars_is_exact_at_two_decimals() {
        assert_eq!(score_to_stars(0), 0.0);
        assert_eq!(score_to_stars(66), 3.3);
        assert_eq!(score_to_stars(67), 3.35);
        assert_eq!(score_to_stars(70), 3.5);
        assert_eq!(score_to_stars(89), 4.45);
        assert_eq!(score_to_stars(90), 4.5);
        assert_eq!(score_to_stars(100), 5.0);
    }

    #[test]
    fn score_to_stars_clamps_above_100() {
        assert_eq!(score_to_stars(101), 5.0);
        assert_eq!(score_to_stars(u64::MAX), 5.0);
    }

    // ─── convert_feedback_list_scores: average + items + list ────────────

    fn assert_score_eq(v: &Value, expected: f64) {
        let got = v.as_f64().expect("expected numeric");
        assert!(
            (got - expected).abs() < 1e-9,
            "expected {expected}, got {got}"
        );
    }

    #[test]
    fn convert_feedback_list_scores_rewrites_average_and_items() {
        let mut v = json!({
            "average": 89,
            "items": [
                { "score": 90 },
                { "score": 70 },
                { "score": 67 },
            ],
        });
        convert_feedback_list_scores(&mut v);
        assert_score_eq(&v["average"], 4.45);
        assert_score_eq(&v["items"][0]["score"], 4.5);
        assert_score_eq(&v["items"][1]["score"], 3.5);
        assert_score_eq(&v["items"][2]["score"], 3.35);
    }

    #[test]
    fn convert_feedback_list_scores_rewrites_list_field() {
        let mut v = json!({ "list": [ { "score": 100 } ] });
        convert_feedback_list_scores(&mut v);
        assert_score_eq(&v["list"][0]["score"], 5.0);
    }

    #[test]
    fn convert_feedback_list_scores_leaves_non_numeric_fields_alone() {
        let mut v = json!({
            "average": "n/a",
            "items": [
                { "score": "n/a" },
                { "other_field": 5 },
            ],
        });
        let before = v.clone();
        convert_feedback_list_scores(&mut v);
        assert_eq!(v, before);
    }

    // ─── normalize_bcp47 ─────────────────────────────────────────────────

    #[test]
    fn normalize_bcp47_canonicalizes_casing_and_separator() {
        assert_eq!(normalize_bcp47(Some("zh-CN")).as_deref(), Some("zh-CN"));
        assert_eq!(normalize_bcp47(Some("zh_CN")).as_deref(), Some("zh-CN"));
        assert_eq!(normalize_bcp47(Some("ZH-cn")).as_deref(), Some("zh-CN"));
        assert_eq!(normalize_bcp47(Some("en_us")).as_deref(), Some("en-US"));
        assert_eq!(
            normalize_bcp47(Some("zh-hant-tw")).as_deref(),
            Some("zh-Hant-TW")
        );
        assert_eq!(normalize_bcp47(Some("  en-US  ")).as_deref(), Some("en-US"));
    }

    #[test]
    fn normalize_bcp47_default_region_completes_bare_language() {
        // Bare supported languages get the product's canonical region.
        assert_eq!(normalize_bcp47(Some("zh")).as_deref(), Some("zh-CN"));
        assert_eq!(normalize_bcp47(Some("ZH")).as_deref(), Some("zh-CN"));
        assert_eq!(normalize_bcp47(Some("en")).as_deref(), Some("en-US"));
        assert_eq!(normalize_bcp47(Some("ja")).as_deref(), Some("ja-JP"));
        // Unmapped bare languages pass through unchanged.
        assert_eq!(normalize_bcp47(Some("fr")).as_deref(), Some("fr"));
        // Tags that already carry a region / script are NOT overridden.
        assert_eq!(normalize_bcp47(Some("zh-TW")).as_deref(), Some("zh-TW"));
        assert_eq!(normalize_bcp47(Some("zh-Hant")).as_deref(), Some("zh-Hant"));
        assert_eq!(normalize_bcp47(Some("en-GB")).as_deref(), Some("en-GB"));
    }

    #[test]
    fn normalize_bcp47_rejects_blank_and_malformed_language() {
        assert_eq!(normalize_bcp47(None), None);
        assert_eq!(normalize_bcp47(Some("")), None);
        assert_eq!(normalize_bcp47(Some("   ")), None);
        assert_eq!(normalize_bcp47(Some("1-CN")), None); // language subtag not alpha
        assert_eq!(normalize_bcp47(Some("z")), None); // too short
    }
}
