//! Shared deterministic helpers for the "sink-to-CLI" optimization (WBW-13651).
//!
//! Every function here is pure/deterministic except [`auto_paginate`] (async — it
//! drives a caller-supplied page fetcher). No floats anywhere: big-integer
//! (hex→decimal) and decimal (prize-pool sum) math use manual string arithmetic
//! so values that exceed `u128` (18-decimal wei on large-supply tokens) stay exact.
//!
//! FR-1: `parse_duration_ms` / `resolve_since_window` — relative `--since` windows.
//! FR-3: `auto_paginate` — cursor auto-pagination.
//! FR-4: `normalize_amount` / `hex_to_decimal_string` — minimal-unit normalization.
//! FR-5: `sum_prize_pool` / `add_decimal_strings` / `format_thousands` — prize-pool pre-sum.

use std::collections::HashMap;

use anyhow::Result;
use serde::Serialize;
use serde_json::Value;

// ── Structured, machine-distinguishable error ───────────────────────────
//
// Validation failures in the shared `fetch_*` / `cmd_*` functions return this
// so BOTH surfaces render the same `errorCode`/`errorField` envelope:
//   - CLI: `main.rs` downcasts it → `output::error_coded(..)` (exit 1).
//   - MCP: `mcp::err(..)` downcasts it → `{ok:false,error,errorCode,errorField?}`.
// Mirrors the existing `CliConfirming` / `CliSetupRequired` downcast precedent.

/// Structured error carrying a stable machine code + optional offending field.
#[derive(Debug, Clone)]
pub struct CodedError {
    pub code: String,
    pub field: Option<String>,
    pub message: String,
}

impl CodedError {
    pub fn new(code: &str, field: Option<&str>, message: impl Into<String>) -> Self {
        Self {
            code: code.to_string(),
            field: field.map(str::to_string),
            message: message.into(),
        }
    }

    /// Shorthand for the most common case: `errorCode:"invalid_input"` + a field.
    pub fn invalid_input(field: &str, message: impl Into<String>) -> Self {
        Self::new("invalid_input", Some(field), message)
    }
}

impl std::fmt::Display for CodedError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.message)
    }
}

impl std::error::Error for CodedError {}

// ── FR-1: duration + relative window ────────────────────────────────────

/// Parse `<int><s|m|h|d>` (or the bare literal `0`) into milliseconds.
///
/// Extends `ws.rs`'s original grammar with `d` (days) — an additive superset,
/// so existing `s|m|h` callers see no regression. `flag` is only used to build
/// the error message (e.g. `"since"`, `"idle-timeout"`).
///
/// `allow_zero` splits the two callers' semantics (WBW-13651 feedback):
/// * `--idle-timeout` passes `true` — `0` / `0s` / `0m` / `0h` all parse to `0`,
///   which `ws.rs` treats as "disable the idle timeout" (the pre-refactor grammar).
/// * `--since` passes `false` — a zero-length window (`begin == end`) produces an
///   empty upstream result with no explanation, so `0` and `0m`/`0h`/`0s`/`0d`
///   are all rejected as invalid input.
pub fn parse_duration_ms(s: &str, flag: &str, allow_zero: bool) -> Result<u64> {
    let t = s.trim();
    if t == "0" {
        if allow_zero {
            return Ok(0);
        }
        anyhow::bail!("invalid --{flag} '{s}'; duration must be positive");
    }
    let (num, mult) = if let Some(n) = t.strip_suffix('d') {
        (n, 86_400_000u64)
    } else if let Some(n) = t.strip_suffix('h') {
        (n, 3_600_000u64)
    } else if let Some(n) = t.strip_suffix('m') {
        (n, 60_000u64)
    } else if let Some(n) = t.strip_suffix('s') {
        (n, 1_000u64)
    } else {
        anyhow::bail!("invalid --{flag} '{s}'; use e.g. 300s, 30m, 24h, 7d");
    };
    let n: u64 = num
        .parse()
        .map_err(|_| anyhow::anyhow!("invalid --{flag} '{s}'; use e.g. 300s, 30m, 24h, 7d"))?;
    if n == 0 {
        if allow_zero {
            return Ok(0);
        }
        anyhow::bail!("invalid --{flag} '{s}'; duration must be positive");
    }
    n.checked_mul(mult)
        .ok_or_else(|| anyhow::anyhow!("--{flag} '{s}' overflows"))
}

/// A resolved absolute time window. Serializes as `{"begin":..,"end":..}`.
#[derive(Serialize, Debug, PartialEq, Eq)]
pub struct ResolvedWindow {
    pub begin: u64,
    pub end: u64,
}

/// `end = now_ms` (caller passes a single captured clock read); `begin = end - dur`.
/// `now_ms` is injected so handlers pass one timestamp and unit tests stay deterministic.
pub fn resolve_since_window(since: &str, now_ms: u64) -> Result<ResolvedWindow> {
    let dur = parse_duration_ms(since, "since", false)?;
    Ok(ResolvedWindow {
        end: now_ms,
        begin: now_ms.saturating_sub(dur),
    })
}

