//! FR-1 signal schema: envelope + per-type params + structure validation.
//!
//! `validate_structure` is the single structure-only entry point, used **outbound**
//! (`deliver --autotrade`) and as the inbound first pass. It never touches the
//! network — it validates shape, charset, unit-by-side, slippage cap, and chain
//! support only.

use serde::{Deserialize, Serialize};

use super::amount::Decimal;
use super::AutoTradeError;

/// Current signal schema version.
pub const SCHEMA_VERSION: u32 = 1;
/// Recommended default applied when `slippageBps` is absent (5%).
pub const DEFAULT_SLIPPAGE_BPS: u32 = 500;
/// Two-sided hard cap; `slippageBps > MAX_SLIPPAGE_BPS` is rejected at the schema layer.
/// Set conservatively to 5% (== `DEFAULT_SLIPPAGE_BPS`), resolving arch §10 Q3: the cap
/// bounds how much slippage an ASP may authorize against a buyer's real funds, so it
/// errs safe and matches the default. Tightening only ever rejects more (fail-safe),
/// never permits more; a higher cap, if product later wants one, is a one-constant edit.
pub const MAX_SLIPPAGE_BPS: u32 = 500;

// Charset / length caps for command-bound strings.
const MAX_DELIVERY_ID: usize = 64;
const MAX_TOKEN_ADDR: usize = 128;
const MAX_PRODUCT_ID: usize = 64;
const MAX_PLATFORM_ID: usize = 64;
const MAX_CONDITION_ID: usize = 128;
const MAX_OUTCOME: usize = 32;
const MAX_TTL_SEC: u64 = 86_400;
/// Polymarket share-price ceiling in cents: a share can never be worth more than
/// $1.00 (100¢), so `maxPriceCents` must be in `1..=100` (renders `--price ≤ 1.00`).
const MAX_PRICE_CENTS: u32 = 100;

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AutoTradeSignal {
    pub schema_version: u32,
    /// 1..=64, charset `[A-Za-z0-9_-]`; the idempotency key.
    pub delivery_id: String,
    pub signal_type: SignalType,
    /// ms; CLI-stamped outbound (`--autotrade` omits it → `default` 0 → stamped);
    /// `0` ⇒ reject at validation.
    #[serde(default)]
    pub signal_time: u64,
    /// 1..=86400.
    pub ttl_sec: u64,
    /// Typed per `signal_type` in a second `deny_unknown_fields` pass.
    pub params: serde_json::Value,
}

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SignalType {
    // v1 enabled (executed)
    DexTrade,
    Polymarket,
    // reserved (frozen; buyer degrades to notify-only, not executed)
    DefiRebalance, // temporarily demoted for v1 (untested); params/command kept for re-enable
    HyperliquidPerp,
    MemeLaunch,
    LimitOrder,
    GmxPerp,
}

impl SignalType {
    pub fn is_enabled(&self) -> bool {
        // v1 executes only dex_trade + polymarket. defi_rebalance is temporarily
        // demoted to reserved (notify-only, not executed) — untested for this release;
        // its params/command code is kept for a later re-enable.
        matches!(self, SignalType::DexTrade | SignalType::Polymarket)
    }

