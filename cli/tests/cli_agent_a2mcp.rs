//! Integration tests for the A2MCP service surface:
//! `onchainos agent a2mcp tool-list`, `onchainos agent a2mcp invoke`, and the
//! `onchainos agent service-list` display change (5 new backend fields).
//!
//! Driven by `integration-plan.csv` (rows IT-001 .. IT-023). Mock rows stand up
//! an in-process loopback HTTP/MCP server (the same `TcpListener` + `Connection:
//! close` pattern the crate's inline tests use — `src/.../mcp_tool_list.rs`,
//! `src/mcp_client.rs`), so they never touch the real network. Offline rows hit
//! pure validation/clap guards. Live rows read their ids from the environment
//! and skip when unset.
//!
//! CSV note: rows IT-006 / IT-018 / IT-020 name the subcommand
//! `agent a2mcp service-list`, but the real CLI has no such subcommand — the
//! `A2mcpCommand` enum exposes only `invoke` + `tool-list`, and `service-list`
//! is a direct `agent` subcommand (`AgentCommand::ServiceList`, per spec §1.3).
//! These tests therefore drive the real `agent service-list` command, which is
//! the only form that can satisfy each row's assertion.

mod common;

use base64::{engine::general_purpose::STANDARD, Engine};
use common::{
    assert_error_contains, assert_ok_and_extract_data, fresh_home, onchainos,
    run_scrubbed_with_retry, run_with_retry, scrubbed,
};
use serde_json::{json, Value};
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};

// ─── Mock HTTP / MCP endpoint harness ───────────────────────────────────────

/// Read one complete HTTP request (headers + Content-Length body). Best-effort:
/// returns whatever was read on EOF / error so the mock never panics.
fn read_http_request(stream: &mut TcpStream) -> String {
    let mut buf = Vec::new();
    let mut chunk = [0u8; 4096];
    loop {
        match stream.read(&mut chunk) {
            Ok(0) => break,
            Ok(n) => buf.extend_from_slice(&chunk[..n]),
            Err(_) => break,
        }
        if let Some(header_end) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
            let headers = String::from_utf8_lossy(&buf[..header_end]);
            let content_length = headers
                .lines()
                .find_map(|line| {
                    line.to_ascii_lowercase()
                        .strip_prefix("content-length:")
                        .and_then(|v| v.trim().parse::<usize>().ok())
                })
                .unwrap_or(0);
            if buf.len() >= header_end + 4 + content_length {
                break;
            }
        }
    }
    String::from_utf8_lossy(&buf).into_owned()
}