/// Current Unix time in milliseconds (wall clock). Handlers capture this once
/// and pass it to [`resolve_since_window`].
pub fn now_ms() -> u64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

// ── FR-3: cursor auto-pagination ────────────────────────────────────────

/// How the CLI truncates an auto-paginated result to the requested `--max-results`.
#[derive(Clone, Copy, Debug)]
pub enum CursorMode {
    /// Precise per-item truncation to exactly ≤ N; `nextCursor` = last kept item's cursor.
    PerItem,
    /// Whole-page truncation (may exceed N by < one page); `nextCursor` = page-level cursor.
    PageLevel,
}

/// Field-path config for one paginated endpoint.
///
/// **Drift D5:** the upstream items-array key and cursor key are not surfaced in
/// Rust today. [`extract_items`] and the cursor extraction below are therefore
/// tolerant (fall back to the page-as-array, then the first array field), so
/// aggregation stays correct even if `items_key` does not match verbatim.
pub struct PageShape {
    /// Preferred key of the items array inside a page `Value` (e.g. `"list"`).
    pub items_key: &'static str,
    /// Per-item cursor field (PerItem) or top-level next-cursor key (PageLevel).
    pub cursor_key: &'static str,
    pub mode: CursorMode,
}

/// A structured error embeddable inside a `data` object for partial results.
#[derive(Serialize, Clone, Debug)]
pub struct PartialError {
    pub code: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub field: Option<String>,
    pub message: String,
    #[serde(rename = "nextCursor", skip_serializing_if = "Option::is_none")]
    pub next_cursor: Option<String>,
}

/// The aggregated `data` shape returned when `--max-results` is supplied.
#[derive(Serialize, Debug)]
pub struct Aggregated {
    pub items: Vec<Value>,
    #[serde(rename = "nextCursor")]
    pub next_cursor: Option<String>,
    #[serde(rename = "fetchedCount")]
    pub fetched_count: usize,
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub partial: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<PartialError>,
}

/// Hard cap on upstream page requests per auto-paginate call.
pub const MAX_PAGES: usize = 10;

/// Parse + range-check a `--max-results` value (`1..=500`). `Ok(None)` when the
/// flag is absent. Returns a [`CodedError`] (`invalid_input` / `max-results`) on
/// a non-integer or out-of-range value so both surfaces render the same envelope.
pub fn parse_max_results(raw: Option<&str>) -> Result<Option<u32>> {
    let Some(s) = raw else { return Ok(None) };
    let s = s.trim();
    let n: u32 = s.parse().map_err(|_| {
        CodedError::invalid_input(
            "max-results",
            format!("--max-results must be an integer between 1 and 500, got '{s}'"),
        )
    })?;
    if !(1..=500).contains(&n) {
        return Err(CodedError::invalid_input(
            "max-results",
            format!("--max-results must be between 1 and 500, got {n}"),
        )
        .into());
    }
    Ok(Some(n))
}

/// Extract the items array from a page. Tolerant of the exact key (drift D5):
/// `page[items_key]` → the page itself if it is an array → the first array field.
fn extract_items(page: &Value, items_key: &str) -> Vec<Value> {
    if let Some(arr) = page.get(items_key).and_then(Value::as_array) {
        return arr.clone();
    }
    if let Some(arr) = page.as_array() {
        return arr.clone();
    }
    if let Some(obj) = page.as_object() {
        for v in obj.values() {
            if let Some(arr) = v.as_array() {
                return arr.clone();
            }
        }
    }
    Vec::new()
}

/// A cursor value rendered as a non-empty string (string or number), else `None`.
fn cursor_as_string(v: &Value) -> Option<String> {
    match v {
        Value::String(s) if !s.is_empty() => Some(s.clone()),
        Value::Number(n) => Some(n.to_string()),
        _ => None,
    }
}

