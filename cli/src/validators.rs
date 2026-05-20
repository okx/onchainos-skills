//! Generic CLI input validators shared across command modules.
//!
//! All validators here are domain-agnostic — they care about the shape of
//! the string (number range, integer-ness, ID format, etc.) rather than
//! about a particular subcommand's business semantics. Domain-specific
//! checks (e.g. `validate_swap_mode`, `validate_receive_address` for
//! cross-chain family pairing) stay in their owning module.

use anyhow::{bail, Result};

// ── numeric validators ───────────────────────────────────────────────

/// Validate that `amount` is a non-empty string of digits (no Infinity, NaN,
/// negative, zero-only, leading-zeros, or other non-numeric values).
pub fn validate_amount(amount: &str) -> Result<()> {
    let amount = amount.trim();
    if amount.is_empty() {
        bail!("--amount must not be empty");
    }
    if amount.contains('.') {
        bail!("--amount must be a whole number in minimal units (no decimals)");
    }
    if !amount.chars().all(|c| c.is_ascii_digit()) {
        bail!(
            "--amount must be a whole number in minimal units, got \"{}\". \
             Infinity, NaN, negative numbers and non-numeric values are not accepted.",
            amount
        );
    }
    if amount.chars().all(|c| c == '0') {
        bail!("--amount must be greater than zero");
    }
    if amount.starts_with('0') {
        bail!("--amount must not have leading zeros, got \"{}\"", amount);
    }
    Ok(())
}

/// Validate that `slippage` is a number strictly greater than 0 and at most 100.
/// Accepts decimals like "0.5", "1", "99.9", "100". Rejects "0", negatives, >100, non-numeric.
pub fn validate_slippage(slippage: &str) -> Result<()> {
    let slippage = slippage.trim();
    let val: f64 = slippage.parse().map_err(|_| {
        anyhow::anyhow!(
            "--slippage must be a number between 0 (exclusive) and 100 (inclusive), got \"{}\"",
            slippage
        )
    })?;
    if val.is_nan() || val.is_infinite() {
        bail!(
            "--slippage must be a finite number between 0 (exclusive) and 100 (inclusive), got \"{}\"",
            slippage
        );
    }
    if val <= 0.0 || val > 100.0 {
        bail!(
            "--slippage must be greater than 0 and at most 100, got \"{}\"",
            slippage
        );
    }
    Ok(())
}

/// Validate non-negative integer string (≥ 0). Used for gasLimit, aaDexTokenAmount,
/// approve amounts (where 0 = revoke), etc.
pub fn validate_non_negative_integer(value: &str, label: &str) -> Result<()> {
    let value = value.trim();
    if value.is_empty() {
        bail!("--{} must not be empty", label);
    }
    if !value.chars().all(|c| c.is_ascii_digit()) {
        bail!(
            "--{} must be a non-negative integer, got \"{}\"",
            label,
            value
        );
    }
    // Allow "0", but reject leading zeros like "007"
    if value.len() > 1 && value.starts_with('0') {
        bail!("--{} must not have leading zeros, got \"{}\"", label, value);
    }
    Ok(())
}

/// Backend expects orderId as Long (≤ 32 digits to stay clearly within i64);
/// reject non-numeric strings early so BE doesn't have to.
pub fn validate_order_id_numeric(id: &str, label: &str) -> Result<()> {
    let trimmed = id.trim();
    if trimmed.is_empty() {
        bail!("--{label} must not be empty");
    }
    if !trimmed.chars().all(|c| c.is_ascii_digit()) {
        bail!("--{label} must be a numeric order id, got `{trimmed}`");
    }
    if trimmed.len() > 32 {
        bail!(
            "--{label} `{trimmed}` is too long ({} digits); order ids must be ≤ 32 digits",
            trimmed.len()
        );
    }
    Ok(())
}

// ── conversion helpers ───────────────────────────────────────────────

