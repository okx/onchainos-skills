//! Exact arithmetic (add / sub / compare) over non-negative decimal strings.
//!
//! Purpose: the staking preflight needs to add/subtract/compare `activeStake`
//! / `amount` / `minCumulativeStake` / `partialUnstakeMinRetainOkb`, all of
//! which are UI strings (in OKB units) coming from backend / config. Plain
//! `f64` subtraction produces artifacts like
//! `0.0012 - 0.0002 = 0.0009999999999999998`, which misclassifies an "exactly
//! meets threshold" case as "just short".
//!
//! Approach: align both operands to the larger fractional precision of the
//! two, drop the decimal point, and run the operation as `u128` integer math
//! — no dependency on token decimals; exact for inputs of any precision.

use anyhow::{anyhow, bail, Result};
use std::cmp::Ordering;

/// Split a non-negative decimal string into (integer part, fractional part);
/// validate that only digits and at most one decimal point are present.
fn split(s: &str) -> Result<(&str, &str)> {
    let s = s.trim();
    if s.is_empty() {
        bail!("decimal string is empty");
    }
    let (int_part, frac_part) = match s.split_once('.') {
        Some((i, f)) => (i, f),
        None => (s, ""),
    };
    let int_part = if int_part.is_empty() { "0" } else { int_part };
    if !int_part.chars().all(|c| c.is_ascii_digit()) {
        bail!("invalid decimal (non-digit in integer part): \"{s}\"");
    }
    if !frac_part.chars().all(|c| c.is_ascii_digit()) {
        bail!("invalid decimal (non-digit in fractional part): \"{s}\"");
    }
    Ok((int_part, frac_part))
}

/// Align two decimal strings to a common precision; returns
/// (integer representation of a, integer representation of b, common precision).
fn align(a: &str, b: &str) -> Result<(u128, u128, usize)> {
    let (ai, af) = split(a)?;
    let (bi, bf) = split(b)?;
    let prec = af.len().max(bf.len());
    let to_u128 = |i: &str, f: &str, original: &str| -> Result<u128> {
        // Pad the fractional part on the right with 0s to `prec` digits
        // (the `width` specifier is a minimum length, not a truncation).
        let combined = format!("{i}{f:0<prec$}");
        let stripped = combined.trim_start_matches('0');
        let normalized = if stripped.is_empty() { "0" } else { stripped };
        normalized
            .parse::<u128>()
            .map_err(|e| anyhow!("decimal exceeds u128 range: \"{original}\": {e}"))
    };
    Ok((to_u128(ai, af, a)?, to_u128(bi, bf, b)?, prec))
}

/// Render a `u128` at the given precision back into a canonical decimal string:
/// - trailing zeros in the fractional part are stripped
/// - whole numbers (all-zero fractional part) carry no decimal point
fn format_at(value: u128, prec: usize) -> String {
    if prec == 0 {
        return value.to_string();
    }
    let scale = 10u128.pow(prec as u32);
    let int_part = value / scale;
    let frac_part = value % scale;
    if frac_part == 0 {
        return int_part.to_string();
    }
    let frac_str = format!("{frac_part:0>prec$}");
    let trimmed = frac_str.trim_end_matches('0');
    format!("{int_part}.{trimmed}")
}

pub fn cmp(a: &str, b: &str) -> Result<Ordering> {
    let (av, bv, _) = align(a, b)?;
    Ok(av.cmp(&bv))
}

/// `a - b`; requires `a >= b`, otherwise returns an underflow error.
pub fn sub(a: &str, b: &str) -> Result<String> {
    let (av, bv, prec) = align(a, b)?;
    let diff = av
        .checked_sub(bv)
        .ok_or_else(|| anyhow!("decimal subtraction underflow: \"{a}\" - \"{b}\""))?;
    Ok(format_at(diff, prec))
}

pub fn add(a: &str, b: &str) -> Result<String> {
    let (av, bv, prec) = align(a, b)?;
    let sum = av
        .checked_add(bv)
        .ok_or_else(|| anyhow!("decimal addition overflow: \"{a}\" + \"{b}\""))?;
    Ok(format_at(sum, prec))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sub_no_fp_artifact() {
        // Same case, f64 vs string-decimal:
        //   - f64    : 0.0012 - 0.0002 = 0.0009999999999999998  (!= 0.001)
        //   - string :                  = "0.001"                (== 0.001)
        let fp_result = 0.0012_f64 - 0.0002_f64;
        assert_ne!(fp_result, 0.001_f64);
        assert_eq!(format!("{fp_result}"), "0.0009999999999999998");

        assert_eq!(sub("0.0012", "0.0002").unwrap(), "0.001");
    }

    #[test]
    fn cmp_handles_uneven_precision() {
        // remaining=0.001 vs retain=0.001 must compare Equal — the alignment
        // logic must not skew the result to < or >.
        assert_eq!(cmp("0.001", "0.0010").unwrap(), Ordering::Equal);
        assert_eq!(cmp("0.0012", "0.001").unwrap(), Ordering::Greater);
        assert_eq!(cmp("0.0009", "0.001").unwrap(), Ordering::Less);
    }

    #[test]
    fn add_simple() {
        assert_eq!(add("0.0012", "0.0008").unwrap(), "0.002");
        assert_eq!(add("1", "0.5").unwrap(), "1.5");
        assert_eq!(add("0", "0").unwrap(), "0");
    }

    #[test]
    fn integer_only_inputs() {
        assert_eq!(sub("100", "30").unwrap(), "70");
        assert_eq!(add("100", "30").unwrap(), "130");
        assert_eq!(cmp("100", "30").unwrap(), Ordering::Greater);
    }

    #[test]
    fn mixed_precision_alignment() {
        // 10.5 (1 frac digit) vs 0.0001 (4 frac digits) → common precision 4
        assert_eq!(sub("10.5", "0.0001").unwrap(), "10.4999");
        assert_eq!(add("10.5", "0.0001").unwrap(), "10.5001");
        assert_eq!(cmp("10.5", "0.0001").unwrap(), Ordering::Greater);
    }

    #[test]
    fn underflow_errors() {
        assert!(sub("0.001", "0.002").is_err());
    }

    #[test]
    fn invalid_inputs_error() {
        assert!(cmp("", "0").is_err());
        assert!(cmp("abc", "0").is_err());
        assert!(cmp("1.2.3", "0").is_err());
        assert!(cmp("-1", "0").is_err());
    }

    #[test]
    fn trailing_zeros_trimmed_in_output() {
        assert_eq!(sub("0.10", "0.05").unwrap(), "0.05");
        assert_eq!(add("0.5", "0.5").unwrap(), "1");
    }
}