/// Drive a paginated endpoint. `fetch_page(cursor)` performs one upstream page
/// request. The loop stops at the first of: N reached / cursor empty / [`MAX_PAGES`]
/// / a page error (→ `partial=true` + `error` carrying the continuation cursor).
pub async fn auto_paginate<F, Fut>(
    start_cursor: Option<String>,
    max_results: usize,
    shape: &PageShape,
    mut fetch_page: F,
) -> Aggregated
where
    F: FnMut(Option<String>) -> Fut,
    Fut: std::future::Future<Output = Result<Value>>,
{
    let mut items: Vec<Value> = Vec::new();
    let mut cursor = start_cursor;
    let mut pages = 0usize;
    let mut last_continuation: Option<String> = None;

    loop {
        if pages >= MAX_PAGES {
            break;
        }
        let attempted = cursor.clone();
        let page = match fetch_page(cursor.clone()).await {
            Ok(p) => p,
            Err(e) => {
                let pe = PartialError {
                    code: "upstream_error".to_string(),
                    field: None,
                    message: format!("page {} request failed: {e:#}", pages + 1),
                    next_cursor: attempted.clone(),
                };
                let fetched = items.len();
                return Aggregated {
                    items,
                    next_cursor: attempted,
                    fetched_count: fetched,
                    partial: true,
                    error: Some(pe),
                };
            }
        };
        pages += 1;

        let page_items = extract_items(&page, shape.items_key);
        let cont = match shape.mode {
            CursorMode::PerItem => page_items
                .last()
                .and_then(|it| cursor_as_string(&it[shape.cursor_key])),
            CursorMode::PageLevel => cursor_as_string(&page[shape.cursor_key]),
        };

        // Cursor-advancement guard (WBW-13651 feedback). A page that yields no
        // items but still hands back a non-empty forward cursor would spin —
        // re-requesting empty pages until MAX_PAGES, burning paid upstream quota
        // for nothing. Stop cleanly: there is nothing more to aggregate, and the
        // last good continuation (if any) is already recorded.
        if page_items.is_empty() && cont.as_deref().is_some_and(|c| !c.is_empty()) {
            break;
        }

        items.extend(page_items);
        last_continuation = cont.clone();

        if items.len() >= max_results {
            break;
        }
        match cont {
            Some(c) if !c.is_empty() => {
                // If upstream echoes back the very cursor we just queried with,
                // advancing would re-fetch the same page indefinitely (up to
                // MAX_PAGES), duplicating items and wasting quota (the news
                // endpoint is metered). Stop and surface a structured partial.
                if attempted.as_deref() == Some(c.as_str()) {
                    let fetched = items.len();
                    return Aggregated {
                        items,
                        next_cursor: Some(c.clone()),
                        fetched_count: fetched,
                        partial: true,
                        error: Some(PartialError {
                            code: "cursor_not_advancing".to_string(),
                            field: None,
                            message: format!(
                                "upstream returned the same cursor '{c}' it was queried \
                                 with; stopping to avoid re-fetching the same page"
                            ),
                            next_cursor: Some(c.clone()),
                        }),
                    };
                }
                cursor = Some(c);
            }
            _ => break,
        }
    }

    match shape.mode {
        CursorMode::PerItem if items.len() > max_results => {
            items.truncate(max_results);
            let next = items
                .last()
                .and_then(|it| cursor_as_string(&it[shape.cursor_key]));
            let fetched = items.len();
            Aggregated {
                items,
                next_cursor: next,
                fetched_count: fetched,
                partial: false,
                error: None,
            }
        }
        _ => {
            let fetched = items.len();
            Aggregated {
                items,
                next_cursor: last_continuation,
                fetched_count: fetched,
                partial: false,
                error: None,
            }
        }
    }
}

// ── FR-4: amount normalization ──────────────────────────────────────────

/// Result of normalizing a `dataList[].value` to a minimal-unit decimal integer.
pub enum AmountNorm {
    Value(String),
    Error(String),
}

/// Normalize a raw `value` into a minimal-unit decimal integer string:
/// null/`""`/`"0"`/`"0x0"` → `"0"`; `0x`-hex → exact decimal integer; a
/// decimal-integer string → leading zeros stripped; a non-negative integer JSON
/// number → its string; anything else (fractional/negative number, non-digit
/// string) → `Error`.
///
/// Both surfaces feed the result straight into `wallet contract-call --amt`,
/// whose `validate_non_negative_integer` only accepts a leading-zero-free
/// non-negative decimal integer. Values it would reject (`1.5`, `-5`, `"007"`)
/// are turned into an explicit `AmountNorm::Error` here so the caller can flag a
/// clear `valueNormalizeError` instead of leaking a confusing contract-call error.
pub fn normalize_amount(raw: &Value) -> AmountNorm {
    match raw {
        Value::Null => AmountNorm::Value("0".to_string()),
        Value::Number(n) => {
            if n.is_u64() {
                AmountNorm::Value(n.to_string())
            } else {
                AmountNorm::Error(format!(
                    "value must be a non-negative integer minimal unit, got '{n}'"
                ))
            }
        }
        Value::String(s) => {
            let t = s.trim();
            if t.is_empty() || t == "0" || t.eq_ignore_ascii_case("0x0") {
                return AmountNorm::Value("0".to_string());
            }
            if let Some(rest) = t.strip_prefix("0x").or_else(|| t.strip_prefix("0X")) {
                let _ = rest; // presence check only; convert the full token below
                return match hex_to_decimal_string(t) {
                    Ok(dec) => AmountNorm::Value(dec),
                    Err(e) => AmountNorm::Error(e),
                };
            }
            if t.bytes().all(|b| b.is_ascii_digit()) {
                // Strip leading zeros ("007" → "7"); an all-zero string → "0".
                let stripped = t.trim_start_matches('0');
                let normalized = if stripped.is_empty() { "0" } else { stripped };
                return AmountNorm::Value(normalized.to_string());
            }
            AmountNorm::Error(format!("unparseable value '{t}'"))
        }
        _ => AmountNorm::Error("unparseable value (unexpected JSON type)".to_string()),
    }
}