/// Convert a human-readable decimal string to minimal units (integer string).
/// Uses string arithmetic to avoid floating-point precision issues.
/// e.g. "0.1" with decimal=6 → "100000", "1.5" with decimal=18 → "1500000000000000000".
pub fn readable_to_minimal_str(amount: &str, decimal: u32) -> Result<String> {
    let (integer, frac) = if let Some(dot_pos) = amount.find('.') {
        (&amount[..dot_pos], &amount[dot_pos + 1..])
    } else {
        (amount, "")
    };
    if integer.is_empty() || !integer.chars().all(|c| c.is_ascii_digit()) {
        bail!(
            "--readable-amount must be a positive number, got \"{}\"",
            amount
        );
    }
    if !frac.chars().all(|c| c.is_ascii_digit()) {
        bail!(
            "--readable-amount must be a positive number, got \"{}\"",
            amount
        );
    }
    let precision = decimal as usize;
    let frac_padded = if frac.len() >= precision {
        if frac[precision..].chars().any(|c| c != '0') {
            bail!(
                "--readable-amount \"{}\" has more decimal places than this token supports ({} decimals)",
                amount, decimal
            );
        }
        frac[..precision].to_string()
    } else {
        format!("{:0<width$}", frac, width = precision)
    };
    let combined = format!("{}{}", integer, frac_padded);
    let stripped = combined.trim_start_matches('0');
    let result = if stripped.is_empty() { "0" } else { stripped };
    if result == "0" {
        bail!(
            "--readable-amount {} is too small for this token ({} decimals); results in zero minimal units",
            amount, decimal
        );
    }
    Ok(result.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── readable_to_minimal_str ───────────────────────────────────────

    #[test]
    fn test_readable_to_minimal_str() {
        // USDC: 6 decimals
        assert_eq!(readable_to_minimal_str("0.1", 6).unwrap(), "100000");
        assert_eq!(readable_to_minimal_str("1.5", 6).unwrap(), "1500000");
        assert_eq!(readable_to_minimal_str("100", 6).unwrap(), "100000000");
        assert_eq!(readable_to_minimal_str("1", 6).unwrap(), "1000000");
        assert_eq!(readable_to_minimal_str("0.000001", 6).unwrap(), "1");
        // ETH: 18 decimals
        assert_eq!(
            readable_to_minimal_str("0.1", 18).unwrap(),
            "100000000000000000"
        );
        assert_eq!(
            readable_to_minimal_str("1", 18).unwrap(),
            "1000000000000000000"
        );
        // SOL: 9 decimals
        assert_eq!(readable_to_minimal_str("1", 9).unwrap(), "1000000000");
        // Excess fractional digits with non-zero content → error
        assert!(readable_to_minimal_str("0.1234567", 6).is_err());
        assert!(readable_to_minimal_str("1.00000002", 2).is_err());
        // Excess fractional digits that are all zero → ok
        assert_eq!(readable_to_minimal_str("1.000", 2).unwrap(), "100");
        assert_eq!(readable_to_minimal_str("0.1230000", 6).unwrap(), "123000");
    }

    // ── slippage validation ────────────────────────────────────────

    #[test]
    fn test_validate_slippage_valid() {
        assert!(validate_slippage("0.5").is_ok());
        assert!(validate_slippage("1").is_ok());
        assert!(validate_slippage("50").is_ok());
        assert!(validate_slippage("99.9").is_ok());
        assert!(validate_slippage("100").is_ok()); // upper bound inclusive
        assert!(validate_slippage("100.0").is_ok());
        assert!(validate_slippage("0.001").is_ok());
        assert!(validate_slippage("0.01").is_ok());
        assert!(validate_slippage("  1  ").is_ok()); // trimmed
    }

    #[test]
    fn test_validate_slippage_boundary_reject() {
        // 0 is exclusive
        assert!(validate_slippage("0").is_err());
        assert!(validate_slippage("0.0").is_err());
        // >100 rejected
        assert!(validate_slippage("100.1").is_err());
    }

    #[test]
    fn test_validate_slippage_out_of_range() {
        assert!(validate_slippage("-1").is_err());
        assert!(validate_slippage("-0.5").is_err());
        assert!(validate_slippage("100.1").is_err());
        assert!(validate_slippage("200").is_err());
    }

    #[test]
    fn test_validate_slippage_non_numeric() {
        assert!(validate_slippage("abc").is_err());
        assert!(validate_slippage("").is_err());
        assert!(validate_slippage("   ").is_err());
        assert!(validate_slippage("NaN").is_err());
        assert!(validate_slippage("inf").is_err());
        assert!(validate_slippage("infinity").is_err());
        assert!(validate_slippage("-inf").is_err());
    }

    // ── amount validation (positive integer) ─────────────────

    #[test]
    fn test_validate_amount_valid() {
        assert!(validate_amount("1").is_ok());
        assert!(validate_amount("1000000").is_ok());
        assert!(validate_amount("999999999999999999").is_ok());
    }

    #[test]
    fn test_validate_amount_reject_decimal() {
        assert!(validate_amount("1.5").is_err());
        assert!(validate_amount("0.1").is_err());
        assert!(validate_amount("100.0").is_err());
    }

    #[test]
    fn test_validate_amount_reject_zero() {
        assert!(validate_amount("0").is_err());
        assert!(validate_amount("000").is_err());
    }

    #[test]
    fn test_validate_amount_reject_negative_and_non_numeric() {
        assert!(validate_amount("-1").is_err());
        assert!(validate_amount("-100").is_err());
        assert!(validate_amount("abc").is_err());
        assert!(validate_amount("12abc").is_err());
        assert!(validate_amount("").is_err());
        assert!(validate_amount("  ").is_err());
    }

    #[test]
    fn test_validate_amount_reject_leading_zeros() {
        assert!(validate_amount("007").is_err());
        assert!(validate_amount("01").is_err());
    }

    // ── non-negative integer validation ───────────────────────────────

    #[test]
    fn test_validate_non_negative_integer_valid() {
        assert!(validate_non_negative_integer("0", "gas-limit").is_ok());
        assert!(validate_non_negative_integer("1", "gas-limit").is_ok());
        assert!(validate_non_negative_integer("21000", "gas-limit").is_ok());
        assert!(validate_non_negative_integer("999999999", "aa-dex-token-amount").is_ok());
    }

    #[test]
    fn test_validate_non_negative_integer_rejects_non_numeric() {
        assert!(validate_non_negative_integer("abc", "gas-limit").is_err());
        assert!(validate_non_negative_integer("-1", "gas-limit").is_err());
        assert!(validate_non_negative_integer("1.5", "gas-limit").is_err());
        assert!(validate_non_negative_integer("", "gas-limit").is_err());
        assert!(validate_non_negative_integer("  ", "gas-limit").is_err());
    }

    #[test]
    fn test_validate_non_negative_integer_rejects_leading_zeros() {
        assert!(validate_non_negative_integer("007", "gas-limit").is_err());
        assert!(validate_non_negative_integer("00", "gas-limit").is_err());
        assert!(validate_non_negative_integer("01", "aa-dex-token-amount").is_err());
    }

    #[test]
    fn test_validate_non_negative_integer_allows_zero() {
        assert!(validate_non_negative_integer("0", "gas-limit").is_ok());
    }

    #[test]
    fn test_validate_non_negative_integer_error_contains_label() {
        let err = validate_non_negative_integer("abc", "gas-limit").unwrap_err();
        assert!(err.to_string().contains("--gas-limit"));
        let err2 = validate_non_negative_integer("-1", "aa-dex-token-amount").unwrap_err();
        assert!(err2.to_string().contains("--aa-dex-token-amount"));
    }

    // ── order id validation ───────────────────────────────────────────

    #[test]
    fn test_validate_order_id_numeric_valid() {
        assert!(validate_order_id_numeric("17296046425729984", "order-id").is_ok());
        assert!(validate_order_id_numeric("1", "order-id").is_ok());
    }

    #[test]
    fn test_validate_order_id_numeric_rejects_non_numeric() {
        assert!(validate_order_id_numeric("abc-123", "order-id").is_err());
        assert!(validate_order_id_numeric("", "order-id").is_err());
        assert!(validate_order_id_numeric("  ", "order-id").is_err());
        assert!(validate_order_id_numeric("12a", "order-id").is_err());
    }

    #[test]
    fn test_validate_order_id_numeric_rejects_too_long() {
        let too_long = "1".repeat(33);
        assert!(validate_order_id_numeric(&too_long, "order-id").is_err());
        let max_len = "1".repeat(32);
        assert!(validate_order_id_numeric(&max_len, "order-id").is_ok());
    }

    #[test]
    fn test_validate_order_id_numeric_error_contains_label() {
        let err = validate_order_id_numeric("abc", "order-ids").unwrap_err();
        assert!(err.to_string().contains("--order-ids"));
    }
}