/// Write one HTTP response and close the connection (forces the client to open
/// a fresh connection for its next request, so each request maps to one accept).
fn write_http_response(stream: &mut TcpStream, status_line: &str, content_type: &str, body: &str) {
    let mut resp = format!("HTTP/1.1 {status_line}\r\n");
    if !content_type.is_empty() {
        resp.push_str(&format!("Content-Type: {content_type}\r\n"));
    }
    resp.push_str(&format!(
        "Content-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    ));
    let _ = stream.write_all(resp.as_bytes());
    let _ = stream.flush();
}

/// The JSON-RPC body of a request (everything after the header terminator).
fn request_body(req: &str) -> &str {
    req.split_once("\r\n\r\n").map(|(_, b)| b).unwrap_or("")
}

/// Extract `(id, method)` from a JSON-RPC request body. `id` is echoed back so
/// `McpToolListClient`'s id-matching (`parse_response_for_id`) succeeds.
fn parse_method_id(req: &str) -> (Option<u64>, String) {
    let value: Value = serde_json::from_str(request_body(req)).unwrap_or(Value::Null);
    let id = value.get("id").and_then(Value::as_u64);
    let method = value
        .get("method")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string();
    (id, method)
}

/// How the mock answers the `initialize` handshake.
#[derive(Clone, Copy)]
enum InitMode {
    /// Valid MCP server advertising `capabilities.tools` and a supported version.
    Mcp,
    /// A plain (non-MCP) JSON API — no `jsonrpc`/`id` envelope → classified HTTP.
    NotMcp,
    /// MCP server that returns a JSON-RPC `error` object to `initialize`.
    InitError,
    /// MCP server that negotiates a version other than the client's preferred version.
    AlternateProtocol,
}

/// Spawn a loopback MCP endpoint. Routes by JSON-RPC method; `tools/list`
/// paginates (first page carries `nextCursor`, the cursored page ends it) so
/// `--all` traversal is genuinely exercised; `tools/call` returns a free result.
/// Returns the endpoint URL. The server thread is detached and dies with the
/// test process.
fn spawn_mcp_mock(mode: InitMode) -> String {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind mcp mock");
    let addr = listener.local_addr().expect("mcp mock addr");
    let url = format!("http://{addr}/mcp");
    std::thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(mut stream) = stream else { break };
            let req = read_http_request(&mut stream);
            let (id, method) = parse_method_id(&req);
            let id = id.unwrap_or(1);
            match method.as_str() {
                "initialize" => match mode {
                    InitMode::NotMcp => write_http_response(
                        &mut stream,
                        "200 OK",
                        "application/json",
                        r#"{"price":"1.23"}"#,
                    ),
                    InitMode::InitError => write_http_response(
                        &mut stream,
                        "200 OK",
                        "application/json",
                        &format!(
                            r#"{{"jsonrpc":"2.0","id":{id},"error":{{"code":-32000,"message":"initialize rejected"}}}}"#
                        ),
                    ),
                    InitMode::AlternateProtocol => write_http_response(
                        &mut stream,
                        "200 OK",
                        "application/json",
                        &format!(
                            r#"{{"jsonrpc":"2.0","id":{id},"result":{{"protocolVersion":"1999-01-01","capabilities":{{"tools":{{}}}},"serverInfo":{{"name":"mock","version":"1.0"}}}}}}"#
                        ),
                    ),
                    InitMode::Mcp => write_http_response(
                        &mut stream,
                        "200 OK",
                        "application/json",
                        &format!(
                            r#"{{"jsonrpc":"2.0","id":{id},"result":{{"protocolVersion":"2025-11-25","capabilities":{{"tools":{{}}}},"serverInfo":{{"name":"mock","version":"1.0"}}}}}}"#
                        ),
                    ),
                },
                "notifications/initialized" => {
                    write_http_response(&mut stream, "202 Accepted", "", "")
                }
                "tools/list" => {
                    let body = if request_body(&req).contains("\"cursor\"") {
                        format!(
                            r#"{{"jsonrpc":"2.0","id":{id},"result":{{"tools":[{{"name":"beta","description":"Beta tool","inputSchema":{{"type":"object"}}}}]}}}}"#
                        )
                    } else {
                        format!(
                            r#"{{"jsonrpc":"2.0","id":{id},"result":{{"tools":[{{"name":"alpha","description":"Alpha tool","inputSchema":{{"type":"object"}}}}],"nextCursor":"page-2"}}}}"#
                        )
                    };
                    write_http_response(&mut stream, "200 OK", "application/json", &body);
                }
                "tools/call" => write_http_response(
                    &mut stream,
                    "200 OK",
                    "application/json",
                    &format!(
                        r#"{{"jsonrpc":"2.0","id":{id},"result":{{"content":[{{"type":"text","text":"ok"}}]}}}}"#
                    ),
                ),
                _ => write_http_response(&mut stream, "404 Not Found", "application/json", "{}"),
            }
        }
    });
    url
}

/// Spawn a plain HTTP endpoint that answers every request with a fixed status
/// and body — used for the HTTP `invoke` rows. Returns the endpoint URL.
fn spawn_http_mock(status_line: &'static str, content_type: &'static str, body: &'static str) -> String {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind http mock");
    let addr = listener.local_addr().expect("http mock addr");
    let url = format!("http://{addr}/svc");
    std::thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(mut stream) = stream else { break };
            let _ = read_http_request(&mut stream);
            write_http_response(&mut stream, status_line, content_type, body);
        }
    });
    url
}

