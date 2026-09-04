//! Redacted local traces for arbitration contract verification.

use serde_json::{json, Map, Value};
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};

/// Record a complete local arbitration contract that does not cross the Task
/// HTTP client boundary, such as decision, choice, list, and detail results.
pub fn record_contract(kind: &str, request: &Value, response: Option<&Value>, error: Option<&str>) {
    let url = format!("onchainos://arbitration/{kind}");
    if is_heartbeat(&url) {
        return;
    }
    let envelope = json!({
        "error": error.map(redact_error),
        "req": redact_value(request),
        "res": response.map(redact_value).unwrap_or(Value::Null),
        "url": url,
    });
    persist(&url, &envelope);
}

fn is_heartbeat(value: &str) -> bool {
    let normalized = value.to_ascii_lowercase();
    normalized.contains("heartbeat")
        || normalized.contains("heart-beat")
        || normalized.contains("wakeup_notify")
        || normalized.contains("wakeup-notify")
}

fn persist(name: &str, envelope: &Value) {
    if let Ok(current_dir) = std::env::current_dir() {
        let _ = write_trace(&current_dir, name, envelope);
    }
}

fn write_trace(root: &Path, url: &str, envelope: &Value) -> std::io::Result<PathBuf> {
    let log_dir = root.join(".claude/logs");
    fs::create_dir_all(&log_dir)?;
    #[cfg(unix)]
    fs::set_permissions(
        &log_dir,
        std::os::unix::fs::PermissionsExt::from_mode(0o700),
    )?;

    let timestamp = chrono::Local::now().format("%Y%m%d%H%M%S");
    let slug = endpoint_slug(url);
    for suffix in 0..1000 {
        let suffix = if suffix == 0 {
            String::new()
        } else {
            format!("-{suffix}")
        };
        let path = log_dir.join(format!("{timestamp}-{slug}{suffix}.json"));
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
        "unable to allocate unique Task API trace filename",
    ))
}

fn endpoint_slug(url: &str) -> String {
    let path = url
        .split('?')
        .next()
        .unwrap_or(url)
        .trim_end_matches('/')
        .rsplit('/')
        .next()
        .unwrap_or("request");
    let slug = path
        .chars()
        .map(|ch| if ch.is_ascii_alphanumeric() { ch } else { '-' })
        .collect::<String>()
        .trim_matches('-')
        .to_ascii_lowercase();
    if slug.is_empty() {
        "request".to_string()
    } else {
        slug
    }
}

fn redact_url(url: &str) -> String {
    url.split('?').next().unwrap_or(url).to_string()
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
        other => other.clone(),
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
    fn recursively_redacts_secrets_but_preserves_arbitration_facts() {
        let value = json!({
            "sessionCert": "secret-cert",
            "nested": {"access_token": "token", "txHash": "0xabc"},
            "jobId": "job-1",
            "agentId": "asp-1",
        });
        let redacted = redact_value(&value);
        assert_eq!(redacted["sessionCert"], "[REDACTED]");
        assert_eq!(redacted["nested"]["access_token"], "[REDACTED]");
        assert_eq!(redacted["nested"]["txHash"], "0xabc");
        assert_eq!(redacted["jobId"], "job-1");
    }

    #[test]
    fn url_query_is_never_persisted() {
        assert_eq!(
            redact_url("https://example.test/task/1?sessionCert=secret"),
            "https://example.test/task/1"
        );
        assert!(redact_error("request failed: Authorization: Bearer abc")
            .starts_with("[REDACTED ERROR"));
    }

    #[test]
    fn writes_under_the_supplied_current_directory() {
        let test_root = std::env::current_dir()
            .unwrap()
            .join("target/api-trace-tests");
        std::fs::create_dir_all(&test_root).unwrap();
        let current_dir = tempfile::tempdir_in(test_root).unwrap();
        let path = write_trace(
            current_dir.path(),
            "https://example.test/task/job-1",
            &json!({"error": null, "req": {}, "res": {}, "url": "https://example.test/task/job-1"}),
        )
        .unwrap();
        assert_eq!(
            path.parent(),
            Some(current_dir.path().join(".claude/logs").as_path())
        );
    }
}
