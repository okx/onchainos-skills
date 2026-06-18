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
    let head: String = token.chars().take(8).collect();
    let tail: String = token.chars().rev().take(6).collect::<String>().chars().rev().collect();
    format!("{}***{}", head, tail)
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

    // Fee is a PLAIN NUMBER only — USDT is the implicit, only currency. A
    // currency token / symbol / any extra text is rejected (validate-listing
    // surfaces the same rule as a P1 finding; create/update bypass validate so
    // we enforce it here too). Empty A2A fee already returned above as allowed.
    if !service.fee.is_empty() && !is_plain_number(&service.fee) {
        bail!("invalid fee in --service: must be a plain number (USDT is the default currency)");
    }

    Ok(service)
}

/// True when `s` is a plain decimal number: `^\d+(\.\d{1,6})?$` (up to 6
/// fractional digits). No sign, no currency token, no whitespace. Shared by
/// `normalize_service` (create/update) and `validate::check_fee` (QA) so both
/// paths enforce the identical fee contract.
pub(super) fn is_plain_number(s: &str) -> bool {
    match s.split_once('.') {
        None => !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit()),
        Some((int, frac)) => {
            !int.is_empty()
                && int.bytes().all(|b| b.is_ascii_digit())
                && (1..=6).contains(&frac.len())
                && frac.bytes().all(|b| b.is_ascii_digit())
        }
    }
}

/// East-Asian display width of `s`: every code point in a wide (full-width)
/// range counts as 2 columns, everything else as 1. This is a faithful port of
/// the Java backend's `displayLen` / `eaWidth` used for the service-description
/// length gates (`AgentLlmReviewServiceImpl`), so the CLI's QA character counts
/// match the backend's exactly (e.g. a CJK character is 2, an ASCII letter 1).
///
/// The wide ranges mirror the backend's `WIDE_CP_RANGES` table verbatim (Hangul
/// Jamo, CJK radicals/ideographs + extensions, Hiragana/Katakana, Hangul
/// syllables, CJK compatibility, full-width forms, and the CJK supplementary
/// planes). Anything outside those ranges is half-width (1).
pub(super) fn display_width(s: &str) -> usize {
    /// (lo, hi) inclusive code-point ranges that render full-width (2 columns).
    /// Verbatim mirror of the backend's `WIDE_CP_RANGES`.
    const WIDE_CP_RANGES: &[(u32, u32)] = &[
        (0x1100, 0x115F),   // Hangul Jamo
        (0x2E80, 0x303E),   // CJK Radicals / Kangxi
        (0x3041, 0x33FF),   // Hiragana / Katakana / etc.
        (0x3400, 0x4DBF),   // CJK Ext-A
        (0x4E00, 0x9FFF),   // CJK Unified Ideographs
        (0xA960, 0xA97F),   // Hangul Jamo Ext-A
        (0xAC00, 0xD7AF),   // Hangul Syllables
        (0xD7B0, 0xD7FF),   // Hangul Jamo Ext-B
        (0xF900, 0xFAFF),   // CJK Compat Ideographs
        (0xFE10, 0xFE6F),   // Vertical / Compat Forms
        (0xFF01, 0xFF60),   // Fullwidth Forms
        (0xFFE0, 0xFFE6),   // Fullwidth Signs
        (0x20000, 0x2FA1F), // CJK Ext B-F + Compat Supp
    ];
    s.chars()
        .map(|c| {
            let cp = c as u32;
            if WIDE_CP_RANGES.iter().any(|&(lo, hi)| cp >= lo && cp <= hi) {
                2
            } else {
                1
            }
        })
        .sum()
}

pub(super) fn normalize_role(role: &str) -> Result<String> {
    match role.trim().to_ascii_lowercase().as_str() {
        "1" | "buyer" | "requestor" | "requester" => Ok("requester".to_string()),
        "2" | "provider" => Ok("provider".to_string()),
        "3" | "evaluator" => Ok("evaluator".to_string()),
        other => bail!("invalid value for --role: {other}"),
    }
}

