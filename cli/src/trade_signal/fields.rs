//! FR-2 field ingestion — the authoritative V1.1 grammar.
//!
//! The wire format is NOT a reorderable `label:value` map. Each asset class is a
//! FIXED, POSITIONAL sequence of `|`-separated fields; a field's meaning comes
//! from its POSITION, its SHAPE, and a small set of reserved KEYWORDS embedded in
//! the field value — the keyword is part of the value, never a `label:`. Example
//! (en perp):
//!
//! ```text
//! 【Futures Signal】ETH-PERP | LONG 3x | Entry 3420-3450 | SL 3300 | TP1 3720 | Position 10% | valid for 4h
//! ```
//!
//! Here `ETH-PERP` is positional (the pair), `LONG 3x` packs direction+leverage,
//! `Entry`/`SL`/`TP1`/`Position` are reserved keyword prefixes, and the zh form
//! wraps TTL as `<ttl> <ttl-suffix zh>` where en wraps it as `valid for <ttl>`. Chain-based
//! spot's subject spans TWO positional fields (`chain | $token (address)`), so
//! the two spot forms are told apart by the SHAPE of the 2nd field, not by a
//! per-class fixed index.
//!
//! All Chinese keyword literals are `\u{…}` escapes (never raw Han bytes) so this
//! production source clears the `onchainos_check` "no CJK in Rust source" lint;
//! the escape compiles to the exact same bytes as the literal glyph. The verbatim
//! bilingual corpus itself lives in `corpus_v1_1.txt` (a non-`.rs` fixture), which
//! is where the byte-exact Chinese is allowed to live.
//!
//! All numeric parse/compare goes through the repo's exact [`Decimal`] (no float,
//! NFR-2). `Decimal::parse` already rejects sign / exponent / whitespace /
//! thousands-separator / lone-dot, which satisfies the PRD numeric grammar.

use crate::commands::agent_commerce::task::common::autotrade::amount::Decimal;

use super::error::ParseError;
use super::{Direction, Language, MarginMode, OptionType, OrderType, Outcome, PriceRange, Side};

// ── Reserved keywords (zh forms are \u{…}-escaped; see module doc) ────────────
//
// Leading keyword + a single ASCII space then the value, e.g. `position(zh) 5%`. Neutral
// keywords (SL / TP / APY / TVL) are Latin in both languages, so zh == en.

/// Position: zh `position(zh)` / en `Position`.
pub const KW_POSITION_ZH: &str = "\u{4ed3}\u{4f4d}";
pub const KW_POSITION_EN: &str = "Position";
/// TTL wrapper: zh suffix `<ttl-suffix zh>` (`<ttl> <ttl-suffix zh>`) / en prefix `valid for` (`valid for <ttl>`).
const KW_TTL_SUFFIX_ZH: &str = "\u{5185}\u{6709}\u{6548}";
const KW_TTL_PREFIX_EN: &str = "valid for";
/// Slippage: zh `slippage(zh)` / en `Slippage`.
pub const KW_SLIPPAGE_ZH: &str = "\u{6ed1}\u{70b9}";
pub const KW_SLIPPAGE_EN: &str = "Slippage";
/// Perp entry range: zh `entry(zh)` / en `Entry`.
pub const KW_ENTRY_ZH: &str = "\u{5165}\u{573a}";
pub const KW_ENTRY_EN: &str = "Entry";
/// Perp stop-loss (neutral): `SL`.
pub const KW_SL: &str = "SL";
/// Perp take-profit tag prefix (neutral): `TP` + a 1-based index.
const KW_TP: &str = "TP";
/// Prediction settle date: zh `settle(zh)` / en `Settle`.
pub const KW_SETTLE_ZH: &str = "\u{7ed3}\u{7b97}";
pub const KW_SETTLE_EN: &str = "Settle";
/// Option strike: zh `strike(zh)` / en `Strike`.
pub const KW_STRIKE_ZH: &str = "\u{884c}\u{6743}\u{4ef7}";
pub const KW_STRIKE_EN: &str = "Strike";
/// Option expiry: zh `expiry(zh)` / en `Expiry`.
pub const KW_EXPIRY_ZH: &str = "\u{5230}\u{671f}";
pub const KW_EXPIRY_EN: &str = "Expiry";
/// Option premium cap: zh `premium(zh)` / en `Premium`.
pub const KW_PREMIUM_ZH: &str = "\u{6743}\u{5229}\u{91d1}";
pub const KW_PREMIUM_EN: &str = "Premium";
/// DeFi APY / TVL (neutral, Latin in both languages).
pub const KW_APY: &str = "APY";
pub const KW_TVL: &str = "TVL";
/// Perp margin-mode values: zh `isolated(zh)`(isolated) / `cross(zh)`(cross); en `isolated` / `cross`.
const MM_ISO_ZH: &str = "\u{9010}\u{4ed3}";
const MM_CROSS_ZH: &str = "\u{5168}\u{4ed3}";
/// Spot CEX order-type values: zh `limit(zh)`(limit) / `market(zh)`(market); en `limit` / `market`.
const OT_LIMIT_ZH: &str = "\u{9650}\u{4ef7}";
const OT_MARKET_ZH: &str = "\u{5e02}\u{4ef7}";
/// The `≤` cap prefix on slippage / premium (`≤1%`, `≤320 USDT`).
const LE: char = '\u{2264}';
/// Hard upper bound for perp `leverage` (IO-IN-01, MR !196 review). It bounds the
/// value that flows into the autotrade/payment path so it cannot reach `u32::MAX`,
/// bringing leverage in line with every other economically-sensitive scalar in this
/// module (position 0.1-20%, slippage ≤5%, odds [0,1], ttl 300..=604800s), all of
/// which are range-checked. Set to 125 — the highest leverage any supported
/// perpetual venue offers, so a value above it cannot correspond to a real
/// executable position and is rejected fail-safe (tightening only ever rejects
/// more, never permits more). A stricter per-venue policy, if product later defines
/// one, is a one-constant edit (mirrors `MAX_SLIPPAGE_BPS` in autotrade `schema.rs`).
const MAX_LEVERAGE: u32 = 125;

