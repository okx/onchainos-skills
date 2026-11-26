//! Local request capture and scoped hostname fixtures for header regressions.
use std::cell::RefCell;

thread_local! {
    static HOSTNAME: RefCell<Option<Option<String>>> = const { RefCell::new(None) };
    static REFRESH_ENDPOINT: RefCell<Option<String>> = const { RefCell::new(None) };
}

pub fn refresh_endpoint() -> Option<String> {
    REFRESH_ENDPOINT.with(|value| value.borrow().clone())
}

pub fn with_refresh_endpoint<T>(url: &str, action: impl FnOnce() -> T) -> T {
    struct Restore(Option<String>);
    impl Drop for Restore {
        fn drop(&mut self) {
            REFRESH_ENDPOINT.with(|value| *value.borrow_mut() = self.0.take());
        }
    }
    let _restore = Restore(REFRESH_ENDPOINT.with(|value| value.replace(Some(url.to_string()))));
    action()
}

/// Caller holds TEST_ENV_MUTEX; restore each variable even if an assertion fails.
pub fn with_env<T>(vars: &[(&str, &str)], action: impl FnOnce() -> T) -> T {
    struct Restore(Vec<(String, Option<std::ffi::OsString>)>);
    impl Drop for Restore {
        fn drop(&mut self) {
            for (key, value) in self.0.iter().rev() {
                match value {
                    Some(value) => std::env::set_var(key, value),
                    None => std::env::remove_var(key),
                }
            }
        }
    }
    let _restore = Restore(
        vars.iter()
            .map(|(key, value)| {
                let old = std::env::var_os(key);
                std::env::set_var(key, value);
                (key.to_string(), old)
            })
            .collect(),
    );
    action()
}

pub fn with_test_home<T>(action: impl FnOnce(&std::path::Path) -> T) -> T {
    let _lock = crate::home::TEST_ENV_MUTEX
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("target/test_tmp");
    std::fs::create_dir_all(&root).unwrap();
    let dir = tempfile::tempdir_in(root).unwrap();
    with_env(
        &[
            ("ONCHAINOS_HOME", dir.path().to_str().unwrap()),
            ("ONCHAINOS_CREDENTIAL_STORE", "file"),
        ],
        || action(dir.path()),
    )
}

pub fn hostname_override() -> Option<Option<String>> {
    HOSTNAME.with(|value| value.borrow().clone())
}

/// Run on this thread (including a current-thread runtime), restoring even on panic.
/// `hostname` represents the normalized result, not a replacement system reader.
pub fn with_hostname<T>(hostname: Option<&str>, action: impl FnOnce() -> T) -> T {
    struct Restore(Option<Option<String>>);
    impl Drop for Restore {
        fn drop(&mut self) {
            HOSTNAME.with(|value| *value.borrow_mut() = self.0.take());
        }
    }
    let _restore = Restore(HOSTNAME.with(|value| value.replace(Some(hostname.map(str::to_owned)))));
    action()
}

pub fn header<'a>(request: &'a str, name: &str) -> Option<&'a str> {
    request
        .split_once("\r\n\r\n")
        .unwrap()
        .0
        .lines()
        .filter_map(|line| line.split_once(':'))
        .find(|(key, _)| key.eq_ignore_ascii_case(name))
        .map(|(_, value)| value.trim())
}

pub fn assert_platform(request: &str, platform: &str) {
    assert_eq!(header(request, "agent-platform"), Some(platform));
    assert_eq!(header(request, "ok-access-client-type"), Some("agent-cli"));
    assert_eq!(header(request, "platform"), Some("agent-cli"));
}

/// Capture local requests and return scripted responses without real credentials.
pub fn mock_http(responses: Vec<(u16, String)>) -> (String, std::thread::JoinHandle<Vec<String>>) {
    mock_http_with_content_types(
        responses
            .into_iter()
            .map(|(status, body)| (status, "application/json", body))
            .collect(),
    )
}

pub fn mock_http_with_content_types(
    responses: Vec<(u16, &'static str, String)>,
) -> (String, std::thread::JoinHandle<Vec<String>>) {
    mock_http_inner(responses, false)
}

/// Keep listening after the script to detect an unexpected replay, including queued requests.
pub fn mock_http_observe_replays(
    responses: Vec<(u16, String)>,
) -> (String, std::thread::JoinHandle<Vec<String>>) {
    mock_http_inner(
        responses
            .into_iter()
            .map(|(status, body)| (status, "application/json", body))
            .collect(),
        true,
    )
}

fn mock_http_inner(
    responses: Vec<(u16, &'static str, String)>,
    observe_replay: bool,
) -> (String, std::thread::JoinHandle<Vec<String>>) {
    use std::io::{Read, Write};
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    let server = std::thread::spawn(move || {
        let mut requests = Vec::new();
        'responses: for response in responses.into_iter().map(Some).chain(std::iter::once(None)) {
            let extra = response.is_none();
            if extra && !observe_replay {
                break;
            }
            let (status, content_type, body) =
                response.unwrap_or((500, "application/json", "unexpected replay".to_string()));
            let timeout = if extra {
                std::time::Duration::from_millis(500)
            } else {
                std::time::Duration::from_secs(30)
            };
            let deadline = std::time::Instant::now() + timeout;
            let mut stream = loop {
                match listener.accept() {
                    Ok((stream, _)) => break stream,
                    Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                        if std::time::Instant::now() >= deadline {
                            if extra {
                                break 'responses;
                            }
                            panic!("missing request");
                        }
                        std::thread::sleep(std::time::Duration::from_millis(5));
                    }
                    Err(e) => panic!("accept: {e}"),
                }
            };
            stream
                .set_read_timeout(Some(std::time::Duration::from_secs(30)))
                .unwrap();
            let mut request = Vec::new();
            loop {
                let mut buf = [0; 4096];
                let n = stream.read(&mut buf).unwrap();
                assert!(n > 0, "incomplete request");
                request.extend_from_slice(&buf[..n]);
                if let Some(end) = request.windows(4).position(|w| w == b"\r\n\r\n") {
                    let length = String::from_utf8_lossy(&request[..end])
                        .lines()
                        .filter_map(|line| line.split_once(':'))
                        .find(|(name, _)| name.eq_ignore_ascii_case("content-length"))
                        .map(|(_, value)| value.trim().parse::<usize>().unwrap())
                        .unwrap_or(0);
                    if request.len() >= end + 4 + length {
                        break;
                    }
                }
            }
            requests.push(String::from_utf8(request).unwrap());
            if status == 1 {
                // Return headers and a partial body before stalling the body read.
                stream.write_all(b"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: 100\r\nConnection: close\r\n\r\n{").unwrap();
                stream.flush().unwrap();
            }
            if status == 0 || status == 1 {
                // 0 stalls headers; 1 stalls the body. Both exceed the client timeout.
                std::thread::sleep(std::time::Duration::from_millis(300));
                continue;
            }
            write!(stream, "HTTP/1.1 {status} Test\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len()).unwrap();
        }
        requests
    });
    (url, server)
}