/// Normalize a role alias to its backend integer code (`"1"` / `"2"` / `"3"`).
/// Accepts the same inputs as [`normalize_role`] (numbers + names), so callers
/// that send the role as a code (e.g. the `agent get` listing filter) share one
/// validation path. Errors on any unrecognized value.
pub(super) fn normalize_role_code(role: &str) -> Result<String> {
    match normalize_role(role)?.as_str() {
        "requester" => Ok("1".to_string()),
        "provider" => Ok("2".to_string()),
        _ => Ok("3".to_string()),
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

// ─── Rating: stars ↔ score ───────────────────────────────────────────────
// Split into `parts/rating.rs` (declared here as a `#[path]` child module so
// `utils::{parse_stars_arg, score_to_stars, convert_feedback_list_scores}`
// stay the same path for callers).
#[path = "parts/rating.rs"]
mod rating;
pub(super) use rating::{convert_feedback_list_scores, parse_stars_arg};
// `score_to_stars` is internal to `rating` (called by convert_feedback_list_scores);
// only the unit tests reference it through `utils`, so re-export it test-only.
#[cfg(test)]
pub(super) use rating::score_to_stars;

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
        1 => Some("Review not submitted"),
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

/// Enrich the `agent detail` (batch-list) response — a BARE array of agent
/// objects (`get_authed` unwraps `data`) — with the SAME per-row display
/// fields as `agent get` (roleLabel / statusLabel / approvalLabel /
/// ratingStars / card). No-op when `v` isn't an array. Additive: raw fields
/// stay intact.
pub(super) fn enrich_agent_detail_rows(v: &mut Value) {
    if let Some(rows) = v.as_array_mut() {
        for row in rows.iter_mut() {
            enrich_agent_row(row);
        }
    }
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

/// Returns true when `fee` represents a zero amount ("0", "0.0", "0 USDT", etc.).
/// The numeric part is the first whitespace-delimited token.
fn is_zero_fee(fee: &str) -> bool {
    fee.split_whitespace()
        .next()
        .and_then(|n| n.parse::<f64>().ok())
        .map(|v| v == 0.0)
        .unwrap_or(false)
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
    let fee_str = match (is_a2a, fee.as_deref()) {
        (true, None) => "negotiable".to_string(),
        (true, Some(f)) if is_zero_fee(f) => "negotiable".to_string(),
        (_, Some(f)) => format!("{f} USDT"),
        (_, None) => "free".to_string(),
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

// ─── registration pre-check (powers `agent pre-check`) ────────────────────
// Split into `parts/precheck.rs` (declared here as a `#[path]` child module so
// `utils::{build_precheck, collect_owned_agents}` stay the same path for callers).
#[path = "parts/precheck.rs"]
mod precheck;
pub(super) use precheck::{build_precheck, collect_owned_agents};

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
    let (type_label, is_a2a) = match raw_type.to_ascii_uppercase().as_str() {
        "A2MCP" => ("API service".to_string(), false),
        "A2A" => ("agent-to-agent".to_string(), true),
        "" => (String::new(), false),
        other => (other.to_string(), false),
    };

    // feeAmount (search) + feeToken (verbatim). A2A with no/zero fee → "negotiable".
    let fee = first_fee(s, &["feeAmount", "fee", "Fee"]);
    let fee_token = first_str(s, &["feeToken", "FeeToken"]);
    let fee_str = match (is_a2a, fee.as_deref(), fee_token) {
        (true, None, _) => "negotiable".to_string(),
        (true, Some(f), _) if is_zero_fee(f) => "negotiable".to_string(),
        (_, Some(f), Some(t)) => format!("{f} {t}"),
        (_, Some(f), None) => f.to_string(),
        (_, None, _) => "free".to_string(),
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

    // Fee: A2A with no/zero fee → "negotiable"; otherwise `<n> USDT` or `free`.
    let fee = first_fee(s, &["fee", "Fee", "feeAmount"]);
    let fee_str = match (is_a2a, fee.as_deref()) {
        (true, None) => "negotiable".to_string(),
        (true, Some(f)) if is_zero_fee(f) => "negotiable".to_string(),
        (_, Some(f)) => format!("{f} USDT"),
        (_, None) => "free".to_string(),
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
#[path = "tests/utils_tests.rs"]
mod tests;
