//! Temporary local trace for the end-to-end ASP arbitration flow.
//!
//! Each business or API step is written as a separate redacted JSON file under
//! `/Users/oker/self/logs` so events from different CLI processes can be ordered
//! by timestamp and correlated by `jobId`.

use serde_json::{json, Map, Value};
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

const TRACE_DIR: &str = "/Users/oker/self/logs";
static TRACE_SEQUENCE: AtomicU64 = AtomicU64::new(0);

pub fn is_flow_event(event: &str) -> bool {
    matches!(
        event,
        "job_rejected"
            | "user_decision_job_rejected"
            | "sub_user_reject"
            | "user_decision_sub_user_reject"
            | "dispute_approved"
            | "job_disputed"
            | "sub_asp_dispute"
    )
}

pub fn record(
    stage: &str,
    job_id: &str,
    request: &Value,
    response: Option<&Value>,
    error: Option<&str>,
) {
    let now = chrono::Local::now();
    let envelope = json!({
        "timestamp": now.to_rfc3339_opts(chrono::SecondsFormat::Millis, true),
        "stage": stage,
        "jobId": job_id,
        "pid": std::process::id(),
        "error": error.map(redact_error),
        "req": redact_value(request),
        "res": response.map(redact_value).unwrap_or(Value::Null),
    });
    let _ = write_trace(
        Path::new(TRACE_DIR),
        &now.format("%Y%m%d-%H%M%S-%3f").to_string(),
        job_id,
        stage,
        &envelope,
    );
}

pub fn record_text(
    stage: &str,
    job_id: &str,
    request: &Value,
    response: &str,
    error: Option<&str>,
) {
    let response =
        serde_json::from_str(response).unwrap_or_else(|_| Value::String(response.to_string()));
    record(stage, job_id, request, Some(&response), error);
}

fn write_trace(
    log_dir: &Path,
    timestamp: &str,
    job_id: &str,
    stage: &str,
    envelope: &Value,
) -> std::io::Result<PathBuf> {
    fs::create_dir_all(log_dir)?;
    #[cfg(unix)]
    fs::set_permissions(log_dir, std::os::unix::fs::PermissionsExt::from_mode(0o700))?;

    let job = slug(job_id).chars().take(18).collect::<String>();
    let stage = slug(stage);
    for _ in 0..1000 {
        let sequence = TRACE_SEQUENCE.fetch_add(1, Ordering::Relaxed);
        let path = log_dir.join(format!(
            "{timestamp}-{}-{sequence:04}-{job}-{stage}.json",
            std::process::id()
        ));
        let mut options = OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        match options.open(&path) {
            Ok(mut file) => {
                serde_json::to_writer_pretty(&mut file, envelope)?;
                file.write_all(b"\n")?;
                return Ok(path);
            }
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(error),
        }
    }
    Err(std::io::Error::new(
        std::io::ErrorKind::AlreadyExists,
        "unable to allocate arbitration trace filename",
    ))
}

fn slug(value: &str) -> String {
    let value = value
        .chars()
        .map(|ch| if ch.is_ascii_alphanumeric() { ch } else { '-' })
        .collect::<String>()
        .trim_matches('-')
        .to_ascii_lowercase();
    if value.is_empty() {
        "unknown".to_string()
    } else {
        value
    }
}

fn redact_error(error: &str) -> String {
    let normalized = error.to_ascii_lowercase();
    if [
        "authorization",
        "bearer ",
        "sessioncert",
        "session_cert",
        "access_token",
        "accesstoken",
        "refresh_token",
        "refreshtoken",
        "privatekey",
        "private_key",
        "mnemonic",
        "seedphrase",
        "signature",
        "rawtransaction",
        "txbytes",
        "uopdata",
    ]
    .iter()
    .any(|needle| normalized.contains(needle))
    {
        "[REDACTED ERROR: sensitive material omitted]".to_string()
    } else {
        error.to_string()
    }
}

fn redact_value(value: &Value) -> Value {
    match value {
        Value::Object(object) => Value::Object(
            object
                .iter()
                .map(|(key, value)| {
                    let value = if is_sensitive_key(key) {
                        Value::String("[REDACTED]".to_string())
                    } else {
                        redact_value(value)
                    };
                    (key.clone(), value)
                })
                .collect::<Map<_, _>>(),
        ),
        Value::Array(values) => Value::Array(values.iter().map(redact_value).collect()),
        Value::String(value) => redact_string(value),
        other => other.clone(),
    }
}

fn redact_string(value: &str) -> Value {
    if let Ok(parsed) = serde_json::from_str::<Value>(value) {
        if parsed.is_object() || parsed.is_array() {
            return Value::String(
                serde_json::to_string(&redact_value(&parsed))
                    .unwrap_or_else(|_| "[REDACTED JSON]".to_string()),
            );
        }
    }

    let normalized = value.to_ascii_lowercase();
    if [
        "authorization",
        "bearer ",
        "sessioncert",
        "session_cert",
        "access_token",
        "accesstoken",
        "refresh_token",
        "refreshtoken",
        "privatekey",
        "private_key",
        "mnemonic",
        "seedphrase",
    ]
    .iter()
    .any(|needle| normalized.contains(needle))
    {
        Value::String("[REDACTED STRING: sensitive material omitted]".to_string())
    } else {
        Value::String(value.to_string())
    }
}

fn is_sensitive_key(key: &str) -> bool {
    let normalized = key
        .chars()
        .filter(|ch| ch.is_ascii_alphanumeric())
        .flat_map(char::to_lowercase)
        .collect::<String>();
    matches!(
        normalized.as_str(),
        "authorization"
            | "password"
            | "passcode"
            | "otp"
            | "verificationcode"
            | "accesstoken"
            | "refreshtoken"
            | "jwt"
            | "apikey"
            | "secret"
            | "clientsecret"
            | "privatekey"
            | "mnemonic"
            | "seed"
            | "seedphrase"
            | "sessionkey"
            | "sessioncert"
            | "signature"
            | "signatures"
            | "signedtx"
            | "rawtransaction"
            | "txbytes"
            | "unsignedhashlist"
            | "uopdata"
            | "uophash"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn redacts_sensitive_fields_inside_embedded_json_strings() {
        let input = json!({
            "extraData": "{\"jobId\":\"job-1\",\"msgForSign\":{\"sessionCert\":\"cert\",\"signature\":\"sig\"}}"
        });

        let redacted = redact_value(&input);
        let embedded = redacted["extraData"]
            .as_str()
            .expect("embedded JSON string");
        let parsed: Value = serde_json::from_str(embedded).expect("valid embedded JSON");

        assert_eq!(parsed["jobId"], "job-1");
        assert_eq!(parsed["msgForSign"]["sessionCert"], "[REDACTED]");
        assert_eq!(parsed["msgForSign"]["signature"], "[REDACTED]");
    }

    #[test]
    fn preserves_arbitration_reason_text() {
        assert_eq!(
            redact_value(&Value::String("交付物符合预期".to_string())),
            Value::String("交付物符合预期".to_string())
        );
    }
}