// ── Field splitting ────────────────────────────────────────────────────────────

/// Split the post-header remainder on `|`, trim each field, and reject an empty
/// remainder ([`ParseError::FieldCountError`]) or any empty field
/// ([`ParseError::EmptyField`]). The returned values are POSITIONAL — no label
/// parsing; each per-class parser interprets them by position + shape.
pub fn split_pipe_fields(remainder: &str) -> Result<Vec<String>, ParseError> {
    if remainder.trim().is_empty() {
        return Err(ParseError::FieldCountError);
    }
    let mut out = Vec::new();
    for part in remainder.split('|') {
        let trimmed = part.trim();
        if trimmed.is_empty() {
            return Err(ParseError::EmptyField);
        }
        out.push(trimmed.to_string());
    }
    Ok(out)
}

// ── Keyword stripping ─────────────────────────────────────────────────────────

/// If `field` starts with `kw` followed by exactly the field (kw-only) or an
/// ASCII space, return the trimmed remainder; `None` otherwise. The trailing-space
/// requirement stops `APY` from matching `APYX`.
fn strip_leading(field: &str, kw: &str) -> Option<String> {
    let rest = field.strip_prefix(kw)?;
    if rest.is_empty() {
        Some(String::new())
    } else if rest.starts_with(' ') {
        Some(rest.trim().to_string())
    } else {
        None
    }
}

/// Strip a leading keyword (in the header's language) and its separator, returning
/// the value. If the field instead carries the OTHER language's keyword →
/// [`ParseError::LanguageMix`]; if it matches neither → [`ParseError::FieldCountError`]
/// (a shape/order violation for this position).
pub fn strip_kw(field: &str, zh: &str, en: &str, lang: Language) -> Result<String, ParseError> {
    let (want, other) = match lang {
        Language::Zh => (zh, en),
        Language::En => (en, zh),
    };
    if let Some(v) = strip_leading(field, want) {
        return Ok(v);
    }
    if want != other && strip_leading(field, other).is_some() {
        return Err(ParseError::LanguageMix);
    }
    Err(ParseError::FieldCountError)
}

// ── Reorder-fallback helpers (FR-2 mixed-language + safe reorder, MR !196) ─────
//
// These support the deterministic canonicalizer in `super::canonical`. They accept
// a field keyword in EITHER language and re-emit it in the header language so the
// existing per-class validators (which are strictly header-language) run unchanged.
// They only touch the leading keyword; the value is preserved verbatim.

/// True if `field` begins with the zh OR the en keyword (kw-only or kw + space).
/// Language-agnostic keyword probe used to classify a field into a canonical slot.
pub fn has_keyword_either(field: &str, zh: &str, en: &str) -> bool {
    strip_leading(field, zh).is_some() || strip_leading(field, en).is_some()
}

/// Rewrite a keyword-prefixed field so its leading keyword is in `lang`, keeping the
/// value verbatim. Returns `None` if the field carries neither language's keyword.
pub fn normalize_keyword(field: &str, zh: &str, en: &str, lang: Language) -> Option<String> {
    let value = strip_leading(field, zh).or_else(|| strip_leading(field, en))?;
    let want = match lang {
        Language::Zh => zh,
        Language::En => en,
    };
    Some(if value.is_empty() {
        want.to_string()
    } else {
        format!("{want} {value}")
    })
}

/// True if `field` is a TTL field in either language (zh `<n> <suffix>` / en
/// `valid for <n>`).
pub fn is_ttl_field(field: &str) -> bool {
    field.ends_with(KW_TTL_SUFFIX_ZH) || field.starts_with(KW_TTL_PREFIX_EN)
}

/// Rewrite a TTL field to `lang`, keeping the numeric core verbatim. Returns `None`
/// if the field is not a TTL field in either language.
pub fn normalize_ttl(field: &str, lang: Language) -> Option<String> {
    let core = if let Some(n) = field.strip_suffix(KW_TTL_SUFFIX_ZH) {
        n.trim()
    } else if let Some(n) = field.strip_prefix(KW_TTL_PREFIX_EN) {
        n.trim()
    } else {
        return None;
    };
    Some(match lang {
        Language::Zh => format!("{core} {KW_TTL_SUFFIX_ZH}"),
        Language::En => format!("{KW_TTL_PREFIX_EN} {core}"),
    })
}

/// Shape-only probe for a bare `<decimal>-<decimal>` price range (spot). Range
/// ordering (`lo < hi`) is validated later by [`parse_range`]; this only locates
/// the field so it can be routed to its canonical slot.
pub fn looks_like_range(field: &str) -> bool {
    let mut parts = field.split('-');
    match (parts.next(), parts.next(), parts.next()) {
        (Some(a), Some(b), None) => Decimal::parse(a).is_ok() && Decimal::parse(b).is_ok(),
        _ => false,
    }
}

/// Shape-only probe for the option `contractCode` `UNDERLYING-YYMMDD-STRIKE-(C|P)`
/// (4 dash-separated parts, last is `C` or `P`). Full consistency is checked later.
pub fn looks_like_contract_code(field: &str) -> bool {
    let parts: Vec<&str> = field.split('-').collect();
    parts.len() == 4 && matches!(parts[3], "C" | "P")
}

/// Shape-only probe for the perp take-profit field: `TP` immediately followed by a
/// digit (`TP1 …`). Validation of the tag sequence is done by [`parse_take_profits`].
pub fn looks_like_take_profit(field: &str) -> bool {
    field
        .strip_prefix(KW_TP)
        .and_then(|rest| rest.chars().next())
        .is_some_and(|c| c.is_ascii_digit())
}

// ── Common trailing fields (position / ttl) ───────────────────────────────────

/// Parse a `position(zh) N%` / `Position N%` field → normalized position percent.
pub fn parse_position_field(field: &str, lang: Language) -> Result<String, ParseError> {
    let v = strip_kw(field, KW_POSITION_ZH, KW_POSITION_EN, lang)?;
    parse_position(&v)
}

