//! Rating: 0.00–5.00 stars (CLI surface) ↔ 0–100 score (backend wire).
//!
//! Single source of truth for the conversion. The CLI takes user input in
//! stars with up to 2 decimal places (step 0.01) and renders 2-decimal
//! stars in responses; the wire format with the backend remains 0–100
//! integers. Skills no longer need to do the multiplication themselves —
//! earlier revisions pushed that onto the skill, which was fragile because
//! skills are prompt-driven; a forgetful prompt would send raw stars to
//! the wire and corrupt the rating.
//!
//! All conversions use **round-half-up** at the displayed precision —
//! consistent with the canonical rule pinned in
//! `skills/okx-agent-identity/SKILL.md` §Amount Display Rules. Note that
//! the wire (0..=100 integer) gives an effective storage grain of 0.05
//! stars per wire unit, so distinct 2-decimal inputs whose ×20 product
//! rounds to the same integer collapse on the wire (e.g. 3.30 / 3.31 /
//! 3.32 all → wire 66). That is a wire limitation, not a parser bug.
//!
//! Split out of `utils.rs` (file-size hygiene); declared there as a `#[path]`
//! child module so `utils::{parse_stars_arg, score_to_stars,
//! convert_feedback_list_scores}` stay the same path for callers.

use anyhow::{anyhow, bail, Result};
use serde_json::Value;

/// Parse a `--score` CLI argument: 0.00–5.00 stars with up to 2 decimal
/// places, returning the 0–100 backend wire value (round-half-up). Pure
/// integer arithmetic to avoid float drift on inputs like 3.33.
pub(crate) fn parse_stars_arg(value: &str, flag: &str) -> Result<u32> {
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
pub(crate) fn score_to_stars(score: u64) -> f64 {
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
pub(crate) fn convert_feedback_list_scores(v: &mut Value) {
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
