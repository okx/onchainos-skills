//! Order status enum + BE error code classification + execution-event
//! terminal-state helper. All integer-keyed; tech-design §5.4 / §5.5.

use anyhow::{anyhow, Result};
use std::fmt;

// ── Order status (TeeSaOpenOrderStatusEnum, 9 values) ──
// SPEEDING_UP (-4) removed 2026-05-08: BE shouldn't emit; if it does,
// TryFrom errors and status_label renders "unknown(-4)".

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(i32)]
pub enum OrderStatus {
    Expired = -7,
    Cancelling = -3,
    Cancelled = -2,
    Failed = -1,
    Trading = 0,
    Completed = 1,
    Creating = 2,
    Active = 3,
    Suspended = 4,
}

impl OrderStatus {
    /// Stable string form surfaced to humans / Agent. Snake-cased per
    /// tech-design.md §5.4 (matches the reference table).
    pub fn as_str(self) -> &'static str {
        match self {
            OrderStatus::Expired => "expired",
            OrderStatus::Cancelling => "cancelling",
            OrderStatus::Cancelled => "cancelled",
            OrderStatus::Failed => "failed",
            OrderStatus::Trading => "processing",
            OrderStatus::Completed => "completed",
            OrderStatus::Creating => "creating",
            OrderStatus::Active => "active",
            OrderStatus::Suspended => "suspended",
        }
    }

    /// Terminal states are immune to most subcommands (cancel, resume).
    pub fn is_terminal(self) -> bool {
        matches!(
            self,
            OrderStatus::Completed
                | OrderStatus::Cancelled
                | OrderStatus::Failed
                | OrderStatus::Expired
        )
    }
}

impl TryFrom<i32> for OrderStatus {
    type Error = anyhow::Error;

    fn try_from(value: i32) -> Result<Self> {
        Ok(match value {
            -7 => OrderStatus::Expired,
            -3 => OrderStatus::Cancelling,
            -2 => OrderStatus::Cancelled,
            -1 => OrderStatus::Failed,
            0 => OrderStatus::Trading,
            1 => OrderStatus::Completed,
            2 => OrderStatus::Creating,
            3 => OrderStatus::Active,
            4 => OrderStatus::Suspended,
            other => return Err(anyhow!("unknown OrderStatus integer: {other}")),
        })
    }
}

/// Convenience for callers that just need the snake_case string.
pub fn status_label(value: i32) -> String {
    OrderStatus::try_from(value)
        .map(|s| s.as_str().to_string())
        .unwrap_or_else(|_| format!("unknown({value})"))
}

// ── BE error codes (tech-design §5.5) ──

/// Only `UpgradeRequired` (60018) triggers retry — all others are fatal.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StrategyError {
    /// 100 — REQUEST_PARAM_ERROR
    RequestParam,
    /// 10019 — INSUFFICIENT_NATIVE_GAS_BALANCE → wallet's native gas token is
    /// below the BE-required minimum. The BE msg includes the required amount
    /// (e.g. `minAmount = 0.001`).
    InsufficientNativeGas,
    /// 10026 — JWT_TOKEN_VERIFY_FAILED → user must re-login
    JwtVerifyFailed,
    /// 10106 — CHAIN_NOT_SUPPORT_ERROR
    ChainNotSupported,
    /// 60002 — NO_ORDER_FOUND
    NoOrderFound,
    /// 60003 — LIMIT_ORDER_NO_AUTHORITY (may also indicate SA not yet activated; check msg)
    NoAuthority,
    /// 60006 — LIMIT_ORDER_OUT_LIMIT_FAIL → suggest cancelling first
    OutOfLimit,
    /// 60009 — LIMIT_ORDER_ILLIQUIDITY_ERROR
    Illiquidity,
    /// 60014 — LIMIT_ORDER_EXPIRED_CANNOT_OPERATE
    ExpiredCannotOperate,
    /// 60015 — LIMIT_ORDER_PENDING_CANNOT_OPERATE
    PendingCannotOperate,
    /// 60017 — LIMIT_ORDER_SUCCESS_CANNOT_OPERATE
    SuccessCannotOperate,
    /// 60018 — LIMIT_ORDER_TEE_SA_VERSION_UPGRADE_REQUIRED → trigger SD-A then retry once
    UpgradeRequired,
    /// 60030 — QUOTA_EXCEEDED
    QuotaExceeded,
    /// 100007 — TEE_SIGN_FAILURE
    TeeSignFailure,
    /// 100010 — ORDER_AMOUNT_TOO_SMALL → order USD value < BE-enforced
    /// minimum (currently $1 USD).
    OrderAmountTooSmall,
    /// 100012 — LIMIT_ORDER_INSUFFICIENT_BALANCE
    InsufficientBalance,
    /// Anything else — preserve the integer for diagnostics.
    Unknown(i32),
}

