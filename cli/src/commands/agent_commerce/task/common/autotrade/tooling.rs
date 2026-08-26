//! Subscription-time execution-tool preflight (`autoTradePreflight`).
//!
//! Deterministic, local, non-networked classification of a service description
//! into a bounded `AssetClass` set, plus a local inventory of candidate tools and
//! a deterministic post-selection Trade Kit probe directive. Attached to every
//! `asp-match` service (see [`super::super::user::asp_ops`]).
//!
//! Safety model (FR-5 / org GR_RESTRICTED_FILE_ACCESS, GR_DATA_LEAKAGE): readiness
//! probes inspect ONLY presence of skill dirs and executables — never credential
//! or configuration state. An installed Trade Kit therefore requires a bounded
//! local compatibility probe after selection. `evidence[]` carries only fixed
//! diagnostic codes, never raw description text.
//!
//! This module is ORTHOGONAL to [`super::schema::SignalType`] (the venue/wire
//! taxonomy). The two are never conflated; a `SignalType → AssetClass` bridge is a
//! follow-up runtime concern (FR-8, see [`build_preflight_from_classes`]).

use std::path::Path;

use serde::{Deserialize, Serialize};

// The repo-wide asset-class taxonomy is defined exactly once at crate root
// (`crate::asset_class::AssetClass`, FR-1.4 / NFR-6 / AC-23). This module re-uses
// that single definition — including its stable `ORDER` and `as_str` — rather than
// declaring its own.
use crate::asset_class::AssetClass;

// ── Chinese keyword literals (as `\u{}` escapes) ──────────────────────────
//
// Localizable strings are kept as escape sequences so they never appear as raw
// CJK bytes in source (repo `onchainos_check.sh` Gate 12 / no-CJK lint), while
// still decoding to the real characters at runtime for matching + display.

// Canonical signal headers (matched case-insensitively against the lowercased desc).
// Comments give the ASCII gloss of each escaped CJK literal (no raw CJK in source).
const CN_HDR_SPOT: &str = "\u{3010}\u{73b0}\u{8d27}\u{4fe1}\u{53f7}\u{3011}"; // [Spot Signal]
const CN_HDR_PERP: &str = "\u{3010}\u{5408}\u{7ea6}\u{4fe1}\u{53f7}\u{3011}"; // [Futures/Perp Signal]
const CN_HDR_PREDICTION: &str = "\u{3010}\u{9884}\u{6d4b}\u{5e02}\u{573a}\u{4fe1}\u{53f7}\u{3011}"; // [Prediction Market Signal]
const CN_HDR_OPTION: &str = "\u{3010}\u{671f}\u{6743}\u{4fe1}\u{53f7}\u{3011}"; // [Option Signal]
const CN_HDR_DEFI: &str = "\u{3010}defi \u{4fe1}\u{53f7}\u{3011}"; // [DeFi Signal]

// Class-semantic keywords.
const CN_SPOT: &str = "\u{73b0}\u{8d27}"; // spot
const CN_PERP: &str = "\u{5408}\u{7ea6}"; // futures/contract
const CN_PERPETUAL: &str = "\u{6c38}\u{7eed}"; // perpetual
const CN_FUTURES: &str = "\u{671f}\u{8d27}"; // futures
const CN_LEVERAGE: &str = "\u{6760}\u{6746}"; // leverage
const CN_TRADING_PAIR: &str = "\u{4ea4}\u{6613}\u{5bf9}"; // trading pair
const CN_PREDICTION: &str = "\u{9884}\u{6d4b}"; // prediction
const CN_OPTION: &str = "\u{671f}\u{6743}"; // option
const CN_STAKE: &str = "\u{8d28}\u{62bc}"; // stake
const CN_LIQUIDITY: &str = "\u{6d41}\u{52a8}\u{6027}"; // liquidity

// Actionable / signal-semantic keywords.
const CN_SIGNAL: &str = "\u{4fe1}\u{53f7}"; // signal
const CN_ENTRY: &str = "\u{5165}\u{573a}"; // entry
const CN_STOP_LOSS: &str = "\u{6b62}\u{635f}"; // stop-loss
const CN_TAKE_PROFIT: &str = "\u{6b62}\u{76c8}"; // take-profit
const CN_POSITION: &str = "\u{4ed3}\u{4f4d}"; // position
const CN_LONG: &str = "\u{505a}\u{591a}"; // long
const CN_SHORT: &str = "\u{505a}\u{7a7a}"; // short
const CN_BUY: &str = "\u{4e70}\u{5165}"; // buy
const CN_SELL: &str = "\u{5356}\u{51fa}"; // sell
const CN_PUSH: &str = "\u{63a8}\u{9001}"; // push
const CN_COPY: &str = "\u{8ddf}\u{5355}"; // copy-trade/follow

// Negation / read-only markers.
const CN_NEG_NO_TRADE_SIGNAL: &str = "\u{4e0d}\u{63d0}\u{4f9b}\u{4ea4}\u{6613}\u{4fe1}\u{53f7}"; // does not provide trading signals
const CN_NEG_NO_SIGNAL: &str = "\u{4e0d}\u{63d0}\u{4f9b}\u{4fe1}\u{53f7}"; // does not provide signals
const CN_NEG_ONLY_QUOTE: &str = "\u{4ec5}\u{63d0}\u{4f9b}\u{884c}\u{60c5}"; // only provides quotes/market data

// ── Types ─────────────────────────────────────────────────────────────────

/// Candidate execution tool tokens (stable strings — part of the data contract).
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum ExecutionTool {
    Onchainos,
    TradeKit,
    PolymarketPlugin,
    HyperliquidPlugin,
}

impl ExecutionTool {
    /// Canonical iteration order (matches enum declaration).
    const ALL: [ExecutionTool; 4] = [
        ExecutionTool::Onchainos,
        ExecutionTool::TradeKit,
        ExecutionTool::PolymarketPlugin,
        ExecutionTool::HyperliquidPlugin,
    ];

    /// Stable wire string (matches serde `snake_case`).
    pub fn token(self) -> &'static str {
        match self {
            ExecutionTool::Onchainos => "onchainos",
            ExecutionTool::TradeKit => "trade_kit",
            ExecutionTool::PolymarketPlugin => "polymarket_plugin",
            ExecutionTool::HyperliquidPlugin => "hyperliquid_plugin",
        }
    }

    /// Human-facing label.
    pub fn display_name(self) -> &'static str {
        match self {
            ExecutionTool::Onchainos => "OnchainOS",
            ExecutionTool::TradeKit => "Trade Kit",
            ExecutionTool::PolymarketPlugin => "Polymarket",
            ExecutionTool::HyperliquidPlugin => "Hyperliquid",
        }
    }

    /// Installable skill/plugin id, if this is a plugin (native tools return `None`).
    pub fn plugin_id(self) -> Option<&'static str> {
        match self {
            ExecutionTool::PolymarketPlugin => Some("polymarket-plugin"),
            ExecutionTool::HyperliquidPlugin => Some("hyperliquid-plugin"),
            ExecutionTool::Onchainos | ExecutionTool::TradeKit => None,
        }
    }
}

/// Local readiness of an execution tool.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Readiness {
    Ready,
    Missing,
    VerificationUnknown,
    Incompatible,
}

/// Stable local reason. This is intentionally narrower than runtime reasons:
/// matching never performs authentication, capability, or network checks.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ToolReadinessReason {
    Ready,
    CliMissing,
    PluginMissing,
    LocalCompatibilityNotChecked,
    Incompatible,
}

/// The kind of a reminder (closed set).
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ReminderKind {
    InstallPlugin,
    ChooseAtFirstSignal,
    ReadinessAdvisory,
}

/// Per-candidate-tool readiness row.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ToolStatus {
    pub tool: ExecutionTool,
    pub display_name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub plugin_id: Option<String>,
    pub readiness: Readiness,
    pub reason: ToolReadinessReason,
    pub checked_at: Option<String>,
}

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum TradeKitProbeMode {
    ProbeBeforeConfirmation,
    DeferredUntilVenueSelection,
    NotApplicable,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct TradeKitProbeDirective {
    pub mode: TradeKitProbeMode,
    pub asset_classes: Vec<AssetClass>,
}

/// A bilingual, non-blocking install/config/choose reminder.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct Reminder {
    pub kind: ReminderKind,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool: Option<ExecutionTool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub plugin_id: Option<String>,
    /// All classes that triggered this reminder (merged), stable order.
    pub asset_classes: Vec<AssetClass>,
    /// ALWAYS `false` for this task.
    pub blocking: bool,
    pub message_en: String,
    pub message_zh: String,
}

/// The object attached to each service returned by `asp-match` or
/// `task-service-select`. STABILITY CONTRACT.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct AutoTradePreflight {
    pub schema_version: u8,
    pub is_trading_signal: bool,
    pub asset_classes: Vec<AssetClass>,
    pub explicit_tools: Vec<ExecutionTool>,
    pub selection_required: bool,
    pub advisory_only: bool,
    pub tools: Vec<ToolStatus>,
    pub reminders: Vec<Reminder>,
    pub trade_kit_probe: TradeKitProbeDirective,
    pub evidence: Vec<String>,
}

/// Output of the free-text classifier.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct ClassifyOutcome {
    pub classes: Vec<AssetClass>,
    pub explicit: Vec<ExecutionTool>,
    pub evidence: Vec<String>,
}

/// Snapshot of local tool readiness, built ONCE per `asp-match` invocation.
#[derive(Debug, Clone)]
pub struct ToolInventory {
    onchainos: Readiness,
    trade_kit: Readiness,
    polymarket: Readiness,
    hyperliquid: Readiness,
}

// ── Class → candidate-tool map (FR-4, single source of truth) ─────────────

/// The ONLY copy of the `AssetClass → candidate tools` map.
pub fn candidate_tools(class: AssetClass) -> &'static [ExecutionTool] {
    match class {
        AssetClass::Spot => &[ExecutionTool::Onchainos, ExecutionTool::TradeKit],
        AssetClass::Perp => &[ExecutionTool::HyperliquidPlugin, ExecutionTool::TradeKit],
        AssetClass::Prediction => &[ExecutionTool::PolymarketPlugin, ExecutionTool::TradeKit],
        AssetClass::Option => &[ExecutionTool::TradeKit],
        AssetClass::Defi => &[ExecutionTool::Onchainos],
    }
}

