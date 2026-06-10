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

// ─── `agent get` row enrichment (computed display fields) ─────────────────
//
// The CLI does the enum→label MAPPING so the prompt-driven skill layer never
// miscomputes it. All labels are English-canonical (the skill still localizes
// the surrounding prose). Each helper returns `None` for unknown / absent
// inputs so the corresponding key is simply omitted (additive, null-safe) —
// the skill renders its own fallback ("No rating yet", etc.).
//
// NOTE: the `map.insert` calls below (roleLabel / statusLabel / approvalLabel /
// ratingStars / card / cells) are INTENTIONALLY unconditional overwrites — the
// CLI-computed display value is authoritative. Do NOT "harden" these into
// `if !contains_key` guards: those field names are CLI-invented (absent from
// the ERC-8004 wire schema), so there is nothing real to clobber, and an
// `if !contains_key` guard would wrongly preserve a stale/partial value over
// the freshly-computed one.

/// Map the raw `role` enum to its canonical English label.
/// Unknown → `None` (omit; do not guess).
fn role_label(role: &str) -> Option<&'static str> {
    match role.trim() {
        "requester" => Some("User Agent"),
        "provider" => Some("Agent Service Provider (ASP)"),
        "evaluator" => Some("Evaluator Agent"),
        _ => None,
    }
}

/// Map a raw `role` value to its canonical English label, accepting BOTH the
/// string enum (`"provider"`) AND the backend integer the live `/agent-list`
/// endpoint actually returns (`1` requester / `2` provider / `3` evaluator —
/// same aliasing as `create`'s `--role`). Unknown → `None`.
fn role_label_from_value(role: &Value) -> Option<&'static str> {
    match role {
        Value::String(s) => role_label(s),
        Value::Number(n) => match n.as_u64()? {
            1 => Some("User Agent"),
            2 => Some("Agent Service Provider (ASP)"),
            3 => Some("Evaluator Agent"),
            _ => None,
        },
        _ => None,
    }
}

/// True when the row's `role` is the provider role, accepting both the string
/// enum (`"provider"`) and the backend integer (`2`). Gates the provider-only
/// service rows in `build_agent_card`.
fn role_is_provider(role: Option<&Value>) -> bool {
    match role {
        Some(Value::String(s)) => s.trim() == "provider",
        Some(Value::Number(n)) => n.as_u64() == Some(2),
        _ => false,
    }
}

/// Apply `f` to every agent-row object in an `agent get` (`/agent-list`)
/// envelope, tolerating BOTH shapes the backend has used:
///   • single-layer  `list[*]`              — the live `/agent-list` today
///   • double-layer   `list[*].agentList[*]` — the older grouped doc schema
/// A `list[*]` element is treated as a wrapper iff it carries an `agentList`
/// array; otherwise the element IS the agent row. No-op when `list` is absent.
fn for_each_agent_row(v: &mut Value, mut f: impl FnMut(&mut Value)) {
    let Some(items) = v.get_mut("list").and_then(Value::as_array_mut) else {
        return;
    };
    for item in items.iter_mut() {
        match item.get_mut("agentList").and_then(Value::as_array_mut) {
            Some(rows) => {
                for row in rows.iter_mut() {
                    f(row);
                }
            }
            None => f(item),
        }
    }
}

/// Map the raw `status` (int or string) to its canonical English label per
/// `SKILL.md §Invariants Lexicon`. `1`/`active` → active, `2` → not listed,
/// `3`/`4`/`5` → unavailable. Unknown → `None`.
fn status_label(status: &Value) -> Option<&'static str> {
    let key = match status {
        Value::Number(n) => n.as_u64().map(|n| n.to_string())?,
        Value::String(s) => s.trim().to_string(),
        _ => return None,
    };
    match key.as_str() {
        "1" | "active" => Some("active"),
        "2" => Some("not listed"),
        "3" | "4" | "5" => Some("unavailable"),
        _ => None,
    }
}

/// Map `approvalDisplayStatus` (integer) to its canonical English label. The
/// value rides on the double-layer `agent get` envelope (`list[*].agentList[*]`).
/// Unknown / absent → `None`. The remark is NOT concatenated here — the skill
/// appends it when status is 5.
fn approval_label(status: u64) -> Option<&'static str> {
    match status {
        1 => Some("Not listed"),
        2 => Some("Listing under review"),
        4 => Some("Listed — eligible for task recommendations"),
        5 => Some("Listing rejected"),
        7 => Some("This agent is currently unavailable"),
        _ => None,
    }
}

/// Compute the `ratingStars` display string from a 0–100 `reputation.score`:
/// `score / 20` with up to 2 decimals, trailing zeros trimmed (`92` → "4.6",
/// `89` → "4.45", `100` → "5"). Returns `None` when there is no usable
/// reputation (so the key is omitted and the skill renders "No rating yet").
fn rating_stars(reputation: &Value) -> Option<String> {
    // No rating yet when count is missing or 0.
    let count = reputation.get("count").and_then(Value::as_u64);
    if count == Some(0) {
        return None;
    }
    let score = reputation.get("score").and_then(Value::as_u64)?;
    Some(format_rating_stars(score))
}

/// Format a 0–100 score as `score/20` stars: up to 2 decimals, trailing
/// zeros (and a bare trailing dot) trimmed. Exact at 2 decimals because the
/// wire grain is one wire unit = 0.05 stars.
fn format_rating_stars(score: u64) -> String {
    let score = score.min(100);
    // Work in hundredths of a star to stay exact: score/20 = score*5 / 100.
    let hundredths = score * 5; // 0..=500
    let whole = hundredths / 100;
    let frac = hundredths % 100; // 0..=99, multiple of 5
    if frac == 0 {
        whole.to_string()
    } else if frac.is_multiple_of(10) {
        format!("{whole}.{}", frac / 10)
    } else {
        format!("{whole}.{frac:02}")
    }
}

/// Enrich a `agent get` response in place: for every agent row — read from
/// the single-layer shape (`list[*]`) or the legacy double-layer shape
/// (`list[*].agentList[*]`), both tolerated by `for_each_agent_row` — ADD the
/// computed `roleLabel` / `statusLabel` / `approvalLabel` / `ratingStars`
/// fields. Raw fields are never removed or altered. Each field is added only
/// when its source maps to a known value.
pub(super) fn enrich_agent_get_rows(v: &mut Value) {
    for_each_agent_row(v, enrich_agent_row);
}

/// Add the 4 computed display fields to a single agent-row object. No-op for
/// non-object values and for sources that don't map to a known label.
fn enrich_agent_row(row: &mut Value) {
    let Value::Object(map) = row else {
        return;
    };

    if let Some(label) = map.get("role").and_then(role_label_from_value) {
        map.insert("roleLabel".to_string(), Value::String(label.to_string()));
    }

    if let Some(label) = map.get("status").and_then(status_label) {
        map.insert("statusLabel".to_string(), Value::String(label.to_string()));
    }

    if let Some(label) = map
        .get("approvalDisplayStatus")
        .and_then(Value::as_u64)
        .and_then(approval_label)
    {
        map.insert(
            "approvalLabel".to_string(),
            Value::String(label.to_string()),
        );
    }

    if let Some(stars) = map.get("reputation").and_then(rating_stars) {
        map.insert("ratingStars".to_string(), Value::String(stars));
    }

    // Additive: a fully-assembled, ordered detail card the skill renders by
    // iterating + localizing the canonical-English `label`. Built from the
    // SAME row data (raw fields stay untouched).
    let card = build_agent_card(map);
    if !card.is_empty() {
        map.insert("card".to_string(), Value::Array(card));
    }
}

