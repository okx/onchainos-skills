//! Address validation + recipient policy for the payment family.
//!
//! - EVM hex / EIP-55 checksum validation (`is_valid_evm_address`,
//!   `require_evm_address`) — used for token contracts, recipients, escrow.
//! - Recipient parsing that also accepts XLayer's cosmetic `XKO...` prefix
//!   (`parse_recipient_addr`, `require_recipient_format`) — splits the input
//!   into a canonical `0x` form for signing and a display form for echoing.

use anyhow::{bail, Result};
use tiny_keccak::{Hasher, Keccak};

/// Validate that `addr` is a 0x-prefixed 20-byte EVM address. If the input
/// carries case information (mixed-case letters), it must satisfy the EIP-55
/// checksum so a typo'd address can't silently route value to the wrong place.
/// All-lowercase and all-uppercase letter forms are accepted as-is (they
/// claim no checksum).
pub(crate) fn is_valid_evm_address(addr: &str) -> bool {
    if !addr.starts_with("0x") || addr.len() != 42 {
        return false;
    }
    let hex = &addr[2..];
    if !hex.bytes().all(|b| b.is_ascii_hexdigit()) {
        return false;
    }
    let has_lower_letter = hex.bytes().any(|b| b.is_ascii_lowercase());
    let has_upper_letter = hex.bytes().any(|b| b.is_ascii_uppercase());
    if !(has_lower_letter && has_upper_letter) {
        return true;
    }
    eip55_checksum_matches(hex)
}

pub(crate) fn require_evm_address(addr: &str, label: &str) -> Result<()> {
    if is_valid_evm_address(addr) {
        Ok(())
    } else {
        bail!("{label} is not a valid EVM address: {addr}")
    }
}

fn eip55_checksum_matches(hex: &str) -> bool {
    let lower = hex.to_ascii_lowercase();
    let mut keccak = Keccak::v256();
    keccak.update(lower.as_bytes());
    let mut hash = [0u8; 32];
    keccak.finalize(&mut hash);
    for (i, ch) in hex.chars().enumerate() {
        if !ch.is_ascii_alphabetic() {
            continue;
        }
        let nibble = (hash[i / 2] >> (4 * (1 - (i % 2)))) & 0x0f;
        let expect_upper = nibble >= 8;
        if expect_upper != ch.is_ascii_uppercase() {
            return false;
        }
    }
    true
}

/// XLayer's user-facing address prefix. `XKOaaaa...` carries the same 20-byte
/// payload as `0xaaaa...`; the prefix is purely cosmetic and only valid on
/// X Layer (chainId 196).
pub(crate) const XKO_PREFIX: &str = "XKO";
const XLAYER_CHAIN_ID: u64 = 196;

/// Parse a recipient address that may be in `0x...` form or `XKO...`
/// (XLayer-only). Returns `(canonical_0x, display)`:
/// - `canonical_0x` — always `0x...`, fed into signing / EIP-712 / alloy `Address`.
/// - `display` — the user/seller's original input string, echoed in CLI output.
///
/// `XKO`-prefixed input on a non-XLayer chain is an error so a misrouted
/// recipient can't silently coerce into a different address space.
pub(crate) fn parse_recipient_addr(input: &str, chain_id: u64) -> Result<(String, String)> {
    if let Some(body) = input.strip_prefix(XKO_PREFIX) {
        if chain_id != XLAYER_CHAIN_ID {
            bail!(
                "XKO-prefixed addresses are only supported on X Layer (chainId {}), got {}",
                XLAYER_CHAIN_ID,
                chain_id
            );
        }
        let canonical = format!("0x{body}");
        if !is_valid_evm_address(&canonical) {
            bail!(
                "XKO address body must be 40 hex chars (EIP-55 checksummed if mixed case): {input}"
            );
        }
        Ok((canonical, input.to_string()))
    } else if is_valid_evm_address(input) {
        Ok((input.to_string(), input.to_string()))
    } else {
        bail!("not a valid EVM address (expected `0x...` or XLayer `XKO...`): {input}");
    }
}

