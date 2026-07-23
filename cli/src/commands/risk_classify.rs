//! Shared risk-classification module (spec §1.6).
//!
//! Single source of truth for the trade-direction parsing, riskLevel
//! normalization, riskLevel×direction action matrix, `combinedAction` reduction
//! (SEC — `security token-scan`), and the per-route swap risk matrix
//! (SW2 — `swap quote` / `swap swap`). Both `security.rs` and `swap.rs` consume
//! this module so the two matrices live in one tested place, avoiding the
//! two-copy maintenance drift flagged in PRD §3.3.
//!
//! ## TBC checkpoint
//!
//! This module is also the requirement's To-Be-Confirmed checkpoint. All items
//! are recorded here; only the taxRate unit (TBC[4]) gates behavior in this file,
//! and it is isolated behind the [`normalize_tax_rate`] seam.
//!
//! TBC[1]: was `--side` ever released? assumption=no → no hidden alias needed (§1.1).
//! TBC[2]: can the buy-side from-token price fetch reuse the decimals/symbol call
//!         to avoid an extra round-trip? assumption=separate call (§1.2 / §6.3).
//! TBC[3]: does backend 100010 carry the real minimum amount? assumption=no, use
//!         local `ceil(1/price)` (§1.5).
//! TBC[4]: BLOCKER — is backend `taxRate` a percentage (15.0 = 15%) or a decimal
//!         (0.15 = 15%)? Until confirmed the `> 10.0` tax branch is NOT functional;
//!         all normalization is funneled through [`normalize_tax_rate`] so the fix
//!         lands in exactly one place (§2.4, finalized by T-tax).
//! TBC[5]: do strict-schema swap consumers exist that would reject the new
//!         `action` / `reason` fields? assumption=no (§10.1).

use serde_json::Value;

/// Trade direction supplied by `security token-scan --trade-direction`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TradeDirection {
    Buy,
    Sell,
}

impl TradeDirection {
    /// Lower-case wire form echoed back in the SEC `tradeDirection` field.
    pub fn as_str(&self) -> &'static str {
        match self {
            TradeDirection::Buy => "buy",
            TradeDirection::Sell => "sell",
        }
    }
}

/// Normalized per-token risk level (§2.1). Missing/unknown backend values map to
/// [`RiskLevel::High`] (the conservative default).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RiskLevel {
    Critical,
    High,
    Medium,
    Low,
}

impl RiskLevel {
    /// Upper-case wire form emitted in the SEC `normalizedRiskLevel` field.
    pub fn as_str(&self) -> &'static str {
        match self {
            RiskLevel::Critical => "CRITICAL",
            RiskLevel::High => "HIGH",
            RiskLevel::Medium => "MEDIUM",
            RiskLevel::Low => "LOW",
        }
    }
}

/// SEC per-token / combined action (§2.1). Severity order: block > pause > warn > safe.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    Block,
    Pause,
    Warn,
    Safe,
}

impl Action {
    /// Wire form emitted in the SEC per-token `action` / top-level `combinedAction`.
    pub fn as_str(&self) -> &'static str {
        match self {
            Action::Block => "block",
            Action::Pause => "pause",
            Action::Warn => "warn",
            Action::Safe => "safe",
        }
    }

    /// Higher = stricter. Drives the [`combined_action`] reduction.
    pub fn severity(&self) -> u8 {
        match self {
            Action::Block => 3,
            Action::Pause => 2,
            Action::Warn => 1,
            Action::Safe => 0,
        }
    }
}

/// SW2 per-route action (§2.4). Distinct from [`Action`] because the swap matrix
/// has no `pause` / `safe` — only `block` / `warn` / `ok`. Severity: block > warn > ok.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SwapAction {
    Block,
    Warn,
    Ok,
}

impl SwapAction {
    /// Wire form emitted in the per-route `action` field.
    pub fn as_str(&self) -> &'static str {
        match self {
            SwapAction::Block => "block",
            SwapAction::Warn => "warn",
            SwapAction::Ok => "ok",
        }
    }

    /// Higher = stricter. Drives the per-route buy/sell-side merge.
    pub fn severity(&self) -> u8 {
        match self {
            SwapAction::Block => 2,
            SwapAction::Warn => 1,
            SwapAction::Ok => 0,
        }
    }

    /// Return whichever of `self` / `other` is stricter (higher severity).
    fn stricter(self, other: SwapAction) -> SwapAction {
        if other.severity() > self.severity() {
            other
        } else {
            self
        }
    }
}

