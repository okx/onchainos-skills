pub const ERR_NOT_LOGGED_IN: &str = "not logged in";

/// Check whether `value` is a hex string (starts with "0x" followed by only hex digits).
/// Mirrors the JS `isHexString(value)` helper (without the optional length check).
pub(crate) fn is_hex_string(value: &str) -> bool {
    value.starts_with("0x")
        && value.len() > 2
        && value[2..].bytes().all(|b| b.is_ascii_hexdigit())
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

    #[test]
    fn is_hex_string_valid_lowercase() {
        assert!(is_hex_string("0xabcdef1234567890"));
    }

    #[test]
    fn is_hex_string_valid_uppercase() {
        assert!(is_hex_string("0xABCDEF1234567890"));
    }

    #[test]
    fn is_hex_string_valid_mixed_case() {
        assert!(is_hex_string("0xaBcDeF"));
    }

    #[test]
    fn is_hex_string_bare_0x_returns_false() {
        assert!(!is_hex_string("0x"));
    }

    #[test]
    fn is_hex_string_no_prefix_returns_false() {
        assert!(!is_hex_string("abcdef"));
    }

    #[test]
    fn is_hex_string_plain_text_returns_false() {
        assert!(!is_hex_string("Hello World"));
    }

    #[test]
    fn is_hex_string_non_hex_after_prefix_returns_false() {
        assert!(!is_hex_string("0xGHIJKL"));
    }

    #[test]
    fn is_hex_string_empty_returns_false() {
        assert!(!is_hex_string(""));
    }
}
