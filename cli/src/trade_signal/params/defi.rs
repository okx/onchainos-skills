//! FR-2.6 DeFi parser (positional V1.1 grammar). 8 fields:
//!
//! `chain | <protocolPool> | APY <pct> | TVL <compact> | <token> | <redeemTerms> | position(zh) N% | <ttl>`
//!
//! `apy` is a non-negative percent; `tvl` is a canonical compact amount captured
//! verbatim (NO float conversion); `chain` / `protocolPool` / `token` /
//! `redeemTerms` are unresolved strings (D-1). `executionSemantics` is fixed to
//! `deposit`.

use super::super::error::ParseError;
use super::super::fields;
use super::super::{DefiParams, ExecutionSemantics, Language, SignalParams};
use super::ClassParse;

pub fn parse(fields: &[String], lang: Language) -> Result<ClassParse, ParseError> {
    if fields.len() != 8 {
        return Err(ParseError::FieldCountError);
    }
    let chain = fields[0].clone();
    let protocol_pool = fields[1].clone();
    let apy = fields::parse_percent_nonneg(&fields::strip_apy(&fields[2], lang)?, "apy")?;
    // TVL keyword is stripped, then validated as a canonical compact amount
    // (no float) — `$2.4M` / `500M` / `1.2B` / a bare int.
    let tvl = fields::parse_compact_amount(&fields::strip_tvl(&fields[3], lang)?, "tvl")?;
    let token = fields[4].clone();
    let redeem_terms = fields[5].clone();
    let position_pct = fields::parse_position_field(&fields[6], lang)?;
    let ttl_sec = fields::parse_ttl_field(&fields[7], lang)?;

    Ok((
        SignalParams::Defi(DefiParams {
            chain,
            protocol_pool,
            apy,
            tvl,
            token,
            redeem_terms,
            execution_semantics: ExecutionSemantics::Deposit,
        }),
        position_pct,
        ttl_sec,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn f(parts: &[&str]) -> Vec<String> {
        parts.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn parses_full_defi_row() {
        let (params, pos, ttl) = parse(
            &f(&[
                "X Layer",
                "ProtocolX USDT-USDG LP",
                "APY 18.6%",
                "TVL $2.4M",
                "USDT",
                "withdraw anytime",
                "Position 5%",
                "valid for 48h",
            ]),
            Language::En,
        )
        .unwrap();
        assert_eq!(pos, "5");
        assert_eq!(ttl, 172_800);
        match params {
            SignalParams::Defi(d) => {
                assert_eq!(d.apy, "18.6");
                assert_eq!(d.tvl, "$2.4M");
                assert_eq!(d.token, "USDT");
            }
            _ => panic!("expected defi"),
        }
    }

    #[test]
    fn missing_field_is_field_count_error() {
        assert_eq!(
            parse(
                &f(&[
                    "X Layer",
                    "ProtocolX USDT-USDG LP",
                    "TVL $2.4M",
                    "USDT",
                    "withdraw anytime",
                    "Position 5%",
                    "valid for 48h",
                ]),
                Language::En,
            )
            .unwrap_err(),
            ParseError::FieldCountError
        );
    }

    /// A non-canonical TVL (free text, not a compact
    /// amount) is now rejected as `invalid_number` attributed to `tvl` — the old
    /// non-empty `require` accepted any text.
    #[test]
    fn non_canonical_tvl_is_rejected() {
        let mk = |tvl: &str| {
            f(&[
                "X Layer",
                "ProtocolX USDT-USDG LP",
                "APY 18.6%",
                tvl,
                "USDT",
                "withdraw anytime",
                "Position 5%",
                "valid for 48h",
            ])
        };
        for tvl in ["TVL lots", "TVL 2.4X", "TVL 1,000M"] {
            assert_eq!(
                parse(&mk(tvl), Language::En).unwrap_err(),
                ParseError::InvalidNumber("tvl"),
                "expected tvl invalid_number for {tvl:?}"
            );
        }
        // control: a legal compact amount without `$` still parses.
        let ok = parse(&mk("TVL 500M"), Language::En).unwrap();
        match ok.0 {
            SignalParams::Defi(d) => assert_eq!(d.tvl, "500M"),
            _ => panic!("expected defi"),
        }
    }
}