/// Parse a `<ttl> <ttl-suffix zh>` (zh) / `valid for <ttl>` (en) field → seconds.
pub fn parse_ttl_field(field: &str, lang: Language) -> Result<u64, ParseError> {
    let raw = match lang {
        Language::Zh => field
            .strip_suffix(KW_TTL_SUFFIX_ZH)
            .ok_or(ParseError::FieldCountError)?,
        Language::En => field
            .strip_prefix(KW_TTL_PREFIX_EN)
            .ok_or(ParseError::FieldCountError)?,
    };
    parse_ttl(raw.trim())
}

// ── Per-position keyword strippers (used by the class parsers) ────────────────

/// Strip the perp entry keyword (`entry(zh)` / `Entry`) → the raw range string.
pub fn strip_entry(field: &str, lang: Language) -> Result<String, ParseError> {
    strip_kw(field, KW_ENTRY_ZH, KW_ENTRY_EN, lang)
}

/// Strip the perp stop-loss keyword (`SL`, neutral) → the raw price string.
pub fn strip_stop_loss(field: &str, lang: Language) -> Result<String, ParseError> {
    strip_kw(field, KW_SL, KW_SL, lang)
}

/// Strip the prediction settle-date keyword (`settle(zh)` / `Settle`) → the raw date.
pub fn strip_settle(field: &str, lang: Language) -> Result<String, ParseError> {
    strip_kw(field, KW_SETTLE_ZH, KW_SETTLE_EN, lang)
}

/// Strip the option strike keyword (`strike(zh)` / `Strike`) → the raw price string.
pub fn strip_strike(field: &str, lang: Language) -> Result<String, ParseError> {
    strip_kw(field, KW_STRIKE_ZH, KW_STRIKE_EN, lang)
}

/// Strip the option expiry keyword (`expiry(zh)` / `Expiry`) → the raw date string.
pub fn strip_expiry(field: &str, lang: Language) -> Result<String, ParseError> {
    strip_kw(field, KW_EXPIRY_ZH, KW_EXPIRY_EN, lang)
}

/// Strip the option premium keyword (`premium(zh)` / `Premium`) → the raw `≤N [CCY]`.
pub fn strip_premium(field: &str, lang: Language) -> Result<String, ParseError> {
    strip_kw(field, KW_PREMIUM_ZH, KW_PREMIUM_EN, lang)
}

/// Strip the DeFi APY keyword (`APY`, neutral) → the raw percent string.
pub fn strip_apy(field: &str, lang: Language) -> Result<String, ParseError> {
    strip_kw(field, KW_APY, KW_APY, lang)
}

/// Strip the DeFi TVL keyword (`TVL`, neutral) → the raw compact-amount string.
pub fn strip_tvl(field: &str, lang: Language) -> Result<String, ParseError> {
    strip_kw(field, KW_TVL, KW_TVL, lang)
}

// ── Spot shape helpers ─────────────────────────────────────────────────────────

/// Parse the on-chain spot subject `$SYMBOL (ADDRESS)` → `(symbol, address)`.
pub fn parse_onchain_token(field: &str) -> Result<(String, String), ParseError> {
    let rest = field.strip_prefix('$').ok_or(ParseError::FieldCountError)?;
    let (sym, addr_part) = rest.split_once('(').ok_or(ParseError::FieldCountError)?;
    let addr = addr_part
        .strip_suffix(')')
        .ok_or(ParseError::FieldCountError)?;
    let symbol = sym.trim();
    let address = addr.trim();
    if symbol.is_empty() || address.is_empty() {
        return Err(ParseError::FieldCountError);
    }
    Ok((symbol.to_string(), address.to_string()))
}

/// Parse the spot on-chain `slippage(zh) ≤N%` / `Slippage ≤N%` field (≤5% ceiling, SR-6).
pub fn parse_slippage_field(field: &str, lang: Language) -> Result<String, ParseError> {
    let v = strip_kw(field, KW_SLIPPAGE_ZH, KW_SLIPPAGE_EN, lang)?;
    let v = v.strip_prefix(LE).unwrap_or(&v).trim();
    parse_percent_max(v, "5", "slippage")
}

/// Parse the spot CEX `<SIDE> [orderType]` field → `(Side, OrderType)`. A bare
/// side defaults `orderType` to `market`; a trailing `limit`/`market(zh)`/… sets it.
pub fn parse_side_order(field: &str, lang: Language) -> Result<(Side, OrderType), ParseError> {
    let mut it = field.split_whitespace();
    let side = parse_side(it.next().ok_or(ParseError::FieldCountError)?)?;
    let order_type = match it.next() {
        None => OrderType::Market,
        Some(tok) => parse_order_type_kw(tok, lang)?,
    };
    if it.next().is_some() {
        return Err(ParseError::FieldCountError);
    }
    Ok((side, order_type))
}

/// Split a CEX pair `BASE/QUOTE` → `(symbol=BASE, market=pair)`. A pair with no
/// `/` uses the whole string for both.
pub fn split_pair(pair: &str) -> (String, String) {
    let symbol = pair.split('/').next().unwrap_or(pair);
    (symbol.to_string(), pair.to_string())
}

// ── Perp shape helpers ─────────────────────────────────────────────────────────

/// Parse the perp `<DIR> <LEV>x [marginMode]` field → `(Direction, leverage, marginMode?)`.
pub fn parse_dir_lev_margin(
    field: &str,
    lang: Language,
) -> Result<(Direction, u32, Option<MarginMode>), ParseError> {
    let mut it = field.split_whitespace();
    let direction = parse_direction(it.next().ok_or(ParseError::FieldCountError)?)?;
    let lev_tok = it.next().ok_or(ParseError::FieldCountError)?;
    let lev_str = lev_tok
        .strip_suffix('x')
        .or_else(|| lev_tok.strip_suffix('X'))
        .unwrap_or(lev_tok);
    let leverage = parse_leverage(lev_str)?;
    let margin_mode = match it.next() {
        None => None,
        Some(tok) => Some(parse_margin_mode_kw(tok, lang)?),
    };
    if it.next().is_some() {
        return Err(ParseError::FieldCountError);
    }
    Ok((direction, leverage, margin_mode))
}

