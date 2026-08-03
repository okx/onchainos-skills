//! Acceptance corpus + invariant tests for the trade-signal parser.
//!
//! The positives are driven by the AUTHORITATIVE V1.1 corpus in `corpus_v1_1.txt`,
//! copied byte-for-byte from the trade-signal specification v1.1 corpus. The
//! corpus lives in a `.txt` fixture — not inline in this `.rs` file — so it stays
//! literally verbatim (no `\u{}` escaping, no reconstruction) while the
//! "no CJK in Rust source" lint (which scans only `cli/**.rs`)
//! stays satisfied. `all_fourteen_normative_examples_parse` is the headline gate:
//! 14/14 of the normative examples MUST parse.
//!
//! The negatives assert the stable snake_case `errorCode` and keep every TD
//! rejection failing closed. They are written in the en grammar (ASCII) where
//! possible; the few zh-specific cases use `\u{}` escapes (byte-identical to the
//! glyphs, lint-safe).

use super::{parse_envelope, parse_signal_text, SignalParams};

/// The verbatim bilingual corpus (7 zh + 7 en), loaded from the fixture. Comment
/// (`#`) and blank lines are skipped; the remaining lines are the 14 examples in
/// the fixed order documented in the fixture header.
fn corpus() -> Vec<&'static str> {
    include_str!("corpus_v1_1.txt")
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
        .collect()
}

fn code(text: &str) -> &'static str {
    parse_signal_text(text).unwrap_err().code()
}

// ── Positives ─────────────────────────────────────────────────────────────────

/// AC-1 (headline): every one of the 14 authoritative V1.1 examples parses.
/// This is the acceptance completion gate — 14/14 normative positives pass.
#[test]
fn all_fourteen_normative_examples_parse() {
    let corpus = corpus();
    assert_eq!(corpus.len(), 14, "expected exactly 14 normative examples");
    for (i, text) in corpus.iter().enumerate() {
        assert!(
            parse_signal_text(text).is_ok(),
            "normative example {i} failed to parse: code={}",
            parse_signal_text(text)
                .map(|_| "ok")
                .unwrap_or_else(|e| e.code())
        );
    }
}

/// AC-9: `assetClass` (top) always equals `params.kind`, for the fixed corpus order.
#[test]
fn asset_class_matches_params_kind_in_fixed_order() {
    // Fixture order: spot,spot, spot,spot, perp,perp, perp,perp, pred,pred,
    // option,option, defi,defi (2 lines per group: zh then en).
    let expected = [
        "spot",
        "spot",
        "spot",
        "spot",
        "perp",
        "perp",
        "perp",
        "perp",
        "prediction",
        "prediction",
        "option",
        "option",
        "defi",
        "defi",
    ];
    let corpus = corpus();
    for (i, text) in corpus.iter().enumerate() {
        let p = parse_signal_text(text).unwrap();
        let kind = match p.params {
            SignalParams::Spot(_) => "spot",
            SignalParams::Perp(_) => "perp",
            SignalParams::Prediction(_) => "prediction",
            SignalParams::Option(_) => "option",
            SignalParams::Defi(_) => "defi",
        };
        assert_eq!(p.asset_class.as_str(), kind, "line {i}");
        assert_eq!(kind, expected[i], "line {i} class");
    }
}

/// AC-3: the two spot forms — on-chain (chain + token/address + slippage) and CEX
/// pair — both parse from the verbatim corpus (lines 0 zh / 1 en on-chain; 2/3 CEX).
#[test]
fn spot_onchain_and_cex_forms() {
    let onchain = parse_signal_text(corpus()[1]).unwrap(); // en on-chain
    match onchain.params {
        SignalParams::Spot(s) => {
            assert_eq!(s.symbol, "TOKEN");
            assert_eq!(s.token_addr.as_deref(), Some("0x12a3\u{2026}9fab"));
            assert_eq!(s.slippage.as_deref(), Some("1"));
            assert_eq!(s.price_range.lo, "0.042");
            assert_eq!(s.price_range.hi, "0.045");
        }
        _ => panic!("expected spot"),
    }
    let cex = parse_signal_text(corpus()[3]).unwrap(); // en CEX
    match cex.params {
        SignalParams::Spot(s) => {
            assert!(s.token_addr.is_none() && s.slippage.is_none());
            assert_eq!(s.symbol, "BTC");
            assert_eq!(s.market, "BTC/USDT");
        }
        _ => panic!("expected spot"),
    }
}