// ── Classifier (FR-2 / FR-3) ──────────────────────────────────────────────

/// Free-text classifier. Returns a stable, de-duplicated class set + evidence.
/// Pure & infallible: undetermined ⇒ empty (never guesses, never errors).
pub fn classify_description(desc: &str) -> ClassifyOutcome {
    if desc.trim().is_empty() {
        return ClassifyOutcome::default();
    }
    let lower = desc.to_lowercase();

    // Hard negation — a direct denial that the service provides any trading
    // signal (e.g. "no trading signals are provided" / its CN equivalent) — fails
    // closed globally, and OUTRANKS a canonical header: an example header such as
    // "[Spot Signal] is only a format example; no trading signals are provided" is
    // a format sample, not a live signal, so the explicit capability denial wins.
    // Non-capability quality claims ("no signal delay" / "no signal loss") are
    // deliberately NOT hard negation (see `has_hard_negation`).
    if has_hard_negation(&lower) {
        return ClassifyOutcome::default();
    }

    // A canonical signal header (e.g. [Spot Signal]) is an explicit trading-signal
    // declaration, so — absent the hard capability denial handled above — a soft
    // read-only / alert marker may not fail it closed.
    let header_any = AssetClass::ORDER.iter().any(|&c| header_present(c, &lower));

    // Soft read-only / alert markers (security alert, risk report, market data
    // only, read-only, ...) describe a pure analytics / data / alert service.
    // They fail closed ONLY for such a pure description — one that neither carries
    // a canonical header nor otherwise declares a trading signal — so an explicit
    // signal in the same description (e.g. "Spot trading signals with BUY entry
    // plus security alerts") is never erased.
    let declares_signal = lower.contains("signal") || lower.contains(CN_SIGNAL);
    if !header_any && !declares_signal && has_soft_readonly(&lower) {
        return ClassifyOutcome::default();
    }

    let tokens: std::collections::HashSet<&str> = lower
        .split(|c: char| !c.is_ascii_alphanumeric())
        .filter(|s| !s.is_empty())
        .collect();

    // Explicit tool names (feed both class semantics and `explicitTools`).
    let names_polymarket = lower.contains("polymarket");
    let names_hyperliquid = lower.contains("hyperliquid");
    let names_trade_kit = lower.contains("trade kit")
        || lower.contains("trade-kit")
        || lower.contains("okx-trade")
        || lower.contains("okx trade")
        || lower.contains("okx cex")
        || lower.contains("okx event");
    let names_onchainos = lower.contains("onchainos") || lower.contains("onchain os");
    let names_dex = tokens.contains("dex");

    let actionable = has_actionable(&lower, &tokens);

    let mut classes: Vec<AssetClass> = Vec::new();
    let mut evidence: Vec<String> = Vec::new();

    for class in AssetClass::ORDER {
        if header_present(class, &lower) {
            classes.push(class);
            evidence.push(format!("{}:header", class.as_str()));
        } else if actionable
            && class_semantic_present(
                class,
                &lower,
                &tokens,
                names_polymarket,
                names_hyperliquid,
                names_dex,
            )
        {
            classes.push(class);
            evidence.push(format!("{}:description", class.as_str()));
        }
    }

    // Scope explicit tools to the candidate set of the recognized classes (FR-3):
    // a named tool may never invent an unrelated class.
    let recognized: std::collections::HashSet<ExecutionTool> = classes
        .iter()
        .flat_map(|c| candidate_tools(*c).iter().copied())
        .collect();
    let named = [
        (ExecutionTool::Onchainos, names_onchainos || names_dex),
        (ExecutionTool::TradeKit, names_trade_kit),
        (ExecutionTool::PolymarketPlugin, names_polymarket),
        (ExecutionTool::HyperliquidPlugin, names_hyperliquid),
    ];
    let mut explicit: Vec<ExecutionTool> = Vec::new();
    for (tool, is_named) in named {
        if is_named && recognized.contains(&tool) {
            explicit.push(tool);
            evidence.push(format!("tool:{}", tool.token()));
        }
    }

    ClassifyOutcome {
        classes,
        explicit,
        evidence,
    }
}

fn header_present(class: AssetClass, lower: &str) -> bool {
    let (cn, en) = match class {
        AssetClass::Spot => (CN_HDR_SPOT, "\u{3010}spot signal\u{3011}"),
        AssetClass::Perp => (CN_HDR_PERP, "\u{3010}futures signal\u{3011}"),
        AssetClass::Prediction => (CN_HDR_PREDICTION, "\u{3010}prediction signal\u{3011}"),
        AssetClass::Option => (CN_HDR_OPTION, "\u{3010}options signal\u{3011}"),
        AssetClass::Defi => (CN_HDR_DEFI, "\u{3010}defi signal\u{3011}"),
    };
    lower.contains(cn) || lower.contains(en)
}

fn class_semantic_present(
    class: AssetClass,
    lower: &str,
    tokens: &std::collections::HashSet<&str>,
    names_polymarket: bool,
    names_hyperliquid: bool,
    names_dex: bool,
) -> bool {
    match class {
        AssetClass::Spot => tokens.contains("spot") || lower.contains(CN_SPOT) || names_dex,
        AssetClass::Perp => {
            // A bare `swap`, and even a single-hyphen `word-swap` such as a DEX
            // "token-swap", is NOT a Perp marker: those are ordinary spot
            // exchanges and must classify as Spot only. Perp requires explicit
            // derivatives context: perp/perpetual/futures (EN) or the CN
            // futures/perpetual keywords (CN_PERPETUAL); a full BASE-QUOTE-SWAP
            // perpetual instrument code (e.g. BTC-USDT-SWAP, detected by
            // `has_perp_swap_instrument`); a named perp venue such as Hyperliquid;
            // or the CN `contract` word ONLY in a trading/derivatives context
            // (`cn_contract_is_perp` — so "smart contract" audit/security text
            // never counts).
            tokens.contains("perp")
                || tokens.contains("perps")
                || tokens.contains("perpetual")
                || tokens.contains("futures")
                || tokens.contains("future")
                || has_perp_swap_instrument(lower)
                || cn_contract_is_perp(lower)
                || lower.contains(CN_PERPETUAL)
                || names_hyperliquid
        }
        AssetClass::Prediction => {
            tokens.contains("prediction")
                || tokens.contains("predictions")
                || lower.contains(CN_PREDICTION)
                || names_polymarket
                || prediction_combo_present(tokens)
        }
        AssetClass::Option => {
            tokens.contains("option")
                || tokens.contains("options")
                || lower.contains(CN_OPTION)
                || option_combo_present(tokens)
        }
        AssetClass::Defi => {
            tokens.contains("defi")
                || tokens.contains("yield")
                || tokens.contains("staking")
                || tokens.contains("stake")
                || tokens.contains("farming")
                || tokens.contains("lp")
                || lower.contains(CN_STAKE)
                || lower.contains(CN_LIQUIDITY)
                || defi_combo_present(lower, tokens)
        }
    }
}

/// True when the description contains a full perpetual instrument code of the
/// shape `BASE-QUOTE-SWAP` (e.g. `BTC-USDT-SWAP`) — the settled marker for a
/// perpetual contract. A bare `swap`, or a single-hyphen `word-swap` (e.g. a DEX
/// `token-swap`, an ordinary spot exchange), is deliberately NOT accepted: only a
/// hyphenated code of three-plus non-empty alphanumeric segments whose final
/// segment is exactly `swap` qualifies. Pure (no I/O) so it is unit-testable.
fn has_perp_swap_instrument(lower: &str) -> bool {
    // Split on any char that is neither ASCII-alphanumeric nor '-', so a code
    // like `btc-usdt-swap` stays a single chunk while a surrounding word such as
    // `token-swap` is isolated by its whitespace/punctuation boundaries.
    lower
        .split(|c: char| !c.is_ascii_alphanumeric() && c != '-')
        .any(|chunk| {
            let segments: Vec<&str> = chunk.split('-').filter(|s| !s.is_empty()).collect();
            segments.len() >= 3 && segments.last() == Some(&"swap")
        })
}

/// The CN word `contract` (`CN_PERP`) is ambiguous: it is the word inside
/// `smart contract` (an audit / security / vulnerability concept) as well as the
/// derivatives "futures / perp contract". It therefore supports Perp ONLY inside
/// a trading / derivatives context — a LONG/SHORT direction, leverage, entry,
/// stop-loss / take-profit, perpetual, futures, a trading pair, or the canonical
/// [Futures/Perp Signal] header. Without such a co-marker (e.g. "smart-contract
/// security signals and vulnerability alerts") the word does NOT imply an asset
/// class. Pure (no I/O) so it is unit-testable.
fn cn_contract_is_perp(lower: &str) -> bool {
    if !lower.contains(CN_PERP) {
        return false;
    }
    // The canonical perp signal header is itself derivatives context.
    if lower.contains(CN_HDR_PERP) {
        return true;
    }
    const CN_PERP_CONTEXT: &[&str] = &[
        CN_LONG,         // long
        CN_SHORT,        // short
        CN_LEVERAGE,     // leverage
        CN_ENTRY,        // entry
        CN_STOP_LOSS,    // stop-loss
        CN_TAKE_PROFIT,  // take-profit
        CN_PERPETUAL,    // perpetual
        CN_FUTURES,      // futures
        CN_TRADING_PAIR, // trading pair
    ];
    CN_PERP_CONTEXT.iter().any(|kw| lower.contains(kw))
}

