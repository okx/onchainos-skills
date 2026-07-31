//! FR-2.5 option parser (positional V1.1 grammar). 7 fields:
//!
//! `contractCode | <SIDE> <Call|Put> | strike(zh) <strike> | expiry(zh) <YYYY-MM-DD> | premium(zh) ≤N [CCY] | position(zh) N% | <ttl>`
//!
//! `contractCode` = `UNDERLYING-YYMMDD-STRIKE-C|P`. Its trailing date, strike, and
//! C/P MUST match the standalone `expiry`, `strike`, and `optionType` fields
//! (2-digit year is interpreted as 20YY).

use super::super::error::ParseError;
use super::super::fields;
use super::super::{Language, OptionParams, OptionType, SignalParams};
use super::ClassParse;

pub fn parse(fields: &[String], lang: Language) -> Result<ClassParse, ParseError> {
    if fields.len() != 7 {
        return Err(ParseError::FieldCountError);
    }
    let contract_code = fields[0].clone();
    let (side, option_type) = fields::parse_side_type(&fields[1], lang)?;
    let strike = fields::parse_decimal(&fields::strip_strike(&fields[2], lang)?, "strike")?;
    let expiry = fields::parse_date(&fields::strip_expiry(&fields[3], lang)?, "expiry")?;
    let premium_cap = fields::parse_premium_cap(&fields::strip_premium(&fields[4], lang)?)?;
    let position_pct = fields::parse_position_field(&fields[5], lang)?;
    let ttl_sec = fields::parse_ttl_field(&fields[6], lang)?;

    check_contract_consistency(&contract_code, option_type, &strike, &expiry)?;

    Ok((
        SignalParams::Option(OptionParams {
            contract_code,
            side,
            option_type,
            strike,
            expiry,
            premium_cap,
        }),
        position_pct,
        ttl_sec,
    ))
}

/// Verify `UNDERLYING-YYMMDD-STRIKE-(C|P)` matches the typed fields.
fn check_contract_consistency(
    code: &str,
    option_type: OptionType,
    strike: &str,
    expiry: &str,
) -> Result<(), ParseError> {
    let parts: Vec<&str> = code.split('-').collect();
    if parts.len() != 4 {
        return Err(ParseError::OptionFieldMismatch);
    }
    let (yymmdd, code_strike, cp) = (parts[1], parts[2], parts[3]);

    let cp_ok = matches!(
        (cp, option_type),
        ("C", OptionType::Call) | ("P", OptionType::Put)
    );
    if !cp_ok {
        return Err(ParseError::OptionFieldMismatch);
    }

    if !fields::equal(code_strike, strike) {
        return Err(ParseError::OptionFieldMismatch);
    }

    if yymmdd.len() != 6 || !yymmdd.bytes().all(|b| b.is_ascii_digit()) {
        return Err(ParseError::OptionFieldMismatch);
    }
    // The 2-digit contract-code year is interpreted in the 21st century (`20YY`,
    // i.e. 2000-2099) — intentional and sufficient for tradeable option expiries.
    // Widening to a full 4-digit code year is a spec change, not a bug fix.
    let code_date = format!("20{}-{}-{}", &yymmdd[0..2], &yymmdd[2..4], &yymmdd[4..6]);
    if code_date != expiry {
        return Err(ParseError::OptionFieldMismatch);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn f(parts: &[&str]) -> Vec<String> {
        parts.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn consistent_contract_parses() {
        let (params, _, _) = parse(
            &f(&[
                "BTC-260327-100000-C",
                "Buy Call",
                "Strike 100000",
                "Expiry 2026-03-27",
                "Premium \u{2264}320 USDT",
                "Position 3%",
                "valid for 24h",
            ]),
            Language::En,
        )
        .unwrap();
        match params {
            SignalParams::Option(o) => {
                assert_eq!(o.strike, "100000");
                assert_eq!(o.expiry, "2026-03-27");
                assert_eq!(o.premium_cap, "320");
            }
            _ => panic!("expected option"),
        }
    }

    #[test]
    fn inconsistent_option_type_rejected() {
        assert_eq!(
            parse(
                &f(&[
                    "BTC-260327-100000-C",
                    "Buy Put",
                    "Strike 100000",
                    "Expiry 2026-03-27",
                    "Premium \u{2264}320 USDT",
                    "Position 3%",
                    "valid for 24h",
                ]),
                Language::En,
            )
            .unwrap_err(),
            ParseError::OptionFieldMismatch
        );
    }
}