impl StrategyError {
    pub fn from_code(code: i32) -> Self {
        match code {
            100 => StrategyError::RequestParam,
            10019 => StrategyError::InsufficientNativeGas,
            10026 => StrategyError::JwtVerifyFailed,
            10106 => StrategyError::ChainNotSupported,
            60002 => StrategyError::NoOrderFound,
            60003 => StrategyError::NoAuthority,
            60006 => StrategyError::OutOfLimit,
            60009 => StrategyError::Illiquidity,
            60014 => StrategyError::ExpiredCannotOperate,
            60015 => StrategyError::PendingCannotOperate,
            60017 => StrategyError::SuccessCannotOperate,
            60018 => StrategyError::UpgradeRequired,
            60030 => StrategyError::QuotaExceeded,
            100007 => StrategyError::TeeSignFailure,
            100010 => StrategyError::OrderAmountTooSmall,
            100012 => StrategyError::InsufficientBalance,
            other => StrategyError::Unknown(other),
        }
    }

    /// User-facing one-liner. Stays English so SKILL.md prose can quote it
    /// directly; CN translation is the Skill's concern, not the CLI's.
    pub fn user_message(self) -> &'static str {
        match self {
            StrategyError::RequestParam => "Request parameters are invalid.",
            StrategyError::InsufficientNativeGas => {
                "Wallet's native token balance is too low to pay this chain's gas fees. \
                 Top up the native gas token (deposit, transfer from another account, \
                 or swap a stablecoin into native via `swap execute`) and retry."
            }
            StrategyError::JwtVerifyFailed => {
                "Session expired. Please run `onchainos wallet login` and retry."
            }
            StrategyError::ChainNotSupported => "This chain is not supported for limit orders.",
            StrategyError::NoOrderFound => "No matching order was found.",
            StrategyError::NoAuthority => {
                "Limit-order permission missing. Trader Mode may not be activated yet."
            }
            StrategyError::OutOfLimit => {
                "Pending order count is at the limit. Cancel some orders before creating new ones."
            }
            StrategyError::Illiquidity => "Insufficient liquidity to place this order.",
            StrategyError::ExpiredCannotOperate => "Order has expired and cannot be modified.",
            StrategyError::PendingCannotOperate => "Order is pending and cannot be modified.",
            StrategyError::SuccessCannotOperate => "Order already completed and cannot be modified.",
            StrategyError::UpgradeRequired => {
                "Trader Mode SA needs to be re-activated; CLI will handle this transparently."
            }
            StrategyError::QuotaExceeded => "Quota exceeded for this account.",
            StrategyError::TeeSignFailure => "TEE signing failed. Try again shortly.",
            StrategyError::OrderAmountTooSmall => {
                "Order value is below the minimum of $1 USD. Increase --amount and retry."
            }
            StrategyError::InsufficientBalance => "Insufficient balance to place this order.",
            StrategyError::Unknown(_) => "Unknown strategy error.",
        }
    }
}

/// Typed error from `check_response`. Callers downcast for `kind` —
/// no string-matching needed.
#[derive(Debug, Clone)]
pub struct StrategyApiError {
    pub code: i32,
    pub msg: String,
    pub kind: StrategyError,
}

impl fmt::Display for StrategyApiError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "BE strategy error code={}: {}", self.code, self.msg)
    }
}

impl std::error::Error for StrategyApiError {}

/// `code == 0` → Ok; otherwise return classified `StrategyApiError`.
pub fn check_response(value: &serde_json::Value) -> Result<()> {
    let code = value
        .get("code")
        .and_then(|v| v.as_i64())
        .map(|c| c as i32)
        .unwrap_or(0);
    if code == 0 {
        return Ok(());
    }
    let kind = StrategyError::from_code(code);
    let msg = value
        .get("msg")
        .and_then(|v| v.as_str())
        .unwrap_or(kind.user_message());
    let msg = crate::client::augment_auth_error_msg(&code.to_string(), msg);
    Err(StrategyApiError { code, msg, kind }.into())
}

/// True if `e` wraps a `StrategyApiError { kind: UpgradeRequired }`.
pub fn is_upgrade_required(e: &anyhow::Error) -> bool {
    e.downcast_ref::<StrategyApiError>()
        .map(|s| matches!(s.kind, StrategyError::UpgradeRequired))
        .unwrap_or(false)
}

