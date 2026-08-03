//! Closed-set parser error type (repo convention: hand-rolled `Display` +
//! `std::error::Error`, NOT `thiserror` — see architecture DR-B / NFR-2 "no new
//! deps"; mirrors `AmountError` / `StrategyError`).
//!
//! SR-3 hard requirement: `code()` / `field()` / `message()` carry ONLY a stable
//! category + stable field NAME — NEVER the raw signal text, `tokenAddr`,
//! `event`, `contractCode` value, or any ASP-authored free text. The field-name
//! payloads on the value/constraint variants are `&'static str` canonical names
//! chosen by the parser (never derived from input), which keeps an input leak
//! structurally impossible.

/// The stable closed-set of parse/validation/envelope failures.
///
/// The `code()` strings (not the Rust variant names) are the external stability
/// contract and are UNCHANGED. Per the specification the
/// five value/constraint variants now carry the offending CANONICAL FIELD NAME so
/// `field()` returns the real field (e.g. `stopLoss` vs `takeProfit`, `settleDate`
/// vs `expiry`, `tvl`) instead of a `number`/`range`/`date` category placeholder.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ParseError {
    /// Empty input string.
    EmptyInput,
    /// First char is neither `{` nor `【`, or there is leading whitespace.
    ///
    /// RESERVED (MR !196 review LOW): `parse_signal_text` never returns this today —
    /// it always reaches `header::parse_header`, which reports `UnknownHeader` for a
    /// bad prefix. The variant is intentionally kept (not dead-removed) as the stable
    /// code for a future format-gated entry point that short-circuits when
    /// `detect_format` is not `V2Text` (Task 3 wiring). Its `code()` is part of the
    /// external contract, so keeping it avoids a later breaking re-add.
    UnsupportedFormat,
    /// Envelope JSON is malformed / has an unknown or missing field.
    InvalidEnvelope,
    /// `schemaVersion` is not the required V2 value.
    InvalidSchemaVersion,
    /// `deliveryId` is absent / too long / has an illegal character.
    InvalidDeliveryId,
    /// `signalTime` is `0` (must be a non-zero epoch-ms stamp).
    InvalidSignalTime,
    /// More than 200 Unicode chars.
    TooLong,
    /// Contains a newline (single-line only).
    MultiLine,
    /// Header not in the 10-item whitelist / preceded by whitespace / half-width `[`.
    UnknownHeader,
    /// Wrong field count/order for the asset class (missing required / extra / reordered).
    FieldCountError,
    /// Any field empty after trim.
    EmptyField,
    /// Mixed zh/en labels in one signal.
    LanguageMix,
    /// Non-whitelist token variant. Carries the offending canonical field name.
    IllegalKeyword(&'static str),
    /// Sci-notation, thousands separator, %-price, or otherwise non-decimal number.
    /// Carries the offending canonical field name.
    InvalidNumber(&'static str),
    /// Value out of the allowed range. Carries the offending canonical field name.
    OutOfRange(&'static str),
    /// Missing year / nonexistent / malformed `YYYY-MM-DD`. Carries the field name.
    InvalidDate(&'static str),
    /// LONG/SHORT SL or TP on the wrong side, or a bad TP set. Carries the field
    /// name that actually failed (`stopLoss` vs `takeProfit`).
    DirectionConstraint(&'static str),
    /// `contractCode` inconsistent with Call/Put, strike, or expiry.
    OptionFieldMismatch,
    /// Emoji, link, out-of-place `@`-mention, or content beyond the field grammar.
    ForbiddenContent,
}

impl ParseError {
    /// Stable machine code string (external contract). MUST NOT change after ship;
    /// the field-name payload does NOT affect the code.
    pub fn code(&self) -> &'static str {
        match self {
            ParseError::EmptyInput => "empty_input",
            ParseError::UnsupportedFormat => "unsupported_format",
            ParseError::InvalidEnvelope => "invalid_envelope",
            ParseError::InvalidSchemaVersion => "invalid_schema_version",
            ParseError::InvalidDeliveryId => "invalid_delivery_id",
            ParseError::InvalidSignalTime => "invalid_signal_time",
            ParseError::TooLong => "too_long",
            ParseError::MultiLine => "multi_line",
            ParseError::UnknownHeader => "unknown_header",
            ParseError::FieldCountError => "field_count_error",
            ParseError::EmptyField => "empty_field",
            ParseError::LanguageMix => "language_mix",
            ParseError::IllegalKeyword(_) => "illegal_keyword",
            ParseError::InvalidNumber(_) => "invalid_number",
            ParseError::OutOfRange(_) => "out_of_range",
            ParseError::InvalidDate(_) => "invalid_date",
            ParseError::DirectionConstraint(_) => "direction_constraint",
            ParseError::OptionFieldMismatch => "option_field_mismatch",
            ParseError::ForbiddenContent => "forbidden_content",
        }
    }

    /// Stable CANONICAL field name for the offending parameter, or `None`. NEVER
    /// the value. The value/constraint variants return their carried field name;
    /// the envelope faults and option mismatch return their fixed field.
    pub fn field(&self) -> Option<&'static str> {
        match self {
            ParseError::InvalidEnvelope => Some("envelope"),
            ParseError::InvalidSchemaVersion => Some("schemaVersion"),
            ParseError::InvalidDeliveryId => Some("deliveryId"),
            ParseError::InvalidSignalTime => Some("signalTime"),
            ParseError::OptionFieldMismatch => Some("contractCode"),
            ParseError::IllegalKeyword(f)
            | ParseError::InvalidNumber(f)
            | ParseError::OutOfRange(f)
            | ParseError::InvalidDate(f)
            | ParseError::DirectionConstraint(f) => Some(f),
            ParseError::EmptyInput
            | ParseError::UnsupportedFormat
            | ParseError::TooLong
            | ParseError::MultiLine
            | ParseError::UnknownHeader
            | ParseError::EmptyField
            | ParseError::LanguageMix
            | ParseError::ForbiddenContent
            | ParseError::FieldCountError => None,
        }
    }

    /// Generic, value-free human message (SR-3 log-leak prevention).
    pub fn message(&self) -> &'static str {
        match self {
            ParseError::EmptyInput => "input is empty",
            ParseError::UnsupportedFormat => "unsupported input format",
            ParseError::InvalidEnvelope => "invalid v2 envelope",
            ParseError::InvalidSchemaVersion => "unsupported schema version",
            ParseError::InvalidDeliveryId => "invalid delivery id",
            ParseError::InvalidSignalTime => "invalid signal time",
            ParseError::TooLong => "input exceeds the 200 character limit",
            ParseError::MultiLine => "input must be a single line",
            ParseError::UnknownHeader => "unrecognized signal header",
            ParseError::FieldCountError => "wrong number or order of fields for the asset class",
            ParseError::EmptyField => "a field is empty after trimming",
            ParseError::LanguageMix => "mixed-language labels are not allowed",
            ParseError::IllegalKeyword(_) => "unrecognized keyword variant",
            ParseError::InvalidNumber(_) => "malformed number",
            ParseError::OutOfRange(_) => "value is out of the allowed range",
            ParseError::InvalidDate(_) => "invalid calendar date",
            ParseError::DirectionConstraint(_) => {
                "stop-loss/take-profit direction constraint violated"
            }
            ParseError::OptionFieldMismatch => "contract code is inconsistent with its fields",
            ParseError::ForbiddenContent => "input contains content beyond the field grammar",
        }
    }
}

impl std::fmt::Display for ParseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.message())
    }
}

