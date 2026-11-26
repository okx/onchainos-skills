//! Real CLI auth flow against a loopback endpoint compiled through cli/.env.
//! Run with cargo test --locked --test cli_credential_refresh -- --ignored --nocapture --test-threads=1.
//! No production endpoint or real credentials are used.

mod common;

use aes_gcm::aead::{Aead, KeyInit};
use aes_gcm::{Aes256Gcm, Nonce};
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use common::{fresh_home, onchainos, parse_stdout_json};
use rand::RngCore;
use serde_json::{json, Value};
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::Path;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc, Mutex,
};
use std::time::Duration;

const REFRESH_PATH: &str = "/priapi/v5/wallet/agentic/auth/refresh";
const REPORT_PATH: &str = "/priapi/v5/wallet/agentic/pre-transaction/report-plugin-info";

fn jwt(exp: i64) -> String {
    format!(
        "{}.{}.fake",
        URL_SAFE_NO_PAD.encode(r#"{"alg":"HS256","typ":"JWT"}"#),
        URL_SAFE_NO_PAD.encode(json!({"exp": exp}).to_string())
    )
}

// Seed the actual encrypted-file format so the child exercises keyring_store.
fn cipher(home: &Path, salt: &[u8]) -> Aes256Gcm {
    let identity = std::fs::read_to_string(home.join("machine-identity")).unwrap();
    let mut key = [0u8; 32];
    scrypt::scrypt(
        identity.trim().as_bytes(),
        salt,
        &scrypt::Params::new(15, 8, 1, 32).unwrap(),
        &mut key,
    )
    .unwrap();
    Aes256Gcm::new_from_slice(&key).unwrap()
}

fn seed(home: &Path, access: &str, refresh: &str) {
    let mut random = [0u8; 76];
    rand::rngs::OsRng.fill_bytes(&mut random);
    std::fs::write(home.join("machine-identity"), hex::encode(&random[..32])).unwrap();
    let salt = &random[32..64];
    let nonce = &random[64..];
    let body = json!({
        "access_token": access,
        "refresh_token": refresh,
        "session_key": "fake-session-key-preserved"
    });
    let ciphertext = cipher(home, salt)
        .encrypt(Nonce::from_slice(nonce), body.to_string().as_bytes())
        .unwrap();
    let mut bytes = salt.to_vec();
    bytes.extend_from_slice(nonce);
    bytes.extend_from_slice(&ciphertext);
    std::fs::write(home.join("keyring.enc"), bytes).unwrap();
    std::fs::write(
        home.join("session.json"),
        json!({"sessionKeyExpireAt": (chrono::Utc::now().timestamp() + 3600).to_string()})
            .to_string(),
    )
    .unwrap();
}

fn stored(home: &Path) -> Value {
    let bytes = std::fs::read(home.join("keyring.enc")).unwrap();
    let plaintext = cipher(home, &bytes[..32])
        .decrypt(Nonce::from_slice(&bytes[32..44]), &bytes[44..])
        .unwrap();
    serde_json::from_slice(&plaintext).unwrap()
}

fn read_request(stream: &mut TcpStream) -> String {
    stream
        .set_read_timeout(Some(Duration::from_secs(5)))
        .unwrap();
    let mut bytes = Vec::new();
    let mut buf = [0u8; 4096];
    loop {
        let n = stream.read(&mut buf).expect("read mock request");
        assert!(n > 0, "incomplete mock request");
        bytes.extend_from_slice(&buf[..n]);
        if let Some(end) = bytes.windows(4).position(|s| s == b"\r\n\r\n") {
            let headers = String::from_utf8_lossy(&bytes[..end]);
            let length = headers
                .lines()
                .find_map(|l| {
                    l.to_ascii_lowercase()
                        .strip_prefix("content-length:")
                        .and_then(|v| v.trim().parse::<usize>().ok())
                })
                .unwrap_or(0);
            if bytes.len() >= end + 4 + length {
                return String::from_utf8(bytes).unwrap();
            }
        }
    }
}

fn bearer(request: &str) -> &str {
    request
        .split_once("\r\n\r\n")
        .unwrap()
        .0
        .lines()
        .filter_map(|line| line.split_once(':'))
        .find(|(name, _)| name.eq_ignore_ascii_case("authorization"))
        .unwrap()
        .1
        .trim()
        .strip_prefix("Bearer ")
        .unwrap()
}

#[derive(Clone, Copy, Debug)]
enum Mode {
    Success,
    Rejected,
    Timeout,
    Forced,
}

struct Mock {
    requests: Arc<Mutex<Vec<String>>>,
    stop: Arc<AtomicBool>,
    worker: Option<std::thread::JoinHandle<()>>,
}

impl Mock {
    fn start(address: &str, mode: Mode, access: String, refresh: String) -> Self {
        let listener = TcpListener::bind(address).expect("bind compiled loopback endpoint");
        listener.set_nonblocking(true).unwrap();
        let requests = Arc::new(Mutex::new(Vec::new()));
        let captured = Arc::clone(&requests);
        let stop = Arc::new(AtomicBool::new(false));
        let stopping = Arc::clone(&stop);
        let worker = std::thread::spawn(move || {
            let mut report_calls = 0;
            while !stopping.load(Ordering::Relaxed) {
                let (mut stream, _) = match listener.accept() {
                    Ok(s) => s,
                    Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                        std::thread::sleep(Duration::from_millis(10));
                        continue;
                    }
                    Err(e) => panic!("mock accept failed: {e}"),
                };
                let request = read_request(&mut stream);
                let is_refresh = request.starts_with(&format!("POST {REFRESH_PATH} "));
                assert!(is_refresh || request.starts_with(&format!("POST {REPORT_PATH} ")));
                captured.lock().unwrap().push(request);
                let body = if is_refresh {
                    match mode {
                        Mode::Timeout => {
                            // The real WalletApiClient transport timeout is 30s.
                            std::thread::sleep(Duration::from_secs(31));
                            continue;
                        }
                        Mode::Rejected => json!({"code": "50113", "msg": "mock refresh rejected"}),
                        _ => json!({"code": 0, "data": [{
                            "accessToken": access, "refreshToken": refresh
                        }]}),
                    }
                } else {
                    report_calls += 1;
                    if matches!(mode, Mode::Forced) && report_calls == 1 {
                        json!({"code": "10008", "msg": "Invalid access token"})
                    } else {
                        json!({"code": 0, "data": {"recorded": true}})
                    }
                }
                .to_string();
                let response = format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                );
                stream.write_all(response.as_bytes()).unwrap();
            }
        });
        Self {
            requests,
            stop,
            worker: Some(worker),
        }
    }
}