/// A valid x402 402 challenge body (plain JSON — `decode_payment_blob` accepts
/// it). The single `accepts[]` entry is USDC/exact/eip3009 on X Layer so the
/// A2MCP candidate branding + `select`/`validate` gates all pass, leaving the
/// missing wallet session as the terminal failure. `extra.name` = "USDC" is the
/// offline symbol fallback when the token-metadata lookup yields nothing.
const CHALLENGE_402: &str = r#"{"resource":"https://merchant.example/x402","accepts":[{"scheme":"exact","network":"eip155:196","asset":"0x0000000000000000000000000000000000001234","payTo":"0x000000000000000000000000000000000000abcd","maxAmountRequired":"1000000","decimals":6,"extra":{"name":"USDC","assetTransferMethod":"eip3009","decimals":6}}]}"#;

/// Base64-encode a JSON value with the STANDARD (padded) alphabet, matching the
/// binary's `decode_json` (`STANDARD.decode`).
fn b64(value: &Value) -> String {
    STANDARD.encode(serde_json::to_vec(value).expect("serialize json for base64"))
}

// ─── tool-list: classification (mock) ───────────────────────────────────────

/// IT-001 — golden MCP classification: probe recognises an MCP endpoint.
#[test]
fn a2mcp_tool_list_classifies_mcp_endpoint() {
    let endpoint = spawn_mcp_mock(InitMode::Mcp);
    let (_home, home) = fresh_home("cli_agent_a2mcp");
    let mut cmd = onchainos();
    let output = scrubbed(&mut cmd, &home)
        .args(["agent", "a2mcp", "tool-list", "--endpoint", &endpoint])
        .output()
        .expect("run tool-list");
    let data = assert_ok_and_extract_data(&output);
    assert_eq!(data["type"], "mcp", "MCP endpoint must classify as mcp: {data}");
}

/// IT-002 — a plain web endpoint is treated as a regular HTTP service.
#[test]
fn a2mcp_tool_list_classifies_http_endpoint() {
    let endpoint = spawn_mcp_mock(InitMode::NotMcp);
    let (_home, home) = fresh_home("cli_agent_a2mcp");
    let mut cmd = onchainos();
    let output = scrubbed(&mut cmd, &home)
        .args(["agent", "a2mcp", "tool-list", "--endpoint", &endpoint])
        .output()
        .expect("run tool-list");
    let data = assert_ok_and_extract_data(&output);
    assert_eq!(data["type"], "http", "non-MCP endpoint must classify as http: {data}");
}

/// IT-003 — `--all` follows every `nextCursor` and returns the complete list.
#[test]
fn a2mcp_tool_list_all_follows_cursor_pages() {
    let endpoint = spawn_mcp_mock(InitMode::Mcp);
    let (_home, home) = fresh_home("cli_agent_a2mcp");
    let mut cmd = onchainos();
    let output = scrubbed(&mut cmd, &home)
        .args(["agent", "a2mcp", "tool-list", "--endpoint", &endpoint, "--all"])
        .output()
        .expect("run tool-list --all");
    let data = assert_ok_and_extract_data(&output);
    let tools = data["tools"].as_array().expect("tools must be an array");
    assert!(!tools.is_empty(), "--all must return at least one tool: {data}");
}

/// IT-022 — filtering by a name that does not exist returns an empty list.
#[test]
fn a2mcp_tool_list_tool_name_filter_no_match_is_empty() {
    let endpoint = spawn_mcp_mock(InitMode::Mcp);
    let (_home, home) = fresh_home("cli_agent_a2mcp");
    let mut cmd = onchainos();
    let output = scrubbed(&mut cmd, &home)
        .args([
            "agent", "a2mcp", "tool-list", "--endpoint", &endpoint, "--tool-name",
            "does-not-exist",
        ])
        .output()
        .expect("run tool-list --tool-name");
    let data = assert_ok_and_extract_data(&output);
    let tools = data["tools"].as_array().expect("tools must be an array");
    assert_eq!(tools.len(), 0, "unknown tool-name must yield an empty list: {data}");
}

// ─── tool-list: error / edge (mock + offline) ────────────────────────────────