/// AC-4: perp LONG single-TP (line 5) & SHORT isolated slash-TP1/TP2 (line 7).
#[test]
fn perp_directions_and_tp_forms() {
    let long = parse_signal_text(corpus()[5]).unwrap(); // en LONG
    match long.params {
        SignalParams::Perp(p) => {
            assert_eq!(p.leverage, 3);
            assert_eq!(p.stop_loss, "3300");
            assert_eq!(p.take_profit, vec!["3720"]);
            assert!(p.margin_mode.is_none());
        }
        _ => panic!("expected perp"),
    }
    let short = parse_signal_text(corpus()[7]).unwrap(); // en SHORT isolated
    match short.params {
        SignalParams::Perp(p) => {
            assert_eq!(p.leverage, 2);
            assert_eq!(p.take_profit, vec!["96000", "94000"]);
            assert!(p.margin_mode.is_some());
        }
        _ => panic!("expected perp"),
    }
}

/// AC-5/6/8: prediction outcome+odds+settle (line 9), option consistency (line 11),
/// DeFi compact TVL + position/ttl boundaries (line 13).
#[test]
fn prediction_option_defi_shapes() {
    let pred = parse_signal_text(corpus()[9]).unwrap(); // en prediction
    match pred.params {
        SignalParams::Prediction(p) => {
            assert_eq!(p.event, "Fed cuts rates in Sept?");
            assert_eq!(p.odds, "0.62");
            assert_eq!(p.settle_date, "2026-09-18");
        }
        _ => panic!("expected prediction"),
    }
    let opt = parse_signal_text(corpus()[11]).unwrap(); // en option
    match opt.params {
        SignalParams::Option(o) => {
            assert_eq!(o.strike, "100000");
            assert_eq!(o.expiry, "2026-03-27");
            assert_eq!(o.premium_cap, "320");
        }
        _ => panic!("expected option"),
    }
    let defi = parse_signal_text(corpus()[13]).unwrap(); // en defi
    assert_eq!(defi.position_pct, "5");
    assert_eq!(defi.ttl_sec, 172_800); // 48h
    match defi.params {
        SignalParams::Defi(d) => {
            assert_eq!(d.apy, "18.6");
            assert_eq!(d.tvl, "$2.4M");
            assert_eq!(d.token, "USDT");
        }
        _ => panic!("expected defi"),
    }
}

// ── Negatives (fail-closed; assert the stable snake_case errorCode) ────────────

const H_SPOT: &str = "\u{3010}Spot Signal\u{3011}";
const H_PERP: &str = "\u{3010}Futures Signal\u{3011}";
const H_PRED: &str = "\u{3010}Prediction Signal\u{3011}";
const H_OPT: &str = "\u{3010}Options Signal\u{3011}";
const H_DEFI: &str = "\u{3010}DeFi Signal\u{3011}";

/// AC-10: header preceded by space / unknown / half-width `[`.
#[test]
fn header_faults() {
    assert_eq!(
        code(&format!(
            " {H_SPOT}BTC/USDT | BUY | 1-2 | Position 5% | valid for 1h"
        )),
        "unknown_header"
    );
    assert_eq!(code("\u{3010}Unknown\u{3011}x"), "unknown_header");
    assert_eq!(code("[Spot Signal]x"), "unknown_header");
}

/// AC-11 (updated for MR !196): a VALUE-level language mix still fails closed. The
/// mixed-language expansion relaxes only field KEYWORDS (position keyword in zh or
/// en, etc.), never the closed value vocabulary. A zh-header signal whose order-type
/// VALUE is the en token `limit` (a value, not a field keyword) is therefore still
/// `language_mix` — canonicalization reorders it but the per-class validator
/// re-rejects the value.
#[test]
fn language_mix() {
    // zh spot CEX header, zh position keyword + zh ttl suffix, but an en order-type
    // VALUE (`limit`) → language_mix.
    let zh_spot = "\u{3010}\u{73b0}\u{8d27}\u{4fe1}\u{53f7}\u{3011}";
    let ttl_zh = "24h \u{5185}\u{6709}\u{6548}";
    let pos_zh = "\u{4ed3}\u{4f4d} 10%";
    assert_eq!(
        code(&format!(
            "{zh_spot}BTC/USDT | BUY limit | 96500-97200 | {pos_zh} | {ttl_zh}"
        )),
        "language_mix"
    );
}