/// Parse the perp take-profit field: `TP1 v1 [/ TP2 v2 [/ TP3 v3]]`. Tags must be
/// contiguous from 1; 1..=3 entries; each value an exact decimal.
pub fn parse_take_profits(field: &str) -> Result<Vec<String>, ParseError> {
    let mut out = Vec::new();
    for (i, entry) in field.split('/').enumerate() {
        let entry = entry.trim();
        let rest = entry
            .strip_prefix(KW_TP)
            .ok_or(ParseError::DirectionConstraint("takeProfit"))?;
        let mut toks = rest.split_whitespace();
        let tag = toks
            .next()
            .ok_or(ParseError::DirectionConstraint("takeProfit"))?;
        let value = toks
            .next()
            .ok_or(ParseError::DirectionConstraint("takeProfit"))?;
        if toks.next().is_some() {
            return Err(ParseError::DirectionConstraint("takeProfit"));
        }
        let n: usize = tag
            .parse()
            .map_err(|_| ParseError::DirectionConstraint("takeProfit"))?;
        if n != i + 1 {
            return Err(ParseError::DirectionConstraint("takeProfit"));
        }
        out.push(parse_decimal(value, "takeProfit")?);
    }
    if out.is_empty() || out.len() > 3 {
        return Err(ParseError::DirectionConstraint("takeProfit"));
    }
    Ok(out)
}

// ── Option shape helpers ────────────────────────────────────────────────────────

/// Parse the option `<SIDE> <Call|Put>` field → `(Side, OptionType)`.
pub fn parse_side_type(field: &str, _lang: Language) -> Result<(Side, OptionType), ParseError> {
    let mut it = field.split_whitespace();
    let side = parse_option_side(it.next().ok_or(ParseError::FieldCountError)?)?;
    let option_type = parse_option_type(it.next().ok_or(ParseError::FieldCountError)?)?;
    if it.next().is_some() {
        return Err(ParseError::FieldCountError);
    }
    Ok((side, option_type))
}

/// Parse the option `≤N [CCY]` premium-cap value (currency token, if present, is
/// accepted but not stored). No float; the number goes through [`Decimal`].
pub fn parse_premium_cap(value: &str) -> Result<String, ParseError> {
    let v = value.strip_prefix(LE).unwrap_or(value).trim();
    let num = v
        .split_whitespace()
        .next()
        .ok_or(ParseError::InvalidNumber("premiumCap"))?;
    parse_decimal(num, "premiumCap")
}

// ── Keyword enums ───────────────────────────────────────────────────────────────

fn parse_order_type_kw(tok: &str, lang: Language) -> Result<OrderType, ParseError> {
    let (limit, market, other_limit, other_market) = match lang {
        Language::En => ("limit", "market", OT_LIMIT_ZH, OT_MARKET_ZH),
        Language::Zh => (OT_LIMIT_ZH, OT_MARKET_ZH, "limit", "market"),
    };
    if tok == limit {
        Ok(OrderType::Limit)
    } else if tok == market {
        Ok(OrderType::Market)
    } else if tok == other_limit || tok == other_market {
        Err(ParseError::LanguageMix)
    } else {
        Err(ParseError::IllegalKeyword("orderType"))
    }
}

fn parse_margin_mode_kw(tok: &str, lang: Language) -> Result<MarginMode, ParseError> {
    let (iso, cross, other_iso, other_cross) = match lang {
        Language::En => ("isolated", "cross", MM_ISO_ZH, MM_CROSS_ZH),
        Language::Zh => (MM_ISO_ZH, MM_CROSS_ZH, "isolated", "cross"),
    };
    if tok == iso {
        Ok(MarginMode::Isolated)
    } else if tok == cross {
        Ok(MarginMode::Cross)
    } else if tok == other_iso || tok == other_cross {
        Err(ParseError::LanguageMix)
    } else {
        Err(ParseError::IllegalKeyword("marginMode"))
    }
}

// ── Forbidden-content scan (SR-2) ─────────────────────────────────────────────

/// True if the input carries content beyond the field grammar: a link or an
/// emoji. CJK glyphs, full-width brackets, `$`, `≤`, `…` and ASCII quotes are NOT
/// flagged. `@` is handled per-field after header classification (the Prediction
/// odds separator is legal), so it is deliberately NOT globally banned here.
///
/// NOTE (IO-IN-01 LOW): this is a deliberately NON-EXHAUSTIVE fail-closed injection
/// guard, not a URL/emoji parser. The `www.` substring test is a heuristic that can
/// false-positive on a legitimate Prediction `event` mentioning a bare `www.` host;
/// that is acceptable because the field grammar never requires a URL, so rejecting
/// one is safe. The emoji block list covers the common ranges, not every pictograph
/// codepoint. Widening either is a data-only edit if a concrete gap is found.
pub fn contains_forbidden(s: &str) -> bool {
    if s.contains("http://") || s.contains("https://") || s.contains("www.") {
        return true;
    }
    s.chars().any(is_emoji)
}

fn is_emoji(c: char) -> bool {
    let cp = c as u32;
    (0x1F000..=0x1FAFF).contains(&cp) // pictographs, emoticons, transport, symbols-extended
        || (0x2600..=0x27BF).contains(&cp) // misc symbols + dingbats
        || (0x2B00..=0x2BFF).contains(&cp) // misc symbols and arrows
        || (0x2100..=0x214F).contains(&cp) // letterlike symbols (™ ℠ ℡ …)
        || (0x2190..=0x21FF).contains(&cp) // arrows (← → ↔ …)
        || (0x2300..=0x23FF).contains(&cp) // misc technical (⌚ ⌛ ⏰ ⏳ …)
        || (0x25A0..=0x25FF).contains(&cp) // geometric shapes (■ ▲ ● ◆ …)
        || cp == 0x200D // zero-width joiner
        || cp == 0xFE0F // variation selector-16
}

// ── Numeric / range / ttl / date validators ───────────────────────────────────