/// Prediction-market recognition beyond the literal `prediction` / CN word /
/// `polymarket`. Bounded co-occurrence only — a single `yes` / `no` is never
/// sufficient (ordinary Q&A must not be misread as a trading service):
/// - `event` + `contract`/`market` (an event-contract / event-market venue), or
/// - `buy`/`sell` + `yes`/`no` + `outcome`/`market` (an outcome-share order).
///
/// Still gated by `has_actionable` (caller) and the global negation rule.
fn prediction_combo_present(tokens: &std::collections::HashSet<&str>) -> bool {
    let event = tokens.contains("event") || tokens.contains("events");
    let contract_or_market = tokens.contains("contract")
        || tokens.contains("contracts")
        || tokens.contains("market")
        || tokens.contains("markets");
    if event && contract_or_market {
        return true;
    }
    let buy_or_sell = tokens.contains("buy") || tokens.contains("sell");
    let yes_or_no = tokens.contains("yes") || tokens.contains("no");
    let outcome_or_market = tokens.contains("outcome")
        || tokens.contains("outcomes")
        || tokens.contains("market")
        || tokens.contains("markets");
    buy_or_sell && yes_or_no && outcome_or_market
}

/// Option recognition beyond the literal `option(s)` / CN word. Bounded
/// co-occurrence only — a lone `call` / `put` is never sufficient (ordinary
/// English "call" / "put" must not be misread):
/// - `call`/`put` + `strike`/`expiry`/`expiration`, or
/// - `strike` + `expiry`/`expiration` together.
///
/// Still gated by `has_actionable` (caller) and the global negation rule.
fn option_combo_present(tokens: &std::collections::HashSet<&str>) -> bool {
    let call_or_put = tokens.contains("call")
        || tokens.contains("calls")
        || tokens.contains("put")
        || tokens.contains("puts");
    let strike = tokens.contains("strike") || tokens.contains("strikes");
    let expiry =
        tokens.contains("expiry") || tokens.contains("expiries") || tokens.contains("expiration");
    (call_or_put && (strike || expiry)) || (strike && expiry)
}

/// DeFi recognition beyond `defi`/`yield`/`staking`/`stake`/`farming`/`lp` and
/// the CN stake/liquidity words. Domain-specific single tokens (`apy`/`apr`/`tvl`,
/// `lending`/`lend`/`borrow`) qualify; a bare `pool` does NOT (it is too generic)
/// unless it co-occurs with a yield/liquidity/on-chain marker, and `liquidity pool`
/// qualifies as a phrase. Still gated by `has_actionable` (caller) and negation.
fn defi_combo_present(lower: &str, tokens: &std::collections::HashSet<&str>) -> bool {
    if tokens.contains("apy")
        || tokens.contains("apr")
        || tokens.contains("tvl")
        || tokens.contains("lending")
        || tokens.contains("lend")
        || tokens.contains("borrow")
        || tokens.contains("borrowing")
    {
        return true;
    }
    if lower.contains("liquidity pool") {
        return true;
    }
    let pool = tokens.contains("pool") || tokens.contains("pools");
    let onchain_yield_marker = tokens.contains("yield")
        || tokens.contains("liquidity")
        || tokens.contains("onchain")
        || tokens.contains("rewards")
        || tokens.contains("reward");
    pool && onchain_yield_marker
}

fn has_actionable(lower: &str, tokens: &std::collections::HashSet<&str>) -> bool {
    const EN_TOKENS: &[&str] = &[
        "signal",
        "signals",
        "entry",
        "buy",
        "sell",
        "long",
        "short",
        "position",
        "positions",
        "tp",
        "sl",
        "enter",
        "exit",
    ];
    if EN_TOKENS.iter().any(|t| tokens.contains(t)) {
        return true;
    }
    const EN_PHRASES: &[&str] = &[
        "stop loss",
        "take profit",
        "scheduled push",
        "copy trade",
        "copy-trade",
    ];
    if EN_PHRASES.iter().any(|p| lower.contains(p)) {
        return true;
    }
    const CN: &[&str] = &[
        CN_SIGNAL,
        CN_ENTRY,
        CN_STOP_LOSS,
        CN_TAKE_PROFIT,
        CN_POSITION,
        CN_LONG,
        CN_SHORT,
        CN_BUY,
        CN_SELL,
        CN_PUSH,
        CN_COPY,
    ];
    CN.iter().any(|p| lower.contains(p))
}

/// Hard negation — a direct denial that the service provides any trading signal.
/// These fail closed globally and OUTRANK a canonical header (see the caller):
/// an example header is a format sample, not a live signal, so an explicit
/// "no trading signals are provided" wins.
///
/// A BARE `no signal` is intentionally NOT in this set: it over-matched
/// quality-of-service claims like "no signal delay" / "no signal loss", which are
/// non-capability negations and must never erase an explicit trading service.
/// Genuine capability denials are spelled out instead.
fn has_hard_negation(lower: &str) -> bool {
    const EN: &[&str] = &[
        "no trading signal", // covers "no trading signal(s) [are provided]"
        "no signals are provided",
        "no signal is provided",
        "no signals provided",
        "no signal provided",
        "provides no signal",
        "provide no signal",
        "does not provide signal",
        "do not provide signal",
        "not a signal service",
        "not a trading signal service",
    ];
    if EN.iter().any(|p| lower.contains(p)) {
        return true;
    }
    [CN_NEG_NO_TRADE_SIGNAL, CN_NEG_NO_SIGNAL]
        .iter()
        .any(|p| lower.contains(p))
}

/// Soft read-only / alert markers — they describe a pure analytics / data / alert
/// service. The caller only fails closed on these when the description does NOT
/// also declare an explicit trading signal (no canonical header, no `signal`
/// word), so they block a pure alert/data false-positive without erasing an
/// explicit signal in the same description.
fn has_soft_readonly(lower: &str) -> bool {
    const EN: &[&str] = &[
        "analytics only",
        "market data only",
        "data feed only",
        "read-only",
        "read only",
        "informational only",
        "for reference only",
        "reference only",
        "security alert",
        "risk report",
    ];
    if EN.iter().any(|p| lower.contains(p)) {
        return true;
    }
    lower.contains(CN_NEG_ONLY_QUOTE)
}

// ── Local readiness probes (FR-5) ─────────────────────────────────────────

impl ToolInventory {
    /// Build a snapshot from the current environment (`$HOME`, `$PATH`).
    /// Filesystem/PATH existence checks only — never reads a credential value.
    /// Any probe failure ⇒ that tool is treated as not-ready (fail-safe).
    pub fn detect() -> Self {
        let home = dirs::home_dir().unwrap_or_default();
        let path_var = std::env::var("PATH").unwrap_or_default();
        Self::detect_with(&home, &path_var)
    }

    /// Testable variant of [`ToolInventory::detect`] with an explicit home dir and
    /// `$PATH` value (no global-env mutation).
    fn detect_with(home: &Path, path_var: &str) -> Self {
        let polymarket = plugin_readiness(home, "polymarket-plugin");
        let hyperliquid = plugin_readiness(home, "hyperliquid-plugin");
        let trade_kit = match super::trade_kit::probe_local_with(home, path_var).readiness() {
            super::trade_kit::LocalReadiness::Missing => Readiness::Missing,
            super::trade_kit::LocalReadiness::VerificationUnknown => Readiness::VerificationUnknown,
        };
        ToolInventory {
            onchainos: Readiness::Ready,
            trade_kit,
            polymarket,
            hyperliquid,
        }
    }

    /// Readiness for a tool from this snapshot.
    pub fn readiness_of(&self, tool: ExecutionTool) -> Readiness {
        match tool {
            ExecutionTool::Onchainos => self.onchainos,
            ExecutionTool::TradeKit => self.trade_kit,
            ExecutionTool::PolymarketPlugin => self.polymarket,
            ExecutionTool::HyperliquidPlugin => self.hyperliquid,
        }
    }

    fn reason_of(&self, tool: ExecutionTool) -> ToolReadinessReason {
        match (tool, self.readiness_of(tool)) {
            (_, Readiness::Ready) => ToolReadinessReason::Ready,
            (ExecutionTool::TradeKit, Readiness::Missing) => ToolReadinessReason::CliMissing,
            (_, Readiness::Missing) => ToolReadinessReason::PluginMissing,
            (ExecutionTool::TradeKit, Readiness::VerificationUnknown) => {
                ToolReadinessReason::LocalCompatibilityNotChecked
            }
            (_, Readiness::VerificationUnknown) => {
                ToolReadinessReason::LocalCompatibilityNotChecked
            }
            (_, Readiness::Incompatible) => ToolReadinessReason::Incompatible,
        }
    }
}

fn plugin_readiness(home: &Path, plugin_id: &str) -> Readiness {
    if crate::commands::upgrade::is_skill_installed_in(home, plugin_id) {
        Readiness::Ready
    } else {
        Readiness::Missing
    }
}

// ── Preflight assembly ────────────────────────────────────────────────────

/// Build the full preflight for a service description. Called by `asp_ops.rs`.
///
/// Pure classification + assembly of already-serializable data — it cannot fail,
/// so it returns [`AutoTradePreflight`] directly. The only genuinely fallible
/// boundary is serializing the object into the `asp-match` response `Value`; that
/// serialization (and its degrade-to-[`degraded_preflight`] fallback) lives at the
/// single write site in `asp_ops.rs`, not here — a redundant pre-serialization
/// probe would add a no-benefit error branch.
pub fn build_preflight(desc: &str, inv: &ToolInventory) -> AutoTradePreflight {
    let outcome = classify_description(desc);
    assemble(&outcome.classes, &outcome.explicit, outcome.evidence, inv)
}

/// Reserved RUNTIME reuse entry (FR-8): build a preflight from an ALREADY-PARSED
/// `AssetClass` set (does NOT re-parse text). Shared by the future signal pipeline.
#[allow(dead_code)] // FR-8: reserved for runtime signal pipeline wiring (follow-up)
pub fn build_preflight_from_classes(
    classes: &[AssetClass],
    explicit: &[ExecutionTool],
    inv: &ToolInventory,
) -> AutoTradePreflight {
    // Runtime callers already know the classes; synthesize description-level evidence
    // (they do not carry canonical-header provenance).
    let mut ordered: Vec<AssetClass> = Vec::new();
    for c in AssetClass::ORDER {
        if classes.contains(&c) && !ordered.contains(&c) {
            ordered.push(c);
        }
    }
    let mut evidence: Vec<String> = ordered
        .iter()
        .map(|c| format!("{}:description", c.as_str()))
        .collect();
    for tool in ExecutionTool::ALL {
        if explicit.contains(&tool) {
            evidence.push(format!("tool:{}", tool.token()));
        }
    }
    assemble(&ordered, explicit, evidence, inv)
}

