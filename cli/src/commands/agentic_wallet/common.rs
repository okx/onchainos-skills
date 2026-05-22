pub const ERR_NOT_LOGGED_IN: &str = "not logged in";

/// Derive the most recent login mode from the persisted `wallets.json`
/// fields. No new field is stored on disk — `is_ak` and `email` already
/// encode the mode of the last successful login.
///
/// - `is_ak == true`            → `Some("ak")`
/// - `is_ak == false && email`  → `Some("email")`
/// - both empty / not logged in → `None`
///
/// Used by `cmd_status` (read-only display) and `cmd_login` (mode-diff
/// pre-check) so both surfaces share the same derivation rule.
pub(super) fn derive_last_login_mode(email: &str, is_ak: bool) -> Option<&'static str> {
    if is_ak {
        Some("ak")
    } else if !email.is_empty() {
        Some("email")
    } else {
        None
    }
}

/// Mask an email address for display in user-facing prompts and audit logs.
/// Keeps the first and last char of the local part, full domain. Local parts
/// of length ≤ 2 collapse to first-char-plus-stars. UTF-8 safe via char iter.
///
/// Examples:
///   `user@example.com` → `u***r@example.com`
///   `ab@example.com`   → `a***@example.com`
///   `a@example.com`    → `a***@example.com`
///   `@example.com`     → `***@example.com`
///   `noatsign`         → `***`
pub(super) fn mask_email(email: &str) -> String {
    match email.find('@') {
        Some(at) => {
            let local = &email[..at];
            let domain = &email[at..];
            let chars: Vec<char> = local.chars().collect();
            match chars.len() {
                0 => format!("***{domain}"),
                1 | 2 => format!("{}***{domain}", chars[0]),
                _ => format!("{}***{}{domain}", chars[0], chars[chars.len() - 1]),
            }
        }
        None => "***".to_string(),
    }
}


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
/// - other ApiCodeError → preserve full `Wallet API error (code=N): msg` form so
///   downstream agents can distinguish 81363 (TEE/broadcast revert) from a bare
///   on-chain "execution reverted" and route accordingly.
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
                anyhow::Error::from(api_err)
            }
        }
        Err(e) => e,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── derive_last_login_mode ───────────────────────────────────────

    #[test]
    fn derive_last_login_mode_ak_empty_email_returns_ak() {
        // FR-1-AC-2: is_ak=true with empty email → "ak".
        assert_eq!(derive_last_login_mode("", true), Some("ak"));
    }

    #[test]
    fn derive_last_login_mode_ak_with_email_returns_ak() {
        // FR-1 priority-note defensive case: is_ak=true wins even when
        // email is non-empty (combination doesn't occur in practice but
        // the priority is part of the contract).
        assert_eq!(derive_last_login_mode("user@example.com", true), Some("ak"));
    }

    #[test]
    fn derive_last_login_mode_email_only_returns_email() {
        assert_eq!(
            derive_last_login_mode("user@example.com", false),
            Some("email")
        );
    }

    #[test]
    fn derive_last_login_mode_empty_returns_none() {
        // Fresh / not-logged-in state: both signals empty/false.
        assert_eq!(derive_last_login_mode("", false), None);
    }

    // ── mask_email ───────────────────────────────────────────────────

    #[test]
    fn mask_email_typical_address_keeps_first_and_last_local_char() {
        assert_eq!(mask_email("user@example.com"), "u***r@example.com");
    }

    #[test]
    fn mask_email_two_char_local_keeps_first_only() {
        assert_eq!(mask_email("ab@example.com"), "a***@example.com");
    }

    #[test]
    fn mask_email_single_char_local_keeps_char() {
        assert_eq!(mask_email("a@example.com"), "a***@example.com");
    }

    #[test]
    fn mask_email_empty_local_uses_stars_only() {
        assert_eq!(mask_email("@example.com"), "***@example.com");
    }

    #[test]
    fn mask_email_no_at_sign_returns_stars() {
        assert_eq!(mask_email("noatsign"), "***");
    }

    #[test]
    fn mask_email_does_not_leak_full_local_part() {
        // PII-guard regression: full local part must never appear in output.
        let masked = mask_email("alicebob@example.com");
        assert!(!masked.contains("alicebob"), "got: {masked}");
        assert!(masked.starts_with('a') && masked.contains("@example.com"));
    }

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