/// IT-007 — a malformed endpoint yields a clear invalid-URL error.
#[test]
fn a2mcp_tool_list_rejects_malformed_endpoint() {
    let (_home, home) = fresh_home("cli_agent_a2mcp");
    let mut cmd = onchainos();
    let output = scrubbed(&mut cmd, &home)
        .args(["agent", "a2mcp", "tool-list", "--endpoint", "not-a-url"])
        .output()
        .expect("run tool-list");
    assert_error_contains(&output, &["invalid endpoint URL"]);
}

/// IT-008 — a non-http scheme (`file://`) is rejected.
#[test]
fn a2mcp_tool_list_rejects_non_http_scheme() {
    let (_home, home) = fresh_home("cli_agent_a2mcp");
    let mut cmd = onchainos();
    let output = scrubbed(&mut cmd, &home)
        .args(["agent", "a2mcp", "tool-list", "--endpoint", "file:///etc/passwd"])
        .output()
        .expect("run tool-list");
    assert_error_contains(&output, &["invalid endpoint URL"]);
}

/// A localhost endpoint is never eligible for A2MCP discovery.
#[test]
fn a2mcp_tool_list_rejects_localhost() {
    let (_home, home) = fresh_home("cli_agent_a2mcp");
    let mut cmd = onchainos();
    let output = scrubbed(&mut cmd, &home)
        .args([
            "agent",
            "a2mcp",
            "tool-list",
            "--endpoint",
            "http://localhost:3000/mcp",
        ])
        .output()
        .expect("run tool-list");
    assert_error_contains(&output, &["localhost"]);
}

/// IT-009 — an unreachable endpoint (closed local port) → ENDPOINT_UNREACHABLE.
#[test]
fn a2mcp_tool_list_reports_unreachable_endpoint() {
    let (_home, home) = fresh_home("cli_agent_a2mcp");
    let mut cmd = onchainos();
    let output = scrubbed(&mut cmd, &home)
        .args(["agent", "a2mcp", "tool-list", "--endpoint", "http://127.0.0.1:1/mcp"])
        .output()
        .expect("run tool-list");
    assert_error_contains(&output, &["ENDPOINT_UNREACHABLE"]);
}

/// IT-010 — MCP server rejects the initialize handshake.
#[test]
fn a2mcp_tool_list_reports_mcp_init_rejected() {
    let endpoint = spawn_mcp_mock(InitMode::InitError);
    let (_home, home) = fresh_home("cli_agent_a2mcp");
    let mut cmd = onchainos();
    let output = scrubbed(&mut cmd, &home)
        .args(["agent", "a2mcp", "tool-list", "--endpoint", &endpoint])
        .output()
        .expect("run tool-list");
    assert_error_contains(&output, &["MCP initialize rejected the request"]);
}

/// IT-011 — the server-selected protocol version is accepted and reused.
#[test]
fn a2mcp_tool_list_accepts_server_selected_protocol() {
    let endpoint = spawn_mcp_mock(InitMode::AlternateProtocol);
    let (_home, home) = fresh_home("cli_agent_a2mcp");
    let mut cmd = onchainos();
    let output = scrubbed(&mut cmd, &home)
        .args(["agent", "a2mcp", "tool-list", "--endpoint", &endpoint])
        .output()
        .expect("run tool-list");
    let data = assert_ok_and_extract_data(&output);
    assert_eq!(data["type"], "mcp");
}

/// IT-021 — `--cursor` and `--all` together is rejected by clap (exit 2).
#[test]
fn a2mcp_tool_list_rejects_cursor_with_all() {
    let (_home, home) = fresh_home("cli_agent_a2mcp");
    let mut cmd = onchainos();
    let output = scrubbed(&mut cmd, &home)
        .args([
            "agent", "a2mcp", "tool-list", "--endpoint", "http://mock.invalid/mcp", "--cursor",
            "abc", "--all",
        ])
        .output()
        .expect("run tool-list");
    assert_error_contains(&output, &["cannot be used with"]);
    assert_eq!(
        output.status.code(),
        Some(2),
        "clap argument conflict must exit 2"
    );
}