    /// The stable wire string (matches serde snake_case), used for `data.signalType`.
    pub fn as_str(&self) -> &'static str {
        match self {
            SignalType::DexTrade => "dex_trade",
            SignalType::DefiRebalance => "defi_rebalance",
            SignalType::Polymarket => "polymarket",
            SignalType::HyperliquidPerp => "hyperliquid_perp",
            SignalType::MemeLaunch => "meme_launch",
            SignalType::LimitOrder => "limit_order",
            SignalType::GmxPerp => "gmx_perp",
        }
    }
}

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Side {
    Buy,
    Sell,
}

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum AmountUnit {
    Quote,
    Base,
    Pct,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DexTradeParams {
    pub chain_index: String,
    pub token_address: String,
    pub side: Side,
    pub amount: String,
    pub amount_unit: AmountUnit,
    pub slippage_bps: Option<u32>,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DefiRebalanceParams {
    pub protocol_product_id: String,
    pub action: DefiAction,
    pub amount: Option<String>,
    pub amount_unit: Option<AmountUnit>,
    pub token_address: Option<String>,
    pub chain_index: Option<String>,
    pub platform_id: Option<String>,
}

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum DefiAction {
    Deposit,
    Withdraw,
    Claim,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PolymarketParams {
    pub condition_id: String,
    pub outcome: String,
    pub side: Side,
    pub amount: String,
    pub amount_unit: AmountUnit,
    pub max_price_cents: Option<u32>,
}

/// The typed, validated params for an enabled signal type.
///
/// Reserved types never reach here (they degrade to notify-only before params
/// are used); `parse_and_validate` returns `TypeDegrade` for them.
#[derive(Clone, Debug)]
pub enum TypedParams {
    Dex(DexTradeParams),
    Defi(DefiRebalanceParams),
    Polymarket(PolymarketParams),
}

/// The shared command-bound charset: `[A-Za-z0-9_-]` — identical to `deliveryId`.
/// Covers hex / base58 addresses, Polymarket outcome tokens, and platform / product ids.
fn is_command_charset(c: char) -> bool {
    c.is_ascii_alphanumeric() || c == '_' || c == '-'
}

/// Reject a command-bound string that could break out of its argument position
/// or inject a flag into the assembled recipe command (AC-2).
///
/// This is a strict *whitelist* (`[A-Za-z0-9_-]`), not a blacklist of shell
/// metacharacters. A blacklist silently passes the shell *expansion* characters —
/// glob (`* ? [ ]`), brace (`{ }`) and `~` — which are not quoted in the assembled
/// recipe, so a malicious ASP could set `tokenAddress` to `0x{a,b}` (brace-split
/// into two arguments) or `*` (glob-expanded against the cwd), splitting or
/// replacing an argument even though no `--flag` can be injected.
fn command_safe(s: &str) -> bool {
    if s.is_empty() {
        return false;
    }
    // Flag injection (e.g. "--slippage"); `-` is otherwise a legal charset member.
    if s.contains("--") {
        return false;
    }
    s.chars().all(is_command_charset)
}

/// Charset-lock + length-cap a command-bound string; `Reject` on violation.
fn check_field(name: &str, value: &str, max_len: usize) -> Result<(), AutoTradeError> {
    if value.chars().count() > max_len {
        return Err(AutoTradeError::Reject(format!(
            "{name} exceeds max length {max_len}"
        )));
    }
    if !command_safe(value) {
        return Err(AutoTradeError::Reject(format!(
            "{name} contains an illegal character"
        )));
    }
    Ok(())
}

/// `deliveryId` charset: `[A-Za-z0-9_-]`, length 1..=64.
fn check_delivery_id(id: &str) -> Result<(), AutoTradeError> {
    if id.is_empty() || id.len() > MAX_DELIVERY_ID {
        return Err(AutoTradeError::Reject(
            "deliveryId length must be 1..=64".to_string(),
        ));
    }
    if let Some(bad) = id.chars().find(|c| !is_command_charset(*c)) {
        return Err(AutoTradeError::Reject(format!(
            "deliveryId contains illegal character '{bad}'"
        )));
    }
    Ok(())
}

/// Parse `amount` as a percentage in the range (0, 100].
fn check_pct(amount: &str) -> Result<Decimal, AutoTradeError> {
    let d = Decimal::parse(amount)
        .map_err(|_| AutoTradeError::Reject("amount is not a valid decimal".to_string()))?;
    if d.is_zero() {
        return Err(AutoTradeError::Reject("pct must be > 0".to_string()));
    }
    let hundred = Decimal::parse("100").expect("literal");
    if !d.le(&hundred) {
        return Err(AutoTradeError::Reject("pct must be <= 100".to_string()));
    }
    Ok(d)
}

/// Parse `amount` as a plain positive decimal.
fn check_positive_decimal(amount: &str, field: &str) -> Result<(), AutoTradeError> {
    let d = Decimal::parse(amount)
        .map_err(|_| AutoTradeError::Reject(format!("{field} is not a valid decimal")))?;
    if d.is_zero() {
        return Err(AutoTradeError::Reject(format!("{field} must be > 0")));
    }
    Ok(())
}

fn resolve_and_check_chain(chain_index: &str) -> Result<(), AutoTradeError> {
    crate::chains::ensure_supported_chain(chain_index, chain_index)
        .map_err(|_| AutoTradeError::Reject(format!("unsupported chainIndex: {chain_index}")))
}

/// Second-pass typed parse + per-type structural rules. Returns the validated
/// typed params for enabled types, or `Degrade(TypeDegrade)` for reserved types.
pub fn parse_and_validate(signal: &AutoTradeSignal) -> Result<TypedParams, AutoTradeError> {
    if !signal.signal_type.is_enabled() {
        return Err(AutoTradeError::Degrade(super::DegradeReason::TypeDegrade));
    }

    match signal.signal_type {
        SignalType::DexTrade => {
            let p: DexTradeParams = serde_json::from_value(signal.params.clone())
                .map_err(|e| AutoTradeError::Reject(format!("dex_trade params invalid: {e}")))?;
            resolve_and_check_chain(&p.chain_index)?;
            check_field("tokenAddress", &p.token_address, MAX_TOKEN_ADDR)?;
            // unit-by-side: buy ⇒ quote only; sell ⇒ base | pct.
            match (p.side, p.amount_unit) {
                (Side::Buy, AmountUnit::Quote) => check_positive_decimal(&p.amount, "amount")?,
                (Side::Buy, _) => {
                    return Err(AutoTradeError::Reject(
                        "dex_trade buy accepts only quote amountUnit".to_string(),
                    ))
                }
                (Side::Sell, AmountUnit::Base) => check_positive_decimal(&p.amount, "amount")?,
                (Side::Sell, AmountUnit::Pct) => {
                    check_pct(&p.amount)?;
                }
                (Side::Sell, AmountUnit::Quote) => {
                    return Err(AutoTradeError::Reject(
                        "dex_trade sell accepts only base or pct amountUnit".to_string(),
                    ))
                }
            }
            if let Some(bps) = p.slippage_bps {
                if bps > MAX_SLIPPAGE_BPS {
                    return Err(AutoTradeError::Reject(format!(
                        "slippageBps {bps} exceeds cap {MAX_SLIPPAGE_BPS}"
                    )));
                }
            }
            Ok(TypedParams::Dex(p))
        }
        SignalType::DefiRebalance => {
            let p: DefiRebalanceParams =
                serde_json::from_value(signal.params.clone()).map_err(|e| {
                    AutoTradeError::Reject(format!("defi_rebalance params invalid: {e}"))
                })?;
            check_field("protocolProductId", &p.protocol_product_id, MAX_PRODUCT_ID)?;
            match p.action {
                DefiAction::Deposit => {
                    // deposit ⇒ quote absolute; requires tokenAddress + chainIndex.
                    if p.amount_unit != Some(AmountUnit::Quote) {
                        return Err(AutoTradeError::Reject(
                            "defi deposit accepts only quote amountUnit".to_string(),
                        ));
                    }
                    let amount = p.amount.as_deref().ok_or_else(|| {
                        AutoTradeError::Reject("defi deposit requires amount".to_string())
                    })?;
                    check_positive_decimal(amount, "amount")?;
                    let token = p.token_address.as_deref().ok_or_else(|| {
                        AutoTradeError::Reject("defi deposit requires tokenAddress".to_string())
                    })?;
                    check_field("tokenAddress", token, MAX_TOKEN_ADDR)?;
                    let chain = p.chain_index.as_deref().ok_or_else(|| {
                        AutoTradeError::Reject("defi deposit requires chainIndex".to_string())
                    })?;
                    resolve_and_check_chain(chain)?;
                }
                DefiAction::Withdraw => {
                    // withdraw ⇒ pct only.
                    if p.amount_unit != Some(AmountUnit::Pct) {
                        return Err(AutoTradeError::Reject(
                            "defi withdraw accepts only pct amountUnit".to_string(),
                        ));
                    }
                    let amount = p.amount.as_deref().ok_or_else(|| {
                        AutoTradeError::Reject("defi withdraw requires amount".to_string())
                    })?;
                    check_pct(amount)?;
                }
                DefiAction::Claim => {
                    // claim ⇒ platformId + chainIndex required; amount unused.
                    let platform = p.platform_id.as_deref().ok_or_else(|| {
                        AutoTradeError::Reject("defi claim requires platformId".to_string())
                    })?;
                    check_field("platformId", platform, MAX_PLATFORM_ID)?;
                    let chain = p.chain_index.as_deref().ok_or_else(|| {
                        AutoTradeError::Reject("defi claim requires chainIndex".to_string())
                    })?;
                    resolve_and_check_chain(chain)?;
                }
            }
            Ok(TypedParams::Defi(p))
        }
        SignalType::Polymarket => {
            let p: PolymarketParams = serde_json::from_value(signal.params.clone())
                .map_err(|e| AutoTradeError::Reject(format!("polymarket params invalid: {e}")))?;
            check_field("conditionId", &p.condition_id, MAX_CONDITION_ID)?;
            check_field("outcome", &p.outcome, MAX_OUTCOME)?;
            // maxPriceCents renders `--price = cents/100`; a share is worth at most
            // $1.00, so cap it at 1..=100 (0 or >100 is meaningless / >$1.00).
            if let Some(cents) = p.max_price_cents {
                if cents == 0 || cents > MAX_PRICE_CENTS {
                    return Err(AutoTradeError::Reject(format!(
                        "maxPriceCents must be in 1..={MAX_PRICE_CENTS}"
                    )));
                }
            }
            // unit-by-side: buy ⇒ quote (spend USDC); sell ⇒ base (sell shares).
            match (p.side, p.amount_unit) {
                (Side::Buy, AmountUnit::Quote) => check_positive_decimal(&p.amount, "amount")?,
                (Side::Sell, AmountUnit::Base) => check_positive_decimal(&p.amount, "amount")?,
                (Side::Buy, _) => {
                    return Err(AutoTradeError::Reject(
                        "polymarket buy accepts only quote amountUnit".to_string(),
                    ))
                }
                (Side::Sell, _) => {
                    return Err(AutoTradeError::Reject(
                        "polymarket sell accepts only base amountUnit".to_string(),
                    ))
                }
            }
            Ok(TypedParams::Polymarket(p))
        }
        // Reserved types handled by the is_enabled() guard above.
        _ => Err(AutoTradeError::Degrade(super::DegradeReason::TypeDegrade)),
    }
}

/// Structure-only validation (FR-1). Envelope fields + per-type typed parse.
///
/// Outbound (`deliver --autotrade`): any `Reject` aborts delivery. Reserved types
/// pass envelope validation (they are delivered and degraded inbound), so a
/// `TypeDegrade` from `parse_and_validate` is **not** treated as a structural
/// failure here.
pub fn validate_structure(signal: &AutoTradeSignal) -> Result<(), AutoTradeError> {
    check_delivery_id(&signal.delivery_id)?;
    // SEC-02: reject a signal from a newer schema — v2 may reinterpret existing fields,
    // and `deny_unknown_fields` only guards NEW fields. Degrade to notify-only, never execute.
    if signal.schema_version > SCHEMA_VERSION {
        return Err(AutoTradeError::Degrade(
            super::DegradeReason::SchemaVersionTooNew,
        ));
    }
    if signal.signal_time == 0 {
        return Err(AutoTradeError::Reject(
            "signalTime must not be 0".to_string(),
        ));
    }
    if signal.ttl_sec == 0 || signal.ttl_sec > MAX_TTL_SEC {
        return Err(AutoTradeError::Reject(
            "ttlSec must be in 1..=86400".to_string(),
        ));
    }
    match parse_and_validate(signal) {
        Ok(_) => Ok(()),
        // Reserved type: envelope is valid; degrade happens inbound, not a reject.
        Err(AutoTradeError::Degrade(super::DegradeReason::TypeDegrade)) => Ok(()),
        Err(e) => Err(e),
    }
}

/// Canonicalize a signal to a single-line JSON string (used to append the
/// `autotrade:` line after `signalTime` stamping).
pub fn canonical_json(signal: &AutoTradeSignal) -> Result<String, AutoTradeError> {
    serde_json::to_string(signal)
        .map_err(|e| AutoTradeError::Reject(format!("failed to serialize signal: {e}")))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dex_signal(params: serde_json::Value) -> AutoTradeSignal {
        AutoTradeSignal {
            schema_version: 1,
            delivery_id: "d-abc123".to_string(),
            signal_type: SignalType::DexTrade,
            signal_time: 1_700_000_000_000,
            ttl_sec: 60,
            params,
        }
    }

    #[test]
    fn ac2_charset_lock_rejects_shell_metachars() {
        for bad in ["0xabc;rm", "0xabc&&x", "0x$(id)", "0xabc --slippage 99"] {
            let sig = dex_signal(serde_json::json!({
                "chainIndex": "8453", "tokenAddress": bad,
                "side": "buy", "amount": "10", "amountUnit": "quote"
            }));
            assert!(
                matches!(validate_structure(&sig), Err(AutoTradeError::Reject(_))),
                "should reject {bad}"
            );
        }
    }

    // ── AC-2: whitelist rejects glob / brace / tilde expansion chars ──
    #[test]
    fn ac2_charset_lock_rejects_glob_brace_tilde() {
        // Chars a shell would expand (glob `* ? [ ]`, brace `{ }`, `~`) that the old
        // metacharacter blacklist let through — all must now be rejected.
        for bad in [
            "0x{a,b}", "0x*", "0x?", "~", "0x[ab]", "*", "{a,b}", "0x~1", "a[0-9]",
        ] {
            let sig = dex_signal(serde_json::json!({
                "chainIndex": "8453", "tokenAddress": bad,
                "side": "buy", "amount": "10", "amountUnit": "quote"
            }));
            assert!(
                matches!(validate_structure(&sig), Err(AutoTradeError::Reject(_))),
                "should reject tokenAddress={bad}"
            );
        }
        // A plain hex address still passes the whitelist.
        let ok = dex_signal(serde_json::json!({
            "chainIndex": "8453", "tokenAddress": "0xAbc123_DEF-456",
            "side": "buy", "amount": "10", "amountUnit": "quote"
        }));
        assert!(validate_structure(&ok).is_ok());
    }

    #[test]
    fn delivery_id_illegal_char_rejected() {
        let mut sig = dex_signal(serde_json::json!({
            "chainIndex": "8453", "tokenAddress": "0xabc",
            "side": "buy", "amount": "10", "amountUnit": "quote"
        }));
        sig.delivery_id = "bad id".to_string();
        let err = validate_structure(&sig).unwrap_err();
        assert!(format!("{err}").contains("deliveryId contains illegal character"));
    }

    #[test]
    fn unit_by_side_dex() {
        // buy+base rejected
        let sig = dex_signal(serde_json::json!({
            "chainIndex": "8453", "tokenAddress": "0xabc",
            "side": "buy", "amount": "10", "amountUnit": "base"
        }));
        assert!(validate_structure(&sig).is_err());
        // buy+pct rejected
        let sig = dex_signal(serde_json::json!({
            "chainIndex": "8453", "tokenAddress": "0xabc",
            "side": "buy", "amount": "10", "amountUnit": "pct"
        }));
        assert!(validate_structure(&sig).is_err());
        // sell+pct accepted
        let sig = dex_signal(serde_json::json!({
            "chainIndex": "8453", "tokenAddress": "0xabc",
            "side": "sell", "amount": "25", "amountUnit": "pct"
        }));
        assert!(validate_structure(&sig).is_ok());
        // sell+quote rejected
        let sig = dex_signal(serde_json::json!({
            "chainIndex": "8453", "tokenAddress": "0xabc",
            "side": "sell", "amount": "25", "amountUnit": "quote"
        }));
        assert!(validate_structure(&sig).is_err());
    }

    #[test]
    fn ttl_bounds() {
        let mut sig = dex_signal(serde_json::json!({
            "chainIndex": "8453", "tokenAddress": "0xabc",
            "side": "buy", "amount": "10", "amountUnit": "quote"
        }));
        sig.ttl_sec = 0;
        assert!(validate_structure(&sig).is_err());
        sig.ttl_sec = 86_401;
        assert!(validate_structure(&sig).is_err());
        sig.ttl_sec = 86_400;
        assert!(validate_structure(&sig).is_ok());
    }

    #[test]
    fn slippage_cap_enforced() {
        let sig = dex_signal(serde_json::json!({
            "chainIndex": "8453", "tokenAddress": "0xabc",
            "side": "buy", "amount": "10", "amountUnit": "quote",
            "slippageBps": MAX_SLIPPAGE_BPS + 1
        }));
        assert!(validate_structure(&sig).is_err());
        let sig = dex_signal(serde_json::json!({
            "chainIndex": "8453", "tokenAddress": "0xabc",
            "side": "buy", "amount": "10", "amountUnit": "quote",
            "slippageBps": MAX_SLIPPAGE_BPS
        }));
        assert!(validate_structure(&sig).is_ok());
    }

    #[test]
    fn enabled_and_reserved_types() {
        assert!(SignalType::DexTrade.is_enabled());
        assert!(!SignalType::DefiRebalance.is_enabled()); // demoted to reserved for v1
        assert!(SignalType::Polymarket.is_enabled());
        assert!(!SignalType::HyperliquidPerp.is_enabled());
        assert!(!SignalType::MemeLaunch.is_enabled());
        assert!(!SignalType::LimitOrder.is_enabled());
        assert!(!SignalType::GmxPerp.is_enabled());
    }

    #[test]
    fn schema_version_newer_than_supported_is_rejected() {
        let mut sig = dex_signal(serde_json::json!({
            "chainIndex": "8453", "tokenAddress": "0xabc",
            "side": "buy", "amount": "10", "amountUnit": "quote"
        }));
        sig.schema_version = SCHEMA_VERSION + 1;
        assert!(matches!(
            validate_structure(&sig),
            Err(AutoTradeError::Degrade(
                super::super::DegradeReason::SchemaVersionTooNew
            ))
        ));
        // current version still passes structure validation
        sig.schema_version = SCHEMA_VERSION;
        assert!(validate_structure(&sig).is_ok());
    }

    #[test]
    fn reserved_type_passes_structure_but_degrades_on_parse() {
        let sig = AutoTradeSignal {
            schema_version: 1,
            delivery_id: "d1".to_string(),
            signal_type: SignalType::GmxPerp,
            signal_time: 1,
            ttl_sec: 60,
            params: serde_json::json!({}),
        };
        // envelope-valid outbound
        assert!(validate_structure(&sig).is_ok());
        // but parse_and_validate degrades
        assert!(matches!(
            parse_and_validate(&sig),
            Err(AutoTradeError::Degrade(
                super::super::DegradeReason::TypeDegrade
            ))
        ));
    }

    #[test]
    fn deny_unknown_fields_envelope() {
        let raw = r#"{"schemaVersion":1,"deliveryId":"d1","signalType":"dex_trade","signalTime":1,"ttlSec":60,"params":{},"extra":true}"#;
        let parsed: Result<AutoTradeSignal, _> = serde_json::from_str(raw);
        assert!(parsed.is_err(), "unknown envelope field must be rejected");
    }

    #[test]
    fn deny_unknown_fields_params() {
        let sig = dex_signal(serde_json::json!({
            "chainIndex": "8453", "tokenAddress": "0xabc",
            "side": "buy", "amount": "10", "amountUnit": "quote",
            "bogus": 1
        }));
        assert!(
            validate_structure(&sig).is_err(),
            "unknown params field must be rejected"
        );
    }

    #[test]
    fn defi_and_polymarket_unit_rules() {
        // defi_rebalance is demoted to reserved for v1: its envelope passes structure but
        // parse_and_validate degrades (notify-only), so per-type unit rules no longer apply.
        let sig = AutoTradeSignal {
            schema_version: 1,
            delivery_id: "d1".into(),
            signal_type: SignalType::DefiRebalance,
            signal_time: 1,
            ttl_sec: 60,
            params: serde_json::json!({"protocolProductId":"p1","action":"deposit","amount":"5","amountUnit":"pct","tokenAddress":"0xabc","chainIndex":"8453"}),
        };
        assert!(validate_structure(&sig).is_ok()); // reserved → envelope-valid, degrades inbound
        assert!(matches!(
            parse_and_validate(&sig),
            Err(AutoTradeError::Degrade(
                super::super::DegradeReason::TypeDegrade
            ))
        ));
        // polymarket sell + quote rejected (still an enabled type)
        let sig = AutoTradeSignal {
            schema_version: 1,
            delivery_id: "d1".into(),
            signal_type: SignalType::Polymarket,
            signal_time: 1,
            ttl_sec: 60,
            params: serde_json::json!({"conditionId":"c1","outcome":"Yes","side":"sell","amount":"5","amountUnit":"quote"}),
        };
        assert!(validate_structure(&sig).is_err());
    }

    // ── FR-1: polymarket maxPriceCents bound (0 < cents <= 100) ──
    #[test]
    fn polymarket_max_price_cents_bounds() {
        let mk = |cents: serde_json::Value| AutoTradeSignal {
            schema_version: 1,
            delivery_id: "d1".into(),
            signal_type: SignalType::Polymarket,
            signal_time: 1,
            ttl_sec: 60,
            params: serde_json::json!({
                "conditionId": "c1", "outcome": "Yes", "side": "buy",
                "amount": "10", "amountUnit": "quote", "maxPriceCents": cents
            }),
        };
        // 0 rejected (meaningless), 101 rejected (> $1.00).
        assert!(validate_structure(&mk(serde_json::json!(0))).is_err());
        assert!(validate_structure(&mk(serde_json::json!(101))).is_err());
        // 100 ok (boundary == $1.00), 55 ok.
        assert!(validate_structure(&mk(serde_json::json!(100))).is_ok());
        assert!(validate_structure(&mk(serde_json::json!(55))).is_ok());
        // absent ok (optional field).
        let absent = AutoTradeSignal {
            schema_version: 1,
            delivery_id: "d1".into(),
            signal_type: SignalType::Polymarket,
            signal_time: 1,
            ttl_sec: 60,
            params: serde_json::json!({
                "conditionId": "c1", "outcome": "Yes", "side": "buy",
                "amount": "10", "amountUnit": "quote"
            }),
        };
        assert!(validate_structure(&absent).is_ok());
    }
}