impl std::error::Error for ParseError {}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every variant's `code()` is the stable snake_case external contract and is
    /// unaffected by the field-name payload.
    #[test]
    fn code_matches_external_contract() {
        assert_eq!(ParseError::EmptyInput.code(), "empty_input");
        assert_eq!(ParseError::UnsupportedFormat.code(), "unsupported_format");
        assert_eq!(ParseError::InvalidEnvelope.code(), "invalid_envelope");
        assert_eq!(
            ParseError::InvalidSchemaVersion.code(),
            "invalid_schema_version"
        );
        assert_eq!(ParseError::InvalidDeliveryId.code(), "invalid_delivery_id");
        assert_eq!(ParseError::InvalidSignalTime.code(), "invalid_signal_time");
        assert_eq!(ParseError::TooLong.code(), "too_long");
        assert_eq!(ParseError::MultiLine.code(), "multi_line");
        assert_eq!(ParseError::UnknownHeader.code(), "unknown_header");
        assert_eq!(ParseError::FieldCountError.code(), "field_count_error");
        assert_eq!(ParseError::EmptyField.code(), "empty_field");
        assert_eq!(ParseError::LanguageMix.code(), "language_mix");
        assert_eq!(ParseError::IllegalKeyword("side").code(), "illegal_keyword");
        assert_eq!(ParseError::InvalidNumber("price").code(), "invalid_number");
        assert_eq!(ParseError::OutOfRange("position").code(), "out_of_range");
        assert_eq!(ParseError::InvalidDate("expiry").code(), "invalid_date");
        assert_eq!(
            ParseError::DirectionConstraint("stopLoss").code(),
            "direction_constraint"
        );
        assert_eq!(
            ParseError::OptionFieldMismatch.code(),
            "option_field_mismatch"
        );
        assert_eq!(ParseError::ForbiddenContent.code(), "forbidden_content");
    }

    /// The value/constraint variants surface the real canonical field name, and
    /// the split envelope faults keep their distinct fields.
    #[test]
    fn field_carries_canonical_name() {
        assert_eq!(ParseError::OutOfRange("position").field(), Some("position"));
        assert_eq!(
            ParseError::InvalidDate("settleDate").field(),
            Some("settleDate")
        );
        assert_eq!(ParseError::InvalidDate("expiry").field(), Some("expiry"));
        assert_eq!(
            ParseError::DirectionConstraint("stopLoss").field(),
            Some("stopLoss")
        );
        assert_eq!(
            ParseError::DirectionConstraint("takeProfit").field(),
            Some("takeProfit")
        );
        assert_eq!(ParseError::InvalidNumber("tvl").field(), Some("tvl"));
        assert_eq!(
            ParseError::InvalidSchemaVersion.field(),
            Some("schemaVersion")
        );
        assert_eq!(ParseError::InvalidDeliveryId.field(), Some("deliveryId"));
        assert_eq!(ParseError::InvalidSignalTime.field(), Some("signalTime"));
        // structural errors carry no field.
        assert_eq!(ParseError::FieldCountError.field(), None);
    }

    #[test]
    fn display_writes_value_free_message() {
        assert_eq!(
            format!("{}", ParseError::OutOfRange("position")),
            "value is out of the allowed range"
        );
    }
}