/// Exact hex→decimal conversion on arbitrary-length input via manual base
/// conversion (no `u128` cap, no float). Accepts an optional `0x`/`0X` prefix.
pub fn hex_to_decimal_string(hex: &str) -> Result<String, String> {
    let h = hex
        .trim()
        .strip_prefix("0x")
        .or_else(|| hex.trim().strip_prefix("0X"))
        .unwrap_or(hex.trim());
    if h.is_empty() {
        return Ok("0".to_string());
    }
    // Little-endian decimal digits, base-10.
    let mut digits: Vec<u8> = vec![0];
    for c in h.chars() {
        let v = c
            .to_digit(16)
            .ok_or_else(|| format!("invalid hex digit '{c}' in '{hex}'"))?;
        let mut carry = v;
        for d in digits.iter_mut() {
            let cur = (*d as u32) * 16 + carry;
            *d = (cur % 10) as u8;
            carry = cur / 10;
        }
        while carry > 0 {
            digits.push((carry % 10) as u8);
            carry /= 10;
        }
    }
    while digits.len() > 1 && *digits.last().unwrap() == 0 {
        digits.pop();
    }
    Ok(digits.iter().rev().map(|d| (b'0' + d) as char).collect())
}

// ── FR-5: prize-pool summation ──────────────────────────────────────────

/// One `{amount, rewardUnit}` bucket of the summed prize pool.
#[derive(Serialize, Debug)]
pub struct PrizePoolEntry {
    pub amount: String,
    #[serde(rename = "rewardUnit")]
    pub reward_unit: String,
}

/// The pre-summed `data.totalPrizePool` shape.
#[derive(Serialize, Debug)]
pub struct TotalPrizePool {
    #[serde(rename = "amountByUnit")]
    pub amount_by_unit: Vec<PrizePoolEntry>,
    pub display: String,
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub partial: bool,
}

/// Sum `totalReward` grouped by `rewardUnit` with exact decimal-string add.
/// `None` when `distributions` is empty. A single unparseable/absent `totalReward`
/// is skipped and sets `partial=true`. `display` joins units by `" + "`.
pub fn sum_prize_pool(distributions: &[Value]) -> Option<TotalPrizePool> {
    if distributions.is_empty() {
        return None;
    }
    let mut order: Vec<String> = Vec::new();
    let mut sums: HashMap<String, String> = HashMap::new();
    let mut partial = false;

    for d in distributions {
        let unit = d
            .get("rewardUnit")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string();
        let reward = match d.get("totalReward") {
            Some(Value::String(s)) => s.trim().to_string(),
            Some(Value::Number(n)) => n.to_string(),
            _ => {
                partial = true;
                continue;
            }
        };
        let running = sums.get(&unit).cloned().unwrap_or_else(|| "0".to_string());
        match add_decimal_strings(&running, &reward) {
            Ok(s) => {
                if !sums.contains_key(&unit) {
                    order.push(unit.clone());
                }
                sums.insert(unit, s);
            }
            Err(_) => partial = true,
        }
    }

    let amount_by_unit: Vec<PrizePoolEntry> = order
        .iter()
        .map(|u| PrizePoolEntry {
            amount: sums[u].clone(),
            reward_unit: u.clone(),
        })
        .collect();
    let display = amount_by_unit
        .iter()
        .map(|e| {
            if e.reward_unit.is_empty() {
                format_thousands(&e.amount)
            } else {
                format!("{} {}", format_thousands(&e.amount), e.reward_unit)
            }
        })
        .collect::<Vec<_>>()
        .join(" + ");

    Some(TotalPrizePool {
        amount_by_unit,
        display,
        partial,
    })
}

/// Grade-school addition of two non-negative integer digit strings.
fn add_int_strings(a: &str, b: &str) -> String {
    let a = a.as_bytes();
    let b = b.as_bytes();
    let mut out: Vec<u8> = Vec::with_capacity(a.len().max(b.len()) + 1);
    let (mut i, mut j) = (a.len() as isize - 1, b.len() as isize - 1);
    let mut carry = 0u8;
    while i >= 0 || j >= 0 || carry > 0 {
        let da = if i >= 0 { a[i as usize] - b'0' } else { 0 };
        let db = if j >= 0 { b[j as usize] - b'0' } else { 0 };
        let sum = da + db + carry;
        out.push(b'0' + (sum % 10));
        carry = sum / 10;
        i -= 1;
        j -= 1;
    }
    out.reverse();
    let s = String::from_utf8(out).unwrap_or_else(|_| "0".to_string());
    let trimmed = s.trim_start_matches('0');
    if trimmed.is_empty() {
        "0".to_string()
    } else {
        trimmed.to_string()
    }
}