/// `a < b` for exact decimals (`Decimal` exposes only `le` + `PartialEq`).
fn decimal_lt(a: &Decimal, b: &Decimal) -> bool {
    a.le(b) && a != b
}

/// `a < b` for two decimal strings (already-validated values). A parse failure is
/// treated as `false` — callers only compare values that parsed successfully.
pub fn less_than(a: &str, b: &str) -> bool {
    match (Decimal::parse(a), Decimal::parse(b)) {
        (Ok(x), Ok(y)) => decimal_lt(&x, &y),
        _ => false,
    }
}

/// `a > b` for two decimal strings.
pub fn greater_than(a: &str, b: &str) -> bool {
    less_than(b, a)
}

/// `a == b` for two decimal strings (scale-normalized exact equality).
pub fn equal(a: &str, b: &str) -> bool {
    match (Decimal::parse(a), Decimal::parse(b)) {
        (Ok(x), Ok(y)) => x == y,
        _ => false,
    }
}

/// Parse a plain absolute decimal price; malformed → [`ParseError::InvalidNumber`]
/// carrying `field`.
pub fn parse_decimal(value: &str, field: &'static str) -> Result<String, ParseError> {
    let d = Decimal::parse(value).map_err(|_| ParseError::InvalidNumber(field))?;
    Ok(d.to_plain_string())
}

/// Parse a `lo-hi` absolute price range for `field`; enforces `lo < hi`.
pub fn parse_range(value: &str, field: &'static str) -> Result<PriceRange, ParseError> {
    let parts: Vec<&str> = value.split('-').collect();
    if parts.len() != 2 {
        return Err(ParseError::InvalidNumber(field));
    }
    let lo = Decimal::parse(parts[0]).map_err(|_| ParseError::InvalidNumber(field))?;
    let hi = Decimal::parse(parts[1]).map_err(|_| ParseError::InvalidNumber(field))?;
    if !decimal_lt(&lo, &hi) {
        return Err(ParseError::OutOfRange(field));
    }
    Ok(PriceRange {
        lo: lo.to_plain_string(),
        hi: hi.to_plain_string(),
    })
}

/// Parse a canonical compact amount `[$]<decimal>[K|M|B|T]` for `field` (e.g.
/// `$2.4M`, `500M`, `1.2B`, `100000`). No float — the numeric core goes through
/// [`Decimal`]. The `$` prefix and magnitude unit are preserved verbatim; any
/// leftover garbage or a non-decimal core → [`ParseError::InvalidNumber`].
pub fn parse_compact_amount(value: &str, field: &'static str) -> Result<String, ParseError> {
    let (had_dollar, body) = match value.strip_prefix('$') {
        Some(rest) => (true, rest),
        None => (false, value),
    };
    let (num, unit) = match body.chars().last() {
        Some(c @ ('K' | 'M' | 'B' | 'T')) => (&body[..body.len() - c.len_utf8()], Some(c)),
        _ => (body, None),
    };
    if num.is_empty() {
        return Err(ParseError::InvalidNumber(field));
    }
    let d = Decimal::parse(num).map_err(|_| ParseError::InvalidNumber(field))?;
    let core = match unit {
        Some(u) => format!("{}{u}", d.to_plain_string()),
        None => d.to_plain_string(),
    };
    Ok(if had_dollar { format!("${core}") } else { core })
}

/// Parse `positionPct`: a single `N%` in `0.1 ..= 20`; normalized to a plain
/// decimal string without the `%`. Range / multi-value / `0` → out of range.
pub fn parse_position(value: &str) -> Result<String, ParseError> {
    let num = value
        .strip_suffix('%')
        .ok_or(ParseError::OutOfRange("position"))?;
    let p = Decimal::parse(num).map_err(|_| ParseError::OutOfRange("position"))?;
    let lo = Decimal::parse("0.1").expect("literal");
    let hi = Decimal::parse("20").expect("literal");
    // 0.1 <= p <= 20
    if decimal_lt(&p, &lo) || decimal_lt(&hi, &p) {
        return Err(ParseError::OutOfRange("position"));
    }
    Ok(p.to_plain_string())
}

/// Parse a percent value with an upper bound (e.g. slippage ≤ 5) for `field`; strips `%`.
pub fn parse_percent_max(
    value: &str,
    max: &str,
    field: &'static str,
) -> Result<String, ParseError> {
    let num = value
        .strip_suffix('%')
        .ok_or(ParseError::InvalidNumber(field))?;
    let p = Decimal::parse(num).map_err(|_| ParseError::InvalidNumber(field))?;
    let cap = Decimal::parse(max).expect("literal");
    if !p.le(&cap) {
        return Err(ParseError::OutOfRange(field));
    }
    Ok(p.to_plain_string())
}

/// Parse a non-negative percent value (e.g. APY) for `field`; strips `%`. `Decimal`
/// is always non-negative, so this only rejects malformed numbers.
pub fn parse_percent_nonneg(value: &str, field: &'static str) -> Result<String, ParseError> {
    let num = value
        .strip_suffix('%')
        .ok_or(ParseError::InvalidNumber(field))?;
    let p = Decimal::parse(num).map_err(|_| ParseError::InvalidNumber(field))?;
    Ok(p.to_plain_string())
}

/// Parse odds: an absolute decimal in `[0, 1]`.
pub fn parse_odds(value: &str) -> Result<String, ParseError> {
    let p = Decimal::parse(value).map_err(|_| ParseError::InvalidNumber("odds"))?;
    let one = Decimal::parse("1").expect("literal");
    if !p.le(&one) {
        return Err(ParseError::OutOfRange("odds"));
    }
    Ok(p.to_plain_string())
}

/// Parse the Prediction `outcome` field `<OUTCOME> @<odds>`. Exactly one `@`: the
/// head is an outcome keyword, the tail an odds decimal in `[0,1]`.
pub fn parse_outcome_odds(value: &str) -> Result<(Outcome, String), ParseError> {
    let (outcome_str, odds_str) = value
        .split_once('@')
        .ok_or(ParseError::IllegalKeyword("outcome"))?;
    if odds_str.contains('@') {
        return Err(ParseError::ForbiddenContent);
    }
    let outcome = parse_outcome(outcome_str.trim())?;
    let odds = parse_odds(odds_str.trim())?;
    Ok((outcome, odds))
}