// ── Execution events (executionHistoryList[].code) ──
//
// Catalog of TEE swap-trade engine event codes. The `message` column is the
// product-authored string that matches the OKX wallet UI; CLI inlines it
// into each `executionHistoryList` entry so the Agent doesn't need to
// look it up from a sidecar markdown file. Unknown codes fall through —
// callers leave whatever BE returned untouched.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ExecutionEvent {
    pub code: i32,
    pub name: &'static str,
    pub message: &'static str,
    pub is_terminal: bool,
}

/// Active series (3xxx, TEE engine) + legacy series (2xxx, old order status).
/// Source of truth: product design 2026-05-13.
#[rustfmt::skip]
const EXECUTION_EVENT_CATALOG: &[ExecutionEvent] = &[
    ExecutionEvent { code: 0,    name: "tradeSuccessed",        message: "Trade successful",                                                            is_terminal: false },
    ExecutionEvent { code: 3005, name: "lessThanMinReceive",    message: "Quoted price is below the minimum amount to receive",                        is_terminal: false },
    ExecutionEvent { code: 3006, name: "preExecutionFailed",    message: "Pre-execution error. Try again",                                              is_terminal: false },
    ExecutionEvent { code: 3007, name: "signFailed",            message: "Failed to verify signature",                                                  is_terminal: false },
    ExecutionEvent { code: 3008, name: "broadcastFailed",       message: "Broadcast failed",                                                            is_terminal: false },
    ExecutionEvent { code: 3010, name: "onchainFailed",         message: "The transaction broadcast was unsuccessful due to an onchain service error", is_terminal: true  },
    ExecutionEvent { code: 3013, name: "insufficientBalance",   message: "Insufficient funds in wallet",                                                is_terminal: false },
    ExecutionEvent { code: 3014, name: "insufficientLamports",  message: "Insufficient funds for network fee",                                          is_terminal: false },
    ExecutionEvent { code: 3015, name: "exceedSlippage",        message: "Price exceeded slippage at trade",                                            is_terminal: false },
    ExecutionEvent { code: 3016, name: "noLiquidty",            message: "No quote due to low liquidity",                                               is_terminal: false },
    ExecutionEvent { code: 3017, name: "unableQuote",           message: "Unable to fetch a quote",                                                     is_terminal: false },
    ExecutionEvent { code: 3018, name: "mevFail",               message: "Anti-MEV provider error",                                                     is_terminal: false },
    ExecutionEvent { code: 3019, name: "riskToken",             message: "Failed to trade due to risky token",                                          is_terminal: true  },
    ExecutionEvent { code: 3020, name: "blackAddress",          message: "Failed to trade due to blocklisted address",                                  is_terminal: true  },
    ExecutionEvent { code: 3023, name: "orderExpired",          message: "Limit order expired",                                                         is_terminal: true  },
    ExecutionEvent { code: 2001, name: "oldCreated",            message: "Order created",                                                               is_terminal: false },
    ExecutionEvent { code: 2002, name: "oldFailedToCreate",     message: "Failed to create order",                                                      is_terminal: false },
    ExecutionEvent { code: 2003, name: "oldEdited",             message: "Order modified",                                                              is_terminal: false },
    ExecutionEvent { code: 2004, name: "oldFailedToEdit",       message: "Failed to edit order",                                                        is_terminal: false },
    ExecutionEvent { code: 2005, name: "oldCanceled",           message: "Order canceled",                                                              is_terminal: false },
    ExecutionEvent { code: 2006, name: "oldFailedToCancel",     message: "Unable to cancel order",                                                      is_terminal: false },
    ExecutionEvent { code: 2007, name: "oldAutoCanceled",       message: "Order auto-canceled",                                                         is_terminal: false },
    ExecutionEvent { code: 2008, name: "oldFailedToAutoCancel", message: "Unable to auto-cancel order",                                                 is_terminal: false },
    ExecutionEvent { code: 2009, name: "oldExpired",            message: "Order expired",                                                               is_terminal: false },
    ExecutionEvent { code: 2010, name: "oldExceedsSlippage",    message: "Price exceeded slippage at trade",                                            is_terminal: false },
    ExecutionEvent { code: 2011, name: "oldNoQuoteLowLiquidity",message: "No quote due to low liquidity",                                               is_terminal: false },
    ExecutionEvent { code: 2012, name: "oldBroadcastFailed",    message: "Broadcast failed",                                                            is_terminal: false },
    ExecutionEvent { code: 2013, name: "oldSuccessful",         message: "Trade successful",                                                            is_terminal: false },
];

