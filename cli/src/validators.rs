//! Generic CLI input validators (numeric / id format).
//! Domain-specific checks (e.g. swap-mode, cross-chain family pairing)
//! stay in their owning module.

use anyhow::{bail, Result};

// ── numeric validators ───────────────────────────────────────────────

/// Validate raw integer amount. Rejects empty, decimals, non-digits,
/// zero, and leading zeros. Example: `"500000"` ✓, `"0"` / `"1.5"` / `"007"` ✗.
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

/// Percent slippage in (0, 100]. Accepts trailing `%`.
/// Examples: `"1"` = 1%, `"20%"` = 20%, `"100"` = 100%. Used by swap / strategy.
pub fn validate_slippage(slippage: &str) -> Result<()> {
    let slippage = slippage.trim().trim_end_matches('%').trim();
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

/// Decimal slippage in (0, 1]. Rejects `%` suffix (use `validate_slippage` for percent mode).
/// Examples: `"0.01"` = 1%, `"0.005"` = 0.5%, `"0.5"` = 50%, `"1"` = 100%. Used by cross-chain.
pub fn validate_slippage_zero_to_one(slippage: &str) -> Result<()> {
    let slippage = slippage.trim();
    // `%` belongs to percent-mode; reject early with a translation hint so
    // `--slippage 0.5%` doesn't get silently stripped into 50%.
    if slippage.ends_with('%') {
        bail!(
            "--slippage is decimal here (e.g. 0.01 for 1%, 0.005 for 0.5%); \
             the '%' suffix only applies to swap/strategy (percent mode). \
             Drop the '%' and divide by 100, got \"{slippage}\""
        );
    }
    let val: f64 = slippage.parse().map_err(|_| {
        anyhow::anyhow!(
            "--slippage must be a decimal number between 0 (exclusive) and 1 (inclusive), got \"{}\"",
            slippage
        )
    })?;
    if val.is_nan() || val.is_infinite() {
        bail!(
            "--slippage must be a finite decimal number between 0 (exclusive) and 1 (inclusive), got \"{}\"",
            slippage
        );
    }
    if val <= 0.0 || val > 1.0 {
        bail!(
            "--slippage must be greater than 0 and at most 1 (decimal form, e.g. 0.01 = 1%), got \"{}\"",
            slippage
        );
    }
    Ok(())
}

/// Non-negative integer string (≥ 0, no leading zeros). Allows `"0"`.
/// Used for gas-limit / aa-dex-token-amount / approve amount (where `"0"` = revoke).
/// Examples: `"0"` ✓, `"21000"` ✓, `"007"` / `"-1"` / `"1.5"` ✗.
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
    if value.len() > 1 && value.starts_with('0') {
        bail!("--{} must not have leading zeros, got \"{}\"", label, value);
    }
    Ok(())
}

/// Numeric order id that fits in BE Java Long (`i64`, max 19 digits).
/// Examples: `"17296046425729984"` ✓, `"abc-1"` / 20-digit overflow ✗.
pub fn validate_order_id_numeric(id: &str, label: &str) -> Result<()> {
    let trimmed = id.trim();
    if trimmed.is_empty() {
        bail!("--{label} must not be empty");
    }
    if !trimmed.chars().all(|c| c.is_ascii_digit()) {
        bail!("--{label} must be a numeric order id, got `{trimmed}`");
    }
    if trimmed.parse::<i64>().is_err() {
        bail!(
            "--{label} `{trimmed}` does not fit in BE Long range (max {})",
            i64::MAX
        );
    }
    Ok(())
}

// ── conversion helpers ───────────────────────────────────────────────