/// Exact decimal-string addition (supports fractional parts, no float).
pub fn add_decimal_strings(a: &str, b: &str) -> Result<String, String> {
    let (ai, af) = split_decimal(a)?;
    let (bi, bf) = split_decimal(b)?;
    let flen = af.len().max(bf.len());
    let af_p = format!("{af:0<flen$}");
    let bf_p = format!("{bf:0<flen$}");
    // Scale both to integers by concatenating the padded fractional part.
    let a_scaled = format!("{ai}{af_p}");
    let b_scaled = format!("{bi}{bf_p}");
    let sum = add_int_strings(&a_scaled, &b_scaled);
    if flen == 0 {
        return Ok(sum);
    }
    // Re-insert the decimal point `flen` digits from the right.
    let sum_padded = format!("{sum:0>width$}", width = flen + 1);
    let split = sum_padded.len() - flen;
    let int_part = &sum_padded[..split];
    let frac_part = sum_padded[split..].trim_end_matches('0');
    if frac_part.is_empty() {
        Ok(int_part.to_string())
    } else {
        Ok(format!("{int_part}.{frac_part}"))
    }
}

/// Split a decimal string into (integer_digits, fractional_digits), validating
/// that both parts are ASCII digits. Rejects signs / multiple dots / non-digits.
fn split_decimal(s: &str) -> Result<(String, String), String> {
    let t = s.trim();
    if t.is_empty() {
        return Err(format!("empty decimal string '{s}'"));
    }
    let mut parts = t.splitn(2, '.');
    let int = parts.next().unwrap_or("");
    let frac = parts.next().unwrap_or("");
    let int_ok = !int.is_empty() && int.bytes().all(|b| b.is_ascii_digit());
    let frac_ok = frac.bytes().all(|b| b.is_ascii_digit());
    if !int_ok || !frac_ok {
        return Err(format!("unparseable decimal '{s}'"));
    }
    Ok((int.to_string(), frac.to_string()))
}