/// Parse leverage: a positive integer in `1..=MAX_LEVERAGE`. Both a `0` and a
/// value above the cap fail as `OutOfRange("leverage")` (IO-IN-01).
pub fn parse_leverage(value: &str) -> Result<u32, ParseError> {
    if value.is_empty() || !value.bytes().all(|b| b.is_ascii_digit()) {
        return Err(ParseError::OutOfRange("leverage"));
    }
    let n: u32 = value
        .parse()
        .map_err(|_| ParseError::OutOfRange("leverage"))?;
    if !(1..=MAX_LEVERAGE).contains(&n) {
        return Err(ParseError::OutOfRange("leverage"));
    }
    Ok(n)
}

/// Parse TTL `Nmin | Nh | Nd` → seconds in `300 ..= 604800` (5min..=7d).
pub fn parse_ttl(value: &str) -> Result<u64, ParseError> {
    let (num, mult) = if let Some(n) = value.strip_suffix("min") {
        (n, 60u64)
    } else if let Some(n) = value.strip_suffix('h') {
        (n, 3_600u64)
    } else if let Some(n) = value.strip_suffix('d') {
        (n, 86_400u64)
    } else {
        return Err(ParseError::OutOfRange("ttl")); // unknown unit
    };
    if num.is_empty() || !num.bytes().all(|b| b.is_ascii_digit()) {
        return Err(ParseError::OutOfRange("ttl"));
    }
    let n: u64 = num.parse().map_err(|_| ParseError::OutOfRange("ttl"))?;
    let secs = n.checked_mul(mult).ok_or(ParseError::OutOfRange("ttl"))?;
    if !(300..=604_800).contains(&secs) {
        return Err(ParseError::OutOfRange("ttl"));
    }
    Ok(secs)
}

fn is_leap(year: i64) -> bool {
    (year % 4 == 0 && year % 100 != 0) || year % 400 == 0
}

fn days_in_month(year: i64, month: u32) -> u32 {
    match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 => {
            if is_leap(year) {
                29
            } else {
                28
            }
        }
        _ => 0,
    }
}

/// Validate a proleptic-Gregorian `YYYY-MM-DD` for `field` (leap-aware, no system
/// clock); returns the zero-padded canonical form.
pub fn parse_date(value: &str, field: &'static str) -> Result<String, ParseError> {
    let parts: Vec<&str> = value.split('-').collect();
    if parts.len() != 3 {
        return Err(ParseError::InvalidDate(field));
    }
    let (ys, ms, ds) = (parts[0], parts[1], parts[2]);
    if ys.len() != 4 || !ys.bytes().all(|b| b.is_ascii_digit()) {
        return Err(ParseError::InvalidDate(field)); // missing / malformed year
    }
    let year: i64 = ys.parse().map_err(|_| ParseError::InvalidDate(field))?;
    let month: u32 = parse_date_component(ms, field)?;
    let day: u32 = parse_date_component(ds, field)?;
    if !(1..=12).contains(&month) {
        return Err(ParseError::InvalidDate(field));
    }
    let dim = days_in_month(year, month);
    if day < 1 || day > dim {
        return Err(ParseError::InvalidDate(field));
    }
    Ok(format!("{year:04}-{month:02}-{day:02}"))
}

fn parse_date_component(s: &str, field: &'static str) -> Result<u32, ParseError> {
    if s.is_empty() || s.len() > 2 || !s.bytes().all(|b| b.is_ascii_digit()) {
        return Err(ParseError::InvalidDate(field));
    }
    s.parse().map_err(|_| ParseError::InvalidDate(field))
}

// ── Keyword whitelists (IllegalKeyword on any non-canonical variant) ──────────

pub fn parse_side(value: &str) -> Result<Side, ParseError> {
    match value {
        "BUY" => Ok(Side::Buy),
        "SELL" => Ok(Side::Sell),
        _ => Err(ParseError::IllegalKeyword("side")),
    }
}

/// Option side accepts the canonical + bilingual variants (FR-2.5).
pub fn parse_option_side(value: &str) -> Result<Side, ParseError> {
    match value {
        "BUY" | "Buy" | "\u{4e70}\u{5165}" => Ok(Side::Buy),
        "SELL" | "Sell" | "\u{5356}\u{51fa}" => Ok(Side::Sell),
        _ => Err(ParseError::IllegalKeyword("side")),
    }
}

pub fn parse_direction(value: &str) -> Result<Direction, ParseError> {
    match value {
        "LONG" => Ok(Direction::Long),
        "SHORT" => Ok(Direction::Short),
        _ => Err(ParseError::IllegalKeyword("direction")),
    }
}

pub fn parse_outcome(value: &str) -> Result<Outcome, ParseError> {
    match value {
        "YES" => Ok(Outcome::Yes),
        "NO" => Ok(Outcome::No),
        "UP" => Ok(Outcome::Up),
        "DOWN" => Ok(Outcome::Down),
        _ => Err(ParseError::IllegalKeyword("outcome")),
    }
}