// ─── `card`: ordered, ready-to-render detail-card rows ────────────────────
//
// Mirrors `skills/okx-agent-identity/SKILL.md §Invariants Card skeleton` +
// `references/discover.md §detail` exactly:
// one ordered `{ "label": <canonical-English>, "value": <string> }` row per
// visible field, omitting a row when its value is unavailable (same omit
// rules the skill uses today). Service rows are PROVIDER-ONLY — the
// provider-vs-requester/evaluator filtering lives here so the skill never has
// to guard it. Labels are canonical English; the skill localizes them.

/// Build one `{ label, value }` card row.
fn card_row(label: &str, value: impl Into<String>) -> Value {
    serde_json::json!({ "label": label, "value": value.into() })
}

/// Short-form address: first 4 + last 4 hex chars of a `0x…` address
/// (`0xABCD…WXYZ`). Returns `None` when the value isn't a usable hex address.
fn short_address(address: &str) -> Option<String> {
    let address = address.trim();
    let hex = address.strip_prefix("0x").or_else(|| address.strip_prefix("0X"))?;
    if hex.len() < 8 || !hex.bytes().all(|b| b.is_ascii_hexdigit()) {
        return None;
    }
    Some(format!("0x{}…{}", &hex[..4], &hex[hex.len() - 4..]))
}

/// Read a non-empty trimmed string from any of the candidate keys (first hit
/// wins). Tolerates the backend's inconsistent service-key casing across
/// endpoints (e.g. `serviceName` / `ServiceName` / `name`).
fn first_str<'a>(map: &'a serde_json::Map<String, Value>, keys: &[&str]) -> Option<&'a str> {
    keys.iter()
        .find_map(|k| map.get(*k).and_then(Value::as_str))
        .map(str::trim)
        .filter(|s| !s.is_empty())
}

/// Read a fee from any of the candidate keys, accepting either a string or a
/// JSON number (the backend uses `feeAmount` as a number on some endpoints,
/// `fee` / `Fee` as a string on others). Returns the verbatim non-empty value.
fn first_fee(map: &serde_json::Map<String, Value>, keys: &[&str]) -> Option<String> {
    keys.iter().find_map(|k| match map.get(*k) {
        Some(Value::String(s)) if !s.trim().is_empty() => Some(s.trim().to_string()),
        Some(Value::Number(n)) => Some(n.to_string()),
        _ => None,
    })
}

/// Format a single provider service into its card value string, mirroring
/// references/discover.md §detail's `<ServiceName> — <Type>, <Fee or free>[, <Endpoint>]`.
/// `Type` maps `A2MCP`→"API service" / `A2A`→"agent-to-agent" (verbatim
/// otherwise); A2A omits the endpoint from the value string. Returns `None`
/// when there is no service name to anchor the row.
fn format_service_value(service: &Value) -> Option<String> {
    let Value::Object(s) = service else {
        return None;
    };
    let name = first_str(s, &["serviceName", "ServiceName", "name"])?;

    let raw_type = first_str(s, &["serviceType", "ServiceType", "servicetype"]).unwrap_or("");
    let (type_label, is_a2a) = match raw_type.to_ascii_uppercase().as_str() {
        "A2MCP" => ("API service".to_string(), false),
        "A2A" => ("agent-to-agent".to_string(), true),
        "" => (String::new(), false),
        other => (other.to_string(), false),
    };

    let fee = first_fee(s, &["fee", "Fee", "feeAmount"]);
    let fee_str = match fee {
        Some(f) => format!("{f} USDT"),
        None => "free".to_string(),
    };

    let endpoint = if is_a2a {
        None
    } else {
        first_str(s, &["endpoint", "Endpoint"]).map(str::to_string)
    };

    // Join the descriptor segments (type, fee, optional endpoint), dropping
    // any empty segment, then prefix `<name> — `.
    let mut segments: Vec<String> = Vec::new();
    if !type_label.is_empty() {
        segments.push(type_label);
    }
    segments.push(fee_str);
    if let Some(ep) = endpoint {
        segments.push(ep);
    }
    Some(format!("{name} — {}", segments.join(", ")))
}

/// Assemble the ordered `card` array per references/discover.md §detail.
fn build_agent_card(map: &serde_json::Map<String, Value>) -> Vec<Value> {
    let mut card: Vec<Value> = Vec::new();

    // 1. Agent ID (omit if absent).
    if let Some(id) = map
        .get("agentId")
        .and_then(|v| v.as_u64().map(|n| n.to_string()).or_else(|| v.as_str().map(str::to_string)))
        .filter(|s| !s.trim().is_empty())
    {
        card.push(card_row("Agent ID", format!("#{id}")));
    }

    // 2. Name (omit if absent/empty).
    if let Some(name) = map.get("name").and_then(Value::as_str).map(str::trim).filter(|s| !s.is_empty()) {
        card.push(card_row("Name", name));
    }

    // 3. Role — the computed label (omit if role unknown). Accepts the
    // backend integer role as well as the string enum.
    if let Some(label) = map.get("role").and_then(role_label_from_value) {
        card.push(card_row("Role", label));
    }

    // 4. Status — the computed label.
    if let Some(label) = map.get("status").and_then(status_label) {
        card.push(card_row("Status", label));
    }

    // 5. Approval status — computed label, with rejection reason parenthetical.
    let approval = map.get("approvalDisplayStatus").and_then(Value::as_u64);
    if let Some(label) = approval.and_then(approval_label) {
        let remark = map
            .get("approvalRemark")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|s| !s.is_empty());
        let value = match (approval, remark) {
            (Some(5), Some(remark)) => format!("{label} (reason: {remark})"),
            _ => label.to_string(),
        };
        card.push(card_row("Approval status", value));
    }

    // 6. Address — short form (omit if absent/unusable). The live backend
    // names the agent's wallet `agentWalletAddress`; the older schema used a
    // bare `address`. Accept either (plus `ownerAddress` as a last resort).
    if let Some(short) =
        first_str(map, &["address", "agentWalletAddress", "ownerAddress"]).and_then(short_address)
    {
        card.push(card_row("Address", short));
    }

    // 7. Description — always emit; "(not set)" when empty/missing. Live
    // backend key is `profileDescription`; older schema used `description`.
    let description = first_str(map, &["description", "profileDescription"]);
    card.push(card_row(
        "Description",
        description.map(str::to_string).unwrap_or_else(|| "(not set)".to_string()),
    ));

    // 8. Profile photo — verbatim URL or "default" when empty. Live backend
    // key is `profilePicture`; older schema used `picture`.
    let picture = first_str(map, &["picture", "profilePicture"]);
    card.push(card_row(
        "Profile photo",
        picture.map(str::to_string).unwrap_or_else(|| "default".to_string()),
    ));

    // 9. Service rows — PROVIDER ROLE ONLY (omit entirely otherwise).
    if role_is_provider(map.get("role")) {
        if let Some(services) = map.get("services").and_then(Value::as_array) {
            let mut index = 0usize;
            for service in services {
                if let Some(value) = format_service_value(service) {
                    index += 1;
                    card.push(card_row(&format!("Service {index}"), value));
                }
            }
        }
    }

    // 10. Rating — omit when count 0 / ratingStars absent.
    if let Some(reputation) = map.get("reputation") {
        if let Some(stars) = rating_stars(reputation) {
            let count = reputation.get("count").and_then(Value::as_u64).unwrap_or(0);
            card.push(card_row(
                "Rating",
                format!("★ {stars} ({count} reviews)"),
            ));
        }
    }

    // 11. txHash — only when present (absent on read-only get).
    if let Some(tx_hash) = map.get("txHash").and_then(Value::as_str).map(str::trim).filter(|s| !s.is_empty()) {
        card.push(card_row("txHash", tx_hash));
    }

    card
}