/// IT-023 — invalid-endpoint error stays clear with `ONCHAINOS_PRETTY=1`.
#[test]
fn a2mcp_tool_list_invalid_endpoint_pretty_mode() {
    let (_home, home) = fresh_home("cli_agent_a2mcp");
    let mut cmd = onchainos();
    let output = scrubbed(&mut cmd, &home)
        .env("ONCHAINOS_PRETTY", "1")
        .args(["agent", "a2mcp", "tool-list", "--endpoint", "not-a-url"])
        .output()
        .expect("run tool-list pretty");
    assert_error_contains(&output, &["invalid endpoint URL"]);
}

// ─── invoke: golden (mock) ───────────────────────────────────────────────────

/// IT-004 — MCP tool invoke returns a free result (no payment required).
#[test]
fn a2mcp_invoke_mcp_free_result_no_payment() {
    let endpoint = spawn_mcp_mock(InitMode::Mcp);
    let service = json!({
        "serviceType": "A2MCP",
        "endpoint": endpoint,
        "reqType": "MCP",
        "toolName": "price",
    });
    let (_home, home) = fresh_home("cli_agent_a2mcp");
    let mut cmd = onchainos();
    let output = scrubbed(&mut cmd, &home)
        .args([
            "agent", "a2mcp", "invoke", "--service-base64", &b64(&service), "--params-base64",
            &b64(&json!({})),
        ])
        .output()
        .expect("run invoke mcp");
    let data = assert_ok_and_extract_data(&output);
    assert_eq!(data["needsConfirm"], false, "free MCP result must not need confirm: {data}");
}