/// Degraded fallback (FR-1): used by the `asp_ops.rs` write site when serializing
/// a built preflight into the `asp-match` response `Value` fails.
pub fn degraded_preflight() -> AutoTradePreflight {
    AutoTradePreflight {
        schema_version: 3,
        is_trading_signal: false,
        asset_classes: Vec::new(),
        explicit_tools: Vec::new(),
        selection_required: false,
        advisory_only: true,
        tools: Vec::new(),
        reminders: Vec::new(),
        trade_kit_probe: TradeKitProbeDirective {
            mode: TradeKitProbeMode::NotApplicable,
            asset_classes: Vec::new(),
        },
        evidence: vec!["preflight:unavailable".to_string()],
    }
}

/// Shared assembly core: turns a recognized class set + explicit tools + evidence
/// into the wire-shaped preflight. Deterministic (ORDER-driven, de-duplicated).
fn assemble(
    classes: &[AssetClass],
    explicit: &[ExecutionTool],
    evidence: Vec<String>,
    inv: &ToolInventory,
) -> AutoTradePreflight {
    let is_trading_signal = !classes.is_empty();

    // Candidate tools across all recognized classes, first-seen order via ORDER.
    let mut tool_list: Vec<ExecutionTool> = Vec::new();
    for c in AssetClass::ORDER {
        if classes.contains(&c) {
            for t in candidate_tools(c) {
                if !tool_list.contains(t) {
                    tool_list.push(*t);
                }
            }
        }
    }
    let tools: Vec<ToolStatus> = tool_list
        .iter()
        .map(|&tool| ToolStatus {
            tool,
            display_name: tool.display_name().to_string(),
            plugin_id: tool.plugin_id().map(str::to_string),
            readiness: inv.readiness_of(tool),
            reason: inv.reason_of(tool),
            checked_at: None,
        })
        .collect();
    let trade_kit_probe = trade_kit_probe_directive(classes, explicit);

    // Reminders + selection flag, iterating classes in stable ORDER.
    let mut reminders: Vec<Reminder> = Vec::new();
    let mut selection_required = false;
    for c in AssetClass::ORDER {
        if !classes.contains(&c) {
            continue;
        }
        let cands = candidate_tools(c);
        let named: Vec<ExecutionTool> = cands
            .iter()
            .copied()
            .filter(|t| explicit.contains(t))
            .collect();
        let effective: Vec<ExecutionTool> = if named.is_empty() {
            cands.to_vec()
        } else {
            named
        };

        if effective.len() == 1 {
            let tool = effective[0];
            match inv.readiness_of(tool) {
                Readiness::Missing => {
                    // OnchainOS is native (always Ready) so never reaches here.
                    if tool != ExecutionTool::Onchainos {
                        merge_reminder(&mut reminders, ReminderKind::InstallPlugin, Some(tool), c);
                    }
                }
                Readiness::Ready | Readiness::VerificationUnknown | Readiness::Incompatible => {}
            }
        } else {
            // Multiple candidates, no explicit single choice (FR-6): advise
            // selection at the first real signal (never pick a venue now).
            selection_required = true;
            merge_reminder(&mut reminders, ReminderKind::ChooseAtFirstSignal, None, c);
            // Readiness advisory (non-blocking): when EVERY candidate for this
            // class lacks confirmed local compatibility, add one merged hint
            // that the user may prepare ANY ONE of them now, or
            // wait for the first real signal to choose. If at least one candidate
            // is already ready, no advisory is added — we do not nudge installing
            // a redundant backup venue; the full `tools[].readiness` already shows
            // the state. This never installs, authorizes, or saves a preference.
            let all_unready = effective
                .iter()
                .all(|t| inv.readiness_of(*t) != Readiness::Ready);
            if all_unready {
                merge_reminder(&mut reminders, ReminderKind::ReadinessAdvisory, None, c);
                // The summary advisory alone is not actionable enough. Surface
                // one non-blocking preparation reminder per unavailable
                // candidate so the user may prepare any one now.
                // This still does NOT select a venue or persist a preference;
                // the first real signal owns that decision.
                for tool in &effective {
                    match inv.readiness_of(*tool) {
                        Readiness::Missing if *tool != ExecutionTool::Onchainos => {
                            merge_reminder(
                                &mut reminders,
                                ReminderKind::InstallPlugin,
                                Some(*tool),
                                c,
                            );
                        }
                        Readiness::Ready
                        | Readiness::Missing
                        | Readiness::VerificationUnknown
                        | Readiness::Incompatible => {}
                    }
                }
            }
        }
    }
    for r in &mut reminders {
        let label = classes_label(&r.asset_classes);
        let (en, zh) = render_messages(r.kind, r.tool, &label);
        r.message_en = en;
        r.message_zh = zh;
    }

    AutoTradePreflight {
        schema_version: 3,
        is_trading_signal,
        asset_classes: classes.to_vec(),
        explicit_tools: explicit.to_vec(),
        selection_required,
        advisory_only: true,
        tools,
        reminders,
        trade_kit_probe,
        evidence,
    }
}

fn trade_kit_probe_directive(
    classes: &[AssetClass],
    explicit: &[ExecutionTool],
) -> TradeKitProbeDirective {
    let relevant: Vec<AssetClass> = AssetClass::ORDER
        .iter()
        .copied()
        .filter(|class| {
            classes.contains(class) && candidate_tools(*class).contains(&ExecutionTool::TradeKit)
        })
        .collect();
    if relevant.is_empty() {
        return TradeKitProbeDirective {
            mode: TradeKitProbeMode::NotApplicable,
            asset_classes: Vec::new(),
        };
    }

    if explicit == [ExecutionTool::TradeKit] {
        return TradeKitProbeDirective {
            mode: TradeKitProbeMode::ProbeBeforeConfirmation,
            asset_classes: relevant,
        };
    }

    let sole_trade_kit: Vec<AssetClass> = relevant
        .iter()
        .copied()
        .filter(|class| candidate_tools(*class) == [ExecutionTool::TradeKit])
        .collect();
    if !sole_trade_kit.is_empty() {
        return TradeKitProbeDirective {
            mode: TradeKitProbeMode::ProbeBeforeConfirmation,
            asset_classes: sole_trade_kit,
        };
    }

    let every_class_has_explicit_alternative = !explicit.contains(&ExecutionTool::TradeKit)
        && relevant.iter().all(|class| {
            candidate_tools(*class)
                .iter()
                .any(|tool| *tool != ExecutionTool::TradeKit && explicit.contains(tool))
        });
    TradeKitProbeDirective {
        mode: if every_class_has_explicit_alternative {
            TradeKitProbeMode::NotApplicable
        } else {
            TradeKitProbeMode::DeferredUntilVenueSelection
        },
        asset_classes: if every_class_has_explicit_alternative {
            Vec::new()
        } else {
            relevant
        },
    }
}

/// Insert a reminder or merge the class into an existing one keyed by (kind, tool).
fn merge_reminder(
    reminders: &mut Vec<Reminder>,
    kind: ReminderKind,
    tool: Option<ExecutionTool>,
    class: AssetClass,
) {
    if let Some(existing) = reminders
        .iter_mut()
        .find(|r| r.kind == kind && r.tool == tool)
    {
        if !existing.asset_classes.contains(&class) {
            existing.asset_classes.push(class);
        }
        return;
    }
    reminders.push(Reminder {
        kind,
        tool,
        plugin_id: tool.and_then(|t| t.plugin_id()).map(str::to_string),
        asset_classes: vec![class],
        blocking: false,
        message_en: String::new(),
        message_zh: String::new(),
    });
}

fn classes_label(classes: &[AssetClass]) -> String {
    classes
        .iter()
        .map(|c| c.as_str())
        .collect::<Vec<_>>()
        .join("/")
}

fn render_messages(
    kind: ReminderKind,
    tool: Option<ExecutionTool>,
    label: &str,
) -> (String, String) {
    match kind {
        ReminderKind::InstallPlugin => {
            let name = tool.map(|t| t.display_name()).unwrap_or("the");
            if tool == Some(ExecutionTool::TradeKit) {
                (
                    format!("Install OKX Agent Skills (`npx skills add okx/agent-skills`) and the Trade Kit CLI (`npm install -g @okx_ai/okx-trade-cli`) to execute {label} signals."),
                    // zh gloss: Install OKX Agent Skills and the Trade Kit CLI.
                    format!(
                        "\u{8fd0}\u{884c} npx skills add okx/agent-skills \u{5b89}\u{88c5} OKX Agent Skills\u{ff0c}\u{5e76}\u{8fd0}\u{884c} npm install -g @okx_ai/okx-trade-cli \u{5b89}\u{88c5} Trade Kit CLI\u{ff0c}\u{4ee5}\u{6267}\u{884c} {label} \u{4fe1}\u{53f7}\u{3002}"
                    ),
                )
            } else {
                (
                    format!("Install the {name} plugin to execute {label} signals."),
                    // zh gloss: Install the {name} plugin to execute {label} signals.
                    format!(
                        "\u{5b89}\u{88c5} {name} \u{63d2}\u{4ef6}\u{4ee5}\u{6267}\u{884c} {label} \u{4fe1}\u{53f7}\u{3002}"
                    ),
                )
            }
        }
        ReminderKind::ChooseAtFirstSignal => (
            format!(
                "Multiple execution venues are available for {label}; choose one when the first real signal arrives."
            ),
            // zh gloss: {label} has multiple execution venues; choose one when the first real signal arrives.
            format!(
                "{label} \u{6709}\u{591a}\u{4e2a}\u{53ef}\u{9009}\u{6267}\u{884c}\u{6e20}\u{9053}\u{ff1b}\u{8bf7}\u{5728}\u{7b2c}\u{4e00}\u{6761}\u{771f}\u{5b9e}\u{4fe1}\u{53f7}\u{5230}\u{8fbe}\u{65f6}\u{9009}\u{62e9}\u{3002}"
            ),
        ),
        ReminderKind::ReadinessAdvisory => (
            format!(
                "None of the {label} execution venues are ready yet; install or configure any one of them now, or wait for the first real signal to choose. Nothing is installed or selected for you."
            ),
            // zh gloss: none of {label}'s candidate execution tools are ready; you may install or configure any one, or wait for the first real signal to choose.
            format!(
                "{label} \u{7684}\u{5019}\u{9009}\u{6267}\u{884c}\u{5de5}\u{5177}\u{5747}\u{672a}\u{5c31}\u{7eea}\u{ff1b}\u{53ef}\u{5b89}\u{88c5}\u{6216}\u{914d}\u{7f6e}\u{5176}\u{4e2d}\u{4efb}\u{4e00}\u{ff0c}\u{6216}\u{7b49}\u{5f85}\u{9996}\u{4e2a}\u{771f}\u{5b9e}\u{4fe1}\u{53f7}\u{5230}\u{8fbe}\u{65f6}\u{518d}\u{9009}\u{62e9}\u{3002}"
            ),
        ),
    }
}