pub fn parse_option_type(value: &str) -> Result<OptionType, ParseError> {
    match value {
        "Call" => Ok(OptionType::Call),
        "Put" => Ok(OptionType::Put),
        _ => Err(ParseError::IllegalKeyword("optionType")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn position_field_strips_keyword_and_bounds() {
        assert_eq!(
            parse_position_field("Position 5%", Language::En).unwrap(),
            "5"
        );
        assert_eq!(
            parse_position_field("\u{4ed3}\u{4f4d} 0.1%", Language::Zh).unwrap(),
            "0.1"
        );
        assert_eq!(
            parse_position_field("Position 0%", Language::En),
            Err(ParseError::OutOfRange("position"))
        );
        // wrong-language keyword under a zh header.
        assert_eq!(
            parse_position_field("Position 5%", Language::Zh),
            Err(ParseError::LanguageMix)
        );
        // missing keyword prefix → shape/order violation.
        assert_eq!(
            parse_position_field("5%", Language::En),
            Err(ParseError::FieldCountError)
        );
    }

    #[test]
    fn ttl_field_zh_suffix_and_en_prefix() {
        assert_eq!(
            parse_ttl_field("24h \u{5185}\u{6709}\u{6548}", Language::Zh).unwrap(),
            86_400
        );
        assert_eq!(
            parse_ttl_field("valid for 5min", Language::En).unwrap(),
            300
        );
        assert_eq!(
            parse_ttl_field("7d", Language::En),
            Err(ParseError::FieldCountError)
        );
    }

    #[test]
    fn onchain_token_shape() {
        assert_eq!(
            parse_onchain_token("$TOKEN (0xabc)").unwrap(),
            ("TOKEN".to_string(), "0xabc".to_string())
        );
        assert_eq!(
            parse_onchain_token("TOKEN (0xabc)"),
            Err(ParseError::FieldCountError)
        ); // no $
        assert_eq!(
            parse_onchain_token("$TOKEN"),
            Err(ParseError::FieldCountError)
        ); // no (addr)
    }

    #[test]
    fn side_order_default_and_explicit() {
        assert_eq!(
            parse_side_order("BUY", Language::En).unwrap(),
            (Side::Buy, OrderType::Market)
        );
        assert_eq!(
            parse_side_order("BUY limit", Language::En).unwrap(),
            (Side::Buy, OrderType::Limit)
        );
        assert_eq!(
            parse_side_order("BUY \u{9650}\u{4ef7}", Language::Zh).unwrap(),
            (Side::Buy, OrderType::Limit)
        );
        // en order-type keyword under a zh header → language mix.
        assert_eq!(
            parse_side_order("BUY limit", Language::Zh),
            Err(ParseError::LanguageMix)
        );
    }

    #[test]
    fn dir_lev_margin_packing() {
        assert_eq!(
            parse_dir_lev_margin("LONG 3x", Language::En).unwrap(),
            (Direction::Long, 3, None)
        );
        assert_eq!(
            parse_dir_lev_margin("SHORT 2x isolated", Language::En).unwrap(),
            (Direction::Short, 2, Some(MarginMode::Isolated))
        );
        assert_eq!(
            parse_dir_lev_margin("SHORT 2x \u{9010}\u{4ed3}", Language::Zh).unwrap(),
            (Direction::Short, 2, Some(MarginMode::Isolated))
        );
        // zero leverage → out of range, attributed to `leverage`.
        assert_eq!(
            parse_dir_lev_margin("LONG 0x", Language::En),
            Err(ParseError::OutOfRange("leverage"))
        );
    }

    #[test]
    fn take_profits_tagged_slash_form() {
        assert_eq!(parse_take_profits("TP1 3720").unwrap(), vec!["3720"]);
        assert_eq!(
            parse_take_profits("TP1 96000 / TP2 94000").unwrap(),
            vec!["96000", "94000"]
        );
        // non-contiguous tag (TP1 then TP3).
        assert_eq!(
            parse_take_profits("TP1 1 / TP3 2"),
            Err(ParseError::DirectionConstraint("takeProfit"))
        );
        // more than three.
        assert_eq!(
            parse_take_profits("TP1 1 / TP2 2 / TP3 3 / TP4 4"),
            Err(ParseError::DirectionConstraint("takeProfit"))
        );
        // malformed value → number error attributed to `takeProfit`.
        assert_eq!(
            parse_take_profits("TP1 1e3"),
            Err(ParseError::InvalidNumber("takeProfit"))
        );
    }

    #[test]
    fn premium_cap_strips_le_and_currency() {
        assert_eq!(parse_premium_cap("\u{2264}320 USDT").unwrap(), "320");
        assert_eq!(parse_premium_cap("320").unwrap(), "320");
    }

    #[test]
    fn slippage_field_ceiling() {
        assert_eq!(
            parse_slippage_field("Slippage \u{2264}1%", Language::En).unwrap(),
            "1"
        );
        assert_eq!(
            parse_slippage_field("Slippage \u{2264}9%", Language::En),
            Err(ParseError::OutOfRange("slippage"))
        );
    }

    /// `tvl` is validated as a canonical compact amount
    /// (no float). Legal forms parse; illegal free text is rejected as
    /// `invalid_number` attributed to `tvl`.
    #[test]
    fn compact_amount_legal_and_illegal() {
        // legal: optional `$`, optional K/M/B/T magnitude, or a bare integer.
        assert_eq!(parse_compact_amount("$2.4M", "tvl").unwrap(), "$2.4M");
        assert_eq!(parse_compact_amount("500M", "tvl").unwrap(), "500M");
        assert_eq!(parse_compact_amount("1.2B", "tvl").unwrap(), "1.2B");
        assert_eq!(parse_compact_amount("100000", "tvl").unwrap(), "100000");
        // illegal: garbage text, bad unit, thousands sep, lone `$`, sci-notation.
        for bad in ["abc", "2.4X", "1,000M", "$", "1e3M", "2.4MM"] {
            assert_eq!(
                parse_compact_amount(bad, "tvl"),
                Err(ParseError::InvalidNumber("tvl")),
                "expected tvl invalid_number for {bad:?}"
            );
        }
    }

    #[test]
    fn position_boundaries() {
        assert_eq!(parse_position("0.1%").unwrap(), "0.1");
        assert_eq!(parse_position("20%").unwrap(), "20");
        assert_eq!(
            parse_position("0%"),
            Err(ParseError::OutOfRange("position"))
        );
        assert_eq!(
            parse_position("20.1%"),
            Err(ParseError::OutOfRange("position"))
        );
        assert_eq!(
            parse_position("5-10%"),
            Err(ParseError::OutOfRange("position"))
        ); // range
        assert_eq!(parse_position("5"), Err(ParseError::OutOfRange("position")));
        // missing %
    }

    #[test]
    fn ttl_units_and_bounds() {
        assert_eq!(parse_ttl("5min").unwrap(), 300);
        assert_eq!(parse_ttl("7d").unwrap(), 604_800);
        assert_eq!(parse_ttl("24h").unwrap(), 86_400);
        assert_eq!(parse_ttl("4min"), Err(ParseError::OutOfRange("ttl"))); // below 5min
        assert_eq!(parse_ttl("8d"), Err(ParseError::OutOfRange("ttl"))); // above 7d
        assert_eq!(parse_ttl("30s"), Err(ParseError::OutOfRange("ttl"))); // unknown unit
    }

    #[test]
    fn range_and_decimal() {
        assert_eq!(
            parse_range("60000-65000", "price").unwrap(),
            PriceRange {
                lo: "60000".into(),
                hi: "65000".into()
            }
        );
        // inverted range → out of range attributed to the passed field.
        assert_eq!(
            parse_range("65000-60000", "entry"),
            Err(ParseError::OutOfRange("entry"))
        );
        assert_eq!(
            parse_decimal("1e3", "stopLoss"),
            Err(ParseError::InvalidNumber("stopLoss"))
        ); // sci-notation
        assert_eq!(
            parse_decimal("1,000", "strike"),
            Err(ParseError::InvalidNumber("strike"))
        ); // thousands sep
        assert_eq!(
            parse_decimal("5%", "price"),
            Err(ParseError::InvalidNumber("price"))
        ); // %-price
    }

    #[test]
    fn calendar_dates() {
        assert_eq!(
            parse_date("2024-02-29", "settleDate").unwrap(),
            "2024-02-29"
        ); // leap
        assert_eq!(
            parse_date("2025-02-29", "settleDate"),
            Err(ParseError::InvalidDate("settleDate"))
        ); // non-leap
        assert_eq!(
            parse_date("2025-13-01", "expiry"),
            Err(ParseError::InvalidDate("expiry"))
        ); // month
        assert_eq!(
            parse_date("12-31", "settleDate"),
            Err(ParseError::InvalidDate("settleDate"))
        ); // missing year
        assert_eq!(
            parse_date("2025-04-31", "expiry"),
            Err(ParseError::InvalidDate("expiry"))
        ); // 30-day month
    }

    #[test]
    fn keyword_whitelists() {
        assert_eq!(parse_direction("LONG").unwrap(), Direction::Long);
        assert_eq!(
            parse_direction("\u{505a}\u{591a}"),
            Err(ParseError::IllegalKeyword("direction"))
        );
        assert_eq!(parse_option_side("\u{4e70}\u{5165}").unwrap(), Side::Buy);
        assert_eq!(
            parse_side("\u{4e70}\u{5165}"),
            Err(ParseError::IllegalKeyword("side"))
        ); // spot side is canonical-only
        assert_eq!(parse_option_type("Call").unwrap(), OptionType::Call);
        assert_eq!(
            parse_option_type("call"),
            Err(ParseError::IllegalKeyword("optionType"))
        );
    }

    #[test]
    fn outcome_odds_field() {
        assert_eq!(
            parse_outcome_odds("YES @0.62").unwrap(),
            (Outcome::Yes, "0.62".to_string())
        );
        // no `@` → illegal_keyword attributed to `outcome`.
        assert_eq!(
            parse_outcome_odds("YES"),
            Err(ParseError::IllegalKeyword("outcome"))
        );
        // odds > 1 → out_of_range attributed to `odds`.
        assert_eq!(
            parse_outcome_odds("YES @1.5"),
            Err(ParseError::OutOfRange("odds"))
        );
        assert_eq!(
            parse_outcome_odds("MAYBE @0.5"),
            Err(ParseError::IllegalKeyword("outcome"))
        );
        assert_eq!(
            parse_outcome_odds("YES @0.5@0.6"),
            Err(ParseError::ForbiddenContent)
        ); // extra @
    }

    #[test]
    fn forbidden_scan() {
        assert!(contains_forbidden("gm https://x.io"));
        assert!(contains_forbidden("gm \u{1F680}"));
        // '@', '$', '≤', '…' and ASCII quotes are NOT globally forbidden.
        assert!(!contains_forbidden("@alpha"));
        assert!(!contains_forbidden("$TOKEN (0x12a3\u{2026}9fab)"));
        assert!(!contains_forbidden("Slippage \u{2264}1%"));
        assert!(!contains_forbidden("\"Fed cuts rates in Sept?\""));
    }

    #[test]
    fn forbidden_scan_extended_emoji_blocks() {
        for s in ["a\u{2122}b", "up \u{2190}", "\u{231A} time", "box \u{25A0}"] {
            assert!(contains_forbidden(s), "expected forbidden: {s:?}");
        }
    }

    #[test]
    fn leverage_rejects_zero_and_non_integer() {
        assert_eq!(parse_leverage("10").unwrap(), 10);
        assert_eq!(parse_leverage("0"), Err(ParseError::OutOfRange("leverage"))); // zero
        assert_eq!(
            parse_leverage("10.5"),
            Err(ParseError::OutOfRange("leverage"))
        ); // non-integer
        assert_eq!(
            parse_leverage("-5"),
            Err(ParseError::OutOfRange("leverage"))
        ); // signed
        assert_eq!(parse_leverage(""), Err(ParseError::OutOfRange("leverage")));
        // empty
        // IO-IN-01: the max leverage parses, one above the cap is out of range, and
        // an astronomically large value (previously accepted up to u32::MAX) is now
        // rejected instead of flowing into the autotrade/payment path.
        assert_eq!(parse_leverage("125").unwrap(), 125);
        assert_eq!(
            parse_leverage("126"),
            Err(ParseError::OutOfRange("leverage"))
        );
        assert_eq!(
            parse_leverage("1000000"),
            Err(ParseError::OutOfRange("leverage"))
        );
    }

    #[test]
    fn split_pipe_fields_trims_and_rejects_empty() {
        assert_eq!(
            split_pipe_fields("a | b |c").unwrap(),
            vec!["a".to_string(), "b".to_string(), "c".to_string()]
        );
        assert_eq!(split_pipe_fields("a||b"), Err(ParseError::EmptyField));
        assert_eq!(split_pipe_fields("   "), Err(ParseError::FieldCountError));
    }
}
