use serde::Serialize;
use serde_json::Value;

use crate::commands::agent_commerce::task::common::deposit_qr::InsufficientBalanceError;
use crate::payment_notify;

/// Serialize agent-facing JSON. **Compact by default** — this JSON is consumed
/// by the agent (which renders it into tables/summaries for the user), so the
/// pretty-print whitespace is pure token overhead. Set `ONCHAINOS_PRETTY=1` to
/// get indented output when inspecting the CLI by hand.
pub fn to_agent_json<T: Serialize>(value: &T) -> serde_json::Result<String> {
    if std::env::var("ONCHAINOS_PRETTY").is_ok_and(|v| v == "1") {
        serde_json::to_string_pretty(value)
    } else {
        serde_json::to_string(value)
    }
}

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
    println!("{}", to_agent_json(&out).unwrap());
}

/// Print a success response with data: `{ "ok": true, "data": ... }`
pub fn success<T: Serialize>(data: T) {
    let out = JsonOutput {
        ok: true,
        data: Some(data),
        error: None,
        notifications: payment_notify::drain_events(),
    };
    println!("{}", to_agent_json(&out).unwrap());
}

#[derive(Debug)]
pub struct CliFundingBlocked {
    pub data: Value,
}

impl std::fmt::Display for CliFundingBlocked {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "insufficient balance")
    }
}

impl std::error::Error for CliFundingBlocked {}

pub fn error_data(data: Value) {
    let out = JsonOutput {
        ok: false,
        data: Some(data),
        error: None,
        notifications: payment_notify::drain_events(),
    };
    println!("{}", to_agent_json(&out).unwrap());
}

// ── Bespoke top-level `{ok,reason?}` (autotrade-grant-check) ──────────────
//
// A deliberately-frozen process contract (cli_command_spec.md / architecture.md
// §6): the polymarket plugin reads ONLY the top-level `ok`, so this must NOT be
// wrapped in the standard `data` envelope. The prints live here (not in the
// handler) so exit-code centralization stays in main.rs and the println-JSON
// lint — which whitelists output.rs — is satisfied.

/// Bespoke allow: `{"ok":true}` (no `data` wrapper, no notifications).
pub fn bespoke_ok() {
    println!("{}", serde_json::json!({ "ok": true }));
}

/// Bespoke deny: `{"ok":false,"reason":"<process-level reason>"}`.
/// NFR-3: `reason` must be a process-level description only — never file content.
pub fn bespoke_deny(reason: &str) {
    println!("{}", serde_json::json!({ "ok": false, "reason": reason }));
}

/// Print an error response: `{ "ok": false, "error": "<msg>" }`
pub fn error(msg: &str) {
    let out: JsonOutput<()> = JsonOutput {
        ok: false,
        data: None,
        error: Some(msg.to_string()),
        notifications: payment_notify::drain_events(),
    };
    println!("{}", to_agent_json(&out).unwrap());
}

/// Print a structured error response with a machine-readable code:
/// `{ "ok": false, "error": "<msg>", "errorCode": "<code>", "errorField": "<field>"? }`
///
/// Exit code 1 (the default error path — no `CliConfirming` / `CliSetupRequired`
/// downcast). `error` stays a human-readable string (backward-compatible with
/// every existing consumer); the sibling `errorCode` (+ optional `errorField`)
/// make the case machine-distinguishable, mirroring the `setup_required`
/// `errorCode` precedent. Used for the new validation errors (invalid_input,
/// upstream_error, no_leaderboards).
pub fn error_coded(code: &str, field: Option<&str>, message: &str) {
    error_coded_details(code, field, message, None, None);
}

/// Print a coded error with optional machine facts and safe read-only continuations.
pub fn error_coded_details(
    code: &str,
    field: Option<&str>,
    message: &str,
    data: Option<&Value>,
    next_steps: Option<&Value>,
) {
    let mut v = serde_json::json!({
        "ok": false,
        "error": message,
        "errorCode": code,
    });
    if let Some(f) = field {
        v["errorField"] = Value::String(f.to_string());
    }
    if let Some(data) = data {
        v["data"] = data.clone();
    }
    if let Some(next_steps) = next_steps {
        v["nextSteps"] = next_steps.clone();
    }
    let events = payment_notify::drain_events();
    if !events.is_empty() {
        v["notifications"] = Value::Array(events);
    }
    println!("{}", to_agent_json(&v).unwrap());
}

// ── Insufficient balance (deposit-address siblings) ───────────────────

/// Build the JSON envelope for an insufficient-balance error. Pure — the actual
/// print/notification handling lives in [`error_insufficient_balance`].
///
/// - `deposit_address == None`  -> `{"ok":false,"error":<message>}` (verbatim degrade, FR-6)
/// - `deposit_address == Some`  -> the base error plus the machine-readable
///   siblings `depositAddress` / `depositChain` / `currency` / `shortfall`.
fn insufficient_balance_json(ib: &InsufficientBalanceError) -> Value {
    match ib.deposit_address.as_deref() {
        None => serde_json::json!({ "ok": false, "error": ib.message }),
        Some(addr) => serde_json::json!({
            "ok": false,
            "error": ib.message,
            "depositAddress": addr,
            "depositChain": ib.deposit_chain,
            "currency": ib.currency,
            "shortfall": ib.shortfall,
        }),
    }
}

