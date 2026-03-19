use serde::Serialize;

#[derive(Serialize)]
struct JsonOutput<T: Serialize> {
    ok: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    data: Option<T>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<String>,
}

/// Print a success response: `{ "ok": true }`
pub fn success_empty() {
    let out: JsonOutput<()> = JsonOutput {
        ok: true,
        data: None,
        error: None,
    };
    println!("{}", serde_json::to_string_pretty(&out).unwrap());
}

/// Print a success response with data: `{ "ok": true, "data": ... }`
pub fn success<T: Serialize>(data: T) {
    let out = JsonOutput {
        ok: true,
        data: Some(data),
        error: None,
    };
    println!("{}", serde_json::to_string_pretty(&out).unwrap());
}

/// Print an error response: `{ "ok": false, "error": "<msg>" }`
pub fn error(msg: &str) {
    let out: JsonOutput<()> = JsonOutput {
        ok: false,
        data: None,
        error: Some(msg.to_string()),
    };
    println!("{}", serde_json::to_string_pretty(&out).unwrap());
}

// ── Confirming ───────────────────────────────────────────────────────

#[derive(Serialize)]
struct ConfirmingOutput {
    confirming: bool,
    message: String,
    next: String,
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
}
