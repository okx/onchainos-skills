pub const ERR_NOT_LOGGED_IN: &str = "not logged in";

/// Check whether `value` is a hex string (starts with "0x" followed by only hex digits).
/// Mirrors the JS `isHexString(value, length?)` helper exactly.
/// When `length` is `Some(n)` with `n > 0`, also checks that the hex part is exactly `n` bytes
/// (i.e. `value.len() == 2 + 2 * n`).
pub(crate) fn is_hex_string(value: &str, length: Option<usize>) -> bool {
    if !value.starts_with("0x") || !value[2..].bytes().all(|b| b.is_ascii_hexdigit()) {
        return false;
    }
    match length {
        Some(n) if n > 0 => value.len() == 2 + 2 * n,
        _ => true,
    }
}

/// Shared error handler for API responses that may require user confirmation.
///
/// - code=81362 and !force → return CliConfirming (needs user confirmation)
/// - other ApiCodeError → extract msg as plain error
/// - non-ApiCodeError → pass through
pub(crate) fn handle_confirming_error(e: anyhow::Error, force: bool) -> anyhow::Error {
    match e.downcast::<crate::wallet_api::ApiCodeError>() {
        Ok(api_err) => {
            if !force && api_err.code == "81362" {
                crate::output::CliConfirming {
                    message: api_err.msg,
                    next: "If the user confirms, re-run the same command with --force flag appended to proceed.".to_string(),
                }
                .into()
            } else {
                anyhow::anyhow!("{}", api_err.msg)
            }
        }
        Err(e) => e,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── no length param (None) ───────────────────────────────────────

    #[test]
    fn is_hex_string_valid_lowercase() {
        assert!(is_hex_string("0xabcdef1234567890", None));
    }

    #[test]
    fn is_hex_string_valid_uppercase() {
        assert!(is_hex_string("0xABCDEF1234567890", None));
    }

    #[test]
    fn is_hex_string_valid_mixed_case() {
        assert!(is_hex_string("0xaBcDeF", None));
    }

    #[test]
    fn is_hex_string_bare_0x_returns_true() {
        // JS: "0x".match(/^0x[0-9A-Fa-f]*$/) matches (* = zero or more)
        assert!(is_hex_string("0x", None));
    }

    #[test]
    fn is_hex_string_no_prefix_returns_false() {
        assert!(!is_hex_string("abcdef", None));
    }

    #[test]
    fn is_hex_string_plain_text_returns_false() {
        assert!(!is_hex_string("Hello World", None));
    }

    #[test]
    fn is_hex_string_non_hex_after_prefix_returns_false() {
        assert!(!is_hex_string("0xGHIJKL", None));
    }

    #[test]
    fn is_hex_string_empty_returns_false() {
        assert!(!is_hex_string("", None));
    }

    // ── with length param ────────────────────────────────────────────

    #[test]
    fn is_hex_string_length_match() {
        // 3 bytes = 6 hex chars → "0x" + 6 = len 8
        assert!(is_hex_string("0xabcdef", Some(3)));
    }

    #[test]
    fn is_hex_string_length_mismatch() {
        // expect 3 bytes (8 chars total) but value has 4 hex chars (2 bytes)
        assert!(!is_hex_string("0xabcd", Some(3)));
    }

    #[test]
    fn is_hex_string_length_32_bytes() {
        // 32 bytes = 64 hex chars → total len 66
        let addr = format!("0x{}", "a".repeat(64));
        assert!(is_hex_string(&addr, Some(32)));
        assert!(!is_hex_string("0xabc", Some(32)));
    }

    #[test]
    fn is_hex_string_length_zero_ignored() {
        // JS: length=0 is falsy → skip length check
        assert!(is_hex_string("0xab", Some(0)));
    }
}