/// IT-005 — HTTP service invoke returns a free result (no payment required).
#[test]
fn a2mcp_invoke_http_free_result_no_payment() {
    let endpoint = spawn_http_mock("200 OK", "application/json", r#"{"result":"ok"}"#);
    let service = json!({
        "serviceType": "A2MCP",
        "endpoint": endpoint,
        "reqType": "HTTP",
        "method": "GET",
    });
    let (_home, home) = fresh_home("cli_agent_a2mcp");
    let mut cmd = onchainos();
    let output = scrubbed(&mut cmd, &home)
        .args([
            "agent", "a2mcp", "invoke", "--service-base64", &b64(&service), "--params-base64",
            &b64(&json!({"symbol": "BTC"})),
        ])
        .output()
        .expect("run invoke http");
    let data = assert_ok_and_extract_data(&output);
    assert_eq!(data["needsConfirm"], false, "free HTTP result must not need confirm: {data}");
}

#[test]
fn a2mcp_identity_verify_free_result_stays_in_identity_flow() {
    let endpoint = spawn_http_mock("200 OK", "application/json", r#"{"result":"ok"}"#);
    let service = json!({
        "serviceType": "A2MCP",
        "endpoint": endpoint,
        "reqType": "HTTP",
        "method": "POST",
    });
    let (_home, home) = fresh_home("cli_agent_a2mcp");
    let mut cmd = onchainos();
    let output = scrubbed(&mut cmd, &home)
        .args([
            "agent",
            "a2mcp",
            "invoke",
            "--service-base64",
            &b64(&service),
            "--params-base64",
            &b64(&json!({"symbol": "BTC"})),
            "--verify",
        ])
        .output()
        .expect("run identity verification");
    let data = assert_ok_and_extract_data(&output);
    assert_eq!(data["callable"], true);
    assert_eq!(data["verificationMode"], "identity");
    assert!(data.get("needsConfirm").is_none());
    assert!(data.get("paymentId").is_none());
}

// ─── invoke: validation errors (offline) ─────────────────────────────────────

/// IT-012 — a service JSON missing a required field is rejected before any call.
#[test]
fn a2mcp_invoke_rejects_missing_required_field() {
    // No `endpoint` → required_string fails before any network use.
    let service = json!({ "serviceType": "A2MCP", "reqType": "MCP", "toolName": "price" });
    let (_home, home) = fresh_home("cli_agent_a2mcp");
    let mut cmd = onchainos();
    let output = scrubbed(&mut cmd, &home)
        .args([
            "agent", "a2mcp", "invoke", "--service-base64", &b64(&service), "--params-base64",
            &b64(&json!({})),
        ])
        .output()
        .expect("run invoke");
    assert_error_contains(&output, &["service is missing required field"]);
}

/// IT-013 — a service that is not A2MCP is rejected.
#[test]
fn a2mcp_invoke_rejects_non_a2mcp_service_type() {
    let service = json!({
        "serviceType": "A2A",
        "endpoint": "http://mock.invalid/svc",
        "reqType": "HTTP",
        "method": "GET",
    });
    let (_home, home) = fresh_home("cli_agent_a2mcp");
    let mut cmd = onchainos();
    let output = scrubbed(&mut cmd, &home)
        .args([
            "agent", "a2mcp", "invoke", "--service-base64", &b64(&service), "--params-base64",
            &b64(&json!({})),
        ])
        .output()
        .expect("run invoke");
    assert_error_contains(&output, &["serviceType must be A2MCP"]);
}

/// IT-014 — a service using an unsupported HTTP method is rejected.
#[test]
fn a2mcp_invoke_rejects_invalid_method() {
    let service = json!({
        "serviceType": "A2MCP",
        "endpoint": "http://mock.invalid/svc",
        "reqType": "HTTP",
        "method": "PUT",
    });
    let (_home, home) = fresh_home("cli_agent_a2mcp");
    let mut cmd = onchainos();
    let output = scrubbed(&mut cmd, &home)
        .args([
            "agent", "a2mcp", "invoke", "--service-base64", &b64(&service), "--params-base64",
            &b64(&json!({})),
        ])
        .output()
        .expect("run invoke");
    assert_error_contains(&output, &["service method must be GET or POST"]);
}

/// IT-015 — a service with an unknown connection type is rejected.
#[test]
fn a2mcp_invoke_rejects_invalid_req_type() {
    let service = json!({
        "serviceType": "A2MCP",
        "endpoint": "http://mock.invalid/svc",
        "reqType": "GRPC",
        "method": "GET",
    });
    let (_home, home) = fresh_home("cli_agent_a2mcp");
    let mut cmd = onchainos();
    let output = scrubbed(&mut cmd, &home)
        .args([
            "agent", "a2mcp", "invoke", "--service-base64", &b64(&service), "--params-base64",
            &b64(&json!({})),
        ])
        .output()
        .expect("run invoke");
    assert_error_contains(&output, &["reqType must be MCP or HTTP"]);
}

/// IT-016 — an MCP service missing its tool name is rejected before connecting.
#[test]
fn a2mcp_invoke_rejects_mcp_missing_tool_name() {
    let service = json!({
        "serviceType": "A2MCP",
        "endpoint": "http://mock.invalid/mcp",
        "reqType": "MCP",
    });
    let (_home, home) = fresh_home("cli_agent_a2mcp");
    let mut cmd = onchainos();
    let output = scrubbed(&mut cmd, &home)
        .args([
            "agent", "a2mcp", "invoke", "--service-base64", &b64(&service), "--params-base64",
            &b64(&json!({})),
        ])
        .output()
        .expect("run invoke");
    assert_error_contains(&output, &["MCP service is missing toolName"]);
}

/// IT-017 — an unexpected server error (500) surfaces as endpoint-unreachable.
#[test]
fn a2mcp_invoke_reports_endpoint_server_error() {
    let endpoint = spawn_http_mock("500 Internal Server Error", "application/json", r#"{"error":"boom"}"#);
    let service = json!({
        "serviceType": "A2MCP",
        "endpoint": endpoint,
        "reqType": "HTTP",
        "method": "POST",
    });
    let (_home, home) = fresh_home("cli_agent_a2mcp");
    let mut cmd = onchainos();
    let output = scrubbed(&mut cmd, &home)
        .args([
            "agent", "a2mcp", "invoke", "--service-base64", &b64(&service), "--params-base64",
            &b64(&json!({})),
        ])
        .output()
        .expect("run invoke");
    assert_error_contains(&output, &["endpoint_unreachable: unexpected HTTP"]);
}

/// IT-019 — a payment-capable endpoint served over plain HTTP is rejected by the
/// HTTPS-only security guard before any payment or wallet-login logic runs.
/// The production code (`A2mcpFrozenRequestV1::new`) enforces HTTPS on every
/// invoke endpoint — this is a pre-existing, intentional security check that
/// prevents payment flows (and all other invocations) on non-secure channels.
/// A plain-TCP loopback mock cannot satisfy the HTTPS requirement, so this test
/// verifies the guard itself rather than the downstream wallet-login branch.
#[test]
fn a2mcp_invoke_requires_wallet_login_on_payment() {
    let endpoint = spawn_http_mock("402 Payment Required", "application/json", CHALLENGE_402);
    let service = json!({
        "serviceType": "A2MCP",
        "endpoint": endpoint,
        "reqType": "HTTP",
        "method": "POST",
    });
    let (_home, home) = fresh_home("cli_agent_a2mcp");
    let mut cmd = onchainos();
    let output = scrubbed(&mut cmd, &home)
        .args([
            "agent", "a2mcp", "invoke", "--service-base64", &b64(&service), "--params-base64",
            &b64(&json!({"symbol": "BTC"})),
        ])
        .output()
        .expect("run invoke 402");
    // The HTTPS guard fires before the 402 challenge / wallet-login path.
    assert_error_contains(&output, &["Endpoint must use HTTPS"]);
}

// ─── service-list: display + auth (live / offline) ───────────────────────────

/// IT-006 — after logging in, the service list surfaces each service's request
/// type / method / tool name (the 5 new backend fields). Live: needs a wallet
/// session and a real agent id (`AGENT_ID`); skipped when the id is not set.
#[test]
fn service_list_surfaces_request_type_cell() {
    let Ok(agent_id) = std::env::var("AGENT_ID") else {
        eprintln!("skipping IT-006: AGENT_ID not set (needs a live logged-in agent)");
        return;
    };
    let output = run_with_retry(&["agent", "service-list", "--agent-id", &agent_id]);
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        output.status.success(),
        "service-list must succeed for a valid agent\nstdout: {stdout}\nstderr: {stderr}"
    );
    assert!(
        stdout.contains("Request type"),
        "service-list output must surface the new \"Request type\" cell\nstdout: {stdout}"
    );
}

/// IT-018 — listing services without choosing an agent (and without a wallet
/// session) fails before even reaching the `--agent-id` validation.
///
/// `service_list_impl` calls `ensure_tokens_refreshed()` BEFORE the
/// `require_non_empty(args.agent_id, "--agent-id")` check, so on a fresh
/// sandbox home (no persisted session) it deterministically hits the
/// session-expired branch first. This is pre-existing, intentional ordering
/// (auth gates everything). The test runs against a scrubbed sandbox to
/// guarantee no ambient session and asserts the actual terminal error.
#[test]
fn service_list_requires_agent_id() {
    let (_home, home) = fresh_home("cli_agent_a2mcp");
    let mut cmd = onchainos();
    let output = scrubbed(&mut cmd, &home)
        .args(["agent", "service-list"])
        .output()
        .expect("run service-list");
    // Auth fires before --agent-id validation; without a session, we get
    // session-expired rather than missing-parameter.
    assert_error_contains(&output, &["session expired"]);
}

/// IT-020 — listing a service without logging in is refused. Fresh sandbox home
/// guarantees no wallet session, so the auth/refresh path fails (ok=false).
#[test]
fn service_list_refuses_without_login() {
    let agent_id = std::env::var("AGENT_ID").unwrap_or_else(|_| "agent-000".to_string());
    let service_id = std::env::var("SERVICE_ID").unwrap_or_else(|_| "svc-000".to_string());
    let (_home, home) = fresh_home("cli_agent_a2mcp");
    let output = run_scrubbed_with_retry(
        &home,
        &["agent", "service-list", "--agent-id", &agent_id, "--service-id", &service_id],
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(!output.status.success(), "unauthenticated service-list must fail: {stdout}");
    let json: Value = serde_json::from_str(&stdout)
        .unwrap_or_else(|e| panic!("expected a JSON error envelope: {e}\nstdout: {stdout}"));
    assert_eq!(json["ok"], Value::Bool(false), "unauthenticated service-list must return ok=false: {json}");
}