// ─── `cells`: ordered, ready-to-render TABLE-ROW cells ────────────────────
//
// `cells` is the table-row analog of `card`: an ordered JSON array of
// `{ "label": <canonical-English-column-label>, "value": <string> }`, ONE
// entry per column, omitting NOTHING (a table row keeps every column —
// missing values render as "—" / "No rating yet" per the per-doc rules).
// Labels are canonical English; the skill localizes them. All formatting
// (truncation, ★ stars, A2A fee, type labels, `—` fallbacks) is done HERE so
// the skill renders the table by simply laying out cells. Mirrors:
//   • references/discover.md   §list         → `build_agent_list_cells`
//   • references/discover.md   §service-list → `build_service_cells`
//   • references/discover.md   §search       → `build_search_cells`
//   • references/reputation.md §feedback-list → `build_feedback_cells`
// All builders are additive: raw fields + existing `card`/labels stay intact.
// The `cells` insert is an intentional unconditional overwrite — see the
// overwrite NOTE in the `agent get` row-enrichment section above.

/// Build one `{ label, value }` table cell (same shape as `card_row`).
fn cell(label: &str, value: impl Into<String>) -> Value {
    serde_json::json!({ "label": label, "value": value.into() })
}

/// Truncate a display name to ≤ `max` chars, appending `…` when truncated.
/// Char-based (not byte-based) so multi-byte names truncate cleanly.
fn truncate_name(name: &str, max: usize) -> String {
    let chars: Vec<char> = name.chars().collect();
    if chars.len() <= max {
        name.to_string()
    } else {
        format!("{}…", chars[..max].iter().collect::<String>())
    }
}

/// Read the `agentId` from a row as a display string (`u64` or string forms).
fn read_agent_id(map: &serde_json::Map<String, Value>) -> Option<String> {
    map.get("agentId")
        .and_then(|v| {
            v.as_u64()
                .map(|n| n.to_string())
                .or_else(|| v.as_str().map(str::to_string))
        })
        .filter(|s| !s.trim().is_empty())
}

// ─── §1 agent-list row cells ──────────────────────────────────────────────
//
// Columns (references/discover.md §list), in order:
//   Agent ID | Name | Role | Status | Approval status | Rating
// Mirrors §1's rules: Name truncate-20; Role/Status via computed labels;
// Approval status via approval_label, with `Review failed (reason: <remark>)`
// when approvalDisplayStatus==5 and approvalRemark non-empty; Rating
// `★ <ratingStars> (<count>)` or `No rating yet` (count 0 / no stars).
// Unknown role/status/approval → `—` (a row keeps all columns).
fn build_agent_list_cells(map: &serde_json::Map<String, Value>) -> Vec<Value> {
    let agent_id = read_agent_id(map)
        .map(|id| format!("#{id}"))
        .unwrap_or_else(|| "—".to_string());

    let name = map
        .get("name")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(|s| truncate_name(s, 20))
        .unwrap_or_else(|| "—".to_string());

    let role = map
        .get("role")
        .and_then(role_label_from_value)
        .unwrap_or("—")
        .to_string();

    let status = map
        .get("status")
        .and_then(status_label)
        .unwrap_or("—")
        .to_string();

    // Approval status: approval_label, with §1's rejection parenthetical.
    let approval_code = map.get("approvalDisplayStatus").and_then(Value::as_u64);
    let approval = match approval_code.and_then(approval_label) {
        Some(label) => {
            let remark = map
                .get("approvalRemark")
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|s| !s.is_empty());
            match (approval_code, remark) {
                (Some(5), Some(remark)) => format!("Review failed (reason: {remark})"),
                (Some(5), None) => "Review failed".to_string(),
                _ => label.to_string(),
            }
        }
        None => "—".to_string(),
    };

    // Rating: `★ <ratingStars> (<count>)`, else `No rating yet`. §1 forbids
    // `—` here — always `No rating yet` when there is no usable rating.
    let rating = match map.get("reputation").and_then(rating_stars) {
        Some(stars) => {
            let count = map
                .get("reputation")
                .and_then(|r| r.get("count"))
                .and_then(Value::as_u64)
                .unwrap_or(0);
            format!("★ {stars} ({count})")
        }
        None => "No rating yet".to_string(),
    };

    vec![
        cell("Agent ID", agent_id),
        cell("Name", name),
        cell("Role", role),
        cell("Status", status),
        cell("Approval status", approval),
        cell("Rating", rating),
    ]
}

/// Add `cells` (per §1) to every agent row in the envelope (single-layer
/// `list[*]` or double-layer `list[*].agentList[*]`). No-op when the shape
/// doesn't match. Additive: never removes fields.
pub(super) fn add_agent_list_cells(v: &mut Value) {
    for_each_agent_row(v, |row| {
        if let Value::Object(map) = row {
            let cells = build_agent_list_cells(map);
            map.insert("cells".to_string(), Value::Array(cells));
        }
    });
}

// ─── §6 search-result row cells ───────────────────────────────────────────
//
// Search uses a DIFFERENT backend schema than
// `agent get`. Columns (references/discover.md §search Field mapping), in order:
//   Agent ID | Name | Rating | Min price | Top service
// Critical schema differences handled HERE:
//   • Rating source is `feedbackRate`, ALREADY a 0–5 float — rendered
//     directly, NO `/20`. `null` → `—`; `0` → `No rating yet` (0 means no
//     feedback yet, never `★ 0`).
//   • Description is `profileDescription` (not surfaced as a column here).
//   • Price is `serviceMinPrice` (bare number, NO unit; `null`/missing → `—`).
//   • `services` may be ABSENT entirely (`@JsonInclude(NON_NULL)`) → `—`.
//   • Per-service fields are camelCase `serviceName` / `serviceType` /
//     `feeAmount` / `feeToken`.
// Forbidden columns (Role / Status / Description / Endpoint) are NOT emitted.
fn build_search_cells(map: &serde_json::Map<String, Value>) -> Vec<Value> {
    let agent_id = read_agent_id(map)
        .map(|id| format!("#{id}"))
        .unwrap_or_else(|| "—".to_string());

    let name = map
        .get("name")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(|s| truncate_name(s, 20))
        .unwrap_or_else(|| "—".to_string());

    // Rating: feedbackRate is already a 0–5 float — render directly (NO /20).
    // null/missing → `—`; exactly 0 → `No rating yet` (never `★ 0`).
    let rating = match map.get("feedbackRate") {
        Some(Value::Number(n)) => match n.as_f64() {
            Some(0.0) => "No rating yet".to_string(),
            Some(rate) => format!("★ {}", format_search_rate(rate)),
            None => "—".to_string(),
        },
        _ => "—".to_string(),
    };

    // Min price: bare number, NO unit. null/missing → `—`.
    let min_price = match map.get("serviceMinPrice") {
        Some(Value::Number(n)) => n.to_string(),
        _ => "—".to_string(),
    };

    // Top service: services[0] → `<serviceName> (<localized type>, <feeAmount>
    // <feeToken>)`, truncated to ≤ 40 chars. services absent / empty → `—`.
    let top_service = map
        .get("services")
        .and_then(Value::as_array)
        .and_then(|arr| arr.first())
        .and_then(format_top_service)
        .unwrap_or_else(|| "—".to_string());

    vec![
        cell("Agent ID", agent_id),
        cell("Name", name),
        cell("Rating", rating),
        cell("Min price", min_price),
        cell("Top service", top_service),
    ]
}