/// A classified view over one backend token `Value` — exposes the normalized
/// risk level, whether it is a native coin, and the resolved per-token action.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TokenResult {
    normalized_risk_level: RiskLevel,
    is_native: bool,
    action: Action,
}

impl TokenResult {
    /// Classify a single backend token object for the given trade direction.
    /// Reads `riskLevel` (missing/unknown → `HIGH` per §2.1) and derives
    /// `is_native` from the absence of a non-empty contract address.
    pub fn classify(token: &Value, direction: TradeDirection) -> Self {
        let normalized_risk_level = normalize_risk_level(token["riskLevel"].as_str());
        let is_native = token_is_native(token);
        let action = resolve_action(normalized_risk_level, direction);
        Self {
            normalized_risk_level,
            is_native,
            action,
        }
    }

    pub fn normalized_risk_level(&self) -> RiskLevel {
        self.normalized_risk_level
    }

    pub fn is_native(&self) -> bool {
        self.is_native
    }

    pub fn action(&self) -> Action {
        self.action
    }
}

/// Candidate JSON keys carrying a token's contract address. The portfolio/balance
/// path uses `tokenContractAddress`; the direct token-scan request uses
/// `contractAddress`.
const ADDRESS_KEYS: [&str; 2] = ["tokenContractAddress", "contractAddress"];

/// A token is native when it has no non-empty contract address under any known key.
fn token_is_native(token: &Value) -> bool {
    !ADDRESS_KEYS.iter().any(|key| {
        let addr = token[*key].as_str().unwrap_or("");
        !addr.trim().is_empty()
    })
}

/// Parse a `--trade-direction` value: `buy` / `sell` (case-insensitive). Usable
/// directly as a clap `value_parser`. Rejects anything else.
pub fn parse_trade_direction_value(raw: &str) -> Result<TradeDirection, String> {
    match raw.trim().to_ascii_lowercase().as_str() {
        "buy" => Ok(TradeDirection::Buy),
        "sell" => Ok(TradeDirection::Sell),
        other => Err(format!(
            "invalid trade direction '{other}'; expected 'buy' or 'sell'"
        )),
    }
}

/// Map a backend `riskLevel` string to [`RiskLevel`]. Missing (`None`), null, or
/// unrecognized values default to [`RiskLevel::High`] (§2.1 per-token field rule).
pub fn normalize_risk_level(raw: Option<&str>) -> RiskLevel {
    match raw.map(str::to_ascii_uppercase).as_deref() {
        Some("CRITICAL") => RiskLevel::Critical,
        Some("HIGH") => RiskLevel::High,
        Some("MEDIUM") => RiskLevel::Medium,
        Some("LOW") => RiskLevel::Low,
        _ => RiskLevel::High,
    }
}

/// The riskLevel × tradeDirection action matrix (§2.1).
pub fn resolve_action(risk: RiskLevel, direction: TradeDirection) -> Action {
    match (risk, direction) {
        (RiskLevel::Critical, TradeDirection::Buy) => Action::Block,
        (RiskLevel::Critical, TradeDirection::Sell) => Action::Warn,
        (RiskLevel::High, TradeDirection::Buy) => Action::Pause,
        (RiskLevel::High, TradeDirection::Sell) => Action::Warn,
        (RiskLevel::Medium, TradeDirection::Buy) => Action::Warn,
        (RiskLevel::Medium, TradeDirection::Sell) => Action::Warn,
        (RiskLevel::Low, TradeDirection::Buy) => Action::Safe,
        (RiskLevel::Low, TradeDirection::Sell) => Action::Safe,
    }
}

