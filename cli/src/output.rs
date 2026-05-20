use serde::Serialize;
use serde_json::Value;

use crate::payment_notify;

#[derive(Serialize)]
struct JsonOutput<T: Serialize> {
    ok: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    data: Option<T>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<String>,
    /// Payment / state notifications emitted during the request. See
    /// `payment_notify` for the event schema. Absent when empty.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    notifications: Vec<Value>,
}

/// Print a success response: `{ "ok": true }`
pub fn success_empty() {
    let out: JsonOutput<()> = JsonOutput {
        ok: true,
        data: None,
        error: None,
        notifications: payment_notify::drain_events(),
    };
    println!("{}", serde_json::to_string_pretty(&out).unwrap());
}

/// Print a success response with data: `{ "ok": true, "data": ... }`
pub fn success<T: Serialize>(data: T) {
    let out = JsonOutput {
        ok: true,
        data: Some(data),
        error: None,
        notifications: payment_notify::drain_events(),
    };
    println!("{}", serde_json::to_string_pretty(&out).unwrap());
}

/// Print an error response: `{ "ok": false, "error": "<msg>" }`
pub fn error(msg: &str) {
    let out: JsonOutput<()> = JsonOutput {
        ok: false,
        data: None,
        error: Some(msg.to_string()),
        notifications: payment_notify::drain_events(),
    };
    println!("{}", serde_json::to_string_pretty(&out).unwrap());
}

// ── Confirming ───────────────────────────────────────────────────────

#[derive(Serialize)]
struct ConfirmingOutput {
    confirming: bool,
    #[serde(skip_serializing_if = "String::is_empty")]
    message: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    next: String,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    notifications: Vec<Value>,
}

/// Print a confirming response:
/// `{ "confirming": true, "message": "...", "next": "..." }`
///
/// Used when the backend returns an error code that requires explicit user
/// confirmation before proceeding. The agent reads this, prompts the user,
/// and follows the `next` instructions if the user confirms.
pub fn confirming(message: &str, next: &str) {
    let out = ConfirmingOutput {
        confirming: true,
        message: message.to_string(),
        next: next.to_string(),
        notifications: payment_notify::drain_events(),
    };
    println!("{}", serde_json::to_string_pretty(&out).unwrap());
}

/// Structured error type for CLI operations that require user confirmation.
///
/// When a command handler detects a confirmable condition (e.g., API returns
/// error code 81362 and `--force` was not set), it returns this error.
/// `main.rs` intercepts it via `downcast` to call `output::confirming()`
/// and exit with code 2.
#[derive(Debug)]
pub struct CliConfirming {
    pub message: String,
    pub next: String,
}

impl std::fmt::Display for CliConfirming {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "confirming: {}", self.message)
    }
}

impl std::error::Error for CliConfirming {}

// ── SetupRequired (exit code 3) ───────────────────────────────────────
//
// Used when a third-party plugin invokes `wallet send` / `wallet contract-call`
// with `--force` on a chain where Gas Station first-time setup is required.
// `--force` semantics says "skip all confirmations" — but first-time GS setup
// is a contractual user-decision gate that cannot be silently auto-confirmed.
// Instead of returning a Confirming (exit 2 — broken for plugins that bail on
// non-zero exit), we return a structured error with `errorCode` so the agent
// can detect the GS setup gap, run `wallet gas-station setup`, then re-invoke
// the plugin command (which will succeed because GS is now active).

/// Print a setup-required response:
/// `{ "ok": false, "errorCode": "...", "message": "...", "data": { ... } }`
pub fn setup_required(error_code: &str, message: &str, data: &serde_json::Value) {
    let v = serde_json::json!({
        "ok": false,
        "errorCode": error_code,
        "message": message,
        "data": data,
    });
    println!("{}", serde_json::to_string_pretty(&v).unwrap());
}

/// Structured error type for CLI operations that require Gas Station setup
/// to be completed before re-attempting. main.rs intercepts via downcast,
/// prints via `output::setup_required()`, and exits with code 3.
#[derive(Debug)]
pub struct CliSetupRequired {
    pub error_code: String,
    pub message: String,
    pub data: serde_json::Value,
}

impl std::fmt::Display for CliSetupRequired {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "setup-required: {}", self.message)
    }
}

impl std::error::Error for CliSetupRequired {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cli_confirming_display() {
        let c = CliConfirming {
            message: "are you sure?".to_string(),
            next: "re-run with --force".to_string(),
        };
        assert_eq!(format!("{c}"), "confirming: are you sure?");
    }

    #[test]
    fn cli_confirming_downcast_from_anyhow() {
        let err: anyhow::Error = CliConfirming {
            message: "msg".to_string(),
            next: "next".to_string(),
        }
        .into();
        let downcasted = err.downcast_ref::<CliConfirming>();
        assert!(downcasted.is_some());
        let c = downcasted.unwrap();
        assert_eq!(c.message, "msg");
        assert_eq!(c.next, "next");
    }

    #[test]
    fn cli_setup_required_display() {
        let s = CliSetupRequired {
            error_code: "GAS_STATION_SETUP_REQUIRED".to_string(),
            message: "first-time setup needed".to_string(),
            data: serde_json::json!({}),
        };
        assert_eq!(format!("{s}"), "setup-required: first-time setup needed");
    }

    #[test]
    fn cli_setup_required_downcast_from_anyhow() {
        let err: anyhow::Error = CliSetupRequired {
            error_code: "GAS_STATION_SETUP_REQUIRED".to_string(),
            message: "msg".to_string(),
            data: serde_json::json!({"chainId": "42161", "scene": "A"}),
        }
        .into();
        let downcasted = err.downcast_ref::<CliSetupRequired>();
        assert!(downcasted.is_some());
        let s = downcasted.unwrap();
        assert_eq!(s.error_code, "GAS_STATION_SETUP_REQUIRED");
        assert_eq!(s.message, "msg");
        assert_eq!(s.data["chainId"], "42161");
        assert_eq!(s.data["scene"], "A");
    }
}