/// AC-12: multi-line / too-long / emoji / link / misplaced `@` / wrong count / empty field.
#[test]
fn forbidden_and_shape() {
    assert_eq!(
        code(&format!("{H_SPOT}BTC/USDT | BUY\n| Position 5%")),
        "multi_line"
    );
    assert_eq!(code(&format!("{H_SPOT}{}", "A".repeat(210))), "too_long");
    assert_eq!(
        code(&format!(
            "{H_SPOT}BTC\u{1F680}/USDT | BUY | 1-2 | Position 5% | valid for 1h"
        )),
        "forbidden_content"
    );
    assert_eq!(
        code(&format!(
            "{H_SPOT}https://x.io | BUY | 1-2 | Position 5% | valid for 1h"
        )),
        "forbidden_content"
    );
    // `@` outside the Prediction outcome field.
    assert_eq!(
        code(&format!(
            "{H_SPOT}@btc | BUY | 1-2 | Position 5% | valid for 1h"
        )),
        "forbidden_content"
    );
    // extra field (6 for a 5-field CEX form).
    assert_eq!(
        code(&format!(
            "{H_SPOT}BTC/USDT | BUY | 1-2 | Position 5% | valid for 1h | EXTRA"
        )),
        "field_count_error"
    );
    // empty field (double pipe).
    assert_eq!(
        code(&format!(
            "{H_SPOT}BTC/USDT || BUY | 1-2 | Position 5% | valid for 1h"
        )),
        "empty_field"
    );
}

/// AC-13: position 0 / 20.1 / range.
#[test]
fn position_bounds() {
    for pos in ["0%", "20.1%", "5-10%"] {
        assert_eq!(
            code(&format!(
                "{H_SPOT}BTC/USDT | BUY | 60000-65000 | Position {pos} | valid for 1h"
            )),
            "out_of_range",
            "position {pos}"
        );
    }
}

/// AC-14: TTL 4min / >7d / unknown unit.
#[test]
fn ttl_bounds() {
    for ttl in ["4min", "8d", "30s"] {
        assert_eq!(
            code(&format!(
                "{H_SPOT}BTC/USDT | BUY | 60000-65000 | Position 5% | valid for {ttl}"
            )),
            "out_of_range",
            "ttl {ttl}"
        );
    }
}

/// AC-15: sci-notation / thousands separator / %-price / inverted range.
#[test]
fn numeric_faults() {
    let base =
        |price: &str| format!("{H_SPOT}BTC/USDT | BUY | {price} | Position 5% | valid for 1h");
    assert_eq!(code(&base("1e3-2e3")), "invalid_number");
    assert_eq!(code(&base("1,000-2,000")), "invalid_number");
    assert_eq!(code(&base("5%-10%")), "invalid_number");
    assert_eq!(code(&base("65000-60000")), "out_of_range");
}

/// AC-16: perp SL/TP wrong direction, TP numbering gap.
#[test]
fn perp_direction_faults() {
    // wrong-direction SL (LONG, SL above entry-low).
    assert_eq!(
        code(&format!(
            "{H_PERP}BTC-PERP | LONG 10x | Entry 60000-61000 | SL 60500 | TP1 62000 | Position 5% | valid for 1h"
        )),
        "direction_constraint"
    );
    // wrong-direction TP (LONG, TP below entry-low).
    assert_eq!(
        code(&format!(
            "{H_PERP}BTC-PERP | LONG 10x | Entry 60000-61000 | SL 59000 | TP1 59500 | Position 5% | valid for 1h"
        )),
        "direction_constraint"
    );
    // TP numbering gap (TP1 then TP3).
    assert_eq!(
        code(&format!(
            "{H_PERP}BTC-PERP | LONG 10x | Entry 60000-61000 | SL 59000 | TP1 62000 / TP3 64000 | Position 5% | valid for 1h"
        )),
        "direction_constraint"
    );
}

