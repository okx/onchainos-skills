//! FR-1: input-format detection — an independent public entry point (FR-1.3),
//! deliberately NOT fused into [`super::parse_signal_text`].

use serde::Serialize;

/// Classification of a raw input string.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum InputFormat {
    /// First char `{` — a JSON signal (classified only, never parsed here).
    ///
    /// NOTE (MR !196): this is a FIRST-CHARACTER classification, so it CANNOT
    /// distinguish a legacy V1 structured-JSON signal from a V2 JSON envelope — both
    /// begin with `{` and both classify as `V1JsonSchema`. Envelope-vs-legacy
    /// discrimination is a CONTENT decision (the `schemaVersion` field checked by
    /// `super::parse_envelope` / `parse_envelope_full`), not a `detect_format`
    /// concern. Callers must route `V1JsonSchema` by inspecting `schemaVersion`, not
    /// by assuming it is the legacy schema.
    V1JsonSchema,
    /// First char `【` (U+3010) — a V2 text signal.
    V2Text,
    /// Empty, leading whitespace, or any other first char.
    Unsupported,
}

/// FR-1.2 first-char rule — no leading-whitespace tolerance.
pub fn detect_format(input: &str) -> InputFormat {
    match input.chars().next() {
        Some('{') => InputFormat::V1JsonSchema,
        Some('【') => InputFormat::V2Text,
        _ => InputFormat::Unsupported,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn first_char_brace_is_v1() {
        assert_eq!(
            detect_format("{\"schemaVersion\":2}"),
            InputFormat::V1JsonSchema
        );
    }

    /// MR !196: `detect_format` cannot tell a V1 legacy JSON signal from a V2 JSON
    /// envelope — both start with `{`, so both classify as `V1JsonSchema`. The
    /// version distinction lives in the `schemaVersion` field, not the first char.
    #[test]
    fn v1_and_v2_json_share_first_char_classification() {
        let v1_legacy = "{\"schemaVersion\":1,\"signalType\":\"dex_trade\"}";
        let v2_envelope =
            "{\"schemaVersion\":2,\"deliveryId\":\"abc\",\"signalTime\":1,\"signalText\":\"x\"}";
        assert_eq!(detect_format(v1_legacy), InputFormat::V1JsonSchema);
        assert_eq!(detect_format(v2_envelope), InputFormat::V1JsonSchema);
        assert_eq!(detect_format(v1_legacy), detect_format(v2_envelope));
    }

    #[test]
    fn first_char_cjk_bracket_is_v2() {
        assert_eq!(
            detect_format("【\u{73b0}\u{8d27}】\u{5e02}\u{573a}:BTC/USDT"),
            InputFormat::V2Text
        );
    }

    #[test]
    fn empty_or_ws_or_other_is_unsupported() {
        assert_eq!(detect_format(""), InputFormat::Unsupported);
        assert_eq!(
            detect_format(" 【\u{73b0}\u{8d27}】"),
            InputFormat::Unsupported
        ); // leading space
        assert_eq!(detect_format("\t{"), InputFormat::Unsupported);
        assert_eq!(
            detect_format("[\u{73b0}\u{8d27}]"),
            InputFormat::Unsupported
        ); // half-width '['
        assert_eq!(detect_format("hello"), InputFormat::Unsupported);
    }
}