/// `None` ⇒ unknown code; caller should leave BE's raw fields as-is.
pub fn execution_event_for(code: i32) -> Option<&'static ExecutionEvent> {
    EXECUTION_EVENT_CATALOG.iter().find(|e| e.code == code)
}

/// Terminal = TEE engine will not retry; Agent should stop polling.
/// Unknown codes default to non-terminal (engine assumed to retry).
pub fn is_terminal_event(code: i32) -> bool {
    execution_event_for(code).map(|e| e.is_terminal).unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    // ── OrderStatus ──────────────────────────────────────────────

    /// Every documented integer maps to an enum and back to itself —
    /// crucial because `0 = TRADING` is a common miss.
    /// Note: -4 SpeedingUp removed 2026-05-08 — see file header comment.
    #[test]
    fn all_documented_values_roundtrip() {
        for &(int, _name) in &[
            (-7, "expired"),
            (-3, "cancelling"),
            (-2, "cancelled"),
            (-1, "failed"),
            (0, "processing"),
            (1, "completed"),
            (2, "creating"),
            (3, "active"),
            (4, "suspended"),
        ] {
            let s: OrderStatus = OrderStatus::try_from(int).expect("known int");
            assert_eq!(s as i32, int, "roundtrip int -> enum -> int");
        }
    }

    #[test]
    fn speeding_up_is_no_longer_recognized() {
        // -4 must Err now — product decision: SPEEDING_UP removed.
        assert!(OrderStatus::try_from(-4).is_err());
    }

    #[test]
    fn as_str_matches_tech_design_table() {
        assert_eq!(OrderStatus::Trading.as_str(), "processing");
        assert_eq!(OrderStatus::Creating.as_str(), "creating");
        assert_eq!(OrderStatus::Active.as_str(), "active");
        assert_eq!(OrderStatus::Suspended.as_str(), "suspended");
        assert_eq!(OrderStatus::Completed.as_str(), "completed");
        assert_eq!(OrderStatus::Cancelled.as_str(), "cancelled");
        assert_eq!(OrderStatus::Failed.as_str(), "failed");
        assert_eq!(OrderStatus::Expired.as_str(), "expired");
        assert_eq!(OrderStatus::Cancelling.as_str(), "cancelling");
    }

    #[test]
    fn unknown_integer_returns_err() {
        assert!(OrderStatus::try_from(999).is_err());
        assert!(OrderStatus::try_from(-100).is_err());
    }

    #[test]
    fn terminal_states() {
        assert!(OrderStatus::Completed.is_terminal());
        assert!(OrderStatus::Cancelled.is_terminal());
        assert!(OrderStatus::Failed.is_terminal());
        assert!(OrderStatus::Expired.is_terminal());
        assert!(!OrderStatus::Active.is_terminal());
        assert!(!OrderStatus::Trading.is_terminal()); // 0 is NOT terminal
        assert!(!OrderStatus::Suspended.is_terminal());
        assert!(!OrderStatus::Creating.is_terminal());
        assert!(!OrderStatus::Cancelling.is_terminal());
    }

    #[test]
    fn status_label_handles_unknown() {
        assert_eq!(status_label(0), "processing");
        assert_eq!(status_label(999), "unknown(999)");
    }

    // ── StrategyError ────────────────────────────────────────────

    /// Covers the full §5.5 table — if BE adds a new code, this test
    /// shouldn't regress (it'll fall through to `Unknown(_)`).
    #[test]
    fn every_documented_code_classifies() {
        let cases = [
            (100, StrategyError::RequestParam),
            (10019, StrategyError::InsufficientNativeGas),
            (10026, StrategyError::JwtVerifyFailed),
            (10106, StrategyError::ChainNotSupported),
            (60002, StrategyError::NoOrderFound),
            (60003, StrategyError::NoAuthority),
            (60006, StrategyError::OutOfLimit),
            (60009, StrategyError::Illiquidity),
            (60014, StrategyError::ExpiredCannotOperate),
            (60015, StrategyError::PendingCannotOperate),
            (60017, StrategyError::SuccessCannotOperate),
            (60018, StrategyError::UpgradeRequired),
            (60030, StrategyError::QuotaExceeded),
            (100007, StrategyError::TeeSignFailure),
            (100010, StrategyError::OrderAmountTooSmall),
            (100012, StrategyError::InsufficientBalance),
        ];
        for (code, expected) in cases {
            assert_eq!(StrategyError::from_code(code), expected, "code {code}");
        }
    }

    #[test]
    fn unknown_code_preserves_integer() {
        match StrategyError::from_code(424242) {
            StrategyError::Unknown(c) => assert_eq!(c, 424242),
            _ => panic!("expected Unknown(_)"),
        }
    }

    #[test]
    fn check_response_passes_on_code_zero() {
        let v = json!({ "code": 0, "msg": "ok", "data": {} });
        assert!(check_response(&v).is_ok());
    }

    #[test]
    fn check_response_errors_on_nonzero() {
        let v = json!({ "code": 60018, "msg": "upgrade required", "data": null });
        let err = check_response(&v).unwrap_err();
        let s = format!("{err:#}");
        assert!(s.contains("60018"), "must include numeric code: {s}");
    }

    #[test]
    fn check_response_handles_missing_msg() {
        let v = json!({ "code": 60002 });
        assert!(check_response(&v).is_err());
    }

    #[test]
    fn is_upgrade_required_detects_60018_via_typed_error() {
        let v = json!({ "code": 60018, "msg": "upgrade required" });
        let err = check_response(&v).unwrap_err();
        assert!(super::is_upgrade_required(&err));
    }

    #[test]
    fn is_upgrade_required_rejects_other_codes() {
        let v = json!({ "code": 60002, "msg": "no order" });
        let err = check_response(&v).unwrap_err();
        assert!(!super::is_upgrade_required(&err));
    }

    #[test]
    fn is_upgrade_required_rejects_unrelated_anyhow_errors() {
        // Stringly-formatted errors no longer trigger upgrade — strategy goes
        // through `*_with_headers_raw` + typed `check_response`, so any
        // non-typed error must NOT be treated as 60018.
        let err = anyhow::anyhow!("API error (code=60018): upgrade required");
        assert!(!super::is_upgrade_required(&err));
        let err = anyhow::anyhow!("network down");
        assert!(!super::is_upgrade_required(&err));
    }

    // ── is_terminal_event ────────────────────────────────────────

    #[test]
    fn terminal_event_codes_are_recognized() {
        assert!(super::is_terminal_event(3010), "3010 ImportPriceLevelExpired");
        assert!(super::is_terminal_event(3019), "3019 riskToken");
        assert!(super::is_terminal_event(3020), "3020 blackAddress");
        assert!(super::is_terminal_event(3023), "3023 orderExpired");
    }

    #[test]
    fn transient_event_codes_are_not_terminal() {
        // Engine retries these — agent should keep waiting, not surface.
        assert!(!super::is_terminal_event(3015), "3015 exceedSlippage");
        assert!(!super::is_terminal_event(3017), "3017 unableQuote");
        assert!(!super::is_terminal_event(3018), "3018 mevFail");
    }

    #[test]
    fn unknown_event_code_defaults_to_non_terminal() {
        // Safer default: unknown codes should not stop the agent from
        // polling — they might be a new transient state we haven't typed yet.
        assert!(!super::is_terminal_event(0), "0 = tradeSuccessed (not terminal-failure)");
        assert!(!super::is_terminal_event(9999));
        assert!(!super::is_terminal_event(-1));
    }

    // ── ExecutionEvent catalog ───────────────────────────────────

    #[test]
    fn execution_event_catalog_hot_codes() {
        let e = super::execution_event_for(3016).expect("3016 in catalog");
        assert_eq!(e.name, "noLiquidty");
        assert_eq!(e.message, "No quote due to low liquidity");
        assert!(!e.is_terminal);

        let e = super::execution_event_for(0).expect("0 in catalog");
        assert_eq!(e.name, "tradeSuccessed");
        assert_eq!(e.message, "Trade successful");

        let e = super::execution_event_for(3023).expect("3023 in catalog");
        assert_eq!(e.message, "Limit order expired");
        assert!(e.is_terminal);
    }

    #[test]
    fn execution_event_unknown_code_passes_through() {
        // Product-design rule: unknown codes => CLI does NOT fabricate a
        // message, caller leaves whatever BE returned as-is.
        assert!(super::execution_event_for(9999).is_none());
        assert!(super::execution_event_for(-42).is_none());
    }

    #[test]
    fn execution_event_terminal_set_matches_is_terminal_event() {
        // Single source of truth: is_terminal_event delegates to the catalog.
        for &code in &[3010, 3019, 3020, 3023] {
            assert!(super::is_terminal_event(code), "{code} should be terminal");
            assert!(super::execution_event_for(code).unwrap().is_terminal);
        }
    }

    #[test]
    fn execution_event_legacy_2xxx_codes_present() {
        // Old order-status series should still resolve so older orders'
        // history doesn't render as raw integers.
        assert_eq!(
            super::execution_event_for(2013).unwrap().message,
            "Trade successful"
        );
        assert_eq!(
            super::execution_event_for(2011).unwrap().message,
            "No quote due to low liquidity"
        );
    }
}
