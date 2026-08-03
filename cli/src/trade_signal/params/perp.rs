//! FR-2.3 perp parser (positional V1.1 grammar). 7 fields:
//!
//! `pair | <DIR> <LEV>x [marginMode] | entry(zh) lo-hi | SL <sl> | TP1 v1 [/ TP2 v2 [/ TP3 v3]] | position(zh) N% | <ttl>`
//!
//! `leverage` is a positive integer (the `x` suffix is stripped); exactly one
//! `stopLoss`; 1..=3 tagged take-profits with contiguous numbering. Direction
//! rules (the ONLY ordering constraint the protocol defines):
//! - LONG:  stopLoss < entryLo; every TP > entryLo.
//! - SHORT: stopLoss > entryHi; every TP < entryHi.

use super::super::error::ParseError;
use super::super::fields;
use super::super::{Direction, Language, PerpParams, SignalParams};
use super::ClassParse;

pub fn parse(fields: &[String], lang: Language) -> Result<ClassParse, ParseError> {
    if fields.len() != 7 {
        return Err(ParseError::FieldCountError);
    }
    let pair = fields[0].clone();
    let (direction, leverage, margin_mode) = fields::parse_dir_lev_margin(&fields[1], lang)?;
    let entry_range = fields::parse_range(&fields::strip_entry(&fields[2], lang)?, "entry")?;
    let stop_loss = fields::parse_decimal(&fields::strip_stop_loss(&fields[3], lang)?, "stopLoss")?;
    let take_profit = fields::parse_take_profits(&fields[4])?;
    let position_pct = fields::parse_position_field(&fields[5], lang)?;
    let ttl_sec = fields::parse_ttl_field(&fields[6], lang)?;

    check_direction(
        direction,
        &entry_range.lo,
        &entry_range.hi,
        &stop_loss,
        &take_profit,
    )?;

    Ok((
        SignalParams::Perp(PerpParams {
            pair,
            direction,
            leverage,
            entry_range,
            stop_loss,
            take_profit,
            margin_mode,
        }),
        position_pct,
        ttl_sec,
    ))
}

/// SL/TP direction integrity — the correct side of the entry range. No monotonic
/// ordering constraint.
fn check_direction(
    direction: Direction,
    entry_lo: &str,
    entry_hi: &str,
    stop_loss: &str,
    tps: &[String],
) -> Result<(), ParseError> {
    match direction {
        Direction::Long => {
            if !fields::less_than(stop_loss, entry_lo) {
                return Err(ParseError::DirectionConstraint("stopLoss"));
            }
            if tps.iter().any(|tp| !fields::greater_than(tp, entry_lo)) {
                return Err(ParseError::DirectionConstraint("takeProfit"));
            }
        }
        Direction::Short => {
            if !fields::greater_than(stop_loss, entry_hi) {
                return Err(ParseError::DirectionConstraint("stopLoss"));
            }
            if tps.iter().any(|tp| !fields::less_than(tp, entry_hi)) {
                return Err(ParseError::DirectionConstraint("takeProfit"));
            }
        }
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
    fn long_single_tp_no_margin_parses() {
        let (params, pos, ttl) = parse(
            &f(&[
                "ETH-PERP",
                "LONG 3x",
                "Entry 3420-3450",
                "SL 3300",
                "TP1 3720",
                "Position 10%",
                "valid for 4h",
            ]),
            Language::En,
        )
        .unwrap();
        assert_eq!(pos, "10");
        assert_eq!(ttl, 14_400);
        match params {
            SignalParams::Perp(p) => {
                assert_eq!(p.leverage, 3);
                assert_eq!(p.take_profit, vec!["3720"]);
                assert!(p.margin_mode.is_none());
            }
            _ => panic!("expected perp"),
        }
    }

    #[test]
    fn short_isolated_slash_tps_parses() {
        let (params, _, _) = parse(
            &f(&[
                "BTC-USDT-SWAP",
                "SHORT 2x isolated",
                "Entry 97800-98200",
                "SL 99500",
                "TP1 96000 / TP2 94000",
                "Position 8%",
                "valid for 8h",
            ]),
            Language::En,
        )
        .unwrap();
        match params {
            SignalParams::Perp(p) => {
                assert_eq!(p.take_profit, vec!["96000", "94000"]);
                assert!(p.margin_mode.is_some());
            }
            _ => panic!("expected perp"),
        }
    }

    #[test]
    fn wrong_side_stop_loss_rejected() {
        // LONG with SL above entry-low → direction_constraint.
        assert_eq!(
            parse(
                &f(&[
                    "ETH-PERP",
                    "LONG 3x",
                    "Entry 3420-3450",
                    "SL 3500",
                    "TP1 3720",
                    "Position 10%",
                    "valid for 4h",
                ]),
                Language::En,
            )
            .unwrap_err(),
            ParseError::DirectionConstraint("stopLoss")
        );
    }

    #[test]
    fn non_monotonic_tps_accepted_if_correct_side() {
        assert!(parse(
            &f(&[
                "ETH-PERP",
                "LONG 3x",
                "Entry 3420-3450",
                "SL 3300",
                "TP1 4000 / TP2 3800",
                "Position 10%",
                "valid for 4h",
            ]),
            Language::En,
        )
        .is_ok());
    }
}
