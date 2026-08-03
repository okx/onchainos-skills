//! V2 text trading-signal parser core (FR-1 / FR-2 / FR-3).
//!
//! Pure-local, deterministic, bilingual (zh/en), fail-closed. Zero I/O, zero
//! network, no `SystemTime`/locale/randomness (NFR-1..NFR-3). Three independent
//! public entry points:
//! - [`detect_format`] — classify a raw string (FR-1).
//! - [`parse_signal_text`] — parse one V1.1 `signalText` → [`ParsedSignal`] (FR-2).
//! - [`parse_envelope`] — validate a V2 wire envelope then delegate (FR-3).
//!
//! ## Input grammar
//! The authoritative wire grammar is trade-signal specification v1.1. Each
//! asset class is a FIXED, POSITIONAL sequence of `|`-separated
//! fields behind a full-title header; a field's meaning comes from its position,
//! its shape, and a small set of reserved keywords embedded IN the value (e.g.
//! `LONG 3x`, `SL 3300`, `position(zh) 5%`) — NOT a reorderable `label:value` map. The
//! byte-exact bilingual acceptance corpus is in `corpus_v1_1.txt`; see [`header`]
//! and [`fields`].
//!
//! The [`ParsedSignal`] result is an **internal** parser model, NOT a second
//! frozen public trading-signal contract. Its field naming follows the runtime
//! autotrade conventions (`positionPct`, `ttlSec`) and it reuses the shared
//! `Decimal` + [`AssetClass`] types. Wiring this output into
//! the runtime `agent_commerce::task::common::autotrade` pipeline is a separate
//! task (Task 3); this module only produces the parse result.

use serde::Serialize;

use crate::asset_class::AssetClass;

pub mod canonical;
pub mod envelope;
pub mod error;
pub mod fields;
pub mod format;
pub mod header;
pub mod params;

#[cfg(test)]
mod tests;

pub use error::ParseError;
pub use format::{detect_format, InputFormat};

/// Signal language, derived from the header (never guessed). Wire: `"zh" | "en"`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Language {
    Zh,
    En,
}

/// An absolute price interval; both bounds are plain decimal strings, `lo < hi`.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PriceRange {
    pub lo: String,
    pub hi: String,
}

/// Trade side. Wire: `"BUY" | "SELL"`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "UPPERCASE")]
pub enum Side {
    Buy,
    Sell,
}

/// Perp direction. Wire: `"LONG" | "SHORT"`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "UPPERCASE")]
pub enum Direction {
    Long,
    Short,
}

/// Spot order type. Wire: `"market" | "limit"` (defaults to `market`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum OrderType {
    Market,
    Limit,
}

/// Perp margin mode. Wire: `"cross" | "isolated"`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum MarginMode {
    Cross,
    Isolated,
}

/// Prediction outcome. Wire: `"YES" | "NO" | "UP" | "DOWN"`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "UPPERCASE")]
pub enum Outcome {
    Yes,
    No,
    Up,
    Down,
}

/// Option type. Wire: `"Call" | "Put"`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum OptionType {
    Call,
    Put,
}

/// DeFi execution semantics — fixed to `deposit` for this text format.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum ExecutionSemantics {
    Deposit,
}

/// Spot params (FR-2.2). `tokenAddr`/`slippage` present only for the on-chain form.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SpotParams {
    pub market: String,
    pub symbol: String,
    pub side: Side,
    pub price_range: PriceRange,
    pub order_type: OrderType,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub token_addr: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub slippage: Option<String>,
}

/// Perp params (FR-2.3).
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PerpParams {
    pub pair: String,
    pub direction: Direction,
    pub leverage: u32,
    pub entry_range: PriceRange,
    pub stop_loss: String,
    pub take_profit: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub margin_mode: Option<MarginMode>,
}

/// Prediction params (FR-2.4). `event` is free text — NEVER echoed in errors (SR-3).
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PredictionParams {
    pub event: String,
    pub outcome: Outcome,
    pub odds: String,
    pub settle_date: String,
}

/// Option params (FR-2.5). `strike`/`expiry`/`optionType` cross-check `contractCode`.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OptionParams {
    pub contract_code: String,
    pub side: Side,
    pub option_type: OptionType,
    pub strike: String,
    pub expiry: String,
    pub premium_cap: String,
}