/// Format a search `feedbackRate` (0–5 float) for display: up to 2 decimals,
/// trailing zeros (and bare trailing dot) trimmed (`4.60` → "4.6", `5.0` →
/// "5", `4.45` → "4.45").
fn format_search_rate(rate: f64) -> String {
    let mut s = format!("{rate:.2}");
    if s.contains('.') {
        s = s.trim_end_matches('0').trim_end_matches('.').to_string();
    }
    s
}

/// Format the §6 `Top service` cell from a search `services[0]` object:
/// `<serviceName> (<localized serviceType>, <feeAmount> <feeToken>)`,
/// truncated to ≤ 40 chars. serviceType localized via the §Service-type
/// short-form (A2MCP → "API service", A2A → "agent-to-agent"); raw enum never
/// surfaces. Returns `None` when there is no service name to anchor the cell.
fn format_top_service(service: &Value) -> Option<String> {
    let Value::Object(s) = service else {
        return None;
    };
    let name = first_str(s, &["serviceName", "ServiceName", "name"])?;

    let raw_type = first_str(s, &["serviceType", "ServiceType", "servicetype"]).unwrap_or("");
    let type_label = match raw_type.to_ascii_uppercase().as_str() {
        "A2MCP" => "API service".to_string(),
        "A2A" => "agent-to-agent".to_string(),
        "" => String::new(),
        other => other.to_string(),
    };

    // feeAmount (search) + feeToken (verbatim). Fall back tolerantly.
    let fee = first_fee(s, &["feeAmount", "fee", "Fee"]);
    let fee_token = first_str(s, &["feeToken", "FeeToken"]);
    let fee_str = match (fee, fee_token) {
        (Some(f), Some(t)) => format!("{f} {t}"),
        (Some(f), None) => f,
        (None, _) => "free".to_string(),
    };

    let mut segments: Vec<String> = Vec::new();
    if !type_label.is_empty() {
        segments.push(type_label);
    }
    segments.push(fee_str);
    let full = format!("{name} ({})", segments.join(", "));
    Some(truncate_name(&full, 40))
}

/// Add `cells` (per §6) to every search row at the flat `list[*]`. No-op when
/// the shape doesn't match. Additive.
pub(super) fn add_search_cells(v: &mut Value) {
    let Some(rows) = v.get_mut("list").and_then(Value::as_array_mut) else {
        return;
    };
    for row in rows.iter_mut() {
        if let Value::Object(map) = row {
            let cells = build_search_cells(map);
            map.insert("cells".to_string(), Value::Array(cells));
        }
    }
}

// ─── §4 service-list row cells ────────────────────────────────────────────
//
// Columns (references/discover.md §service-list), in order:
//   # | Name | Type | Fee | Endpoint | Description
// service-list returns PascalCase keys
// (`ServiceName` / `ServiceType` / `Fee` / `Endpoint`); we read tolerantly.
// Type: A2MCP → "API service", A2A → "agent-to-agent" (verbatim otherwise).
// Fee: `<n> USDT` or `free` (A2A → its fee or `free`). Endpoint: `—` for A2A,
// the URL for A2MCP. Description: truncated per references/discover.md §service-list (≤ 80 chars).
fn build_service_cells(index: usize, service: &Value) -> Option<Vec<Value>> {
    let Value::Object(s) = service else {
        return None;
    };
    let name = first_str(s, &["serviceName", "ServiceName", "name"])?;

    let raw_type = first_str(s, &["serviceType", "ServiceType", "servicetype"]).unwrap_or("");
    let (type_label, is_a2a) = match raw_type.to_ascii_uppercase().as_str() {
        "A2MCP" => ("API service".to_string(), false),
        "A2A" => ("agent-to-agent".to_string(), true),
        "" => ("—".to_string(), false),
        other => (other.to_string(), false),
    };

    // Fee: `<n> USDT` or `free` (A2A → its fee or `free`; A2MCP same).
    let fee = first_fee(s, &["fee", "Fee", "feeAmount"]);
    let fee_str = match fee {
        Some(f) => format!("{f} USDT"),
        None => "free".to_string(),
    };

    // Endpoint: `—` for A2A; the URL for A2MCP (absent → `—`).
    let endpoint = if is_a2a {
        "—".to_string()
    } else {
        first_str(s, &["endpoint", "Endpoint"])
            .map(str::to_string)
            .unwrap_or_else(|| "—".to_string())
    };

    // Description: truncate per §4 (~80 chars). Empty → `—`.
    let description = first_str(s, &["serviceDescription", "ServiceDescription", "servicedescription"])
        .map(|d| truncate_name(d, 80))
        .unwrap_or_else(|| "—".to_string());

    Some(vec![
        cell("#", index.to_string()),
        cell("Name", name),
        cell("Type", type_label),
        cell("Fee", fee_str),
        cell("Endpoint", endpoint),
        cell("Description", description),
    ])
}

/// Add `cells` (per §4) to every service. The `#` column is 1-based over the
/// rendered services. Tolerates BOTH shapes:
///   • live backend: `data` is an ARRAY of `{ agentInfo, list:[service…] }`
///     wrappers — services live under each wrapper's `list`.
///   • older/synthetic: a single object carrying a flat `services` array.
/// No-op when neither shape matches. Additive.
pub(super) fn add_service_list_cells(v: &mut Value) {
    match v {
        Value::Array(wrappers) => {
            for wrapper in wrappers.iter_mut() {
                add_service_cells_to_node(wrapper);
            }
        }
        node => add_service_cells_to_node(node),
    }
}

/// Locate this node's service array (under `list` then `services`) and stamp a
/// `cells` array onto each service object. No-op when no service array exists.
fn add_service_cells_to_node(node: &mut Value) {
    let Some(map) = node.as_object_mut() else {
        return;
    };
    let key = ["list", "services"]
        .into_iter()
        .find(|k| map.get(*k).map(Value::is_array).unwrap_or(false));
    let Some(key) = key else {
        return;
    };
    let Some(services) = map.get_mut(key).and_then(Value::as_array_mut) else {
        return;
    };
    let mut index = 0usize;
    for service in services.iter_mut() {
        index += 1;
        if let Some(cells) = build_service_cells(index, service) {
            if let Value::Object(m) = service {
                m.insert("cells".to_string(), Value::Array(cells));
            }
        } else {
            // No usable service name → don't consume an index number.
            index -= 1;
        }
    }
}

// ─── §5 feedback-list row cells ───────────────────────────────────────────
//
// references/reputation.md §feedback-list is a prose entry per review, not a strict
// table, but we surface the same fields as ordered cells so the skill can lay
// them out directly. Fields (per §feedback-list):
//   Score (`★ <score>` — score is ALREADY a 0.00–5.00 float, set by
//     convert_feedback_list_scores; render directly, trailing zeros trimmed),
//   Reviewer (creatorId → `#<id>`), Task (taskId), Date (createdAt),
//   Comment (description verbatim, or `(no comment)` when empty).
// Missing optional fields render `—` (the row keeps all cells) EXCEPT comment
// which uses §5's `(no comment)` placeholder.
fn build_feedback_cells(map: &serde_json::Map<String, Value>) -> Vec<Value> {
    // score: already a 0.00–5.00 float (convert_feedback_list_scores ran).
    let score = match map.get("score") {
        Some(Value::Number(n)) => match n.as_f64() {
            Some(v) => format!("★ {}", format_search_rate(v)),
            None => "—".to_string(),
        },
        _ => "—".to_string(),
    };

    let reviewer = map
        .get("creatorId")
        .and_then(|v| {
            v.as_u64()
                .map(|n| n.to_string())
                .or_else(|| v.as_str().map(str::to_string))
        })
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .map(|id| format!("#{id}"))
        .unwrap_or_else(|| "—".to_string());

    let task = map
        .get("taskId")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
        .unwrap_or_else(|| "—".to_string());

    let date = map
        .get("createdAt")
        .and_then(|v| {
            v.as_str()
                .map(str::to_string)
                .or_else(|| v.as_u64().map(|n| n.to_string()))
        })
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "—".to_string());

    // Comment: §5 placeholder `(no comment)` when empty / missing.
    let comment = map
        .get("description")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
        .unwrap_or_else(|| "(no comment)".to_string());

    vec![
        cell("Score", score),
        cell("Reviewer", reviewer),
        cell("Task", task),
        cell("Date", date),
        cell("Comment", comment),
    ]
}

