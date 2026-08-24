use anyhow::{bail, Context, Result};
use num_bigint::BigUint;
use serde_json::Value;

/// Parses a minimal-unit integer, using `field` in errors and optionally allowing zero.
pub fn parse_minimal(value: &str, field: &str, allow_zero: bool) -> Result<BigUint> {
    let value = value.trim();
    if value.is_empty() || !value.bytes().all(|byte| byte.is_ascii_digit()) {
        bail!("{field} must be a non-negative integer in minimal units");
    }
    if value.len() > 1 && value.starts_with('0') {
        bail!("{field} must not contain leading zeros");
    }
    let parsed = value
        .parse::<BigUint>()
        .with_context(|| format!("{field} is outside the supported integer range"))?;
    if !allow_zero && parsed == BigUint::from(0u8) {
        bail!("{field} must be greater than zero");
    }
    Ok(parsed)
}

/// Converts a user-readable amount with `decimals` precision into minimal units.
pub fn readable_to_minimal(value: &str, decimals: u32) -> Result<String> {
    validate_decimals(decimals)?;
    let minimal = crate::validators::readable_to_minimal_str(value, decimals)?;
    parse_minimal(&minimal, "readable-amount", false)?;
    Ok(minimal)
}

/// Converts a minimal-unit integer into a trimmed user-readable decimal string.
pub fn minimal_to_readable(value: &str, decimals: u32) -> Result<String> {
    validate_decimals(decimals)?;
    parse_minimal(value, "amount", true)?;
    if decimals == 0 {
        return Ok(value.to_string());
    }
    let decimals = usize::try_from(decimals).context("decimal is too large")?;
    let padded = if value.len() <= decimals {
        format!("{}{}", "0".repeat(decimals + 1 - value.len()), value)
    } else {
        value.to_string()
    };
    let split = padded.len() - decimals;
    let integer = &padded[..split];
    let fraction = padded[split..].trim_end_matches('0');
    if fraction.is_empty() {
        Ok(integer.to_string())
    } else {
        Ok(format!("{integer}.{fraction}"))
    }
}

/// Rejects decimal precision outside the amount conversion limit.
fn validate_decimals(decimals: u32) -> Result<()> {
    if decimals > 255 {
        bail!("asset decimal exceeds the supported limit");
    }
    Ok(())
}

/// Returns a JSON string or unsigned integer value as its decimal text form.
pub fn value_as_decimal_string(value: &Value) -> Option<String> {
    value
        .as_str()
        .map(str::to_string)
        .or_else(|| value.as_u64().map(|number| number.to_string()))
}

/// Reads `decimal` or `decimals` metadata and returns the parsed precision.
pub fn decimal_field(value: &Value) -> Option<u32> {
    ["decimal", "decimals"]
        .iter()
        .find_map(|key| value.get(key))
        .and_then(value_as_decimal_string)
        .and_then(|value| value.parse().ok())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn conversion_is_exact_for_btc_brc20_and_sui() {
        assert_eq!(readable_to_minimal("1.00000001", 8).unwrap(), "100000001");
        assert_eq!(readable_to_minimal("1", 18).unwrap(), "1000000000000000000");
        assert_eq!(readable_to_minimal("1.000000001", 9).unwrap(), "1000000001");
    }
}