/// DeFi params (FR-2.6). `chain`/`protocolPool` are unresolved strings (D-1).
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DefiParams {
    pub chain: String,
    pub protocol_pool: String,
    pub apy: String,
    pub tvl: String,
    pub token: String,
    pub redeem_terms: String,
    pub execution_semantics: ExecutionSemantics,
}

/// Closed 1:1 variant set keyed by asset class (internally tagged as `kind`).
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum SignalParams {
    Spot(SpotParams),
    Perp(PerpParams),
    Prediction(PredictionParams),
    Option(OptionParams),
    Defi(DefiParams),
}

/// The public parse result — an internal parser model (NOT a frozen public
/// contract), named to match the runtime autotrade conventions (`positionPct`,
/// `ttlSec`). `assetClass` (top) and `params.kind` are always equal.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ParsedSignal {
    pub asset_class: AssetClass,
    pub language: Language,
    pub position_pct: String,
    pub ttl_sec: u64,
    pub params: SignalParams,
}

/// Parse one V1.1-spec `signalText` into a typed [`ParsedSignal`] (FR-2). Fixed,
/// fail-closed order; errors NEVER echo the raw input (SR-3).
///
/// The canonical fixed-order parse is the fast path. When it fails, a deterministic
/// fallback ([`canonical::canonicalize`]) accepts bilingual field-keyword mixing and
/// safe field reordering — but only when every required field maps exactly once and
/// unambiguously (MR !196). If the fallback cannot canonicalize, or its re-validated
/// parse fails, the ORIGINAL fast-path error is returned so the error contract for a
/// genuinely-invalid signal is unchanged.
pub fn parse_signal_text(input: &str) -> Result<ParsedSignal, ParseError> {
    // 1. length / single-line guards (NFR-5).
    if input.is_empty() {
        return Err(ParseError::EmptyInput);
    }
    if input.contains('\n') || input.contains('\r') {
        return Err(ParseError::MultiLine);
    }
    if input.chars().count() > 200 {
        return Err(ParseError::TooLong);
    }
    // SR-2: no injected content (emoji / link) anywhere in the input.
    if fields::contains_forbidden(input) {
        return Err(ParseError::ForbiddenContent);
    }

    // 2-3. exact header → (asset class, language) + remainder.
    let (asset_class, language, remainder) = header::parse_header(input)?;

    // 4. split on '|', trim, reject empties → ordered positional fields.
    let raw_fields = fields::split_pipe_fields(remainder)?;

    // 5. fast path: canonical positional order, header-language keywords.
    match assemble(asset_class, language, &raw_fields) {
        Ok(signal) => Ok(signal),
        Err(fast_err) => match canonical::canonicalize(asset_class, language, &raw_fields) {
            // 6. deterministic mixed-language + safe-reorder fallback.
            Ok(canonical_fields) => {
                assemble(asset_class, language, &canonical_fields).map_err(|_| fast_err)
            }
            Err(_) => Err(fast_err),
        },
    }
}

/// Assemble a [`ParsedSignal`] from `fields` already in canonical positional order.
/// Shared by the fast path (raw fields) and the reorder fallback (canonicalized
/// fields), so the SR-2 `@` guard and the per-class validators apply identically to
/// both. `@` is legal ONLY in the Prediction outcome field (canonical index 1, the
/// `<OUTCOME> @<odds>` separator); anywhere else it is injected content.
fn assemble(
    asset_class: AssetClass,
    language: Language,
    fields: &[String],
) -> Result<ParsedSignal, ParseError> {
    for (i, f) in fields.iter().enumerate() {
        let at_ok = asset_class == AssetClass::Prediction && i == 1;
        if !at_ok && f.contains('@') {
            return Err(ParseError::ForbiddenContent);
        }
    }

    // per-class positional parse (incl. the class-placed position/ttl).
    let (params, position_pct, ttl_sec) = params::dispatch(asset_class, language, fields)?;

    Ok(ParsedSignal {
        asset_class,
        language,
        position_pct,
        ttl_sec,
        params,
    })
}

pub use envelope::{parse_envelope, parse_envelope_full, ParsedEnvelope};