/// AC-17: prediction illegal outcome, odds out of [0,1], missing year, nonexistent date.
#[test]
fn prediction_faults() {
    assert_eq!(
        code(&format!(
            "{H_PRED}x | MAYBE @0.5 | Position 5% | Settle 2025-12-31 | valid for 1d"
        )),
        "illegal_keyword"
    );
    assert_eq!(
        code(&format!(
            "{H_PRED}x | YES @1.5 | Position 5% | Settle 2025-12-31 | valid for 1d"
        )),
        "out_of_range"
    );
    assert_eq!(
        code(&format!(
            "{H_PRED}x | YES @0.5 | Position 5% | Settle 12-31 | valid for 1d"
        )),
        "invalid_date"
    );
    assert_eq!(
        code(&format!(
            "{H_PRED}x | YES @0.5 | Position 5% | Settle 2025-02-30 | valid for 1d"
        )),
        "invalid_date"
    );
}

/// AC-18: option contractCode inconsistent with Call/Put, strike, or expiry.
#[test]
fn option_mismatch() {
    let mk = |ot: &str, strike: &str, expiry: &str| {
        format!(
            "{H_OPT}BTC-251231-60000-C | Buy {ot} | Strike {strike} | Expiry {expiry} | Premium 1500 | Position 5% | valid for 5d"
        )
    };
    assert_eq!(
        code(&mk("Put", "60000", "2025-12-31")),
        "option_field_mismatch"
    );
    assert_eq!(
        code(&mk("Call", "59000", "2025-12-31")),
        "option_field_mismatch"
    );
    assert_eq!(
        code(&mk("Call", "60000", "2025-12-30")),
        "option_field_mismatch"
    );
}

/// AC-19: DeFi missing a required field (7 fields for an 8-field form).
#[test]
fn defi_missing_field() {
    assert_eq!(
        code(&format!(
            "{H_DEFI}Ethereum | AaveV3 | TVL 1.2B | USDC | flexible | Position 10% | valid for 7d"
        )),
        "field_count_error"
    );
}

/// AC-20: envelope schemaVersion ≠ 2 / signalTime = 0 / illegal deliveryId — each a
/// distinct fine-grained code.
#[test]
fn envelope_faults() {
    let text =
        "\u{3010}Spot Signal\u{3011}BTC/USDT | BUY | 60000-65000 | Position 5% | valid for 1h";
    let mk = |schema: u32, delivery: &str, time: u64| {
        format!("{{\"schemaVersion\":{schema},\"deliveryId\":\"{delivery}\",\"signalTime\":{time},\"signalText\":\"{text}\"}}")
    };
    assert!(parse_envelope(&mk(2, "abc123", 1)).is_ok());
    assert_eq!(
        parse_envelope(&mk(1, "abc123", 1)).unwrap_err().code(),
        "invalid_schema_version"
    );
    assert_eq!(
        parse_envelope(&mk(2, "abc123", 0)).unwrap_err().code(),
        "invalid_signal_time"
    );
    assert_eq!(
        parse_envelope(&mk(2, "bad id", 1)).unwrap_err().code(),
        "invalid_delivery_id"
    );
}

/// Empty-input integration case (MR !196 review LOW — test gap): the public
/// `parse_signal_text` entry maps an empty string to the stable `empty_input`
/// code (previously only covered at the guard level, never end-to-end).
#[test]
fn empty_input_integration() {
    assert_eq!(parse_signal_text("").unwrap_err().code(), "empty_input");
}

/// Envelope `deny_unknown_fields` integration case (MR !196 review LOW — test
/// gap): an otherwise-valid envelope carrying an unexpected top-level key is
/// rejected as `invalid_envelope`, not silently accepted.
#[test]
fn envelope_rejects_unknown_field() {
    let text =
        "\u{3010}Spot Signal\u{3011}BTC/USDT | BUY | 60000-65000 | Position 5% | valid for 1h";
    let with_extra = format!(
        "{{\"schemaVersion\":2,\"deliveryId\":\"abc123\",\"signalTime\":1,\"signalText\":\"{text}\",\"unexpected\":true}}"
    );
    assert_eq!(
        parse_envelope(&with_extra).unwrap_err().code(),
        "invalid_envelope"
    );
}

