//! FR-2.4 prediction parser (positional V1.1 grammar). 5 fields:
//!
//! `"<event>" | <OUTCOME> @<odds> | position(zh) N% | settle(zh) <YYYY-MM-DD> | <ttl>`
//!
//! `position` sits BEFORE the settle date (class-specific ordering). `outcome` ∈
//! {YES,NO,UP,DOWN}, `odds` an absolute decimal in [0,1] after exactly one `@`.
//! `event` is free text (optionally wrapped in ASCII quotes) and is NEVER echoed
//! in an error (SR-3) — it is only captured on success.

use super::super::error::ParseError;
use super::super::fields;
use super::super::{Language, PredictionParams, SignalParams};
use super::ClassParse;

pub fn parse(fields: &[String], lang: Language) -> Result<ClassParse, ParseError> {
    if fields.len() != 5 {
        return Err(ParseError::FieldCountError);
    }
    let event = strip_quotes(&fields[0]);
    let (outcome, odds) = fields::parse_outcome_odds(&fields[1])?;
    let position_pct = fields::parse_position_field(&fields[2], lang)?;
    let settle_date = fields::parse_date(&fields::strip_settle(&fields[3], lang)?, "settleDate")?;
    let ttl_sec = fields::parse_ttl_field(&fields[4], lang)?;

    Ok((
        SignalParams::Prediction(PredictionParams {
            event,
            outcome,
            odds,
            settle_date,
        }),
        position_pct,
        ttl_sec,
    ))
}

/// Strip a matching pair of surrounding ASCII double quotes, if present.
fn strip_quotes(s: &str) -> String {
    let t = s.trim();
    if t.len() >= 2 && t.starts_with('"') && t.ends_with('"') {
        t[1..t.len() - 1].to_string()
    } else {
        t.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::super::super::Outcome;
    use super::*;

    fn f(parts: &[&str]) -> Vec<String> {
        parts.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn parses_event_outcome_odds_and_settle() {
        let (params, pos, ttl) = parse(
            &f(&[
                "\"Fed cuts rates in Sept?\"",
                "YES @0.62",
                "Position 5%",
                "Settle 2026-09-18",
                "valid for 24h",
            ]),
            Language::En,
        )
        .unwrap();
        assert_eq!(pos, "5");
        assert_eq!(ttl, 86_400);
        match params {
            SignalParams::Prediction(p) => {
                assert_eq!(p.event, "Fed cuts rates in Sept?");
                assert_eq!(p.outcome, Outcome::Yes);
                assert_eq!(p.odds, "0.62");
                assert_eq!(p.settle_date, "2026-09-18");
            }
            _ => panic!("expected prediction"),
        }
    }

    #[test]
    fn odds_out_of_range_rejected() {
        assert_eq!(
            parse(
                &f(&[
                    "x",
                    "YES @1.5",
                    "Position 5%",
                    "Settle 2026-09-18",
                    "valid for 1d",
                ]),
                Language::En,
            )
            .unwrap_err(),
            ParseError::OutOfRange("odds")
        );
    }
}