/// The strictest [`Action`] among all **non-native** tokens; [`Action::Safe`] when
/// there are no non-native tokens (§2.1 `combinedAction`).
pub fn combined_action(tokens: &[TokenResult]) -> Action {
    tokens
        .iter()
        .filter(|token| !token.is_native())
        .map(TokenResult::action)
        .max_by_key(Action::severity)
        .unwrap_or(Action::Safe)
}

/// TBC[4] normalization seam for the SW2 taxRate comparison.
///
/// `classify_swap_route` compares `normalize_tax_rate(raw_tax) > 10.0`, so the
/// eventual percentage-vs-decimal fix lands in exactly one place.
pub fn normalize_tax_rate(raw: f64) -> f64 {
    // TBC[4]: backend taxRate unit unconfirmed (percentage vs decimal). Until
    // confirmed this returns raw; the >10.0 branch is NOT functional. Finalized by T-tax.
    raw
}

/// Classify one swap route in place, appending `action` (`ok` / `warn` / `block`)
/// and `reason` (semicolon-joined, deduped; empty when `ok`) per §2.4.
///
/// Buy side = to-token (`toToken`), sell side = from-token (`fromToken`). The
/// honeypot branch is functional; the taxRate branch is routed through the
/// [`normalize_tax_rate`] seam and is NOT functional until TBC[4] is confirmed.
/// (See T-sw2 for how routes are enumerated from the quote response.)
pub fn classify_swap_route(route: &mut Value) {
    let (buy_action, mut reasons) = classify_swap_side(route.get("toToken"), true);
    let (sell_action, sell_reasons) = classify_swap_side(route.get("fromToken"), false);
    reasons.extend(sell_reasons);

    let action = buy_action.stricter(sell_action);
    let reason = join_dedup(&reasons);

    if let Some(obj) = route.as_object_mut() {
        obj.insert("action".to_string(), Value::from(action.as_str()));
        obj.insert("reason".to_string(), Value::from(reason));
    }
}

/// Classify the honeypot + tax signals for one side of a route.
/// `is_buy` selects the to-token (buy) vs from-token (sell) message + severity.
fn classify_swap_side(token: Option<&Value>, is_buy: bool) -> (SwapAction, Vec<String>) {
    let mut action = SwapAction::Ok;
    let mut reasons: Vec<String> = Vec::new();

    let Some(token) = token else {
        return (action, reasons);
    };

    // Honeypot — functional, TBC[4]-independent.
    if token["isHoneyPot"].as_bool().unwrap_or(false) {
        let (side_action, reason) = if is_buy {
            (SwapAction::Block, "to-token is a honeypot")
        } else {
            (SwapAction::Warn, "from-token is a honeypot; exit allowed")
        };
        action = action.stricter(side_action);
        reasons.push(reason.to_string());
    }

    // Tax rate — TBC[4] seam. NOT functional until `normalize_tax_rate` is finalized.
    let tax_over_threshold = token["taxRate"]
        .as_f64()
        .map(|raw_tax| normalize_tax_rate(raw_tax) > 10.0)
        .unwrap_or(false);
    if tax_over_threshold {
        let reason = if is_buy {
            "to-token tax rate exceeds 10%"
        } else {
            "from-token tax rate exceeds 10%"
        };
        action = action.stricter(SwapAction::Warn);
        reasons.push(reason.to_string());
    }

    (action, reasons)
}

