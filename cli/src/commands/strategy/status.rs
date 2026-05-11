//! Order status enum + BE error code classification.
//!
//! Both topics share the same shape: integer keyed → typed enum + tests.
//! Keeping them in one file because:
//! - Both are referenced by every subcommand handler
//! - tech-design.md §5.4 / §5.5 lists them as a pair
//! - Neither is large enough to warrant its own file
//!
//! Sources:
//! - status: `.claude/strategyTrading/api/dex-list-orders.md` (Status enum section)
//! - errors: `.claude/strategyTrading/tech-design.md` §5.5

use anyhow::{anyhow, Result};
use std::fmt;

// ────────────────────────────────────────────────────────────────────
// Order status (9 values — TeeSaOpenOrderStatusEnum)
//
// SPEEDING_UP (-4) was removed 2026-05-08 — product decision: not surfaced
// to users and CLI does not advertise it as a filter option. If BE ever
// emits it, `TryFrom` will return Err and `status_label` will render
// "unknown(-4)".
//
// Why integer-keyed and not string-matched: tech-design.md §5.5 explicitly
// warns "do NOT match by msg string". The API contract for `status` is a
// number; matching anything else is brittle.
// ────────────────────────────────────────────────────────────────────

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

// ────────────────────────────────────────────────────────────────────
// BE error codes (tech-design §5.5)
// ────────────────────────────────────────────────────────────────────

/// Documented BE error codes that the CLI must distinguish.
///
/// `UpgradeRequired` (60018) is the **only** code that triggers retry —
/// see `trader_mode::retry_on_upgrade`. Every other variant is fatal and
/// bubbled to the user with a tailored message.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StrategyError {
    /// 100 — REQUEST_PARAM_ERROR
    RequestParam,
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
    /// 100012 — LIMIT_ORDER_INSUFFICIENT_BALANCE
    InsufficientBalance,
    /// Anything else — preserve the integer for diagnostics.
    Unknown(i32),
}

impl StrategyError {
    pub fn from_code(code: i32) -> Self {
        match code {
            100 => StrategyError::RequestParam,
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
            100012 => StrategyError::InsufficientBalance,
            other => StrategyError::Unknown(other),
        }
    }

    /// User-facing one-liner. Stays English so SKILL.md prose can quote it
    /// directly; CN translation is the Skill's concern, not the CLI's.
    pub fn user_message(self) -> &'static str {
        match self {
            StrategyError::RequestParam => "Request parameters are invalid.",
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
            StrategyError::InsufficientBalance => "Insufficient balance to place this order.",
            StrategyError::Unknown(_) => "Unknown strategy error.",
        }
    }
}

/// Concrete error returned by `check_response`. Carries the integer `code`,
/// the BE-supplied `msg`, and the classified `kind`.
///
/// Why a struct + impl Error: `retry_on_upgrade` must detect
/// `UpgradeRequired` to drive SD-A. Returning this through `anyhow::Error`
/// lets the wrapper `.downcast_ref::<StrategyApiError>()` and inspect
/// `kind` exactly — no string-matching on display output.
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

/// Inspect a JSON response. If `code != 0`, classify and return an `Err`
/// carrying a `StrategyApiError`.
///
/// Returns `Ok(())` when the response indicates success. Callers chain this
/// before reading `data` to keep handler bodies linear.
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
        .unwrap_or(kind.user_message())
        .to_string();
    Err(StrategyApiError { code, msg, kind }.into())
}

/// True when `e` is (or wraps) a `StrategyApiError` with `kind == UpgradeRequired`.
/// Used by `trader_mode::retry_on_upgrade` and the inline retry sites in
/// `handlers.rs`.
///
/// Strategy uses `ApiClient::*_with_headers_raw` and runs `check_response`
/// itself, so any non-zero BE code arrives as a typed `StrategyApiError` —
/// no string matching needed.
pub fn is_upgrade_required(e: &anyhow::Error) -> bool {
    e.downcast_ref::<StrategyApiError>()
        .map(|s| matches!(s.kind, StrategyError::UpgradeRequired))
        .unwrap_or(false)
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
}
