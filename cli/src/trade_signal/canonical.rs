//! FR-2 usability expansion (MR !196): a deterministic mixed-language + safe
//! field-reordering fallback.
//!
//! The canonical fixed-order parser stays the fast path. Only when it fails does
//! [`super::parse_signal_text`] call [`canonicalize`], which classifies each
//! pipe-delimited field into a canonical positional slot using the CLOSED zh+en
//! keyword/shape grammar, reorders the fields into canonical order, and normalizes
//! each field keyword to the header language so the existing per-class validators
//! run unchanged. It then re-runs those validators — every numeric/date/range/
//! direction/option-consistency check is preserved.
//!
//! Determinism / fail-closed rules (no AI, no fuzzy guessing):
//! - Header still fixes `assetClass`; only field KEYWORDS may cross languages
//!   (the closed value vocabulary — order type, margin mode — is NOT relaxed).
//! - A field is reordered only when every required canonical field maps EXACTLY
//!   ONCE and unambiguously. A missing, duplicate, unknown, or multi-match field
//!   makes the whole canonicalization reject (the caller then returns the original
//!   fast-path error) — never guess, never silently drop a field.
//! - Free-text fields carry no distinguishing label/shape, so they are NOT
//!   reordered among themselves: they fill their canonical slots in their original
//!   relative order (DeFi `chain | protocolPool | token | redeemTerms`; the
//!   Prediction `event` is the single remaining free-text slot, assigned only after
//!   every uniquely shaped required field has been consumed).

use crate::asset_class::AssetClass;

use super::error::ParseError;
use super::{fields, Language};

/// One canonical positional slot and how a raw field is recognized as filling it.
enum Slot {
    /// Leading-keyword field, matched in either language and normalized to the
    /// header language (e.g. position `Position`/zh `position(zh)`, entry
    /// `Entry`/zh `entry(zh)`, neutral `SL`).
    Keyword { zh: &'static str, en: &'static str },
    /// The TTL field (zh suffix form `<n> <ttl-suffix zh>` / en prefix
    /// `valid for <n>`); normalized to the header language.
    Ttl,
    /// A field identified purely by shape (side, direction, range, `$token`,
    /// outcome `@`, contract code, `Call`/`Put`, take-profit). Emitted verbatim.
    Shape(fn(&str) -> bool),
    /// A free-text field with no label/shape. Emitted verbatim; filled from the
    /// leftover fields in their original relative order.
    FreeText,
}

fn kw(zh: &'static str, en: &'static str) -> Slot {
    Slot::Keyword { zh, en }
}

// ── Shape predicates ─────────────────────────────────────────────────────────

fn first_token_is(field: &str, a: &str, b: &str) -> bool {
    matches!(field.split_whitespace().next(), Some(t) if t == a || t == b)
}

/// Spot side: first token `BUY`/`SELL` (an optional order-type value follows).
fn is_spot_side(field: &str) -> bool {
    first_token_is(field, "BUY", "SELL")
}

/// Perp direction: first token `LONG`/`SHORT` (leverage / margin follow).
fn is_perp_dir(field: &str) -> bool {
    first_token_is(field, "LONG", "SHORT")
}

/// On-chain spot subject `$SYMBOL (ADDRESS)`.
fn is_onchain_token(field: &str) -> bool {
    field.starts_with('$')
}

/// Prediction outcome+odds: the only field allowed to carry `@`.
fn has_at(field: &str) -> bool {
    field.contains('@')
}

/// Option `<SIDE> <Call|Put>`: carries a `Call` or `Put` token.
fn has_option_type(field: &str) -> bool {
    field.split_whitespace().any(|t| t == "Call" || t == "Put")
}

// ── Per-class canonical slot layouts (in the order each per-class parser reads) ──

fn spot_cex_slots() -> Vec<Slot> {
    vec![
        Slot::FreeText, // pair BASE/QUOTE
        Slot::Shape(is_spot_side),
        Slot::Shape(fields::looks_like_range),
        kw(fields::KW_POSITION_ZH, fields::KW_POSITION_EN),
        Slot::Ttl,
    ]
}

fn spot_onchain_slots() -> Vec<Slot> {
    vec![
        Slot::FreeText, // chain
        Slot::Shape(is_onchain_token),
        Slot::Shape(is_spot_side),
        Slot::Shape(fields::looks_like_range),
        kw(fields::KW_SLIPPAGE_ZH, fields::KW_SLIPPAGE_EN),
        kw(fields::KW_POSITION_ZH, fields::KW_POSITION_EN),
        Slot::Ttl,
    ]
}

fn perp_slots() -> Vec<Slot> {
    vec![
        Slot::FreeText, // pair
        Slot::Shape(is_perp_dir),
        kw(fields::KW_ENTRY_ZH, fields::KW_ENTRY_EN),
        kw(fields::KW_SL, fields::KW_SL),
        Slot::Shape(fields::looks_like_take_profit),
        kw(fields::KW_POSITION_ZH, fields::KW_POSITION_EN),
        Slot::Ttl,
    ]
}

fn prediction_slots() -> Vec<Slot> {
    vec![
        Slot::FreeText, // event
        Slot::Shape(has_at),
        kw(fields::KW_POSITION_ZH, fields::KW_POSITION_EN),
        kw(fields::KW_SETTLE_ZH, fields::KW_SETTLE_EN),
        Slot::Ttl,
    ]
}