// ── Tests ───────────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    fn classes(desc: &str) -> Vec<AssetClass> {
        classify_description(desc).classes
    }

    // ── classify: canonical headers (subset of the 14 CN/EN examples) ──────
    #[test]
    fn classify_canonical_headers_single_class() {
        // [Spot Signal] (EN) and the escaped CN 'spot signal' header -> spot
        assert_eq!(
            classes("\u{3010}Spot Signal\u{3011} Buy BTC on-chain, entry 60000, SL 58000"),
            vec![AssetClass::Spot]
        );
        assert_eq!(
            // escaped CN 'spot signal' header + 'buy BTC 60000'
            classes("\u{3010}\u{73b0}\u{8d27}\u{4fe1}\u{53f7}\u{3011}BTC \u{4e70}\u{5165} 60000"),
            vec![AssetClass::Spot]
        );
        // [Futures Signal] (EN) and the escaped CN 'futures signal' header -> perp
        assert_eq!(
            classes("\u{3010}Futures Signal\u{3011} LONG ETH-PERP, entry 3000, TP 3500"),
            vec![AssetClass::Perp]
        );
        assert_eq!(
            // escaped CN 'futures signal' header + 'long' ETH-PERP
            classes("\u{3010}\u{5408}\u{7ea6}\u{4fe1}\u{53f7}\u{3011}\u{505a}\u{591a} ETH-PERP"),
            vec![AssetClass::Perp]
        );
        // [Prediction Signal] (EN) and the escaped CN 'prediction market signal' header -> prediction
        assert_eq!(
            classes("\u{3010}Prediction Signal\u{3011} Polymarket BUY YES on the outcome"),
            vec![AssetClass::Prediction]
        );
        assert_eq!(
            // escaped CN 'prediction market signal' header + Polymarket buy YES
            classes("\u{3010}\u{9884}\u{6d4b}\u{5e02}\u{573a}\u{4fe1}\u{53f7}\u{3011}Polymarket BUY YES"),
            vec![AssetClass::Prediction]
        );
        // [Options Signal] (EN) and the escaped CN 'option signal' header -> option
        assert_eq!(
            classes("\u{3010}Options Signal\u{3011} BUY BTC call option, strike 70000"),
            vec![AssetClass::Option]
        );
        assert_eq!(
            // escaped CN 'option signal' header + buy BTC call option
            classes("\u{3010}\u{671f}\u{6743}\u{4fe1}\u{53f7}\u{3011}BTC call option"),
            vec![AssetClass::Option]
        );
        // [DeFi Signal] (EN) and the escaped CN 'DeFi signal' header -> defi
        assert_eq!(
            classes("\u{3010}DeFi Signal\u{3011} rebalance yield position into higher-APR pool"),
            vec![AssetClass::Defi]
        );
        assert_eq!(
            // escaped CN 'DeFi signal' header + 'stake signal, adjust position'
            classes("\u{3010}DeFi \u{4fe1}\u{53f7}\u{3011}\u{8d28}\u{62bc}\u{4fe1}\u{53f7}"),
            vec![AssetClass::Defi]
        );
    }

    // ── classify: free-text pairs (spot pair CN/EN, perp swap CN/EN) ───────
    #[test]
    fn classify_free_text_single_class() {
        assert_eq!(
            classes("Spot BTC/USDT trading signal with entry and stop loss"),
            vec![AssetClass::Spot]
        );
        assert_eq!(
            // escaped CN: 'spot BTC/USDT signal, includes entry and stop-loss'
            classes(
                "\u{73b0}\u{8d27} BTC/USDT \u{4fe1}\u{53f7}\u{ff0c}\u{5165}\u{573a}\u{548c}\u{6b62}\u{635f}"
            ),
            vec![AssetClass::Spot]
        );
        assert_eq!(
            classes("Perpetual BTC-USDT-SWAP signal, LONG, entry 60000, TP1 62000 TP2 64000"),
            vec![AssetClass::Perp]
        );
        assert_eq!(
            // escaped CN: 'perpetual BTC-USDT-SWAP futures signal, long, entry 60000'
            classes(
                "\u{6c38}\u{7eed} BTC-USDT-SWAP \u{5408}\u{7ea6}\u{4fe1}\u{53f7}\u{ff0c}\u{505a}\u{591a}"
            ),
            vec![AssetClass::Perp]
        );
    }

    // ── classify: bare `swap` is Spot, not Perp (oli-feedback P0) ──────────
    #[test]
    fn classify_bare_swap_is_spot_not_perp() {
        // A plain DEX/token swap is a spot exchange: the bare word `swap` must
        // NOT pull in Perp. "DEX swap signal with BUY entry" → [spot] only.
        assert_eq!(
            classes("DEX swap signal with BUY entry"),
            vec![AssetClass::Spot]
        );
        // Equivalent Chinese input (escaped \u{} below; gloss: 'DEX exchange buy signal') -> [spot] only.
        assert_eq!(
            classes("DEX \u{5151}\u{6362} \u{4e70}\u{5165} \u{4fe1}\u{53f7}"),
            vec![AssetClass::Spot]
        );
        // An explicit `*-SWAP` perpetual contract code still classifies as Perp,
        // via the `-swap` infix — even without the literal "perpetual" word.
        assert_eq!(
            classes("Signal on BTC-USDT-SWAP, LONG entry 60000"),
            vec![AssetClass::Perp]
        );
        // And with the explicit perp context word intact.
        assert_eq!(
            classes("BTC-USDT-SWAP perpetual signal"),
            vec![AssetClass::Perp]
        );
        // Round4 regressions: a single-hyphen `token-swap` DEX spot description
        // must NOT be pulled into Perp — it stays [spot] …
        assert_eq!(
            classes("DEX token-swap signal with BUY entry"),
            vec![AssetClass::Spot]
        );
        // … while a full BASE-QUOTE-SWAP instrument code is Perp even without the
        // literal "perpetual" word.
        assert_eq!(
            classes("BTC-USDT-SWAP signal with LONG entry"),
            vec![AssetClass::Perp]
        );
    }

    // ── classify: cross-language, multi-class, de-dup, all-five (AC-2) ─────
    #[test]
    fn classify_multi_class_cross_language_stable_dedup() {
        // escaped CN 'provide spot and futures signals, entry/SL/position' (CN class + EN class + CN action)
        let out = classify_description(
            "\u{63d0}\u{4f9b} spot \u{548c} futures \u{4ea4}\u{6613}\u{4fe1}\u{53f7}\u{ff0c}\u{5165}\u{573a}\u{6b62}\u{635f}\u{4ed3}\u{4f4d}",
        );
        assert_eq!(out.classes, vec![AssetClass::Spot, AssetClass::Perp]);

        // All five classes, single description → ORDER-stable, unique.
        assert_eq!(
            classes(
                "Signals for spot, perp, prediction, option, and defi with entry and stop loss"
            ),
            AssetClass::ORDER.to_vec()
        );

        // Repeated declarations de-duplicate.
        assert_eq!(
            classes("spot spot spot trading signal, entry"),
            vec![AssetClass::Spot]
        );
    }

    // ── classify: explicit tool scoping + evidence (FR-3) ──────────────────
    #[test]
    fn classify_explicit_tools_scoped_and_evidence() {
        let out = classify_description(
            "\u{3010}Prediction Signal\u{3011} Polymarket BUY YES on the outcome",
        );
        assert_eq!(out.classes, vec![AssetClass::Prediction]);
        assert_eq!(out.explicit, vec![ExecutionTool::PolymarketPlugin]);
        assert!(out.evidence.contains(&"prediction:header".to_string()));
        assert!(out.evidence.contains(&"tool:polymarket_plugin".to_string()));

        let okx_event = classify_description(
            "【Prediction Signal】 Execute BUY YES event contracts through OKX Event",
        );
        assert_eq!(okx_event.classes, vec![AssetClass::Prediction]);
        assert_eq!(okx_event.explicit, vec![ExecutionTool::TradeKit]);
        assert!(okx_event.evidence.contains(&"tool:trade_kit".to_string()));

        // A named tool never invents an unrelated class: "hyperliquid" with no
        // actionable/class semantic beyond it still needs perp to be recognized —
        // here it is (hyperliquid → perp), but polymarket is absent so no prediction.
        let out2 = classify_description("Hyperliquid perp LONG signal, entry 3000");
        assert_eq!(out2.classes, vec![AssetClass::Perp]);
        assert_eq!(out2.explicit, vec![ExecutionTool::HyperliquidPlugin]);
        assert!(!out2.explicit.contains(&ExecutionTool::PolymarketPlugin));
    }

    // ── classify: negation / read-only → [] (AC-3) ─────────────────────────
    #[test]
    fn classify_negation_read_only_empty() {
        assert!(classes("").is_empty());
        assert!(classes("   ").is_empty());
        assert!(classes("Spot market analytics only; no trading signals").is_empty());
        assert!(
            // escaped CN: 'only provides market data, does not provide trading signals'
            classes(
                "\u{4ec5}\u{63d0}\u{4f9b}\u{884c}\u{60c5}\u{ff0c}\u{4e0d}\u{63d0}\u{4f9b}\u{4ea4}\u{6613}\u{4fe1}\u{53f7}"
            )
            .is_empty()
        );
        // word-boundary: spotlight / optional must NOT match spot / option
        assert!(classes("Spotlight on optional features, entry-level guide").is_empty());
        // security alerts / risk report
        assert!(classes("Smart-contract security alerts and daily risk report").is_empty());
        // no class at all
        assert!(classes("Daily crypto news digest and market commentary").is_empty());
    }

    // ── classify: explicit signal survives soft read-only/alert markers (oli-feedback P0) ──
    #[test]
    fn classify_explicit_signal_survives_soft_markers() {
        // An explicit Spot trading signal is NOT erased by an incidental
        // "security alert" mention in the same description.
        assert_eq!(
            classes("Spot trading signals with BUY entry plus security alerts"),
            vec![AssetClass::Spot]
        );
        // A canonical [Spot Signal] header wins even when the text also contains a
        // "market data only" read-only marker (here negated by "not").
        assert_eq!(
            classes("\u{3010}Spot Signal\u{3011}not market data only; BUY BTC with entry"),
            vec![AssetClass::Spot]
        );
        // Pure alert / data-only descriptions (no explicit signal) still fail
        // closed — the original security/risk/data-only counterexamples are kept.
        assert!(classes("Smart-contract security alerts and daily risk report").is_empty());
        assert!(classes("market data only").is_empty());
        assert!(classes("Spot market analytics only; no trading signals").is_empty());
    }

    // ── classify: smart-contract security context is NOT an asset class, and
    //    capability negation outranks an example header (oli-feedback P0 round5) ──
    #[test]
    fn classify_smart_contract_negation_context() {
        // CN "smart-contract security signals and vulnerability alerts": the CN
        // `contract` word here lives inside `smart contract` (an audit/security
        // concept), NOT a derivatives contract, so it must NOT pull in Perp -> [].
        assert!(classes(
            "\u{667a}\u{80fd}\u{5408}\u{7ea6}\u{5b89}\u{5168}\u{4fe1}\u{53f7}\u{4e0e}\u{6f0f}\u{6d1e}\u{544a}\u{8b66}"
        )
        .is_empty());
        // CN "smart-contract audit and risk alert" -> [].
        assert!(classes(
            "\u{667a}\u{80fd}\u{5408}\u{7ea6}\u{5ba1}\u{8ba1}\u{4e0e}\u{98ce}\u{9669}\u{544a}\u{8b66}"
        )
        .is_empty());
        // EN equivalent -> [] (no derivatives / trading context).
        assert!(classes("Smart-contract security signals and vulnerability alerts").is_empty());

        // CN `contract` DOES support Perp in a real trading/derivatives context:
        // "contract trading signal: LONG BTC-PERP, entry 60000, stop-loss 59000".
        assert_eq!(
            classes(
                "\u{5408}\u{7ea6}\u{4ea4}\u{6613}\u{4fe1}\u{53f7}\u{ff1a}LONG BTC-PERP\u{ff0c}\u{5165}\u{573a} 60000\u{ff0c}\u{6b62}\u{635f} 59000"
            ),
            vec![AssetClass::Perp]
        );

        // Non-capability "no signal delay" must NOT erase an explicit spot service.
        assert_eq!(
            classes("Spot trading signals with BUY entry and no signal delay"),
            vec![AssetClass::Spot]
        );
        // A direct capability denial outranks an EXAMPLE header: the header is only
        // a format sample, so "no trading signals are provided" wins -> [].
        assert!(classes(
            "\u{3010}Spot Signal\u{3011} is only a format example; no trading signals are provided"
        )
        .is_empty());
        // The pre-existing explicit-signal-plus-alert case still classifies as spot.
        assert_eq!(
            classes("Spot trading signals with BUY entry plus security alerts"),
            vec![AssetClass::Spot]
        );
    }

    // ── classify: prediction bounded combos (event / outcome) + counterexamples ──
    #[test]
    fn classify_prediction_bounded_combos() {
        // event + contract / market — no literal "prediction" / "polymarket" word.
        assert_eq!(
            classes("Event contract signal: BUY YES on the outcome"),
            vec![AssetClass::Prediction]
        );
        assert_eq!(
            classes("Event market entry signal, YES position"),
            vec![AssetClass::Prediction]
        );
        // buy / sell + yes / no + outcome / market.
        assert_eq!(
            classes("Signal to BUY YES outcome shares, entry now"),
            vec![AssetClass::Prediction]
        );
        assert_eq!(
            classes("Sell NO on the market, take profit"),
            vec![AssetClass::Prediction]
        );
        // CN x EN cross: escaped CN 'buy' with an English event-contract signal.
        assert_eq!(
            // escaped CN: 'buy' + 'signal' around an English event contract / YES outcome
            classes("event contract, \u{4e70}\u{5165} YES outcome \u{4fe1}\u{53f7}"),
            vec![AssetClass::Prediction]
        );

        // Counterexamples — still actionable (a spot signal), so the semantic branch
        // is reached, but prediction must NOT be added:
        // bare yes/no in ordinary Q&A is not an outcome order …
        assert_eq!(
            classify_description(
                "Spot BTC entry signal; the bot replies yes or no to your questions"
            )
            .classes,
            vec![AssetClass::Spot]
        );
        // … and a lone "event" with no contract/market is not a prediction market.
        assert_eq!(
            classify_description("Upcoming event calendar plus a spot entry signal").classes,
            vec![AssetClass::Spot]
        );
    }

    // ── classify: option / defi bounded combos (table-driven) + counterexamples ──
    #[test]
    fn classify_option_defi_bounded_combos() {
        // (description, expected classes) — each row is a TD-style case.
        let cases: &[(&str, Vec<AssetClass>)] = &[
            // Option: call/put + strike/expiry, no literal "option" word.
            (
                "Signal: BUY BTC 70000 call, strike 70000, expiry Friday",
                vec![AssetClass::Option],
            ),
            (
                "Sell ETH put at strike 3000, take profit signal",
                vec![AssetClass::Option],
            ),
            (
                "Weekly strike + expiration roll signal, enter now",
                vec![AssetClass::Option],
            ),
            // DeFi: APY/APR/TVL, lending/borrow, liquidity pool, pool+marker.
            (
                "Signal: rotate into the highest-APY vault this week",
                vec![AssetClass::Defi],
            ),
            (
                "Lending/borrow rate signal, enter the best APR market",
                vec![AssetClass::Defi],
            ),
            (
                "TVL-momentum entry signal across protocols",
                vec![AssetClass::Defi],
            ),
            (
                "Provide liquidity pool position signal, enter now",
                vec![AssetClass::Defi],
            ),
            (
                "Yield pool rotation signal, entry and exit",
                vec![AssetClass::Defi],
            ),
        ];
        for (desc, want) in cases {
            assert_eq!(&classes(desc), want, "case: {desc}");
        }

        // CN x EN cross: escaped CN 'buy' + English call/strike is still option.
        assert_eq!(
            // escaped CN: 'buy' around an English call/strike option signal
            classes("\u{4e70}\u{5165} BTC call strike 70000 \u{4fe1}\u{53f7}"),
            vec![AssetClass::Option]
        );

        // Counterexamples — actionable spot signals that must NOT gain option/defi:
        // a lone English "call" / "put" is not an options contract …
        assert_eq!(
            classify_description("Spot entry signal; we call you and put in the order").classes,
            vec![AssetClass::Spot]
        );
        // … and a bare "pool" with no yield/liquidity marker is not DeFi.
        assert_eq!(
            classify_description("Spot signal from a shared trading pool of ideas, entry").classes,
            vec![AssetClass::Spot]
        );
    }

    // ── classify: trading-signal ⇔ non-empty invariant ─────────────────────
    #[test]
    fn classify_trading_signal_invariant() {
        let inv = ready_inventory();
        for desc in [
            "\u{3010}Spot Signal\u{3011} buy BTC entry 60000",
            "",
            "market data only",
            "Signals for spot and perp with entry",
        ] {
            let pf = build_preflight(desc, &inv);
            assert_eq!(pf.is_trading_signal, !pf.asset_classes.is_empty());
        }
    }

    // ── local preflight: installation is never authorization readiness ─────
    #[test]
    fn installed_trade_kit_is_unknown_and_option_requires_preconfirmation_probe() {
        let tmp = tempfile::tempdir().unwrap();
        let home = tmp.path();
        let bin = home.join("bin");
        std::fs::create_dir_all(&bin).unwrap();
        std::fs::write(bin.join("okx"), b"#!/bin/sh\n").unwrap();
        let inv = ToolInventory::detect_with(home, bin.to_str().unwrap());
        assert_eq!(
            serde_json::to_value(inv.readiness_of(ExecutionTool::TradeKit)).unwrap(),
            serde_json::json!("verification_unknown")
        );

        // Option is Trade-Kit-only. Local matching stays process/network-free
        // while the deterministic directive requires a local compatibility
        // probe after service selection and before subscription confirmation.
        let pf = build_preflight(
            "\u{3010}Options Signal\u{3011} buy BTC call option, strike 70000",
            &inv,
        );
        assert_eq!(pf.asset_classes, vec![AssetClass::Option]);
        let wire = serde_json::to_value(&pf).unwrap();
        assert_eq!(wire["schemaVersion"], 3);
        assert_eq!(wire["tools"][0]["readiness"], "verification_unknown");
        assert_eq!(
            wire["tools"][0]["reason"],
            "local_compatibility_not_checked"
        );
        assert!(wire["tools"][0]["checkedAt"].is_null());
        assert_eq!(wire["tradeKitProbe"]["mode"], "probe_before_confirmation");
        assert_eq!(
            wire["tradeKitProbe"]["assetClasses"],
            serde_json::json!(["option"])
        );
        assert!(pf.reminders.is_empty());
    }

    #[test]
    fn explicit_trade_kit_batches_all_relevant_classes_before_confirmation() {
        let inv = ready_inventory();
        let pf = build_preflight_from_classes(
            &[AssetClass::Perp, AssetClass::Spot],
            &[ExecutionTool::TradeKit],
            &inv,
        );

        assert_eq!(
            pf.trade_kit_probe,
            TradeKitProbeDirective {
                mode: TradeKitProbeMode::ProbeBeforeConfirmation,
                asset_classes: vec![AssetClass::Spot, AssetClass::Perp],
            }
        );
    }

    #[test]
    fn explicit_non_trade_kit_candidate_makes_probe_not_applicable() {
        let inv = ready_inventory();
        let pf = build_preflight_from_classes(
            &[AssetClass::Prediction],
            &[ExecutionTool::PolymarketPlugin],
            &inv,
        );

        assert_eq!(
            pf.trade_kit_probe,
            TradeKitProbeDirective {
                mode: TradeKitProbeMode::NotApplicable,
                asset_classes: Vec::new(),
            }
        );
    }

    #[test]
    fn multiple_explicit_venues_defer_trade_kit_probe_until_selection() {
        let inv = ToolInventory {
            onchainos: Readiness::Ready,
            trade_kit: Readiness::VerificationUnknown,
            polymarket: Readiness::Missing,
            hyperliquid: Readiness::Missing,
        };
        let pf = build_preflight(
            "Prediction signal: execute through Polymarket or Trade Kit after venue selection",
            &inv,
        );

        assert!(pf.selection_required);
        assert_eq!(
            pf.explicit_tools,
            vec![ExecutionTool::TradeKit, ExecutionTool::PolymarketPlugin,]
        );
        assert_eq!(
            pf.trade_kit_probe,
            TradeKitProbeDirective {
                mode: TradeKitProbeMode::DeferredUntilVenueSelection,
                asset_classes: vec![AssetClass::Prediction],
            }
        );
    }

    // ── ToolInventory readiness ────────────────────────────────────────────
    fn ready_inventory() -> ToolInventory {
        // Everything not-installed → OnchainOS ready, plugins missing, trade kit missing.
        let tmp = tempfile::tempdir().unwrap();
        ToolInventory::detect_with(tmp.path(), "")
    }

    #[test]
    fn readiness_onchainos_always_ready_plugins_by_install() {
        let tmp = tempfile::tempdir().unwrap();
        // No skills installed.
        let inv = ToolInventory::detect_with(tmp.path(), "");
        assert_eq!(inv.readiness_of(ExecutionTool::Onchainos), Readiness::Ready);
        assert_eq!(
            inv.readiness_of(ExecutionTool::PolymarketPlugin),
            Readiness::Missing
        );
        assert_eq!(
            inv.readiness_of(ExecutionTool::HyperliquidPlugin),
            Readiness::Missing
        );
        assert_eq!(
            inv.readiness_of(ExecutionTool::TradeKit),
            Readiness::Missing
        );

        // Install the polymarket plugin under a canonical skills-home dir
        // (a real install carries a SKILL.md inside the plugin directory).
        let poly_dir = tmp.path().join(".agents/skills/polymarket-plugin");
        std::fs::create_dir_all(&poly_dir).unwrap();
        std::fs::write(poly_dir.join("SKILL.md"), b"# polymarket").unwrap();
        let inv2 = ToolInventory::detect_with(tmp.path(), "");
        assert_eq!(
            inv2.readiness_of(ExecutionTool::PolymarketPlugin),
            Readiness::Ready
        );
        assert_eq!(
            inv2.readiness_of(ExecutionTool::HyperliquidPlugin),
            Readiness::Missing
        );
    }

    #[test]
    fn readiness_trade_kit_presence_is_missing_or_verification_unknown() {
        let tmp = tempfile::tempdir().unwrap();
        let home = tmp.path();
        let readiness = |path_var: &str| {
            serde_json::to_value(
                ToolInventory::detect_with(home, path_var).readiness_of(ExecutionTool::TradeKit),
            )
            .unwrap()
        };

        // (1) CLI absent → Missing
        assert_eq!(readiness(""), serde_json::json!("missing"));

        // (2) CLI present → authentication is outside this local snapshot.
        let bin = home.join("bin");
        std::fs::create_dir_all(&bin).unwrap();
        std::fs::write(bin.join("okx"), b"#!/bin/sh\n").unwrap();
        let path_var = bin.to_str().unwrap();
        assert_eq!(
            readiness(path_var),
            serde_json::json!("verification_unknown")
        );

        // (3) Credential-file markers cannot upgrade local readiness.
        std::fs::create_dir_all(home.join(".okx")).unwrap();
        std::fs::write(home.join(".okx/config.toml"), b"[trade]\nk = 1\n").unwrap();
        assert_eq!(
            readiness(path_var),
            serde_json::json!("verification_unknown")
        );

        // An empty config marker does not change subscription-time readiness.
        std::fs::write(home.join(".okx/config.toml"), b"").unwrap();
        assert_eq!(
            readiness(path_var),
            serde_json::json!("verification_unknown")
        );

        // Other configuration markers are likewise irrelevant at subscription time.
        std::fs::remove_file(home.join(".okx/config.toml")).unwrap();
        std::fs::write(home.join(".okx/config.json"), b"{\"k\":1}").unwrap();
        assert_eq!(
            readiness(path_var),
            serde_json::json!("verification_unknown")
        );
    }

    // ── reminders: plugin missing → single install; installed → none ───────
    #[test]
    fn reminders_explicit_plugin_missing_single_install() {
        let tmp = tempfile::tempdir().unwrap();
        // Trade Kit installed-but-unverified, so it does not add an install reminder.
        let bin = tmp.path().join("bin");
        std::fs::create_dir_all(&bin).unwrap();
        std::fs::write(bin.join("okx"), b"x").unwrap();
        std::fs::create_dir_all(tmp.path().join(".okx")).unwrap();
        std::fs::write(tmp.path().join(".okx/config.toml"), b"[t]\nk=1\n").unwrap();
        let inv = ToolInventory::detect_with(tmp.path(), bin.to_str().unwrap());

        let pf = build_preflight("\u{3010}Prediction Signal\u{3011} Polymarket BUY YES", &inv);
        // Both candidates listed; Polymarket missing, Trade Kit unverified.
        assert_eq!(pf.tools.len(), 2);
        assert_eq!(pf.tools[0].tool, ExecutionTool::PolymarketPlugin);
        assert_eq!(pf.tools[0].readiness, Readiness::Missing);
        assert_eq!(pf.tools[1].tool, ExecutionTool::TradeKit);
        assert_eq!(
            serde_json::to_value(pf.tools[1].readiness).unwrap(),
            serde_json::json!("verification_unknown")
        );
        // Exactly one reminder: install the polymarket plugin. Not selection-required
        // (explicit single choice), all reminders non-blocking + bilingual.
        assert_eq!(pf.reminders.len(), 1);
        assert_eq!(pf.reminders[0].kind, ReminderKind::InstallPlugin);
        assert_eq!(pf.reminders[0].tool, Some(ExecutionTool::PolymarketPlugin));
        assert_eq!(
            pf.reminders[0].plugin_id.as_deref(),
            Some("polymarket-plugin")
        );
        assert!(!pf.selection_required);
        for r in &pf.reminders {
            assert!(!r.blocking);
            assert!(!r.message_en.is_empty());
            assert!(!r.message_zh.is_empty());
        }

        // Installed plugin (dir with a real SKILL.md) → no install reminder.
        let poly_dir = tmp.path().join(".claude/skills/polymarket-plugin");
        std::fs::create_dir_all(&poly_dir).unwrap();
        std::fs::write(poly_dir.join("SKILL.md"), b"# polymarket").unwrap();
        let inv2 = ToolInventory::detect_with(tmp.path(), bin.to_str().unwrap());
        let pf2 = build_preflight(
            "\u{3010}Prediction Signal\u{3011} Polymarket BUY YES",
            &inv2,
        );
        assert!(pf2.reminders.is_empty());
    }

    // ── reminders: multi-venue no preference → choose + actionable preparation ──
    #[test]
    fn reminders_multi_venue_no_preference() {
        let inv = ready_inventory(); // trade kit missing, plugins missing, onchainos ready
                                     // plain "prediction" (no venue named) → two candidates → choose advisory,
                                     // a merged readiness advisory, and one preparation reminder per candidate.
        let pf = build_preflight("prediction market signal, buy YES, entry", &inv);
        assert_eq!(pf.asset_classes, vec![AssetClass::Prediction]);
        assert!(pf.selection_required);
        assert_eq!(pf.reminders.len(), 4);
        assert!(pf
            .reminders
            .iter()
            .any(|r| r.kind == ReminderKind::ChooseAtFirstSignal));
        assert!(pf
            .reminders
            .iter()
            .any(|r| r.kind == ReminderKind::ReadinessAdvisory));
        let installs: Vec<&Reminder> = pf
            .reminders
            .iter()
            .filter(|r| r.kind == ReminderKind::InstallPlugin)
            .collect();
        assert_eq!(installs.len(), 2);
        assert!(installs
            .iter()
            .any(|r| r.tool == Some(ExecutionTool::PolymarketPlugin)));
        assert!(installs
            .iter()
            .any(|r| r.tool == Some(ExecutionTool::TradeKit)));
        // every reminder is non-blocking + bilingual
        for r in &pf.reminders {
            assert!(!r.blocking);
            assert!(!r.message_en.is_empty());
            assert!(!r.message_zh.is_empty());
        }
    }

    // ── reminders: readiness advisory only when EVERY candidate is unready ─────
    #[test]
    fn reminders_readiness_advisory_multi_venue() {
        // (1) generic Prediction; Polymarket missing + Trade Kit installed with no
        //     local compatibility probe → Trade Kit is unknown. Do not infer auth state or
        //     probe while the venue is still unselected.
        let tmp = tempfile::tempdir().unwrap();
        let home = tmp.path();
        let bin = home.join("bin");
        std::fs::create_dir_all(&bin).unwrap();
        std::fs::write(bin.join("okx"), b"#!/bin/sh\n").unwrap(); // CLI present, no config
        let inv = ToolInventory::detect_with(home, bin.to_str().unwrap());
        assert_eq!(
            serde_json::to_value(inv.readiness_of(ExecutionTool::TradeKit)).unwrap(),
            serde_json::json!("verification_unknown")
        );
        assert_eq!(
            inv.readiness_of(ExecutionTool::PolymarketPlugin),
            Readiness::Missing
        );

        let pf = build_preflight("prediction market signal, buy YES, entry", &inv);
        assert_eq!(pf.asset_classes, vec![AssetClass::Prediction]);
        assert!(pf.selection_required);
        let wire = serde_json::to_value(&pf).unwrap();
        assert_eq!(
            wire["tradeKitProbe"]["mode"],
            "deferred_until_venue_selection"
        );
        assert!(pf
            .reminders
            .iter()
            .any(|r| r.kind == ReminderKind::ReadinessAdvisory));
        assert_eq!(
            pf.reminders
                .iter()
                .filter(|r| r.kind == ReminderKind::ChooseAtFirstSignal)
                .count(),
            1
        );

        // (2) generic Prediction with Polymarket READY → at least one candidate
        //     ready → NO readiness advisory (no redundant-backup nudge).
        let poly_dir = home.join(".agents/skills/polymarket-plugin");
        std::fs::create_dir_all(&poly_dir).unwrap();
        std::fs::write(poly_dir.join("SKILL.md"), b"# polymarket").unwrap();
        let inv2 = ToolInventory::detect_with(home, bin.to_str().unwrap());
        assert_eq!(
            inv2.readiness_of(ExecutionTool::PolymarketPlugin),
            Readiness::Ready
        );
        let pf2 = build_preflight("prediction market signal, buy YES, entry", &inv2);
        assert!(pf2.selection_required);
        assert!(
            pf2.reminders
                .iter()
                .all(|r| r.kind != ReminderKind::ReadinessAdvisory),
            "no readiness advisory when a candidate is already ready"
        );

        // (3) EXPLICIT Polymarket missing → existing single-venue install behavior,
        //     NOT selection-required and NO readiness advisory.
        let tmp3 = tempfile::tempdir().unwrap();
        let inv3 = ToolInventory::detect_with(tmp3.path(), ""); // polymarket missing
        let pf3 = build_preflight(
            "\u{3010}Prediction Signal\u{3011} Polymarket BUY YES",
            &inv3,
        );
        assert!(!pf3.selection_required);
        let installs: Vec<&Reminder> = pf3
            .reminders
            .iter()
            .filter(|r| r.kind == ReminderKind::InstallPlugin)
            .collect();
        assert_eq!(installs.len(), 1);
        assert_eq!(installs[0].tool, Some(ExecutionTool::PolymarketPlugin));
        assert!(pf3
            .reminders
            .iter()
            .all(|r| r.kind != ReminderKind::ReadinessAdvisory));
    }

    // ── reminders: merged across classes lists all triggering classes ─────
    #[test]
    fn reminders_merged_across_classes() {
        let inv = ready_inventory(); // trade kit missing
                                     // option + <another Trade-Kit-only-ish>… option is Trade Kit only; add a
                                     // second Trade-Kit-only path via option twice won't merge classes. Instead
                                     // use option + prediction-with-explicit-tradekit so Trade Kit install merges.
        let pf = build_preflight(
            "\u{3010}Options Signal\u{3011} buy BTC call option; also okx-trade CEX spot signal entry",
            &inv,
        );
        // Trade Kit is the single effective tool for option AND (explicit) for spot.
        let tk_install: Vec<&Reminder> = pf
            .reminders
            .iter()
            .filter(|r| {
                r.kind == ReminderKind::InstallPlugin && r.tool == Some(ExecutionTool::TradeKit)
            })
            .collect();
        assert_eq!(tk_install.len(), 1, "one merged Trade Kit install reminder");
        assert!(tk_install[0].asset_classes.contains(&AssetClass::Spot));
        assert!(tk_install[0].asset_classes.contains(&AssetClass::Option));
    }

    // ── shared-entry reuse equivalence (AC-9) ──────────────────────────────
    #[test]
    fn build_preflight_from_classes_matches_description_derived() {
        let inv = ready_inventory();
        let desc = "prediction market signal, buy YES, entry"; // → [prediction]
        let from_desc = build_preflight(desc, &inv);
        let from_classes = build_preflight_from_classes(&[AssetClass::Prediction], &[], &inv);
        assert_eq!(from_desc.tools, from_classes.tools);
        assert_eq!(from_desc.reminders, from_classes.reminders);
        assert_eq!(from_desc.asset_classes, from_classes.asset_classes);
        assert_eq!(from_desc.is_trading_signal, from_classes.is_trading_signal);
    }

    // ── degraded shape ─────────────────────────────────────────────────────
    #[test]
    fn degraded_preflight_shape() {
        let pf = degraded_preflight();
        let wire = serde_json::to_value(&pf).unwrap();
        assert_eq!(wire["schemaVersion"], 3);
        assert_eq!(wire["tradeKitProbe"]["mode"], "not_applicable");
        assert!(!pf.is_trading_signal);
        assert!(pf.asset_classes.is_empty());
        assert!(pf.tools.is_empty());
        assert!(pf.reminders.is_empty());
        assert!(pf.advisory_only);
        assert_eq!(pf.evidence, vec!["preflight:unavailable".to_string()]);
    }

    // ── credential-hygiene: evidence carries only fixed codes ──────────────
    #[test]
    fn evidence_never_contains_raw_description() {
        let inv = ready_inventory();
        let desc = "\u{3010}Spot Signal\u{3011} buy SECRETKEY0xdeadbeef entry 60000";
        let pf = build_preflight(desc, &inv);
        for code in &pf.evidence {
            assert!(
                !code.contains("SECRETKEY"),
                "evidence leaked raw text: {code}"
            );
            assert!(
                !code.contains("deadbeef"),
                "evidence leaked raw text: {code}"
            );
            // every code is from the fixed vocabulary
            assert!(
                code.ends_with(":header")
                    || code.ends_with(":description")
                    || code.starts_with("tool:")
                    || code == "preflight:unavailable",
                "unexpected evidence code: {code}"
            );
        }
    }

    // ── candidate_tools is the map (FR-4) ──────────────────────────────────
    #[test]
    fn candidate_tools_map() {
        assert_eq!(
            candidate_tools(AssetClass::Spot),
            &[ExecutionTool::Onchainos, ExecutionTool::TradeKit]
        );
        assert_eq!(
            candidate_tools(AssetClass::Perp),
            &[ExecutionTool::HyperliquidPlugin, ExecutionTool::TradeKit]
        );
        assert_eq!(
            candidate_tools(AssetClass::Prediction),
            &[ExecutionTool::PolymarketPlugin, ExecutionTool::TradeKit]
        );
        assert_eq!(
            candidate_tools(AssetClass::Option),
            &[ExecutionTool::TradeKit]
        );
        assert_eq!(
            candidate_tools(AssetClass::Defi),
            &[ExecutionTool::Onchainos]
        );
    }

    // ── serialized wire shape matches cli_command_spec.md (AC-10 offline) ──
    #[test]
    fn serialized_json_matches_contract_keys() {
        let tmp = tempfile::tempdir().unwrap();
        // Polymarket installed (dir with a real SKILL.md) → prediction candidate
        // Ready, Trade Kit missing.
        let poly_dir = tmp.path().join(".agents/skills/polymarket-plugin");
        std::fs::create_dir_all(&poly_dir).unwrap();
        std::fs::write(poly_dir.join("SKILL.md"), b"# polymarket").unwrap();
        let inv = ToolInventory::detect_with(tmp.path(), "");
        let pf = build_preflight(
            "\u{3010}Prediction Signal\u{3011} Polymarket BUY YES, entry",
            &inv,
        );
        let v = serde_json::to_value(&pf).unwrap();

        // Top-level keys are camelCase per the stability contract.
        for key in [
            "schemaVersion",
            "isTradingSignal",
            "assetClasses",
            "explicitTools",
            "selectionRequired",
            "advisoryOnly",
            "tools",
            "reminders",
            "tradeKitProbe",
            "evidence",
        ] {
            assert!(v.get(key).is_some(), "missing contract key {key}: {v}");
        }
        assert_eq!(v["isTradingSignal"], serde_json::json!(true));
        assert_eq!(v["assetClasses"], serde_json::json!(["prediction"]));
        assert_eq!(v["advisoryOnly"], serde_json::json!(true));
        // Closed-set enum values serialize to snake_case tokens.
        assert_eq!(v["explicitTools"], serde_json::json!(["polymarket_plugin"]));
        let tool0 = &v["tools"][0];
        assert_eq!(tool0["tool"], serde_json::json!("polymarket_plugin"));
        assert_eq!(tool0["displayName"], serde_json::json!("Polymarket"));
        assert_eq!(tool0["pluginId"], serde_json::json!("polymarket-plugin"));
        assert_eq!(tool0["readiness"], serde_json::json!("ready"));
        assert_eq!(tool0["reason"], serde_json::json!("ready"));
        assert!(tool0["checkedAt"].is_null());
        // Native tool omits pluginId.
        let tool1 = &v["tools"][1];
        assert_eq!(tool1["tool"], serde_json::json!("trade_kit"));
        assert!(
            tool1.get("pluginId").is_none(),
            "native tool must omit pluginId"
        );
        assert_eq!(tool1["readiness"], serde_json::json!("missing"));
        assert_eq!(tool1["reason"], serde_json::json!("cli_missing"));
        // Every reminder is non-blocking with both message languages present.
        for r in v["reminders"].as_array().unwrap() {
            assert_eq!(r["blocking"], serde_json::json!(false));
            assert!(!r["messageEn"].as_str().unwrap().is_empty());
            assert!(!r["messageZh"].as_str().unwrap().is_empty());
            assert!(r["kind"].is_string());
        }

        // Degraded sentinel shape.
        let dv = serde_json::to_value(degraded_preflight()).unwrap();
        assert_eq!(dv["evidence"], serde_json::json!(["preflight:unavailable"]));
    }
}