/// Format-only recipient check used when chain context isn't available locally
/// (e.g. `a2a-pay create` where the server resolves the chain from symbol).
/// Accepts both `0x...` (any chain) and `XKO...` (XLayer convention) forms and
/// returns `Ok(())`; the caller forwards the original string verbatim, leaving
/// chain validation to whichever downstream service has the chain context.
pub(crate) fn require_recipient_format(input: &str, label: &str) -> Result<()> {
    if let Some(body) = input.strip_prefix(XKO_PREFIX) {
        let canonical = format!("0x{body}");
        if !is_valid_evm_address(&canonical) {
            bail!("{label}: XKO address body must be 40 hex chars (EIP-55 if mixed case): {input}");
        }
        Ok(())
    } else if is_valid_evm_address(input) {
        Ok(())
    } else {
        bail!("{label} is not a valid EVM address (expected `0x...` or XLayer `XKO...`): {input}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── is_valid_evm_address (EIP-55) ────────────────────────────────

    #[test]
    fn evm_addr_eip55_canonical_vectors_pass() {
        // Canonical EIP-55 test vectors from https://eips.ethereum.org/EIPS/eip-55
        for addr in [
            "0x5aAeb6053F3E94C9b9A09f33669435E7Ef1BeAed",
            "0xfB6916095ca1df60bB79Ce92cE3Ea74c37c5d359",
            "0xdbF03B407c01E7cD3CBea99509d93f8DDDC8C6FB",
            "0xD1220A0cf47c7B9Be7A2E6BA89F429762e7b9aDb",
        ] {
            assert!(is_valid_evm_address(addr), "expected EIP-55 valid: {addr}");
        }
    }

    #[test]
    fn evm_addr_all_lowercase_accepted() {
        // No checksum claimed → treat as opaque hex.
        assert!(is_valid_evm_address(
            "0x5aaeb6053f3e94c9b9a09f33669435e7ef1beaed"
        ));
    }

    #[test]
    fn evm_addr_all_uppercase_accepted() {
        // No checksum claimed → treat as opaque hex.
        assert!(is_valid_evm_address(
            "0x5AAEB6053F3E94C9B9A09F33669435E7EF1BEAED"
        ));
    }

    #[test]
    fn evm_addr_mixed_case_with_bad_checksum_rejected() {
        // Same address, one letter case flipped → fails EIP-55.
        assert!(!is_valid_evm_address(
            "0x5AAeb6053F3E94C9b9A09f33669435E7Ef1BeAed"
        ));
    }

    #[test]
    fn evm_addr_format_violations_rejected() {
        assert!(!is_valid_evm_address("0x123"));
        assert!(!is_valid_evm_address(
            "5aAeb6053F3E94C9b9A09f33669435E7Ef1BeAed"
        ));
        assert!(!is_valid_evm_address(
            "0xZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZ"
        ));
    }

    #[test]
    fn require_evm_address_error_message() {
        let err = require_evm_address("0xnope", "challenge.request.currency").unwrap_err();
        assert!(err.to_string().contains("challenge.request.currency"));
    }

    // ── parse_recipient_addr ─────────────────────────────────────────

    #[test]
    fn parse_recipient_0x_passthrough() {
        let (canonical, display) =
            parse_recipient_addr("0x5aAeb6053F3E94C9b9A09f33669435E7Ef1BeAed", 196).unwrap();
        assert_eq!(canonical, "0x5aAeb6053F3E94C9b9A09f33669435E7Ef1BeAed");
        assert_eq!(display, "0x5aAeb6053F3E94C9b9A09f33669435E7Ef1BeAed");
    }

    #[test]
    fn parse_recipient_0x_works_on_any_chain() {
        // 0x form is canonical EVM; valid on any chain.
        let (c, d) = parse_recipient_addr("0x5aaeb6053f3e94c9b9a09f33669435e7ef1beaed", 1).unwrap();
        assert_eq!(c, d);
        assert!(c.starts_with("0x"));
    }

    #[test]
    fn parse_recipient_xko_on_xlayer() {
        let (canonical, display) =
            parse_recipient_addr("XKO5aAeb6053F3E94C9b9A09f33669435E7Ef1BeAed", 196).unwrap();
        assert_eq!(canonical, "0x5aAeb6053F3E94C9b9A09f33669435E7Ef1BeAed");
        assert_eq!(display, "XKO5aAeb6053F3E94C9b9A09f33669435E7Ef1BeAed");
    }

    #[test]
    fn parse_recipient_xko_lowercase_hex_body() {
        // All-lowercase body claims no checksum — accepted.
        let (canonical, display) =
            parse_recipient_addr("XKO5aaeb6053f3e94c9b9a09f33669435e7ef1beaed", 196).unwrap();
        assert_eq!(canonical, "0x5aaeb6053f3e94c9b9a09f33669435e7ef1beaed");
        assert_eq!(display, "XKO5aaeb6053f3e94c9b9a09f33669435e7ef1beaed");
    }

    #[test]
    fn parse_recipient_xko_on_non_xlayer_rejected() {
        let err =
            parse_recipient_addr("XKO5aAeb6053F3E94C9b9A09f33669435E7Ef1BeAed", 1).unwrap_err();
        assert!(
            err.to_string().contains("only supported on X Layer"),
            "unexpected error: {err}"
        );
    }

    #[test]
    fn parse_recipient_xko_lowercase_prefix_rejected() {
        // Strict uppercase: only the literal `XKO` prefix is recognized.
        let err =
            parse_recipient_addr("xko5aAeb6053F3E94C9b9A09f33669435E7Ef1BeAed", 196).unwrap_err();
        assert!(
            err.to_string().contains("not a valid EVM address"),
            "unexpected error: {err}"
        );
    }

    #[test]
    fn parse_recipient_xko_bad_checksum_rejected() {
        // Mixed case with one letter case flipped — fails EIP-55.
        let err =
            parse_recipient_addr("XKO5AAeb6053F3E94C9b9A09f33669435E7Ef1BeAed", 196).unwrap_err();
        assert!(
            err.to_string().contains("EIP-55"),
            "unexpected error: {err}"
        );
    }

    #[test]
    fn parse_recipient_xko_wrong_length_rejected() {
        let err = parse_recipient_addr("XKO123", 196).unwrap_err();
        assert!(
            err.to_string().contains("40 hex chars"),
            "unexpected error: {err}"
        );
    }

    #[test]
    fn parse_recipient_xko_non_hex_body_rejected() {
        let err =
            parse_recipient_addr("XKOZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZ", 196).unwrap_err();
        assert!(
            err.to_string().contains("40 hex chars"),
            "unexpected error: {err}"
        );
    }

    #[test]
    fn parse_recipient_garbage_rejected() {
        let err = parse_recipient_addr("not-an-address", 196).unwrap_err();
        assert!(err.to_string().contains("not a valid EVM address"));
    }
}