/// Add thousands separators to the integer part; preserve the fractional part.
pub fn format_thousands(decimal: &str) -> String {
    let (int, frac) = match decimal.split_once('.') {
        Some((i, f)) => (i, Some(f)),
        None => (decimal, None),
    };
    let mut grouped = String::new();
    let bytes = int.as_bytes();
    let len = bytes.len();
    for (idx, b) in bytes.iter().enumerate() {
        if idx > 0 && (len - idx) % 3 == 0 {
            grouped.push(',');
        }
        grouped.push(*b as char);
    }
    match frac {
        Some(f) => format!("{grouped}.{f}"),
        None => grouped,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    // ── FR-1 ──
    #[test]
    fn parse_duration_known_units() {
        assert_eq!(parse_duration_ms("300s", "since", false).unwrap(), 300_000);
        assert_eq!(parse_duration_ms("30m", "since", false).unwrap(), 1_800_000);
        assert_eq!(parse_duration_ms("24h", "since", false).unwrap(), 86_400_000);
        assert_eq!(parse_duration_ms("7d", "since", false).unwrap(), 604_800_000);
    }

    #[test]
    fn parse_duration_rejects_bad_input() {
        assert!(parse_duration_ms("-5m", "since", false).is_err());
        assert!(parse_duration_ms("10", "since", false).is_err());
        assert!(parse_duration_ms("10x", "since", false).is_err());
        assert!(parse_duration_ms("", "since", false).is_err());
    }

    #[test]
    fn parse_duration_overflow_guard() {
        assert!(parse_duration_ms("100000000000000000d", "since", false).is_err());
    }

    #[test]
    fn parse_duration_zero_disabled_for_since() {
        // `--since` is positive-only: bare `0` and every `0<unit>` form is rejected
        // (a zero-width window returns an unexplained empty result).
        assert!(parse_duration_ms("0", "since", false).is_err());
        assert!(parse_duration_ms("0s", "since", false).is_err());
        assert!(parse_duration_ms("0m", "since", false).is_err());
        assert!(parse_duration_ms("0h", "since", false).is_err());
        assert!(parse_duration_ms("0d", "since", false).is_err());
    }

    #[test]
    fn parse_duration_zero_allowed_for_idle_timeout() {
        // `--idle-timeout` keeps the pre-refactor "disable" semantics: bare `0`
        // and `0s`/`0m`/`0h`/`0d` all parse to 0 (= no idle timeout).
        assert_eq!(parse_duration_ms("0", "idle-timeout", true).unwrap(), 0);
        assert_eq!(parse_duration_ms("0s", "idle-timeout", true).unwrap(), 0);
        assert_eq!(parse_duration_ms("0m", "idle-timeout", true).unwrap(), 0);
        assert_eq!(parse_duration_ms("0h", "idle-timeout", true).unwrap(), 0);
        assert_eq!(parse_duration_ms("0d", "idle-timeout", true).unwrap(), 0);
        // Non-zero durations still parse identically regardless of allow_zero.
        assert_eq!(parse_duration_ms("30m", "idle-timeout", true).unwrap(), 1_800_000);
    }

    #[test]
    fn resolve_since_window_rejects_zero() {
        // The public `--since` entry point rejects a zero-length window.
        assert!(resolve_since_window("0", 1000).is_err());
        assert!(resolve_since_window("0m", 1000).is_err());
    }

    #[test]
    fn resolve_since_window_invariants() {
        let now = 1_721_086_400_000u64;
        let w = resolve_since_window("24h", now).unwrap();
        assert_eq!(w.end, now);
        assert_eq!(w.end - w.begin, 86_400_000);
    }

    #[test]
    fn resolve_since_window_saturates() {
        let w = resolve_since_window("7d", 1000).unwrap();
        assert_eq!(w.end, 1000);
        assert_eq!(w.begin, 0);
    }

    // ── FR-3 ──
    #[test]
    fn parse_max_results_range() {
        assert_eq!(parse_max_results(None).unwrap(), None);
        assert_eq!(parse_max_results(Some("50")).unwrap(), Some(50));
        assert_eq!(parse_max_results(Some("1")).unwrap(), Some(1));
        assert_eq!(parse_max_results(Some("500")).unwrap(), Some(500));
        assert!(parse_max_results(Some("0")).is_err());
        assert!(parse_max_results(Some("999")).is_err());
        assert!(parse_max_results(Some("abc")).is_err());
    }

    #[test]
    fn parse_max_results_error_is_coded() {
        let e = parse_max_results(Some("999")).unwrap_err();
        let c = e.downcast_ref::<CodedError>().expect("coded error");
        assert_eq!(c.code, "invalid_input");
        assert_eq!(c.field.as_deref(), Some("max-results"));
    }

    fn item(cursor: &str) -> Value {
        json!({ "id": cursor, "cursor": cursor })
    }

    #[tokio::test]
    async fn auto_paginate_per_item_exact_truncation() {
        let shape = PageShape {
            items_key: "list",
            cursor_key: "cursor",
            mode: CursorMode::PerItem,
        };
        // Each page returns 3 items; ask for 5 → truncate to exactly 5.
        let agg = auto_paginate(None, 5, &shape, |cur| {
            let page = match cur.as_deref() {
                None => json!({ "list": [item("a"), item("b"), item("c")] }),
                Some("c") => json!({ "list": [item("d"), item("e"), item("f")] }),
                _ => json!({ "list": [] }),
            };
            async move { Ok(page) }
        })
        .await;
        assert_eq!(agg.fetched_count, 5);
        assert_eq!(agg.items.len(), 5);
        assert_eq!(agg.next_cursor.as_deref(), Some("e")); // last kept item's cursor
        assert!(!agg.partial);
    }

    #[tokio::test]
    async fn auto_paginate_page_level_keeps_whole_page() {
        let shape = PageShape {
            items_key: "list",
            cursor_key: "cursor",
            mode: CursorMode::PageLevel,
        };
        // Page-level cursor lives at the page top-level; result may exceed N.
        let agg = auto_paginate(None, 4, &shape, |cur| {
            let page = match cur.as_deref() {
                None => json!({ "list": [item("a"), item("b"), item("c")], "cursor": "p2" }),
                Some("p2") => json!({ "list": [item("d"), item("e"), item("f")], "cursor": "p3" }),
                _ => json!({ "list": [], "cursor": null }),
            };
            async move { Ok(page) }
        })
        .await;
        assert_eq!(agg.fetched_count, 6); // whole second page kept (>4)
        assert_eq!(agg.next_cursor.as_deref(), Some("p3")); // page-level cursor
        assert!(!agg.partial);
    }

    #[tokio::test]
    async fn auto_paginate_stops_on_empty_cursor() {
        let shape = PageShape {
            items_key: "list",
            cursor_key: "cursor",
            mode: CursorMode::PageLevel,
        };
        let agg = auto_paginate(None, 100, &shape, |cur| {
            let page = match cur.as_deref() {
                None => json!({ "list": [item("a")], "cursor": "" }),
                _ => json!({ "list": [] }),
            };
            async move { Ok(page) }
        })
        .await;
        assert_eq!(agg.fetched_count, 1);
        assert_eq!(agg.next_cursor, None);
    }

    #[tokio::test]
    async fn auto_paginate_ten_page_cap() {
        let shape = PageShape {
            items_key: "list",
            cursor_key: "cursor",
            mode: CursorMode::PerItem,
        };
        // Every page returns 1 item with a DISTINCT forward cursor, so the loop
        // advances legitimately and is bounded only by MAX_PAGES.
        let mut n = 0u32;
        let agg = auto_paginate(None, 1000, &shape, |_cur| {
            n += 1;
            let page = json!({ "list": [item(&format!("c{n}"))] });
            async move { Ok(page) }
        })
        .await;
        assert_eq!(agg.fetched_count, MAX_PAGES); // capped at 10 pages × 1 item
        assert_eq!(agg.next_cursor.as_deref(), Some("c10"));
        assert!(!agg.partial);
    }

    #[tokio::test]
    async fn auto_paginate_stops_when_cursor_not_advancing() {
        let shape = PageShape {
            items_key: "list",
            cursor_key: "cursor",
            mode: CursorMode::PerItem,
        };
        // Upstream keeps echoing back the same cursor "x" it was queried with.
        // Without the guard this spins to MAX_PAGES (10 duplicate items); with
        // it, we aggregate the current page then stop before the next (repeat)
        // fetch, per the "compare before advancing, return aggregated data"
        // contract — bounding the damage to a single duplicate instead of nine.
        let agg = auto_paginate(None, 1000, &shape, |_cur| {
            let page = json!({ "list": [item("x")] });
            async move { Ok(page) }
        })
        .await;
        assert!(agg.partial);
        // Page 1 (cursor None → "x") and page 2 (requested with "x" → "x" again)
        // are aggregated; the guard fires before a third, wasted fetch.
        assert_eq!(agg.fetched_count, 2);
        assert_eq!(agg.next_cursor.as_deref(), Some("x"));
        let err = agg.error.expect("partial error");
        assert_eq!(err.code, "cursor_not_advancing");
        assert_eq!(err.next_cursor.as_deref(), Some("x"));
    }

    #[tokio::test]
    async fn auto_paginate_stops_on_empty_page_with_cursor() {
        let shape = PageShape {
            items_key: "list",
            cursor_key: "cursor",
            mode: CursorMode::PageLevel,
        };
        // First page has items + a forward cursor; the second page is empty but
        // still advertises a (different) non-empty cursor. Continuing would only
        // fetch more empty pages, so we stop cleanly with what we have.
        let agg = auto_paginate(None, 100, &shape, |cur| {
            let page = match cur.as_deref() {
                None => json!({ "list": [item("a"), item("b")], "cursor": "p2" }),
                _ => json!({ "list": [], "cursor": "p3" }),
            };
            async move { Ok(page) }
        })
        .await;
        assert!(!agg.partial);
        assert_eq!(agg.fetched_count, 2);
        // last_continuation is the first page's cursor (the empty page is skipped).
        assert_eq!(agg.next_cursor.as_deref(), Some("p2"));
    }

    #[tokio::test]
    async fn auto_paginate_mid_page_error_is_partial() {
        let shape = PageShape {
            items_key: "list",
            cursor_key: "cursor",
            mode: CursorMode::PerItem,
        };
        let agg = auto_paginate(None, 100, &shape, |cur| {
            let cur2 = cur.clone();
            async move {
                match cur2.as_deref() {
                    None => Ok(json!({ "list": [item("a"), item("b")] })),
                    _ => Err(anyhow::anyhow!("boom")),
                }
            }
        })
        .await;
        assert!(agg.partial);
        assert_eq!(agg.fetched_count, 2);
        assert_eq!(agg.next_cursor.as_deref(), Some("b")); // continuation to retry
        let err = agg.error.expect("partial error");
        assert_eq!(err.code, "upstream_error");
        assert_eq!(err.next_cursor.as_deref(), Some("b"));
    }

    // ── FR-4 ──
    #[test]
    fn normalize_amount_zero_forms() {
        assert!(matches!(normalize_amount(&Value::Null), AmountNorm::Value(v) if v == "0"));
        assert!(matches!(normalize_amount(&json!("")), AmountNorm::Value(v) if v == "0"));
        assert!(matches!(normalize_amount(&json!("0")), AmountNorm::Value(v) if v == "0"));
        assert!(matches!(normalize_amount(&json!("0x0")), AmountNorm::Value(v) if v == "0"));
    }

    #[test]
    fn normalize_amount_hex_and_decimal() {
        assert!(matches!(normalize_amount(&json!("0x1a")), AmountNorm::Value(v) if v == "26"));
        assert!(
            matches!(normalize_amount(&json!("123456")), AmountNorm::Value(v) if v == "123456")
        );
        assert!(matches!(
            normalize_amount(&json!("abc")),
            AmountNorm::Error(_)
        ));
    }

    #[test]
    fn normalize_amount_strips_leading_zeros() {
        // Decimal-string leading zeros are stripped so `--amt` accepts the value.
        assert!(matches!(normalize_amount(&json!("007")), AmountNorm::Value(v) if v == "7"));
        assert!(matches!(normalize_amount(&json!("00123")), AmountNorm::Value(v) if v == "123"));
        // An all-zero decimal string collapses to canonical "0".
        assert!(matches!(normalize_amount(&json!("00")), AmountNorm::Value(v) if v == "0"));
        assert!(matches!(normalize_amount(&json!("000")), AmountNorm::Value(v) if v == "0"));
    }

    #[test]
    fn normalize_amount_rejects_non_integer_numbers() {
        // JSON numbers that are not non-negative integers must become Error, not
        // a `valueNormalized` the downstream `--amt` validator will reject.
        assert!(matches!(normalize_amount(&json!(1.5)), AmountNorm::Error(_)));
        assert!(matches!(normalize_amount(&json!(-5)), AmountNorm::Error(_)));
        assert!(matches!(normalize_amount(&json!(-1.0)), AmountNorm::Error(_)));
        // A non-negative integer JSON number is still accepted.
        assert!(matches!(normalize_amount(&json!(42)), AmountNorm::Value(v) if v == "42"));
        assert!(matches!(normalize_amount(&json!(0)), AmountNorm::Value(v) if v == "0"));
    }

    #[test]
    fn normalize_amount_exceeds_u128() {
        // 0x + 33 'f' nibbles → far beyond u128::MAX; must stay exact.
        let hex = format!("0x{}", "f".repeat(33));
        let expected = hex_to_decimal_string(&hex).unwrap();
        assert!(matches!(normalize_amount(&json!(hex)), AmountNorm::Value(v) if v == expected));
        // sanity: value is longer than u128::MAX's decimal width (39 digits)
        assert!(expected.len() > 39);
    }

    #[test]
    fn hex_to_decimal_boundaries() {
        assert_eq!(hex_to_decimal_string("0x0").unwrap(), "0");
        assert_eq!(hex_to_decimal_string("0xff").unwrap(), "255");
        assert_eq!(hex_to_decimal_string("0x10").unwrap(), "16");
        // 2^128 = 340282366920938463463374607431768211456
        assert_eq!(
            hex_to_decimal_string("0x100000000000000000000000000000000").unwrap(),
            "340282366920938463463374607431768211456"
        );
        assert!(hex_to_decimal_string("0xZZ").is_err());
    }

    // ── FR-5 ──
    #[test]
    fn add_decimal_strings_integer_and_fractional() {
        assert_eq!(add_decimal_strings("40000", "0").unwrap(), "40000");
        assert_eq!(add_decimal_strings("40000", "200").unwrap(), "40200");
        assert_eq!(add_decimal_strings("1.5", "2.75").unwrap(), "4.25");
        assert_eq!(add_decimal_strings("0.1", "0.2").unwrap(), "0.3");
        // exceeds u128
        let big = "340282366920938463463374607431768211456";
        assert_eq!(
            add_decimal_strings(big, "1").unwrap(),
            "340282366920938463463374607431768211457"
        );
        assert!(add_decimal_strings("1.2.3", "1").is_err());
    }

    #[test]
    fn format_thousands_groups() {
        assert_eq!(format_thousands("40000"), "40,000");
        assert_eq!(format_thousands("999"), "999");
        assert_eq!(format_thousands("1234567"), "1,234,567");
        assert_eq!(format_thousands("40000.5"), "40,000.5");
    }

    #[test]
    fn sum_prize_pool_same_unit() {
        let dists = vec![
            json!({ "totalReward": "10000", "rewardUnit": "USDC" }),
            json!({ "totalReward": "30000", "rewardUnit": "USDC" }),
        ];
        let tp = sum_prize_pool(&dists).unwrap();
        assert_eq!(tp.amount_by_unit.len(), 1);
        assert_eq!(tp.amount_by_unit[0].amount, "40000");
        assert_eq!(tp.display, "40,000 USDC");
        assert!(!tp.partial);
    }

    #[test]
    fn sum_prize_pool_multi_unit_join() {
        let dists = vec![
            json!({ "totalReward": "40000", "rewardUnit": "USDC" }),
            json!({ "totalReward": "200", "rewardUnit": "DJT" }),
        ];
        let tp = sum_prize_pool(&dists).unwrap();
        assert_eq!(tp.display, "40,000 USDC + 200 DJT");
    }

    #[test]
    fn sum_prize_pool_empty_is_none() {
        assert!(sum_prize_pool(&[]).is_none());
    }

    #[test]
    fn sum_prize_pool_bad_entry_is_partial() {
        let dists = vec![
            json!({ "totalReward": "40000", "rewardUnit": "USDC" }),
            json!({ "totalReward": "not-a-number", "rewardUnit": "USDC" }),
        ];
        let tp = sum_prize_pool(&dists).unwrap();
        assert_eq!(tp.amount_by_unit[0].amount, "40000");
        assert!(tp.partial);
    }
}