fn option_slots() -> Vec<Slot> {
    vec![
        Slot::Shape(fields::looks_like_contract_code),
        Slot::Shape(has_option_type),
        kw(fields::KW_STRIKE_ZH, fields::KW_STRIKE_EN),
        kw(fields::KW_EXPIRY_ZH, fields::KW_EXPIRY_EN),
        kw(fields::KW_PREMIUM_ZH, fields::KW_PREMIUM_EN),
        kw(fields::KW_POSITION_ZH, fields::KW_POSITION_EN),
        Slot::Ttl,
    ]
}

fn defi_slots() -> Vec<Slot> {
    vec![
        Slot::FreeText, // chain
        Slot::FreeText, // protocolPool
        kw(fields::KW_APY, fields::KW_APY),
        kw(fields::KW_TVL, fields::KW_TVL),
        Slot::FreeText, // token
        Slot::FreeText, // redeemTerms
        kw(fields::KW_POSITION_ZH, fields::KW_POSITION_EN),
        Slot::Ttl,
    ]
}

/// Pick the canonical slot layout for `class`. Spot has two positional forms, told
/// apart the same way the fast-path parser does: a `$`-prefixed subject field (and
/// a 7-field count) selects the on-chain form, a 5-field count the CEX form. A
/// count that matches neither form yields `None` (the caller rejects it).
fn slots_for(class: AssetClass, raw: &[String]) -> Option<Vec<Slot>> {
    match class {
        AssetClass::Spot => {
            let has_onchain_subject = raw.iter().any(|f| f.starts_with('$'));
            if raw.len() == 7 && has_onchain_subject {
                Some(spot_onchain_slots())
            } else if raw.len() == 5 {
                Some(spot_cex_slots())
            } else {
                None
            }
        }
        AssetClass::Perp => Some(perp_slots()),
        AssetClass::Prediction => Some(prediction_slots()),
        AssetClass::Option => Some(option_slots()),
        AssetClass::Defi => Some(defi_slots()),
    }
}

// ── Slot matching / normalization ──────────────────────────────────────────────

fn slot_matches(slot: &Slot, field: &str) -> bool {
    match slot {
        Slot::Keyword { zh, en } => fields::has_keyword_either(field, zh, en),
        Slot::Ttl => fields::is_ttl_field(field),
        Slot::Shape(pred) => pred(field),
        Slot::FreeText => false,
    }
}

fn slot_normalize(slot: &Slot, field: &str, lang: Language) -> String {
    match slot {
        Slot::Keyword { zh, en } => {
            fields::normalize_keyword(field, zh, en, lang).unwrap_or_else(|| field.to_string())
        }
        Slot::Ttl => fields::normalize_ttl(field, lang).unwrap_or_else(|| field.to_string()),
        Slot::Shape(_) | Slot::FreeText => field.to_string(),
    }
}

/// Assign each identified (non-free-text) slot to the single raw field that matches
/// it. Returns the per-slot raw index and marks the raw fields it consumed, or an
/// error if any slot is missing (0 matches), ambiguous (≥2 matches), or would reuse
/// a raw field already claimed by another slot (multi-match). Fail-closed.
fn assign_identified(
    slots: &[Slot],
    raw: &[String],
    slot_fill: &mut [Option<usize>],
    raw_used: &mut [bool],
) -> Result<(), ParseError> {
    for (si, slot) in slots.iter().enumerate() {
        if matches!(slot, Slot::FreeText) {
            continue;
        }
        let mut found: Option<usize> = None;
        for (ri, field) in raw.iter().enumerate() {
            if slot_matches(slot, field) {
                if found.is_some() {
                    return Err(ParseError::FieldCountError); // ambiguous / duplicate
                }
                found = Some(ri);
            }
        }
        let ri = found.ok_or(ParseError::FieldCountError)?; // missing required field
        if raw_used[ri] {
            return Err(ParseError::FieldCountError); // field claimed by two slots
        }
        raw_used[ri] = true;
        slot_fill[si] = Some(ri);
    }
    Ok(())
}

/// Canonicalize `raw` into the fixed positional order for `class`, normalizing field
/// keywords to the header `lang`. See the module doc for the determinism rules.
pub fn canonicalize(
    class: AssetClass,
    lang: Language,
    raw: &[String],
) -> Result<Vec<String>, ParseError> {
    let slots = slots_for(class, raw).ok_or(ParseError::FieldCountError)?;
    if raw.len() != slots.len() {
        return Err(ParseError::FieldCountError);
    }
    let n = slots.len();
    let mut slot_fill: Vec<Option<usize>> = vec![None; n];
    let mut raw_used: Vec<bool> = vec![false; n];

    // Pass 1: uniquely shaped/keyworded required fields.
    assign_identified(&slots, raw, &mut slot_fill, &mut raw_used)?;

    // Pass 2: free-text slots take the leftover fields in their original order.
    let mut leftovers = (0..n).filter(|ri| !raw_used[*ri]);
    for (si, slot) in slots.iter().enumerate() {
        if matches!(slot, Slot::FreeText) {
            slot_fill[si] = Some(leftovers.next().ok_or(ParseError::FieldCountError)?);
        }
    }
    if leftovers.next().is_some() {
        return Err(ParseError::FieldCountError); // an unclassifiable extra field
    }

    // Emit canonical order, normalizing keyword/ttl fields to the header language.
    let mut out = Vec::with_capacity(n);
    for (si, slot) in slots.iter().enumerate() {
        let ri = slot_fill[si].ok_or(ParseError::FieldCountError)?;
        out.push(slot_normalize(slot, &raw[ri], lang));
    }
    Ok(out)
}