// ── Invariants ──────────────────────────────────────────────────────────────
/// AC-24: no error path echoes the raw signal text / tokenAddr / event / contractCode.
#[test]
fn errors_never_leak_input() {
    let leaky_inputs = [
        // tokenAddr in an otherwise-bad on-chain spot (slippage over the ceiling).
        format!(
            "{H_SPOT}base | $X (0xSECRETADDR) | BUY | 1-2 | Slippage \u{2264}9% | Position 5% | valid for 1h"
        ),
        // event free text in a bad prediction (odds out of range).
        format!("{H_PRED}SECRETEVENTTEXT | YES @9 | Position 5% | Settle 2025-12-31 | valid for 1d"),
        // contractCode in a mismatched option.
        format!(
            "{H_OPT}SECRETCODE-251231-60000-C | Buy Put | Strike 60000 | Expiry 2025-12-31 | Premium 1 | Position 5% | valid for 5d"
        ),
    ];
    let secrets = ["0xSECRETADDR", "SECRETEVENTTEXT", "SECRETCODE"];
    for input in &leaky_inputs {
        let e = parse_signal_text(input).unwrap_err();
        let rendered = format!("{} {} {:?}", e.code(), e.message(), e.field());
        for secret in secrets {
            assert!(
                !rendered.contains(secret),
                "error leaked '{secret}' for input starting {}",
                &input[..12.min(input.len())]
            );
        }
    }
}

/// A rejected signal's error carries the REAL canonical
/// field (not a `number`/`range`/`date` placeholder, and `stopLoss` vs
/// `takeProfit` correctly distinguished).
#[test]
fn errors_carry_canonical_field() {
    let field = |text: &str| parse_signal_text(text).unwrap_err().field();
    // perp: SL on the wrong side → stopLoss (NOT the old always-takeProfit).
    assert_eq!(
        field(&format!(
            "{H_PERP}BTC-PERP | LONG 10x | Entry 60000-61000 | SL 60500 | TP1 62000 | Position 5% | valid for 1h"
        )),
        Some("stopLoss")
    );
    // perp: TP on the wrong side → takeProfit.
    assert_eq!(
        field(&format!(
            "{H_PERP}BTC-PERP | LONG 10x | Entry 60000-61000 | SL 59000 | TP1 59500 | Position 5% | valid for 1h"
        )),
        Some("takeProfit")
    );
    // prediction: bad settle date → settleDate.
    assert_eq!(
        field(&format!(
            "{H_PRED}x | YES @0.5 | Position 5% | Settle 2025-02-30 | valid for 1d"
        )),
        Some("settleDate")
    );
    // option: bad expiry date → expiry.
    assert_eq!(
        field(&format!(
            "{H_OPT}BTC-251231-60000-C | Buy Call | Strike 60000 | Expiry 2025-13-40 | Premium 1 | Position 5% | valid for 5d"
        )),
        Some("expiry")
    );
    // defi: non-canonical tvl → tvl.
    assert_eq!(
        field(&format!(
            "{H_DEFI}Ethereum | AaveV3 | APY 5% | TVL lots | USDC | flexible | Position 10% | valid for 7d"
        )),
        Some("tvl")
    );
    // common scalar fields.
    assert_eq!(
        field(&format!(
            "{H_SPOT}BTC/USDT | BUY | 60000-65000 | Position 0% | valid for 1h"
        )),
        Some("position")
    );
    assert_eq!(
        field(&format!(
            "{H_SPOT}BTC/USDT | BUY | 60000-65000 | Position 5% | valid for 8d"
        )),
        Some("ttl")
    );
    assert_eq!(
        field(&format!(
            "{H_PRED}x | YES @1.5 | Position 5% | Settle 2025-12-31 | valid for 1d"
        )),
        Some("odds")
    );
    assert_eq!(
        field(&format!(
            "{H_SPOT}base | $X (0xabc) | BUY | 1-2 | Slippage \u{2264}9% | Position 5% | valid for 1h"
        )),
        Some("slippage")
    );
}

/// AC-23: exactly one `AssetClass` type is referenced crate-wide (the crate-root
/// module). A second definition would be a duplicate flagged by review / onchainos_check.
#[test]
fn single_asset_class_type() {
    let c: crate::asset_class::AssetClass = crate::asset_class::AssetClass::Spot;
    assert_eq!(c.as_str(), "spot");
}

