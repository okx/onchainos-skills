//! FR-3: V2 wire envelope. Validate `schemaVersion` / `deliveryId` / `signalTime`
//! FIRST (each mapped to its own decidable error), then delegate the inner
//! `signalText` to [`super::parse_signal_text`].
//!
//! Per V1.1 specification alignment the `deliveryId` check REUSES
//! the existing autotrade schema validator
//! ([`crate::commands::agent_commerce::task::common::autotrade::schema::check_delivery_id`])
//! rather than maintaining a second copy of the length/charset rules — so the
//! rule cannot drift. The envelope only validates protocol fields and delegates.

use serde::{Deserialize, Serialize};

use crate::commands::agent_commerce::task::common::autotrade::schema::check_delivery_id;

use super::error::ParseError;
use super::{parse_signal_text, ParsedSignal};

/// The required schema version for a V2 text envelope.
const V2_SCHEMA_VERSION: u32 = 2;

/// The V2 wire envelope. `deny_unknown_fields` mirrors the repo convention: a
/// newer schema may reinterpret fields, so unexpected keys are rejected.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct V2Envelope {
    pub schema_version: u32,
    pub delivery_id: String,
    /// Non-zero epoch milliseconds.
    pub signal_time: u64,
    pub signal_text: String,
}

/// A fully-validated V2 envelope: the parsed inner signal PLUS the protocol metadata
/// that was validated on the way in. Task 3 consumes `delivery_id` / `signal_time`
/// (the idempotency key and the freshness stamp) without re-parsing the envelope, so
/// no validated field is lost between this parser and the runtime pipeline (FR-3).
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ParsedEnvelope {
    pub delivery_id: String,
    pub signal_time: u64,
    pub signal: ParsedSignal,
}

/// Validate a V2 envelope JSON string, returning the validated protocol metadata
/// (`deliveryId` / `signalTime`) together with the parsed inner signal. This is the
/// NON-LOSSY entry point (FR-3): the metadata passes exactly the same checks as in
/// [`parse_envelope`], but is RETAINED for the caller instead of discarded. Each
/// protocol field still maps to its own fine-grained error.
pub fn parse_envelope_full(input: &str) -> Result<ParsedEnvelope, ParseError> {
    let env: V2Envelope = serde_json::from_str(input).map_err(|_| ParseError::InvalidEnvelope)?;

    if env.schema_version != V2_SCHEMA_VERSION {
        return Err(ParseError::InvalidSchemaVersion);
    }
    // Reuse the shared autotrade deliveryId validator (single source of truth).
    check_delivery_id(&env.delivery_id).map_err(|_| ParseError::InvalidDeliveryId)?;
    if env.signal_time == 0 {
        return Err(ParseError::InvalidSignalTime);
    }

    let signal = parse_signal_text(&env.signal_text)?;
    Ok(ParsedEnvelope {
        delivery_id: env.delivery_id,
        signal_time: env.signal_time,
        signal,
    })
}

/// Validate a V2 envelope JSON string then parse its `signalText`, returning only
/// the inner signal. A thin wrapper over [`parse_envelope_full`] for callers that do
/// not need the envelope metadata; the validation performed is identical.
pub fn parse_envelope(input: &str) -> Result<ParsedSignal, ParseError> {
    parse_envelope_full(input).map(|e| e.signal)
}

#[cfg(test)]
mod tests {
    use super::*;

    const VALID_TEXT: &str =
        "\u{3010}Spot Signal\u{3011}BTC/USDT | BUY | 60000-65000 | Position 5% | valid for 1h";

    fn envelope(schema: u32, delivery: &str, time: u64) -> String {
        format!(
            "{{\"schemaVersion\":{schema},\"deliveryId\":\"{delivery}\",\"signalTime\":{time},\"signalText\":\"{VALID_TEXT}\"}}"
        )
    }

    #[test]
    fn valid_envelope_delegates_to_text_parse() {
        let json = envelope(2, "abc123", 1_700_000_000_000);
        let parsed = parse_envelope(&json).unwrap();
        assert_eq!(parsed.asset_class.as_str(), "spot");
    }

    /// FR-3 (MR !196): the non-lossy entry point retains the validated `deliveryId`
    /// and `signalTime` alongside the parsed signal, and its inner signal is
    /// byte-identical to what `parse_envelope` returns.
    #[test]
    fn parse_envelope_full_retains_validated_metadata() {
        let json = envelope(2, "abc123", 1_700_000_000_000);
        let full = parse_envelope_full(&json).unwrap();
        assert_eq!(full.delivery_id, "abc123");
        assert_eq!(full.signal_time, 1_700_000_000_000);
        assert_eq!(full.signal.asset_class.as_str(), "spot");
        // parity: the thin wrapper returns exactly the embedded signal.
        assert_eq!(parse_envelope(&json).unwrap(), full.signal);
    }

    /// The metadata entry point rejects the same bad envelopes with the same codes —
    /// it must not become a laxer path than `parse_envelope`.
    #[test]
    fn parse_envelope_full_rejects_bad_fields() {
        assert_eq!(
            parse_envelope_full(&envelope(1, "abc123", 1))
                .unwrap_err()
                .code(),
            "invalid_schema_version"
        );
        assert_eq!(
            parse_envelope_full(&envelope(2, "abc123", 0))
                .unwrap_err()
                .code(),
            "invalid_signal_time"
        );
        assert_eq!(
            parse_envelope_full(&envelope(2, "bad id", 1))
                .unwrap_err()
                .code(),
            "invalid_delivery_id"
        );
    }

    #[test]
    fn rejects_bad_envelope_fields_with_distinct_codes() {
        // schemaVersion != 2 → invalid_schema_version.
        assert_eq!(
            parse_envelope(&envelope(1, "abc123", 1))
                .unwrap_err()
                .code(),
            "invalid_schema_version"
        );
        // signalTime == 0 → invalid_signal_time.
        assert_eq!(
            parse_envelope(&envelope(2, "abc123", 0))
                .unwrap_err()
                .code(),
            "invalid_signal_time"
        );
        // illegal deliveryId (space) → invalid_delivery_id.
        assert_eq!(
            parse_envelope(&envelope(2, "bad id", 1))
                .unwrap_err()
                .code(),
            "invalid_delivery_id"
        );
        // malformed JSON → invalid_envelope.
        assert_eq!(
            parse_envelope("not json").unwrap_err().code(),
            "invalid_envelope"
        );
    }
}
