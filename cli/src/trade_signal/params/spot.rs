//! FR-2.2 spot parser. Two positional forms, told apart by the SHAPE of the 2nd
//! field (on-chain Spot's subject spans chain | token(addr), don't mis-cut
//! with one fixed index per class):
//!
//! - on-chain (7 fields): `chain | $SYMBOL (ADDRESS) | side | lo-hi | slippage(zh) ≤N% | position(zh) N% | <ttl>`
//! - CEX pair (5 fields): `BASE/QUOTE | side [orderType] | lo-hi | position(zh) N% | <ttl>`
//!
//! The on-chain form carries `tokenAddr` + `slippage` (≤5%) and is always a market
//! order; the CEX form carries neither and may set `orderType` to `limit`.

use super::super::error::ParseError;
use super::super::fields;
use super::super::{Language, OrderType, SignalParams, SpotParams};
use super::ClassParse;

pub fn parse(fields: &[String], lang: Language) -> Result<ClassParse, ParseError> {
    // The on-chain subject is `$SYMBOL (ADDRESS)` — a `$` 2nd field selects it.
    let onchain = fields.get(1).is_some_and(|f| f.starts_with('$'));
    if onchain {
        parse_onchain(fields, lang)
    } else {
        parse_cex(fields, lang)
    }
}

fn parse_onchain(fields: &[String], lang: Language) -> Result<ClassParse, ParseError> {
    if fields.len() != 7 {
        return Err(ParseError::FieldCountError);
    }
    let market = fields[0].clone(); // chain
    let (symbol, token_addr) = fields::parse_onchain_token(&fields[1])?;
    let side = fields::parse_side(&fields[2])?;
    let price_range = fields::parse_range(&fields[3], "price")?;
    let slippage = fields::parse_slippage_field(&fields[4], lang)?;
    let position_pct = fields::parse_position_field(&fields[5], lang)?;
    let ttl_sec = fields::parse_ttl_field(&fields[6], lang)?;

    Ok((
        SignalParams::Spot(SpotParams {
            market,
            symbol,
            side,
            price_range,
            order_type: OrderType::Market,
            token_addr: Some(token_addr),
            slippage: Some(slippage),
        }),
        position_pct,
        ttl_sec,
    ))
}

fn parse_cex(fields: &[String], lang: Language) -> Result<ClassParse, ParseError> {
    if fields.len() != 5 {
        return Err(ParseError::FieldCountError);
    }
    let (symbol, market) = fields::split_pair(&fields[0]);
    let (side, order_type) = fields::parse_side_order(&fields[1], lang)?;
    let price_range = fields::parse_range(&fields[2], "price")?;
    let position_pct = fields::parse_position_field(&fields[3], lang)?;
    let ttl_sec = fields::parse_ttl_field(&fields[4], lang)?;

    Ok((
        SignalParams::Spot(SpotParams {
            market,
            symbol,
            side,
            price_range,
            order_type,
            token_addr: None,
            slippage: None,
        }),
        position_pct,
        ttl_sec,
    ))
}

#[cfg(test)]
mod tests {
    use super::super::super::Side;
    use super::*;

    fn f(parts: &[&str]) -> Vec<String> {
        parts.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn onchain_form_parses_token_addr_and_slippage() {
        let (params, pos, ttl) = parse(
            &f(&[
                "base",
                "$DEGEN (0xabc123)",
                "BUY",
                "0.01-0.02",
                "Slippage \u{2264}5%",
                "Position 20%",
                "valid for 7d",
            ]),
            Language::En,
        )
        .unwrap();
        assert_eq!(pos, "20");
        assert_eq!(ttl, 604_800);
        match params {
            SignalParams::Spot(s) => {
                assert_eq!(s.symbol, "DEGEN");
                assert_eq!(s.token_addr.as_deref(), Some("0xabc123"));
                assert_eq!(s.slippage.as_deref(), Some("5"));
                assert_eq!(s.order_type, OrderType::Market);
            }
            _ => panic!("expected spot"),
        }
    }

    #[test]
    fn cex_form_defaults_market_and_splits_pair() {
        let (params, _, _) = parse(
            &f(&[
                "BTC/USDT",
                "BUY",
                "60000-65000",
                "Position 5%",
                "valid for 1h",
            ]),
            Language::En,
        )
        .unwrap();
        match params {
            SignalParams::Spot(s) => {
                assert_eq!(s.symbol, "BTC");
                assert_eq!(s.market, "BTC/USDT");
                assert_eq!(s.side, Side::Buy);
                assert!(s.token_addr.is_none() && s.slippage.is_none());
                assert_eq!(s.order_type, OrderType::Market);
            }
            _ => panic!("expected spot"),
        }
    }

    #[test]
    fn onchain_slippage_above_ceiling_is_out_of_range() {
        assert_eq!(
            parse(
                &f(&[
                    "base",
                    "$DEGEN (0xabc)",
                    "BUY",
                    "1-2",
                    "Slippage \u{2264}9%",
                    "Position 5%",
                    "valid for 1h",
                ]),
                Language::En,
            )
            .unwrap_err(),
            ParseError::OutOfRange("slippage")
        );
    }

    #[test]
    fn wrong_field_count_is_field_count_error() {
        assert_eq!(
            parse(
                &f(&["BTC/USDT", "BUY", "60000-65000", "Position 5%"]),
                Language::En
            )
            .unwrap_err(),
            ParseError::FieldCountError
        );
    }
}