/// Print an insufficient-balance error to stdout. When no deposit address was
/// resolved, degrades to plain `error(&message)` — byte-for-byte today's output
/// (FR-6). With an address, emits the base error plus the four deposit siblings,
/// honoring pending notifications like `error_coded`/`setup_required`. Does NOT
/// call `process::exit` — exit stays centralized in `main.rs`.
pub fn error_insufficient_balance(ib: &InsufficientBalanceError) {
    match ib.deposit_address.as_ref() {
        None => error(&ib.message),
        Some(_) => {
            let mut v = insufficient_balance_json(ib);
            let events = payment_notify::drain_events();
            if !events.is_empty() {
                v["notifications"] = Value::Array(events);
            }
            println!("{}", to_agent_json(&v).unwrap());
        }
    }
}

// ── Confirming ───────────────────────────────────────────────────────

#[derive(Serialize)]
struct ConfirmingOutput {
    confirming: bool,
    /// Machine-readable outcome discriminator (e.g. Gas Station scene). When
    /// present, the agent maps it directly to the matching fixed copy / flow
    /// instead of parsing `message`/`next` prose.
    #[serde(skip_serializing_if = "Option::is_none")]
    scene: Option<String>,
    #[serde(skip_serializing_if = "String::is_empty")]
    message: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    next: String,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    notifications: Vec<Value>,
}

#[derive(Serialize)]
struct AgenticWalletConfirmingOutput {
    confirming: bool,
    scene: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    message: String,
    preview: Value,
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
    confirming_scene(message, next, None);
}

/// Like [`confirming`], but also emits a machine-readable `scene` discriminator
/// so the agent can map the outcome to fixed copy / flow without parsing prose.
pub fn confirming_scene(message: &str, next: &str, scene: Option<&str>) {
    let out = ConfirmingOutput {
        confirming: true,
        scene: scene.map(|s| s.to_string()),
        message: message.to_string(),
        next: next.to_string(),
        notifications: payment_notify::drain_events(),
    };
    println!("{}", to_agent_json(&out).unwrap());
}

/// Prints an Agentic Wallet confirmation with its structured operation preview.
pub(crate) fn agentic_wallet_confirming(message: &str, next: &str, scene: &str, preview: &Value) {
    let out = AgenticWalletConfirmingOutput {
        confirming: true,
        scene: scene.to_string(),
        message: message.to_string(),
        preview: preview.clone(),
        next: next.to_string(),
        notifications: payment_notify::drain_events(),
    };
    println!("{}", to_agent_json(&out).unwrap());
}

/// Structured error type for CLI operations that require user confirmation.
///
/// When a command handler detects a confirmable condition (e.g., API returns
/// error code 81362 and `--force` was not set), it returns this error.
/// `main.rs` intercepts it via `downcast` to call `output::confirming()`
/// and exit with code 2.
#[derive(Debug, Default)]
pub struct CliConfirming {
    pub message: String,
    pub next: String,
    /// Optional machine-readable outcome discriminator (e.g. `"gs_first_time"`).
    pub scene: Option<String>,
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
    println!("{}", to_agent_json(&v).unwrap());
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
            scene: None,
        };
        assert_eq!(format!("{c}"), "confirming: are you sure?");
    }

    #[test]
    fn cli_confirming_downcast_from_anyhow() {
        let err: anyhow::Error = CliConfirming {
            message: "msg".to_string(),
            next: "next".to_string(),
            scene: None,
        }
        .into();
        let downcasted = err.downcast_ref::<CliConfirming>();
        assert!(downcasted.is_some());
        let c = downcasted.unwrap();
        assert_eq!(c.message, "msg");
        assert_eq!(c.next, "next");
    }

    #[test]
    fn agentic_wallet_confirming_output_keeps_structured_preview() {
        let value = serde_json::to_value(AgenticWalletConfirmingOutput {
            confirming: true,
            scene: "btc_utxo_manage".to_string(),
            message: "review".to_string(),
            preview: serde_json::json!({"outpoints": ["a:0"]}),
            next: "onchainos wallet utxo lock --force".to_string(),
            notifications: Vec::new(),
        })
        .unwrap();
        assert_eq!(value["scene"], "btc_utxo_manage");
        assert_eq!(value["preview"]["outpoints"][0], "a:0");
        assert!(value.get("notifications").is_none());
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

    // ── error_insufficient_balance JSON shape ─────────────────────────
    use crate::commands::agent_commerce::task::common::deposit_qr::InsufficientBalanceError;

    // None (address unresolved) -> exactly {"ok":false,"error":…}, no siblings (FR-6).
    #[test]
    fn insufficient_balance_json_none_degrades_verbatim() {
        let ib = InsufficientBalanceError::new(
            "Insufficient USDT balance".to_string(),
            "USDT",
            50.0,
            0.0,
        );
        let v = insufficient_balance_json(&ib);
        assert_eq!(
            v,
            serde_json::json!({ "ok": false, "error": "Insufficient USDT balance" })
        );
        assert!(v.get("depositAddress").is_none());
        assert!(v.get("depositChain").is_none());
        assert!(v.get("currency").is_none());
        assert!(v.get("shortfall").is_none());
    }

    // Some(addr) -> base error + all four deposit siblings, depositChain=="XLayer".
    #[test]
    fn insufficient_balance_json_some_emits_siblings() {
        let mut ib = InsufficientBalanceError::new(
            "Insufficient USDT balance".to_string(),
            "USDT",
            50.0,
            0.0,
        );
        ib.deposit_address = Some("0xDEADBEEF".to_string());
        let v = insufficient_balance_json(&ib);
        assert_eq!(v["ok"], serde_json::json!(false));
        assert_eq!(v["error"], "Insufficient USDT balance");
        assert_eq!(v["depositAddress"], "0xDEADBEEF");
        assert_eq!(v["depositChain"], "XLayer");
        assert_eq!(v["currency"], "USDT");
        assert_eq!(v["shortfall"], "50");
    }
}