// ── FR-2 usability expansion: mixed language + safe reorder (MR !196) ──────────
//
// The fallback accepts bilingual field-keyword mixing and field reordering ONLY
// when every required field maps exactly once and unambiguously; a mixed/reordered
// form must produce a ParsedSignal byte-identical to its canonical form, and any
// ambiguous/missing/duplicate/unknown field must fail closed.

// zh headers.
const ZH_SPOT: &str = "\u{3010}\u{73b0}\u{8d27}\u{4fe1}\u{53f7}\u{3011}";
const ZH_PERP: &str = "\u{3010}\u{5408}\u{7ea6}\u{4fe1}\u{53f7}\u{3011}";
const ZH_PRED: &str = "\u{3010}\u{9884}\u{6d4b}\u{5e02}\u{573a}\u{4fe1}\u{53f7}\u{3011}";
const ZH_OPT: &str = "\u{3010}\u{671f}\u{6743}\u{4fe1}\u{53f7}\u{3011}";
const ZH_DEFI: &str = "\u{3010}DeFi \u{4fe1}\u{53f7}\u{3011}";
// zh field keywords / values (\u-escaped; byte-identical to the glyphs).
const ZH_POS: &str = "\u{4ed3}\u{4f4d}"; // position
const ZH_TTL: &str = "\u{5185}\u{6709}\u{6548}"; // ttl suffix
const ZH_SLIP: &str = "\u{6ed1}\u{70b9}"; // slippage
const ZH_ENTRY: &str = "\u{5165}\u{573a}"; // entry
const ZH_SETTLE: &str = "\u{7ed3}\u{7b97}"; // settle
const ZH_STRIKE: &str = "\u{884c}\u{6743}\u{4ef7}"; // strike
const ZH_EXPIRY: &str = "\u{5230}\u{671f}"; // expiry
const ZH_PREMIUM: &str = "\u{6743}\u{5229}\u{91d1}"; // premium
const ZH_LIMIT: &str = "\u{9650}\u{4ef7}"; // order-type value: limit
const LE: &str = "\u{2264}";
const ELLIPSIS: &str = "\u{2026}";

fn parse_ok(text: &str) -> super::ParsedSignal {
    parse_signal_text(text)
        .unwrap_or_else(|e| panic!("expected parse ok, got {} for {text:?}", e.code()))
}

/// A canonical form and a mixed-language + reordered form must yield an identical
/// ParsedSignal — across all five asset classes, both zh-header/en-fields and
/// en-header/zh-fields.
#[test]
fn mixed_language_and_reorder_equals_canonical() {
    // Spot CEX — en header, zh `Position` keyword, position moved before range.
    let spot_canon =
        format!("{H_SPOT}BTC/USDT | BUY limit | 96500-97200 | Position 10% | valid for 12h");
    let spot_mixed =
        format!("{H_SPOT}BTC/USDT | BUY limit | {ZH_POS} 10% | 96500-97200 | valid for 12h");
    assert_eq!(parse_ok(&spot_canon), parse_ok(&spot_mixed));

    // Spot on-chain — zh header, en `Position`/`Slippage` keywords, reordered; the
    // free-text chain stays a single leftover slot.
    let onchain_canon = format!(
        "{ZH_SPOT}X Layer | $TOKEN (0x12a3{ELLIPSIS}9fab) | BUY | 0.042-0.045 | {ZH_SLIP} {LE}1% | {ZH_POS} 5% | 24h {ZH_TTL}"
    );
    let onchain_mixed = format!(
        "{ZH_SPOT}BUY | X Layer | $TOKEN (0x12a3{ELLIPSIS}9fab) | Position 5% | 0.042-0.045 | Slippage {LE}1% | 24h {ZH_TTL}"
    );
    assert_eq!(parse_ok(&onchain_canon), parse_ok(&onchain_mixed));

    // Perp — en header, zh `Entry`/`Position` keywords, reordered.
    let perp_canon = format!(
        "{H_PERP}ETH-PERP | LONG 3x | Entry 3420-3450 | SL 3300 | TP1 3720 | Position 10% | valid for 4h"
    );
    let perp_mixed = format!(
        "{H_PERP}LONG 3x | ETH-PERP | {ZH_POS} 10% | SL 3300 | {ZH_ENTRY} 3420-3450 | TP1 3720 | valid for 4h"
    );
    assert_eq!(parse_ok(&perp_canon), parse_ok(&perp_mixed));

    // Prediction — en header, zh `Settle`/`Position`, reordered; event is the single
    // remaining free-text slot, assigned last.
    let pred_canon = format!(
        "{H_PRED}\"Fed cuts rates in Sept?\" | YES @0.62 | Position 5% | Settle 2026-09-18 | valid for 24h"
    );
    let pred_mixed = format!(
        "{H_PRED}YES @0.62 | {ZH_SETTLE} 2026-09-18 | \"Fed cuts rates in Sept?\" | {ZH_POS} 5% | valid for 24h"
    );
    assert_eq!(parse_ok(&pred_canon), parse_ok(&pred_mixed));

    // Option — zh header, en keywords throughout, fully reordered (no free-text).
    let opt_canon = format!(
        "{ZH_OPT}BTC-260327-100000-C | Buy Call | {ZH_STRIKE} 100000 | {ZH_EXPIRY} 2026-03-27 | {ZH_PREMIUM} {LE}320 USDT | {ZH_POS} 3% | 24h {ZH_TTL}"
    );
    let opt_mixed = format!(
        "{ZH_OPT}Strike 100000 | Buy Call | BTC-260327-100000-C | Premium {LE}320 USDT | Expiry 2026-03-27 | Position 3% | 24h {ZH_TTL}"
    );
    assert_eq!(parse_ok(&opt_canon), parse_ok(&opt_mixed));

    // DeFi — en header, zh `Position`, identifiable fields reordered; the four
    // free-text fields (chain|pool|token|redeemTerms) keep their relative order.
    let defi_canon = format!(
        "{H_DEFI}X Layer | ProtocolX USDT-USDG LP | APY 18.6% | TVL $2.4M | USDT | withdraw anytime | Position 5% | valid for 48h"
    );
    let defi_mixed = format!(
        "{H_DEFI}APY 18.6% | X Layer | ProtocolX USDT-USDG LP | {ZH_POS} 5% | TVL $2.4M | USDT | withdraw anytime | valid for 48h"
    );
    assert_eq!(parse_ok(&defi_canon), parse_ok(&defi_mixed));
}