/// Human decimal → raw integer string. Uses string arithmetic (no f64).
/// Accepts leading `.` (`".5"` == `"0.5"`) and trims whitespace. Rejects results of 0.
/// Examples: `("0.1", 6)` → `"100000"`, `("1.5", 18)` → `"1500000000000000000"`.
pub fn readable_to_minimal_str(amount: &str, decimal: u32) -> Result<String> {
    let amount = amount.trim();
    let (integer, frac) = if let Some(dot_pos) = amount.find('.') {
        (&amount[..dot_pos], &amount[dot_pos + 1..])
    } else {
        (amount, "")
    };
    let integer = if integer.is_empty() { "0" } else { integer };
    if !integer.chars().all(|c| c.is_ascii_digit()) {
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

    #[test]
    fn test_readable_to_minimal_str_too_small_rejects() {
        // Per the function contract, sub-precision amounts that round to zero
        // minimal units must bail rather than silently produce "0".
        assert!(readable_to_minimal_str("0.0000001", 6).is_err());
        assert!(readable_to_minimal_str("0.0", 18).is_err());
        assert!(readable_to_minimal_str("0", 6).is_err());
    }

    #[test]
    fn test_readable_to_minimal_str_accepts_leading_dot_and_whitespace() {
        // Match trader_mode::human_decimal_to_raw_integer's contract: ".5" == "0.5", and
        // surrounding whitespace is trimmed.
        assert_eq!(readable_to_minimal_str(".5", 6).unwrap(), "500000");
        assert_eq!(readable_to_minimal_str("  1.5  ", 6).unwrap(), "1500000");
        assert_eq!(readable_to_minimal_str(" .000001 ", 6).unwrap(), "1");
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
    fn test_validate_slippage_accepts_percent_suffix() {
        // Caller docs (handlers.rs CreateLimitArgs::slippage) promise both
        // "20" and "20%" work; mirror that here.
        assert!(validate_slippage("20%").is_ok());
        assert!(validate_slippage("0.5%").is_ok());
        assert!(validate_slippage(" 100% ").is_ok());
        assert!(validate_slippage("0%").is_err()); // still rejects 0
        assert!(validate_slippage("101%").is_err()); // still range-checks
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

    // ── slippage decimal validation ──────────────────────────

    #[test]
    fn test_validate_slippage_zero_to_one_valid() {
        assert!(validate_slippage_zero_to_one("0.01").is_ok()); // 1%
        assert!(validate_slippage_zero_to_one("0.5").is_ok()); // 50%
        assert!(validate_slippage_zero_to_one("1").is_ok()); // 100% upper bound inclusive
        assert!(validate_slippage_zero_to_one("1.0").is_ok());
        assert!(validate_slippage_zero_to_one("0.002").is_ok()); // BE lower bound
        assert!(validate_slippage_zero_to_one("0.5").is_ok()); // BE upper bound
        assert!(validate_slippage_zero_to_one("  0.05  ").is_ok()); // trimmed
    }

    #[test]
    fn test_validate_slippage_zero_to_one_rejects_out_of_range() {
        assert!(validate_slippage_zero_to_one("0").is_err()); // 0 exclusive
        assert!(validate_slippage_zero_to_one("0.0").is_err());
        assert!(validate_slippage_zero_to_one("-0.01").is_err());
        assert!(validate_slippage_zero_to_one("1.01").is_err()); // > 1
        assert!(validate_slippage_zero_to_one("50").is_err()); // percent value, not decimal
        assert!(validate_slippage_zero_to_one("100").is_err());
    }

    #[test]
    fn test_validate_slippage_zero_to_one_rejects_non_numeric() {
        assert!(validate_slippage_zero_to_one("abc").is_err());
        assert!(validate_slippage_zero_to_one("").is_err());
        assert!(validate_slippage_zero_to_one("NaN").is_err());
        assert!(validate_slippage_zero_to_one("inf").is_err());
    }

    #[test]
    fn test_validate_slippage_zero_to_one_rejects_percent_suffix() {
        // The `%` suffix only makes sense in percent mode (validate_slippage);
        // here it almost always indicates the user mixed up the two modes.
        let err = validate_slippage_zero_to_one("0.5%").unwrap_err();
        let msg = format!("{err:#}");
        assert!(msg.contains("decimal here"), "want translation hint, got: {msg}");
        assert!(msg.contains("divide by 100"), "want translation hint, got: {msg}");
        assert!(validate_slippage_zero_to_one("50%").is_err());
        assert!(validate_slippage_zero_to_one("0.01%").is_err());
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
    fn test_validate_order_id_numeric_rejects_i64_overflow() {
        // i64::MAX = 9_223_372_036_854_775_807 (19 digits)
        assert!(validate_order_id_numeric("9223372036854775807", "order-id").is_ok());
        // One past the max → fails parse::<i64>()
        assert!(validate_order_id_numeric("9223372036854775808", "order-id").is_err());
        // 20 digits → overflows
        let twenty = "1".repeat(20);
        assert!(validate_order_id_numeric(&twenty, "order-id").is_err());
        // Real-world BE ids fit comfortably (17-18 digits)
        assert!(validate_order_id_numeric("17296046425729984", "order-id").is_ok());
    }

    #[test]
    fn test_validate_order_id_numeric_error_contains_label() {
        let err = validate_order_id_numeric("abc", "order-ids").unwrap_err();
        assert!(err.to_string().contains("--order-ids"));
    }
}