impl Drop for Mock {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        self.worker
            .take()
            .unwrap()
            .join()
            .expect("join mock server");
    }
}

#[test]
#[ignore = "requires a loopback-only cli/.env build; see implementation report reproduction recipe"]
fn credential_refresh_real_cli_mock_scenarios() {
    let base = env!("ONCHAINOS_COMPILED_BASE_URL");
    let address = base
        .strip_prefix("http://127.0.0.1:")
        .expect("refuse to test against a non-loopback endpoint");
    let port = address.parse::<u16>().expect("loopback port only");
    let address = format!("127.0.0.1:{port}");

    // Exact 181/180 and 301/300 boundaries use the deterministic unit clock.
    // CLI scenarios leave enough headroom for real time and encrypted-store IO.
    for (label, access_secs, refresh_secs, mode, refresh_calls, success) in [
        ("outside windows", 600, 900, Mode::Success, 0, true),
        ("access window", 120, 900, Mode::Success, 1, true),
        ("rolling refresh window", 600, 240, Mode::Success, 1, true),
        ("both windows", 120, 240, Mode::Success, 1, true),
        ("expired refresh", 600, 0, Mode::Success, 0, false),
        ("invalid refresh", 120, 900, Mode::Success, 0, false),
        ("server rejection", 120, 900, Mode::Rejected, 1, false),
        ("transport timeout", 120, 900, Mode::Timeout, 1, false),
        ("forced refresh and retry", 600, 900, Mode::Forced, 1, true),
    ] {
        let (_guard, home) = fresh_home("credential_refresh");
        let now = chrono::Utc::now().timestamp();
        let access = jwt(now + access_secs);
        let refresh = if label == "invalid refresh" {
            "invalid".to_string()
        } else {
            jwt(now + refresh_secs)
        };
        let new_access = jwt(now + 3600);
        let new_refresh = jwt(now + 30 * 86400);
        seed(&home, &access, &refresh);
        let original = std::fs::read(home.join("keyring.enc")).unwrap();
        let mock = Mock::start(&address, mode, new_access.clone(), new_refresh.clone());
        let output = onchainos()
            .env("ONCHAINOS_HOME", &home)
            .env("ONCHAINOS_CREDENTIAL_STORE", "file")
            .timeout(Duration::from_secs(45))
            .args([
                "wallet",
                "report-plugin-info",
                "--plugin-parameter",
                "mock-payload",
            ])
            .output()
            .expect("run credential-refresh CLI");
        assert_eq!(output.status.success(), success, "{label}: {output:?}");
        let result = parse_stdout_json(&output);
        assert_eq!(result["ok"], success, "{label}: {result}");
        let requests = mock.requests.lock().unwrap().clone();
        let refresh_requests: Vec<_> = requests
            .iter()
            .filter(|r| r.starts_with(&format!("POST {REFRESH_PATH} ")))
            .collect();
        assert_eq!(
            refresh_requests.len(),
            refresh_calls,
            "{label}: {requests:?}"
        );
        for request in refresh_requests {
            let body: Value =
                serde_json::from_str(request.split_once("\r\n\r\n").unwrap().1).unwrap();
            assert_eq!(body, json!({"refreshToken": refresh}), "{label}");
        }
        let reports: Vec<_> = requests
            .iter()
            .filter(|r| r.starts_with(&format!("POST {REPORT_PATH} ")))
            .collect();
        assert_eq!(
            reports.len(),
            if !success {
                0
            } else if matches!(mode, Mode::Forced) {
                2
            } else {
                1
            },
            "{label}"
        );
        if success {
            assert_eq!(result["data"]["recorded"], true, "{label}");
            let effective = if refresh_calls == 0 {
                &access
            } else {
                &new_access
            };
            assert_eq!(bearer(reports.last().unwrap()), effective, "{label}");
            for report in &reports {
                let body: Value =
                    serde_json::from_str(report.split_once("\r\n\r\n").unwrap().1).unwrap();
                assert_eq!(body, json!({"pluginParameter": "mock-payload"}), "{label}");
            }
            if matches!(mode, Mode::Forced) {
                assert_eq!(bearer(reports[0]), access, "{label}");
                assert!(requests[1].starts_with(&format!("POST {REFRESH_PATH} ")));
            }
        } else {
            let error = String::from_utf8_lossy(&output.stdout);
            match mode {
                Mode::Timeout => assert!(error.contains("Network unavailable"), "{label}: {error}"),
                Mode::Rejected => assert!(
                    error.contains("50113") && error.contains("mock refresh rejected"),
                    "{label}: {error}"
                ),
                _ => {
                    assert!(error.contains("onchainos wallet login"), "{label}: {error}");
                    assert!(
                        String::from_utf8_lossy(&output.stderr).contains("Session expired"),
                        "{label}"
                    );
                }
            }
        }
        let saved = stored(&home);
        if success && refresh_calls == 1 {
            assert_eq!(saved["access_token"], new_access, "{label}");
            assert_eq!(saved["refresh_token"], new_refresh, "{label}");
        } else {
            assert_eq!(
                std::fs::read(home.join("keyring.enc")).unwrap(),
                original,
                "{label}: credentials changed"
            );
            assert_eq!(saved["access_token"], access);
            assert_eq!(saved["refresh_token"], refresh);
        }
        assert_eq!(
            saved["session_key"], "fake-session-key-preserved",
            "{label}"
        );
        println!(
            "PASS: {label}; refresh calls={refresh_calls}; report calls={}",
            reports.len()
        );
        drop(mock);
    }
}