/// Add `cells` (per §5) to every feedback entry. The backend array is `list`
/// on the live endpoint and `items` on older/synthetic shapes (mirrors
/// `convert_feedback_list_scores`, which already accepts both). No-op when
/// neither exists. Additive — runs AFTER convert_feedback_list_scores so
/// `score` is already a 0.00–5.00 float.
pub(super) fn add_feedback_list_cells(v: &mut Value) {
    let Value::Object(map) = v else {
        return;
    };
    for key in ["items", "list"] {
        if let Some(items) = map.get_mut(key).and_then(Value::as_array_mut) {
            for item in items.iter_mut() {
                if let Value::Object(entry) = item {
                    let cells = build_feedback_cells(entry);
                    entry.insert("cells".to_string(), Value::Array(cells));
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

    // ─── agent get row enrichment: label mappings ────────────────────────

    #[test]
    fn role_label_maps_known_and_omits_unknown() {
        assert_eq!(role_label("requester"), Some("User Agent"));
        assert_eq!(role_label("provider"), Some("Agent Service Provider (ASP)"));
        assert_eq!(role_label("evaluator"), Some("Evaluator Agent"));
        assert_eq!(role_label(" provider "), Some("Agent Service Provider (ASP)"));
        assert_eq!(role_label("buyer"), None); // raw alias not a canonical enum
        assert_eq!(role_label(""), None);
    }

    #[test]
    fn status_label_maps_int_and_string() {
        assert_eq!(status_label(&json!(1)), Some("active"));
        assert_eq!(status_label(&json!("active")), Some("active"));
        assert_eq!(status_label(&json!(2)), Some("not listed"));
        assert_eq!(status_label(&json!("2")), Some("not listed"));
        assert_eq!(status_label(&json!(3)), Some("unavailable"));
        assert_eq!(status_label(&json!(4)), Some("unavailable"));
        assert_eq!(status_label(&json!(5)), Some("unavailable"));
        assert_eq!(status_label(&json!(99)), None);
        assert_eq!(status_label(&json!(null)), None);
    }

    #[test]
    fn approval_label_maps_known_codes() {
        assert_eq!(approval_label(1), Some("Not listed"));
        assert_eq!(approval_label(2), Some("Listing under review"));
        assert_eq!(
            approval_label(4),
            Some("Listed — eligible for task recommendations")
        );
        assert_eq!(approval_label(5), Some("Listing rejected"));
        assert_eq!(approval_label(7), Some("This agent is currently unavailable"));
        assert_eq!(approval_label(3), None);
        assert_eq!(approval_label(0), None);
    }

    // ─── agent get row enrichment: ratingStars ───────────────────────────

    #[test]
    fn format_rating_stars_representative_values() {
        assert_eq!(format_rating_stars(92), "4.6"); // 4.60 → trailing zero trimmed
        assert_eq!(format_rating_stars(89), "4.45");
        assert_eq!(format_rating_stars(100), "5"); // whole
        assert_eq!(format_rating_stars(0), "0");
        assert_eq!(format_rating_stars(90), "4.5");
        assert_eq!(format_rating_stars(85), "4.25");
        assert_eq!(format_rating_stars(70), "3.5");
        assert_eq!(format_rating_stars(66), "3.3");
        assert_eq!(format_rating_stars(101), "5"); // clamped
    }

    #[test]
    fn rating_stars_omitted_when_no_reputation() {
        // count == 0 → omit
        assert_eq!(rating_stars(&json!({ "score": 0, "count": 0 })), None);
        // score absent → omit
        assert_eq!(rating_stars(&json!({ "count": 3 })), None);
        // present score + nonzero count → Some
        assert_eq!(
            rating_stars(&json!({ "score": 92, "count": 18 })).as_deref(),
            Some("4.6")
        );
        // score 0 with positive count is a real "0 stars" rating → keep
        assert_eq!(
            rating_stars(&json!({ "score": 0, "count": 2 })).as_deref(),
            Some("0")
        );
    }

    // ─── agent get row enrichment: full row + envelope walk ──────────────

    #[test]
    fn enrich_agent_row_adds_all_four_fields() {
        let mut row = json!({
            "agentId": 42,
            "role": "provider",
            "status": 1,
            "approvalDisplayStatus": 4,
            "reputation": { "score": 92, "count": 18 },
        });
        enrich_agent_row(&mut row);
        assert_eq!(row["roleLabel"], json!("Agent Service Provider (ASP)"));
        assert_eq!(row["statusLabel"], json!("active"));
        assert_eq!(
            row["approvalLabel"],
            json!("Listed — eligible for task recommendations")
        );
        assert_eq!(row["ratingStars"], json!("4.6"));
        // Raw fields untouched.
        assert_eq!(row["role"], json!("provider"));
        assert_eq!(row["status"], json!(1));
        assert_eq!(row["approvalDisplayStatus"], json!(4));
        assert_eq!(row["reputation"], json!({ "score": 92, "count": 18 }));
    }

    #[test]
    fn enrich_agent_row_omits_unknown_and_absent() {
        let mut row = json!({
            "agentId": 7,
            "role": "buyer",        // alias, not canonical → omit roleLabel
            "status": 99,            // unknown → omit statusLabel
            // no approvalDisplayStatus → omit approvalLabel
            "reputation": { "count": 0 }, // count 0 → omit ratingStars
        });
        enrich_agent_row(&mut row);
        assert!(row.get("roleLabel").is_none());
        assert!(row.get("statusLabel").is_none());
        assert!(row.get("approvalLabel").is_none());
        assert!(row.get("ratingStars").is_none());
    }

    // ─── agent get row enrichment: `card` array ─────────────────────────

    #[test]
    fn build_agent_card_provider_full_ordered_with_services_and_rating() {
        let mut row = json!({
            "agentId": 42,
            "name": "DeFi Analyzer",
            "role": "provider",
            "status": 1,
            "approvalDisplayStatus": 4,
            "address": "0xabcdef0123456789abcdef0123456789abcd1234",
            "description": "On-chain data analysis.",
            "picture": "https://cdn.example.com/a.png",
            "services": [
                { "serviceName": "TVL Query", "serviceType": "A2MCP", "fee": "10",
                  "endpoint": "https://api.example.com/mcp" },
                { "serviceName": "Yield Check", "serviceType": "A2A" },
                { "serviceName": "Whale Alert", "serviceType": "A2A", "fee": "5" },
            ],
            "reputation": { "score": 92, "count": 18 },
        });
        enrich_agent_row(&mut row);
        let card = row["card"].as_array().expect("card present");
        let pairs: Vec<(&str, &str)> = card
            .iter()
            .map(|r| (r["label"].as_str().unwrap(), r["value"].as_str().unwrap()))
            .collect();
        assert_eq!(
            pairs,
            vec![
                ("Agent ID", "#42"),
                ("Name", "DeFi Analyzer"),
                ("Role", "Agent Service Provider (ASP)"),
                ("Status", "active"),
                ("Approval status", "Listed — eligible for task recommendations"),
                ("Address", "0xabcd…1234"),
                ("Description", "On-chain data analysis."),
                ("Profile photo", "https://cdn.example.com/a.png"),
                ("Service 1", "TVL Query — API service, 10 USDT, https://api.example.com/mcp"),
                ("Service 2", "Yield Check — agent-to-agent, free"),
                ("Service 3", "Whale Alert — agent-to-agent, 5 USDT"),
                ("Rating", "★ 4.6 (18 reviews)"),
            ]
        );
        // Raw fields untouched.
        assert_eq!(row["role"], json!("provider"));
        assert!(row["services"].is_array());
    }

    #[test]
    fn build_agent_card_requester_has_no_service_rows_and_description_not_set() {
        let mut row = json!({
            "agentId": 58,
            "name": "MyBuyer",
            "role": "requester",
            "status": 1,
            // empty description → "(not set)"; no picture → "default".
            "description": "",
            // Anomaly: backend returned services for a non-provider — must be dropped.
            "services": [
                { "serviceName": "Should Not Appear", "serviceType": "A2MCP", "fee": "1",
                  "endpoint": "https://x" },
            ],
            "reputation": { "score": 0, "count": 0 },
        });
        enrich_agent_row(&mut row);
        let card = row["card"].as_array().expect("card present");
        let labels: Vec<&str> = card.iter().map(|r| r["label"].as_str().unwrap()).collect();
        // No Service rows at all, even though services[] is non-empty.
        assert!(labels.iter().all(|l| !l.starts_with("Service")));
        // Description always emitted with "(not set)" when empty.
        let desc = card
            .iter()
            .find(|r| r["label"] == json!("Description"))
            .expect("description row");
        assert_eq!(desc["value"], json!("(not set)"));
        // Profile photo defaults to "default".
        let photo = card
            .iter()
            .find(|r| r["label"] == json!("Profile photo"))
            .expect("photo row");
        assert_eq!(photo["value"], json!("default"));
        // ratingStars omitted when count 0 → no Rating row.
        assert!(labels.iter().all(|l| *l != "Rating"));
    }

    #[test]
    fn build_agent_card_omits_rating_when_count_zero() {
        let mut row = json!({
            "agentId": 7,
            "role": "evaluator",
            "reputation": { "score": 80, "count": 0 },
        });
        enrich_agent_row(&mut row);
        let card = row["card"].as_array().expect("card present");
        assert!(card.iter().all(|r| r["label"] != json!("Rating")));
        // Evaluator is not a provider → no Service rows.
        assert!(card
            .iter()
            .all(|r| !r["label"].as_str().unwrap().starts_with("Service")));
    }

    #[test]
    fn build_agent_card_includes_txhash_when_present() {
        let mut row = json!({
            "agentId": 1,
            "role": "requester",
            "txHash": "0xabcdef0f12",
        });
        enrich_agent_row(&mut row);
        let card = row["card"].as_array().unwrap();
        let tx = card.iter().find(|r| r["label"] == json!("txHash")).expect("txHash row");
        assert_eq!(tx["value"], json!("0xabcdef0f12"));
    }

    // ─── `cells` helpers: truncate_name ─────────────────────────────────

    #[test]
    fn truncate_name_appends_ellipsis_only_when_longer() {
        assert_eq!(truncate_name("short", 20), "short");
        // 21-char name → truncated to 20 + ellipsis.
        let n21 = "abcdefghijklmnopqrstu"; // 21 chars
        assert_eq!(n21.chars().count(), 21);
        assert_eq!(truncate_name(n21, 20), "abcdefghijklmnopqrst…");
        // exactly 20 → unchanged.
        let n20 = "abcdefghijklmnopqrst"; // 20 chars
        assert_eq!(truncate_name(n20, 20), n20);
    }

    // ─── §1 agent-list cells ─────────────────────────────────────────────

    fn cell_pairs(cells: &Value) -> Vec<(String, String)> {
        cells
            .as_array()
            .expect("cells is an array")
            .iter()
            .map(|c| {
                (
                    c["label"].as_str().unwrap().to_string(),
                    c["value"].as_str().unwrap().to_string(),
                )
            })
            .collect()
    }

    #[test]
    fn build_agent_list_cells_full_provider_row() {
        let row = json!({
            "agentId": 42,
            "name": "DeFi Analyzer",
            "role": "provider",
            "status": 1,
            "approvalDisplayStatus": 4,
            "reputation": { "score": 92, "count": 18 },
        });
        let cells = build_agent_list_cells(row.as_object().unwrap());
        assert_eq!(
            cell_pairs(&Value::Array(cells)),
            vec![
                ("Agent ID".to_string(), "#42".to_string()),
                ("Name".to_string(), "DeFi Analyzer".to_string()),
                ("Role".to_string(), "Agent Service Provider (ASP)".to_string()),
                ("Status".to_string(), "active".to_string()),
                (
                    "Approval status".to_string(),
                    "Listed — eligible for task recommendations".to_string()
                ),
                ("Rating".to_string(), "★ 4.6 (18)".to_string()),
            ]
        );
    }

    #[test]
    fn build_agent_list_cells_count_zero_no_rating_and_truncates_name() {
        let row = json!({
            "agentId": "58",
            "name": "A really long agent name that exceeds twenty",
            "role": "requester",
            "status": 1,
            "reputation": { "score": 0, "count": 0 },
        });
        let cells = build_agent_list_cells(row.as_object().unwrap());
        let pairs = cell_pairs(&Value::Array(cells));
        // 6 columns always present.
        assert_eq!(pairs.len(), 6);
        // First 20 chars of the name, then `…` (char 20 happens to be a space).
        assert_eq!(pairs[1], ("Name".to_string(), "A really long agent …".to_string()));
        assert_eq!(pairs[2].1, "User Agent");
        // count 0 → No rating yet (never `—` in list view).
        assert_eq!(pairs[5], ("Rating".to_string(), "No rating yet".to_string()));
        // no approvalDisplayStatus → `—`.
        assert_eq!(pairs[4], ("Approval status".to_string(), "—".to_string()));
    }

    #[test]
    fn build_agent_list_cells_review_failed_with_reason() {
        let row = json!({
            "agentId": 7,
            "name": "RejectedAgent",
            "role": "provider",
            "status": 2,
            "approvalDisplayStatus": 5,
            "approvalRemark": "Name violates policy",
            "reputation": { "score": 80, "count": 3 },
        });
        let cells = build_agent_list_cells(row.as_object().unwrap());
        let pairs = cell_pairs(&Value::Array(cells));
        assert_eq!(
            pairs[4],
            (
                "Approval status".to_string(),
                "Review failed (reason: Name violates policy)".to_string()
            )
        );
        // status 2 → not listed.
        assert_eq!(pairs[3], ("Status".to_string(), "not listed".to_string()));
    }

    #[test]
    fn build_agent_list_cells_review_failed_empty_remark() {
        let row = json!({
            "agentId": 8,
            "name": "X",
            "role": "provider",
            "approvalDisplayStatus": 5,
            "approvalRemark": "   ",
        });
        let cells = build_agent_list_cells(row.as_object().unwrap());
        let pairs = cell_pairs(&Value::Array(cells));
        assert_eq!(pairs[4], ("Approval status".to_string(), "Review failed".to_string()));
        // unknown status (absent) → `—`.
        assert_eq!(pairs[3], ("Status".to_string(), "—".to_string()));
    }

    #[test]
    fn add_agent_list_cells_walks_envelope_and_skips_detail_unaffected() {
        let mut env = json!({
            "total": 1,
            "list": [
                {
                    "agentList": [
                        { "agentId": 1, "name": "A", "role": "requester", "status": 1 },
                    ],
                },
            ],
        });
        add_agent_list_cells(&mut env);
        let cells = &env["list"][0]["agentList"][0]["cells"];
        assert!(cells.is_array());
        assert_eq!(cells.as_array().unwrap().len(), 6);
    }

    // ─── §6 search cells ─────────────────────────────────────────────────

    #[test]
    fn build_search_cells_feedbackrate_not_divided() {
        // feedbackRate is ALREADY 0–5: 4.6 must render as ★ 4.6, NOT 4.6/20.
        let row = json!({
            "agentId": "1128",
            "name": "DeFi Analyzer",
            "profileDescription": "On-chain data analysis",
            "feedbackRate": 4.6,
            "serviceMinPrice": 10.0,
            "services": [
                { "serviceName": "TVL Query", "serviceType": "A2MCP",
                  "feeAmount": 10.0, "feeToken": "USDT", "endpoint": "https://x" }
            ],
        });
        let cells = build_search_cells(row.as_object().unwrap());
        assert_eq!(
            cell_pairs(&Value::Array(cells)),
            vec![
                ("Agent ID".to_string(), "#1128".to_string()),
                ("Name".to_string(), "DeFi Analyzer".to_string()),
                ("Rating".to_string(), "★ 4.6".to_string()),
                ("Min price".to_string(), "10.0".to_string()),
                (
                    "Top service".to_string(),
                    "TVL Query (API service, 10.0 USDT)".to_string()
                ),
            ]
        );
    }

    #[test]
    fn build_search_cells_null_rate_null_price_absent_services() {
        // feedbackRate null → `—`; serviceMinPrice null → `—`; services key
        // absent (NON_NULL) → `—` Top service.
        let row = json!({
            "agentId": "1129",
            "name": "On-chain Insights",
            "profileDescription": "Analytics",
            "feedbackRate": null,
            "serviceMinPrice": null,
        });
        let cells = build_search_cells(row.as_object().unwrap());
        let pairs = cell_pairs(&Value::Array(cells));
        assert_eq!(pairs[2], ("Rating".to_string(), "—".to_string()));
        assert_eq!(pairs[3], ("Min price".to_string(), "—".to_string()));
        assert_eq!(pairs[4], ("Top service".to_string(), "—".to_string()));
    }

    #[test]
    fn build_search_cells_feedbackrate_zero_is_no_rating_yet() {
        // 0 means no feedback yet — never `★ 0`.
        let row = json!({
            "agentId": "1130",
            "name": "NewAgent",
            "feedbackRate": 0,
            "serviceMinPrice": 1.0,
            "services": [
                { "serviceName": "Free Tier", "serviceType": "A2A" }
            ],
        });
        let cells = build_search_cells(row.as_object().unwrap());
        let pairs = cell_pairs(&Value::Array(cells));
        assert_eq!(pairs[2], ("Rating".to_string(), "No rating yet".to_string()));
        // A2A with no fee → "free"; no token appended.
        assert_eq!(
            pairs[4],
            (
                "Top service".to_string(),
                "Free Tier (agent-to-agent, free)".to_string()
            )
        );
    }

    #[test]
    fn add_search_cells_walks_flat_list() {
        let mut env = json!({
            "total": 1,
            "list": [ { "agentId": "1", "name": "A", "feedbackRate": null } ],
        });
        add_search_cells(&mut env);
        assert_eq!(env["list"][0]["cells"].as_array().unwrap().len(), 5);
    }

    // ─── §4 service-list cells ───────────────────────────────────────────

    #[test]
    fn build_service_cells_a2mcp_pascalcase() {
        // service-list returns PascalCase keys per references/discover.md §service-list.
        let svc = json!({
            "ServiceName": "TVL Query",
            "ServiceType": "A2MCP",
            "Fee": "10",
            "Endpoint": "https://api.example.com/mcp",
            "ServiceDescription": "Query protocol TVL by chain.",
        });
        let cells = build_service_cells(1, &svc).expect("cells");
        assert_eq!(
            cell_pairs(&Value::Array(cells)),
            vec![
                ("#".to_string(), "1".to_string()),
                ("Name".to_string(), "TVL Query".to_string()),
                ("Type".to_string(), "API service".to_string()),
                ("Fee".to_string(), "10 USDT".to_string()),
                ("Endpoint".to_string(), "https://api.example.com/mcp".to_string()),
                ("Description".to_string(), "Query protocol TVL by chain.".to_string()),
            ]
        );
    }

    #[test]
    fn build_service_cells_a2a_no_fee_no_endpoint() {
        let svc = json!({ "ServiceName": "Yield Check", "ServiceType": "A2A" });
        let cells = build_service_cells(2, &svc).expect("cells");
        let pairs = cell_pairs(&Value::Array(cells));
        assert_eq!(pairs[2], ("Type".to_string(), "agent-to-agent".to_string()));
        // A2A no fee → free.
        assert_eq!(pairs[3], ("Fee".to_string(), "free".to_string()));
        // A2A endpoint always `—`.
        assert_eq!(pairs[4], ("Endpoint".to_string(), "—".to_string()));
        // missing description → `—`.
        assert_eq!(pairs[5], ("Description".to_string(), "—".to_string()));
    }

    #[test]
    fn build_service_cells_a2a_with_fee() {
        let svc = json!({ "ServiceName": "Whale Alert", "ServiceType": "A2A", "Fee": "5" });
        let cells = build_service_cells(3, &svc).expect("cells");
        let pairs = cell_pairs(&Value::Array(cells));
        assert_eq!(pairs[3], ("Fee".to_string(), "5 USDT".to_string()));
        assert_eq!(pairs[4], ("Endpoint".to_string(), "—".to_string()));
    }

    #[test]
    fn build_service_cells_returns_none_without_name() {
        let svc = json!({ "ServiceType": "A2MCP", "Fee": "1" });
        assert!(build_service_cells(1, &svc).is_none());
    }

    #[test]
    fn add_service_list_cells_indexes_one_based_and_skips_nameless() {
        let mut resp = json!({
            "agentId": 42,
            "services": [
                { "ServiceName": "First", "ServiceType": "A2A" },
                { "ServiceType": "A2MCP" }, // nameless → no cells, no index burn
                { "ServiceName": "Third", "ServiceType": "A2MCP", "Fee": "2", "Endpoint": "https://z" },
            ],
        });
        add_service_list_cells(&mut resp);
        let svcs = resp["services"].as_array().unwrap();
        assert_eq!(svcs[0]["cells"][0]["value"], json!("1"));
        assert!(svcs[1].get("cells").is_none());
        // Third keeps index 2 (nameless one did not consume an index).
        assert_eq!(svcs[2]["cells"][0]["value"], json!("2"));
    }

    // ─── §5 feedback-list cells ──────────────────────────────────────────

    #[test]
    fn build_feedback_cells_full_entry() {
        // score is already a 0.00–5.00 float (convert_feedback_list_scores ran).
        let item = json!({
            "creatorId": 88,
            "score": 5.0,
            "description": "Timely delivery, accurate data",
            "taskId": "0xabc03e8",
            "createdAt": "2026-04-20",
        });
        let cells = build_feedback_cells(item.as_object().unwrap());
        assert_eq!(
            cell_pairs(&Value::Array(cells)),
            vec![
                ("Score".to_string(), "★ 5".to_string()),
                ("Reviewer".to_string(), "#88".to_string()),
                ("Task".to_string(), "0xabc03e8".to_string()),
                ("Date".to_string(), "2026-04-20".to_string()),
                ("Comment".to_string(), "Timely delivery, accurate data".to_string()),
            ]
        );
    }

    #[test]
    fn build_feedback_cells_no_comment_and_missing_task() {
        let item = json!({
            "creatorId": 77,
            "score": 4.45,
            "createdAt": "2026-04-15",
        });
        let cells = build_feedback_cells(item.as_object().unwrap());
        let pairs = cell_pairs(&Value::Array(cells));
        assert_eq!(pairs[0], ("Score".to_string(), "★ 4.45".to_string()));
        // missing taskId → `—`.
        assert_eq!(pairs[2], ("Task".to_string(), "—".to_string()));
        // empty/missing description → `(no comment)`.
        assert_eq!(pairs[4], ("Comment".to_string(), "(no comment)".to_string()));
    }

    #[test]
    fn add_feedback_list_cells_walks_items() {
        let mut resp = json!({
            "agentId": 42,
            "average": 4.45,
            "items": [
                { "creatorId": 88, "score": 4.5, "description": "ok", "createdAt": "2026-04-20" },
            ],
        });
        add_feedback_list_cells(&mut resp);
        assert_eq!(resp["items"][0]["cells"].as_array().unwrap().len(), 5);
        assert_eq!(resp["items"][0]["cells"][0]["value"], json!("★ 4.5"));
    }

    #[test]
    fn enrich_agent_get_rows_walks_double_layer_envelope() {
        let mut env = json!({
            "total": 1,
            "list": [
                {
                    "ownerAddress": "0xabc",
                    "agentList": [
                        { "agentId": 1, "role": "requester", "status": 2,
                          "approvalDisplayStatus": 5 },
                        { "agentId": 2, "role": "evaluator", "status": 3,
                          "reputation": { "score": 100, "count": 4 } },
                    ],
                },
            ],
        });
        enrich_agent_get_rows(&mut env);
        let rows = &env["list"][0]["agentList"];
        assert_eq!(rows[0]["roleLabel"], json!("User Agent"));
        assert_eq!(rows[0]["statusLabel"], json!("not listed"));
        assert_eq!(rows[0]["approvalLabel"], json!("Listing rejected"));
        assert!(rows[0].get("ratingStars").is_none());
        assert_eq!(rows[1]["roleLabel"], json!("Evaluator Agent"));
        assert_eq!(rows[1]["statusLabel"], json!("unavailable"));
        assert_eq!(rows[1]["ratingStars"], json!("5"));
    }

    // ─── REAL backend shapes (live /agent-list & /service-list verified) ──
    //
    // The live `/agent-list` endpoint returns a SINGLE-layer `list[*]` of flat
    // agent rows with INTEGER role and `profileDescription` / `profilePicture`
    // / `agentWalletAddress` field names (NOT the double-layer / string-role /
    // `description` schema the older doc + synthetic tests above assume). These
    // tests pin the tolerant handling against the real shapes.

    #[test]
    fn enrich_agent_row_accepts_integer_role() {
        // role=2 (provider) as an integer, the live backend form.
        let mut row = json!({ "agentId": 392, "role": 2, "status": 1, "approvalDisplayStatus": 1 });
        enrich_agent_row(&mut row);
        assert_eq!(row["roleLabel"], json!("Agent Service Provider (ASP)"));
        assert_eq!(row["statusLabel"], json!("active"));
        assert_eq!(row["approvalLabel"], json!("Not listed"));
        // role untouched (still the integer).
        assert_eq!(row["role"], json!(2));
    }

    #[test]
    fn enrich_agent_get_rows_walks_single_layer_envelope() {
        // Live `/agent-list` shape: data.list[*] are flat agent rows, NO
        // `agentList` sub-layer.
        let mut env = json!({
            "total": 1,
            "list": [
                { "agentId": 392, "name": "Agent 392", "role": 2, "status": 1,
                  "approvalDisplayStatus": 1 },
            ],
        });
        enrich_agent_get_rows(&mut env);
        let row = &env["list"][0];
        assert_eq!(row["roleLabel"], json!("Agent Service Provider (ASP)"));
        assert_eq!(row["statusLabel"], json!("active"));
        assert_eq!(row["approvalLabel"], json!("Not listed"));
        // `card` was assembled too.
        assert!(row["card"].is_array());
    }

    #[test]
    fn build_agent_card_reads_live_backend_field_names() {
        // profileDescription / profilePicture / agentWalletAddress + int role.
        let mut row = json!({
            "agentId": 392,
            "name": "Agent 392",
            "role": 2,
            "status": 1,
            "approvalDisplayStatus": 1,
            "agentWalletAddress": "0x30c140554508a515a8da0fe1e2377c4d8eff59d7",
            "profileDescription": "On-chain data analysis.",
            "profilePicture": "https://cdn.example.com/x.png",
        });
        enrich_agent_row(&mut row);
        let card = row["card"].as_array().expect("card present");
        let pairs: Vec<(&str, &str)> = card
            .iter()
            .map(|r| (r["label"].as_str().unwrap(), r["value"].as_str().unwrap()))
            .collect();
        assert!(pairs.contains(&("Role", "Agent Service Provider (ASP)")));
        assert!(pairs.contains(&("Status", "active")));
        assert!(pairs.contains(&("Address", "0x30c1…59d7")));
        assert!(pairs.contains(&("Description", "On-chain data analysis.")));
        assert!(pairs.contains(&("Profile photo", "https://cdn.example.com/x.png")));
    }

    #[test]
    fn add_service_list_cells_walks_array_of_wrappers_with_list_key() {
        // Live `/service-list` shape: data is an ARRAY of
        // `{ agentInfo, list:[service…] }`; services under `list`.
        let mut data = json!([
            {
                "agentInfo": { "agentId": "392", "name": "Agent 392" },
                "list": [
                    { "serviceName": "Mock Service 1", "serviceType": "A2MCP",
                      "fee": "0.3", "endpoint": "https://x", "serviceDescription": "desc" },
                    { "serviceName": "Mock Service 2", "serviceType": "A2A" },
                ],
                "page": 1, "pageSize": 20, "total": 2,
            }
        ]);
        add_service_list_cells(&mut data);
        let svcs = &data[0]["list"];
        assert_eq!(svcs[0]["cells"][0], json!({ "label": "#", "value": "1" }));
        assert_eq!(svcs[0]["cells"][1], json!({ "label": "Name", "value": "Mock Service 1" }));
        assert_eq!(svcs[0]["cells"][2], json!({ "label": "Type", "value": "API service" }));
        assert_eq!(svcs[1]["cells"][0], json!({ "label": "#", "value": "2" }));
        assert_eq!(svcs[1]["cells"][2], json!({ "label": "Type", "value": "agent-to-agent" }));
    }

    #[test]
    fn add_feedback_list_cells_walks_list_key() {
        // Live `/feedback-list` shape: entries under `list` (not `items`).
        let mut data = json!({
            "list": [
                { "score": 5.0, "creatorId": 88, "taskId": "0xabc", "createdAt": "2026-04-20",
                  "description": "Great" },
            ],
            "total": 1,
        });
        add_feedback_list_cells(&mut data);
        let cells = &data["list"][0]["cells"];
        assert_eq!(cells[0], json!({ "label": "Score", "value": "★ 5" }));
        assert_eq!(cells[1], json!({ "label": "Reviewer", "value": "#88" }));
        assert_eq!(cells[4], json!({ "label": "Comment", "value": "Great" }));
    }
}