/// zh-header + en order-type VALUE combined with a reordered/mixed field set still
/// canonicalizes, and the value-level order-type (en `limit` / zh `limit(zh)`)
/// distinction is preserved.
#[test]
fn zh_header_reorder_with_zh_value_keeps_value_semantics() {
    let canon =
        format!("{ZH_SPOT}BTC/USDT | BUY {ZH_LIMIT} | 96500-97200 | {ZH_POS} 10% | 24h {ZH_TTL}");
    // reorder: range before side, en Position keyword.
    let mixed =
        format!("{ZH_SPOT}96500-97200 | BTC/USDT | BUY {ZH_LIMIT} | Position 10% | 24h {ZH_TTL}");
    let a = parse_ok(&canon);
    let b = parse_ok(&mixed);
    assert_eq!(a, b);
    match b.params {
        SignalParams::Spot(s) => assert_eq!(s.order_type, super::OrderType::Limit),
        _ => panic!("expected spot"),
    }
}

/// Fail-closed: a missing, duplicate/ambiguous (multi-match), or unknown field must
/// reject — the fallback never guesses or silently drops a field.
#[test]
fn reorder_fallback_rejects_ambiguous_and_incomplete() {
    // Missing required field (spot CEX with only 4 fields — no ttl).
    assert!(parse_signal_text(&format!(
        "{H_SPOT}BTC/USDT | BUY | 96500-97200 | Position 10%"
    ))
    .is_err());
    // Ambiguous / multi-match: two range-shaped fields, no position field.
    assert!(parse_signal_text(&format!(
        "{H_SPOT}BTC/USDT | BUY | 96500-97200 | 60000-65000 | valid for 12h"
    ))
    .is_err());
    // Duplicate keyword field: two `Position` fields, no ttl.
    assert!(parse_signal_text(&format!(
        "{H_SPOT}BTC/USDT | BUY | 96500-97200 | Position 10% | Position 5%"
    ))
    .is_err());
    // Unknown field where a required perp `Entry` field should be.
    assert!(parse_signal_text(&format!(
        "{H_PERP}ETH-PERP | LONG 3x | GARBAGE 1-2 | SL 3300 | TP1 3720 | Position 10% | valid for 4h"
    ))
    .is_err());
}