/// Join reasons with `;`, dropping later duplicates while preserving first-seen order.
fn join_dedup(reasons: &[String]) -> String {
    let mut seen: Vec<&str> = Vec::new();
    for reason in reasons {
        let s = reason.as_str();
        if !seen.contains(&s) {
            seen.push(s);
        }
    }
    seen.join(";")
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// Build a classified token from a riskLevel + contract address + direction.
    fn tok(risk: &str, addr: &str, dir: TradeDirection) -> TokenResult {
        TokenResult::classify(
            &json!({"riskLevel": risk, "tokenContractAddress": addr}),
            dir,
        )
    }

    // ── parse_trade_direction_value ────────────────────────────────────

    #[test]
    fn parse_trade_direction_accepts_buy_case_insensitive() {
        assert_eq!(parse_trade_direction_value("buy"), Ok(TradeDirection::Buy));
        assert_eq!(parse_trade_direction_value("BUY"), Ok(TradeDirection::Buy));
    }

    #[test]
    fn parse_trade_direction_accepts_sell_case_insensitive() {
        assert_eq!(
            parse_trade_direction_value("sell"),
            Ok(TradeDirection::Sell)
        );
        assert_eq!(
            parse_trade_direction_value("Sell"),
            Ok(TradeDirection::Sell)
        );
    }

    #[test]
    fn parse_trade_direction_rejects_other_values() {
        assert!(parse_trade_direction_value("hold").is_err());
        assert!(parse_trade_direction_value("").is_err());
    }

    // ── normalize_risk_level ───────────────────────────────────────────

    #[test]
    fn normalize_risk_level_maps_known_levels() {
        assert_eq!(normalize_risk_level(Some("CRITICAL")), RiskLevel::Critical);
        // Case-insensitive: `high` upper-cases to HIGH, which also equals the
        // unknown-case default of HIGH — either policy yields High here.
        assert_eq!(normalize_risk_level(Some("high")), RiskLevel::High);
        assert_eq!(normalize_risk_level(Some("MEDIUM")), RiskLevel::Medium);
        assert_eq!(normalize_risk_level(Some("low")), RiskLevel::Low);
    }

    #[test]
    fn normalize_risk_level_defaults_missing_and_unknown_to_high() {
        assert_eq!(normalize_risk_level(None), RiskLevel::High);
        assert_eq!(normalize_risk_level(Some("weird")), RiskLevel::High);
    }

    // ── resolve_action — full 4×2 matrix ───────────────────────────────

    #[test]
    fn resolve_action_full_matrix() {
        use Action::*;
        use RiskLevel::*;
        use TradeDirection::*;
        assert_eq!(resolve_action(Critical, Buy), Block);
        assert_eq!(resolve_action(Critical, Sell), Warn);
        assert_eq!(resolve_action(High, Buy), Pause);
        assert_eq!(resolve_action(High, Sell), Warn);
        assert_eq!(resolve_action(Medium, Buy), Warn);
        assert_eq!(resolve_action(Medium, Sell), Warn);
        assert_eq!(resolve_action(Low, Buy), Safe);
        assert_eq!(resolve_action(Low, Sell), Safe);
    }

    // ── combined_action ────────────────────────────────────────────────

    #[test]
    fn combined_action_picks_strictest_non_native() {
        // pause + block among non-native tokens; the native critical token is excluded.
        let tokens = vec![
            tok("HIGH", "0xaaa", TradeDirection::Buy),
            tok("CRITICAL", "0xbbb", TradeDirection::Buy),
            tok("CRITICAL", "", TradeDirection::Buy),
        ];
        assert_eq!(combined_action(&tokens), Action::Block);
    }

    #[test]
    fn combined_action_empty_is_safe() {
        assert_eq!(combined_action(&[]), Action::Safe);
    }

    #[test]
    fn combined_action_single_native_block_token_is_excluded() {
        // A native token (empty contract address) that would classify as `block`.
        let native_block = tok("CRITICAL", "", TradeDirection::Buy);
        assert!(native_block.is_native());
        assert_eq!(native_block.action(), Action::Block);
        assert_eq!(
            combined_action(std::slice::from_ref(&native_block)),
            Action::Safe
        );
    }

    #[test]
    fn contract_address_key_marks_token_non_native() {
        // The alternate `contractAddress` key also counts as a contract address.
        let token = TokenResult::classify(
            &json!({"riskLevel": "LOW", "contractAddress": "0xccc"}),
            TradeDirection::Buy,
        );
        assert!(!token.is_native());
        assert_eq!(combined_action(std::slice::from_ref(&token)), Action::Safe);
    }

    // ── classify_swap_route (honeypot only — TBC[4]-independent) ────────

    #[test]
    fn classify_swap_route_to_token_honeypot_blocks() {
        let mut route = json!({
            "toToken": { "isHoneyPot": true },
            "fromToken": { "isHoneyPot": false }
        });
        classify_swap_route(&mut route);
        assert_eq!(route["action"], json!("block"));
        assert!(route["reason"]
            .as_str()
            .unwrap()
            .contains("to-token is a honeypot"));
    }

    #[test]
    fn classify_swap_route_from_token_honeypot_warns() {
        let mut route = json!({
            "toToken": { "isHoneyPot": false },
            "fromToken": { "isHoneyPot": true }
        });
        classify_swap_route(&mut route);
        assert_eq!(route["action"], json!("warn"));
        assert!(route["reason"].as_str().unwrap().contains("exit allowed"));
    }

    #[test]
    fn classify_swap_route_both_honeypot_stricter_wins_and_joins() {
        let mut route = json!({
            "toToken": { "isHoneyPot": true },
            "fromToken": { "isHoneyPot": true }
        });
        classify_swap_route(&mut route);
        // block (buy side) beats warn (sell side).
        assert_eq!(route["action"], json!("block"));
        let reason = route["reason"].as_str().unwrap();
        assert!(reason.contains("to-token is a honeypot"));
        assert!(reason.contains("from-token is a honeypot; exit allowed"));
        assert!(reason.contains(';'));
    }

    #[test]
    fn classify_swap_route_no_signals_is_ok_with_empty_reason() {
        let mut route = json!({
            "toToken": { "isHoneyPot": false },
            "fromToken": { "isHoneyPot": false }
        });
        classify_swap_route(&mut route);
        assert_eq!(route["action"], json!("ok"));
        assert_eq!(route["reason"], json!(""));
    }

    #[test]
    fn classify_swap_route_missing_token_objects_is_ok() {
        let mut route = json!({});
        classify_swap_route(&mut route);
        assert_eq!(route["action"], json!("ok"));
        assert_eq!(route["reason"], json!(""));
    }

    #[test]
    fn classify_swap_route_is_idempotent() {
        let mut route = json!({
            "toToken": { "isHoneyPot": true },
            "fromToken": { "isHoneyPot": false }
        });
        classify_swap_route(&mut route);
        let first = route["reason"].as_str().unwrap().to_string();
        classify_swap_route(&mut route);
        assert_eq!(route["reason"].as_str().unwrap(), first);
        assert_eq!(route["action"], json!("block"));
    }

    // ── join_dedup ─────────────────────────────────────────────────────

    #[test]
    fn join_dedup_removes_duplicates_preserving_order() {
        let reasons = vec!["a".to_string(), "b".to_string(), "a".to_string()];
        assert_eq!(join_dedup(&reasons), "a;b");
    }

    #[test]
    fn join_dedup_empty_is_empty_string() {
        assert_eq!(join_dedup(&[]), "");
    }

    // ── enum as_str / severity ─────────────────────────────────────────

    #[test]
    fn action_as_str_and_severity_ordering() {
        assert_eq!(Action::Block.as_str(), "block");
        assert_eq!(Action::Pause.as_str(), "pause");
        assert_eq!(Action::Warn.as_str(), "warn");
        assert_eq!(Action::Safe.as_str(), "safe");
        assert!(Action::Block.severity() > Action::Pause.severity());
        assert!(Action::Pause.severity() > Action::Warn.severity());
        assert!(Action::Warn.severity() > Action::Safe.severity());
    }

    #[test]
    fn swap_action_as_str_and_severity_ordering() {
        assert_eq!(SwapAction::Block.as_str(), "block");
        assert_eq!(SwapAction::Warn.as_str(), "warn");
        assert_eq!(SwapAction::Ok.as_str(), "ok");
        assert!(SwapAction::Block.severity() > SwapAction::Warn.severity());
        assert!(SwapAction::Warn.severity() > SwapAction::Ok.severity());
    }

    #[test]
    fn risk_level_as_str() {
        assert_eq!(RiskLevel::Critical.as_str(), "CRITICAL");
        assert_eq!(RiskLevel::High.as_str(), "HIGH");
        assert_eq!(RiskLevel::Medium.as_str(), "MEDIUM");
        assert_eq!(RiskLevel::Low.as_str(), "LOW");
    }

    #[test]
    fn trade_direction_as_str() {
        assert_eq!(TradeDirection::Buy.as_str(), "buy");
        assert_eq!(TradeDirection::Sell.as_str(), "sell");
    }
}
