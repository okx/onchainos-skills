//! Payment **MCP client** transport (JSON-RPC 2.0 over Streamable HTTP / SSE).
//!
//! This is the *client* half of A2MCP over MCP transport — distinct from
//! `cli/src/mcp/mod.rs`, which is the rmcp *server* that exposes `onchainos`
//! capabilities as MCP tools. Here `onchainos payment quote` / `payment pay`
//! speak JSON-RPC to a user-supplied A2MCP endpoint whose paywall sits at the
//! `tools/call` layer (a bare probe / `tools/list` is free; only a real
//! `tools/call` returns the x402 402 challenge).
//!
//! The module owns its own `reqwest::Client` (payment never uses the
//! host-locked `ApiClient`). Transport failures surface as token-prefixed
//! `anyhow` errors (`endpoint_unreachable …`) so the always-on envelope keeps
//! the same grep-able first-word contract as the REST path.

use anyhow::{anyhow, Result};
use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};

/// MCP protocol version advertised on `initialize`. A server-side mismatch is
/// handled by the server's own fallback, not a hard client failure.
pub const MCP_PROTOCOL_VERSION: &str = "2025-06-18";

/// Machine token (leading word of `output::error`) for MCP transport failures.
/// Kept string-identical to the REST path's token so the skill greps one word.
const TOKEN_ENDPOINT_UNREACHABLE: &str = "endpoint_unreachable";

/// Per-endpoint probe timeout (seconds) — MCP hosts are arbitrary, bound it.
const MCP_TIMEOUT_SECS: u64 = 30;

/// One discovered MCP tool (FR-2). Re-serialized into `QuoteData.mcpTools[]`
/// and parsed from a `tools/list` result — hence both `Serialize` (into the
/// CLI envelope) and `Deserialize` (from the JSON-RPC catalog).
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct McpTool {
    #[serde(default)]
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(
        rename = "inputSchema",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub input_schema: Option<Value>,
}

/// Result of a `tools/call`: either a free (unpaid / first-N-free) tool result,
/// or a 402 challenge that must be decoded into the x402 `accepts[]` flow.
#[derive(Clone, Debug)]
pub enum ToolCallOutcome {
    /// Non-402 response carrying the tool's JSON-RPC `result`.
    Free(Value),
    /// HTTP 402 — the challenge header value + the raw response body.
    Paid { header: String, body: String },
}

/// Everything `payment pay` needs to replay the SAME `tools/call` that landed
/// the 402 during `payment quote`. Persisted (as `mcpTool` + coerced arguments)
/// in `PaymentState`; the `session_id` is process-local (NFR-3) — `quote` and
/// `pay` are separate processes and each re-handshakes, so it is normally
/// `None` and the client's own freshly-captured session id is used.
#[derive(Clone, Debug)]
pub struct McpReplay {
    pub session_id: Option<String>,
    pub tool: String,
    pub arguments: Value,
}

/// Heuristic: does this URL *look* like an MCP endpoint? Strip the scheme,
/// query, and fragment, lowercase, drop a trailing `/`, then test whether the
/// path ends in `/mcp` or `/sse`.
pub fn url_looks_like_mcp(url: &str) -> bool {
    let lower = url.to_ascii_lowercase();
    let after_scheme = lower
        .split_once("://")
        .map(|(_, rest)| rest)
        .unwrap_or(lower.as_str());
    let path = after_scheme
        .split(['?', '#'])
        .next()
        .unwrap_or(after_scheme)
        .trim_end_matches('/');
    path.ends_with("/mcp") || path.ends_with("/sse")
}

/// Does a bare-probe response *signal* MCP transport? True when the
/// `Content-Type` advertises `text/event-stream`, OR the body (trimmed, and
/// beginning with `{` or `data:`) contains a `"jsonrpc"` marker. A bare 405 or
/// a plain REST JSON body does not signal MCP.
pub fn probe_signals_mcp(content_type: &str, body: &str) -> bool {
    if content_type
        .to_ascii_lowercase()
        .contains("text/event-stream")
    {
        return true;
    }
    let trimmed = body.trim_start();
    if trimmed.starts_with('{') || trimmed.starts_with("data:") {
        return trimmed.contains("\"jsonrpc\"");
    }
    false
}

/// Coerce each CLI `--param key=value` (all strings) to the JSON type the tool's
/// `inputSchema.properties[key].type` declares (FR-3 / AC-3):
/// - `integer` / `number` → JSON number (parse failure → kept as string)
/// - `boolean` → JSON bool (`true`/`false`; anything else → string)
/// - `object` / `array` → parsed JSON (parse failure → string)
/// - any other type / no schema → kept as string
/// - a value that is already non-string → passed through unchanged
pub fn coerce_arguments(params: &Map<String, Value>, input_schema: Option<&Value>) -> Value {
    let mut out = Map::new();
    for (k, v) in params {
        let raw = match v {
            Value::String(s) => s.clone(),
            other => {
                // Already non-string (from a caller that pre-typed it) → passthrough.
                out.insert(k.clone(), other.clone());
                continue;
            }
        };
        let ty = input_schema
            .and_then(|schema| schema.get("properties"))
            .and_then(|props| props.get(k))
            .and_then(|prop| prop.get("type"))
            .and_then(|t| t.as_str());
        out.insert(k.clone(), coerce_one(&raw, ty));
    }
    Value::Object(out)
}

/// Coerce a single string per its declared JSON-schema `type`.
fn coerce_one(raw: &str, ty: Option<&str>) -> Value {
    match ty {
        Some("integer") | Some("number") => serde_json::from_str::<serde_json::Number>(raw)
            .map(Value::Number)
            .unwrap_or_else(|_| Value::String(raw.to_string())),
        Some("boolean") => match raw {
            "true" => Value::Bool(true),
            "false" => Value::Bool(false),
            _ => Value::String(raw.to_string()),
        },
        Some("object") | Some("array") => {
            serde_json::from_str::<Value>(raw).unwrap_or_else(|_| Value::String(raw.to_string()))
        }
        _ => Value::String(raw.to_string()),
    }
}

/// Parse a Streamable-HTTP body (plain JSON *or* an SSE stream) into the first
/// JSON-RPC envelope carrying a `result`/`error`. In-stream notifications (e.g.
/// `progress`, which carry neither) are skipped (FR-5 / AC-6).
pub fn parse_streamable_body(body: &str) -> Result<Value> {
    // Fast path: a plain JSON body (not framed as SSE).
    let trimmed = body.trim_start();
    if trimmed.starts_with('{') {
        if let Ok(v) = serde_json::from_str::<Value>(trimmed) {
            if v.get("result").is_some() || v.get("error").is_some() {
                return Ok(v);
            }
        }
    }
    // SSE path: take the first `data:` line whose JSON has result/error.
    for line in body.lines() {
        let Some(data) = line.trim().strip_prefix("data:") else {
            continue;
        };
        let data = data.trim();
        if data.is_empty() {
            continue;
        }
        let Ok(v) = serde_json::from_str::<Value>(data) else {
            continue;
        };
        if v.get("result").is_some() || v.get("error").is_some() {
            return Ok(v);
        }
    }
    Err(anyhow!(
        "{TOKEN_ENDPOINT_UNREACHABLE}: no JSON-RPC result/error in MCP response"
    ))
}

/// Unwrap a JSON-RPC 2.0 envelope: return `result`, or map `error` to a
/// token-prefixed `anyhow` error.
pub fn jsonrpc_result(envelope: &Value) -> Result<Value> {
    if let Some(err) = envelope.get("error") {
        let msg = err
            .get("message")
            .and_then(|m| m.as_str())
            .unwrap_or("unknown JSON-RPC error");
        // Preserve the JSON-RPC error `code` (e.g. -32601 method-not-found,
        // -32000 server error) so the agent can distinguish failure classes.
        return Err(match err.get("code").and_then(|c| c.as_i64()) {
            Some(code) => anyhow!("{TOKEN_ENDPOINT_UNREACHABLE}: JSON-RPC error {code}: {msg}"),
            None => anyhow!("{TOKEN_ENDPOINT_UNREACHABLE}: JSON-RPC error: {msg}"),
        });
    }
    envelope
        .get("result")
        .cloned()
        .ok_or_else(|| anyhow!("{TOKEN_ENDPOINT_UNREACHABLE}: JSON-RPC response missing result"))
}

/// A signed-replay outcome, mirroring `replay_merchant`'s tuple contract:
/// `(status_code, PAYMENT-RESPONSE header, parsed tool result)`.
pub type SignedReplay = (u16, Option<String>, Value);

/// Cap a server error body so a large/HTML 4xx/5xx page does not flood the
/// error envelope, while still surfacing the server's explanation for triage.
const MAX_ERROR_BODY_CHARS: usize = 500;

/// Build an `endpoint_unreachable` error for a non-2xx MCP HTTP response,
/// carrying the (truncated) response body so the server's 4xx/5xx explanation
/// is not lost when diagnosing a failed handshake / call.
fn http_error(stage: &str, status: u16, body: &str) -> anyhow::Error {
    let trimmed = body.trim();
    let truncated: String = trimmed.chars().take(MAX_ERROR_BODY_CHARS).collect();
    let suffix = if truncated.is_empty() {
        String::new()
    } else if truncated.len() < trimmed.len() {
        format!(": {truncated}…")
    } else {
        format!(": {truncated}")
    };
    anyhow!("{TOKEN_ENDPOINT_UNREACHABLE}: {stage} returned HTTP {status}{suffix}")
}

/// JSON-RPC / SSE client for one A2MCP endpoint. Constructs its own
/// `reqwest::Client`; captures `Mcp-Session-Id` on `initialize` and reuses it
/// for subsequent calls within this process only.
pub struct McpClient {
    http: reqwest::Client,
    url: String,
    session_id: Option<String>,
}

impl McpClient {
    /// Build a client for `url` with a bounded timeout.
    pub fn new(url: &str) -> Result<Self> {
        let http = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(MCP_TIMEOUT_SECS))
            .build()
            .map_err(|e| anyhow!("{TOKEN_ENDPOINT_UNREACHABLE}: {e}"))?;
        Ok(Self {
            http,
            url: url.to_string(),
            session_id: None,
        })
    }

    /// The `Mcp-Session-Id` captured on `initialize` (process-local).
    pub fn session_id(&self) -> Option<&str> {
        self.session_id.as_deref()
    }

    /// Refresh the process-local `Mcp-Session-Id` from any response that carries
    /// one. The id is issued on `initialize`, but capturing it on every response
    /// keeps a rotating-session server in sync (the spec allows the server to
    /// re-issue it).
    fn capture_session_id(&mut self, resp: &reqwest::Response) {
        if let Some(sid) = resp
            .headers()
            .get("Mcp-Session-Id")
            .and_then(|v| v.to_str().ok())
        {
            self.session_id = Some(sid.to_string());
        }
    }

    /// Send one JSON-RPC message. `id = None` → notification (no `id`, response
    /// body ignored). Attaches the captured `Mcp-Session-Id` when present.
    async fn send_rpc(
        &self,
        id: Option<u64>,
        method: &str,
        params: Value,
    ) -> Result<reqwest::Response> {
        let mut body = json!({ "jsonrpc": "2.0", "method": method });
        if let Some(id) = id {
            body["id"] = json!(id);
        }
        if !params.is_null() {
            body["params"] = params;
        }
        let payload =
            serde_json::to_vec(&body).map_err(|e| anyhow!("{TOKEN_ENDPOINT_UNREACHABLE}: {e}"))?;
        let mut req = self
            .http
            .post(&self.url)
            .header(reqwest::header::CONTENT_TYPE, "application/json")
            .header(
                reqwest::header::ACCEPT,
                "application/json, text/event-stream",
            )
            .body(payload);
        if let Some(sid) = &self.session_id {
            req = req.header("Mcp-Session-Id", sid);
        }
        req.send()
            .await
            .map_err(|e| anyhow!("{TOKEN_ENDPOINT_UNREACHABLE}: {e}"))
    }

    /// Handshake: `initialize` (id 1), capture `Mcp-Session-Id`, then fire the
    /// `notifications/initialized` notification.
    pub async fn initialize(&mut self) -> Result<()> {
        let params = json!({
            "protocolVersion": MCP_PROTOCOL_VERSION,
            "capabilities": {},
            "clientInfo": { "name": "onchainos", "version": env!("CARGO_PKG_VERSION") },
        });
        let resp = self.send_rpc(Some(1), "initialize", params).await?;
        self.capture_session_id(&resp);
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        if !status.is_success() {
            return Err(http_error("initialize", status.as_u16(), &body));
        }
        let envelope = parse_streamable_body(&body)?;
        jsonrpc_result(&envelope)?;
        // Best-effort notification; a transport failure here surfaces on the
        // next real call, so its own error is not propagated.
        let _ = self
            .send_rpc(None, "notifications/initialized", Value::Null)
            .await;
        Ok(())
    }

    /// Discovery: `tools/list` (id 2) → the endpoint's tool catalog.
    pub async fn list_tools(&mut self) -> Result<Vec<McpTool>> {
        let resp = self.send_rpc(Some(2), "tools/list", json!({})).await?;
        self.capture_session_id(&resp);
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        if !status.is_success() {
            return Err(http_error("tools/list", status.as_u16(), &body));
        }
        let envelope = parse_streamable_body(&body)?;
        let result = jsonrpc_result(&envelope)?;
        let entries = result
            .get("tools")
            .and_then(|v| v.as_array())
            .cloned()
            .unwrap_or_default();
        // Per-entry tolerance: a single non-conforming tool (e.g. missing
        // `name`, which degrades to "") must not fail the whole catalog — the
        // remaining tools stay discoverable. An entry that is not even an object
        // is skipped rather than aborting discovery.
        let tools = entries
            .into_iter()
            .filter_map(|entry| serde_json::from_value::<McpTool>(entry).ok())
            .collect();
        Ok(tools)
    }

    /// `tools/call` (id 3). HTTP 402 → `Paid` (the x402 challenge to decode);
    /// any other success → `Free` (the tool's result). Non-402/non-2xx →
    /// `endpoint_unreachable`.
    pub async fn call_tool(&mut self, tool: &str, arguments: &Value) -> Result<ToolCallOutcome> {
        let params = json!({ "name": tool, "arguments": arguments });
        let resp = self.send_rpc(Some(3), "tools/call", params).await?;
        self.capture_session_id(&resp);
        let status = resp.status();
        let header = resp
            .headers()
            .get("PAYMENT-REQUIRED")
            .or_else(|| resp.headers().get("WWW-Authenticate"))
            .and_then(|v| v.to_str().ok())
            .map(|s| s.to_string());
        let body = resp.text().await.unwrap_or_default();
        if status.as_u16() == 402 {
            // Some servers put the challenge in the body rather than a header.
            let header = header.unwrap_or_else(|| body.clone());
            return Ok(ToolCallOutcome::Paid { header, body });
        }
        if !status.is_success() {
            return Err(http_error("tools/call", status.as_u16(), &body));
        }
        let envelope = parse_streamable_body(&body)?;
        let result = jsonrpc_result(&envelope)?;
        Ok(ToolCallOutcome::Free(result))
    }

    /// Signed replay (id 4): replay the SAME `tools/call` with a TEE-signed
    /// payment header. `header_name` is the payment-flow-computed header name
    /// (normally `PAYMENT-SIGNATURE`, but threaded through — like the REST
    /// `replay_merchant` — so a future scheme/version that emits a different
    /// header name still sends the correct one). Returns `(status_code,
    /// PAYMENT-RESPONSE, result)` — the caller (`replay_mcp`) maps the status to
    /// success/pending/failed and never discards a signed authorization.
    pub async fn call_tool_signed(
        &mut self,
        replay: &McpReplay,
        header_name: &str,
        payment_signature: &str,
    ) -> Result<SignedReplay> {
        if replay.session_id.is_some() {
            self.session_id = replay.session_id.clone();
        }
        let params = json!({ "name": replay.tool, "arguments": replay.arguments });
        let body = json!({ "jsonrpc": "2.0", "id": 4, "method": "tools/call", "params": params });
        let payload =
            serde_json::to_vec(&body).map_err(|e| anyhow!("{TOKEN_ENDPOINT_UNREACHABLE}: {e}"))?;
        let mut req = self
            .http
            .post(&self.url)
            .header(reqwest::header::CONTENT_TYPE, "application/json")
            .header(
                reqwest::header::ACCEPT,
                "application/json, text/event-stream",
            )
            .header(header_name, payment_signature)
            .body(payload);
        if let Some(sid) = &self.session_id {
            req = req.header("Mcp-Session-Id", sid);
        }
        let resp = req
            .send()
            .await
            .map_err(|e| anyhow!("{TOKEN_ENDPOINT_UNREACHABLE}: {e}"))?;
        self.capture_session_id(&resp);
        let status_code = resp.status().as_u16();
        let payment_response = resp
            .headers()
            .get("PAYMENT-RESPONSE")
            .and_then(|v| v.to_str().ok())
            .map(|s| s.to_string());
        let raw = resp.text().await.unwrap_or_default();
        // Best-effort result parse (SSE → JSON-RPC result, else raw JSON, else
        // the raw text) so a non-200 outcome still carries a diagnostic body.
        let result = parse_streamable_body(&raw)
            .ok()
            .and_then(|env| jsonrpc_result(&env).ok())
            .unwrap_or_else(|| {
                serde_json::from_str::<Value>(&raw).unwrap_or(Value::String(raw.clone()))
            });
        Ok((status_code, payment_response, result))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn url_looks_like_mcp_detects_mcp_and_sse_paths() {
        assert!(url_looks_like_mcp("https://api.example.com/mcp"));
        assert!(url_looks_like_mcp("https://api.example.com/sse/"));
        assert!(url_looks_like_mcp(
            "https://api.example.com/api/sse?token=x"
        ));
        assert!(url_looks_like_mcp("HTTPS://API.EXAMPLE.COM/MCP"));
        // Trailing fragment stripped too.
        assert!(url_looks_like_mcp("https://api.example.com/mcp#section"));
        // A path that merely starts with `mcp` is not an MCP endpoint.
        assert!(!url_looks_like_mcp("https://api.example.com/mcphammer"));
        assert!(!url_looks_like_mcp("https://api.example.com/pay"));
        assert!(!url_looks_like_mcp("https://api.example.com/"));
    }

    #[test]
    fn probe_signals_mcp_matches_event_stream_and_jsonrpc_body() {
        // Content-Type event-stream → MCP.
        assert!(probe_signals_mcp("text/event-stream; charset=utf-8", ""));
        // JSON-RPC body → MCP.
        assert!(probe_signals_mcp(
            "application/json",
            r#"{"jsonrpc":"2.0","id":1,"result":{}}"#
        ));
        // SSE data line carrying jsonrpc → MCP.
        assert!(probe_signals_mcp(
            "text/plain",
            "data: {\"jsonrpc\":\"2.0\"}\n"
        ));
        // Bare 405 with an empty body → NOT MCP.
        assert!(!probe_signals_mcp("text/html", ""));
        // Plain REST JSON (no jsonrpc marker) → NOT MCP.
        assert!(!probe_signals_mcp("application/json", r#"{"price":"1.5"}"#));
    }

    #[test]
    fn coerce_arguments_applies_schema_types() {
        let schema = json!({
            "type": "object",
            "properties": {
                "n":   { "type": "integer" },
                "zip": { "type": "string" },
                "flag": { "type": "boolean" },
                "opts": { "type": "object" }
            }
        });
        let mut params = Map::new();
        params.insert("n".into(), Value::String("5".into()));
        params.insert("zip".into(), Value::String("01234".into()));
        params.insert("flag".into(), Value::String("true".into()));
        params.insert("opts".into(), Value::String(r#"{"a":1}"#.into()));
        let out = coerce_arguments(&params, Some(&schema));
        assert_eq!(out["n"], json!(5));
        // A leading-zero string stays a string (not truncated to a number).
        assert_eq!(out["zip"], json!("01234"));
        assert_eq!(out["flag"], json!(true));
        assert_eq!(out["opts"], json!({ "a": 1 }));
    }

    #[test]
    fn coerce_arguments_defaults_to_string_without_schema() {
        let mut params = Map::new();
        params.insert("q".into(), Value::String("42".into()));
        // No schema → keep as string.
        let out = coerce_arguments(&params, None);
        assert_eq!(out["q"], json!("42"));
        // Integer type but a non-numeric value → parse failure → string.
        let schema = json!({ "properties": { "q": { "type": "integer" } } });
        let mut p2 = Map::new();
        p2.insert("q".into(), Value::String("not-a-number".into()));
        let out2 = coerce_arguments(&p2, Some(&schema));
        assert_eq!(out2["q"], json!("not-a-number"));
    }

    #[test]
    fn parse_streamable_body_skips_notifications_before_response() {
        let sse = "\
event: message\n\
data: {\"jsonrpc\":\"2.0\",\"method\":\"notifications/progress\",\"params\":{\"progress\":0.5}}\n\
\n\
event: message\n\
data: {\"jsonrpc\":\"2.0\",\"id\":3,\"result\":{\"ok\":true}}\n\
\n";
        let v = parse_streamable_body(sse).unwrap();
        assert_eq!(v["id"], json!(3));
        assert_eq!(v["result"], json!({ "ok": true }));
    }

    #[test]
    fn parse_streamable_body_reads_plain_json() {
        let body = r#"{"jsonrpc":"2.0","id":2,"result":{"tools":[]}}"#;
        let v = parse_streamable_body(body).unwrap();
        assert_eq!(v["result"], json!({ "tools": [] }));
    }

    #[test]
    fn parse_streamable_body_errors_when_no_response() {
        let sse = "data: {\"jsonrpc\":\"2.0\",\"method\":\"notifications/progress\"}\n";
        let err = parse_streamable_body(sse).unwrap_err();
        assert!(err.to_string().starts_with(TOKEN_ENDPOINT_UNREACHABLE));
    }

    #[test]
    fn jsonrpc_result_unwraps_result_and_maps_error() {
        let ok = json!({ "jsonrpc": "2.0", "id": 1, "result": { "v": 1 } });
        assert_eq!(jsonrpc_result(&ok).unwrap(), json!({ "v": 1 }));
        let err =
            json!({ "jsonrpc": "2.0", "id": 1, "error": { "code": -32000, "message": "boom" } });
        let e = jsonrpc_result(&err).unwrap_err();
        assert!(e.to_string().starts_with(TOKEN_ENDPOINT_UNREACHABLE));
        assert!(e.to_string().contains("boom"));
        // The JSON-RPC error code is preserved for failure-class triage.
        assert!(e.to_string().contains("-32000"));
    }

    #[test]
    fn http_error_carries_truncated_body() {
        // A short body is surfaced verbatim.
        let e = http_error("tools/call", 500, "  server exploded  ");
        assert!(e.to_string().contains("HTTP 500"));
        assert!(e.to_string().contains("server exploded"));
        // An empty body yields no trailing separator.
        let e2 = http_error("initialize", 503, "   ");
        assert!(e2.to_string().ends_with("HTTP 503"));
        // An oversized body is truncated with an ellipsis marker.
        let big = "x".repeat(MAX_ERROR_BODY_CHARS + 50);
        let e3 = http_error("tools/list", 500, &big);
        assert!(e3.to_string().contains('…'));
        assert!(e3.to_string().len() < big.len() + 80);
    }

    // ── call_tool / list_tools result mapping (mock endpoint, hermetic) ──────
    //
    // The pure helpers above are covered, but `call_tool`'s HTTP-status → outcome
    // decision (402 → Paid = user is charged, 2xx → Free, other → Err) is what
    // actually determines whether the user pays. These tests stand up a one-shot
    // local HTTP endpoint (same pattern as payment_flow.rs `spawn_mock_merchant`)
    // and drive the real `reqwest` round-trip so the 402/2xx/other branches and
    // the body-vs-header challenge fallback are exercised end-to-end.

    /// Bind a one-shot HTTP endpoint on an ephemeral loopback port that answers
    /// the first request with `status_line`, an optional pre-formatted
    /// `extra_headers` block (each line ending in `\r\n`, or `""`), `content_type`,
    /// and `body`, then closes. Returns the URL to hit and the server handle.
    fn spawn_mock_mcp(
        status_line: &'static str,
        extra_headers: &'static str,
        content_type: &'static str,
        body: &'static str,
    ) -> (String, std::thread::JoinHandle<()>) {
        use std::io::{Read, Write};
        use std::net::TcpListener;

        let listener = TcpListener::bind("127.0.0.1:0").expect("bind mock mcp");
        let addr = listener.local_addr().expect("mock mcp addr");
        let url = format!("http://{addr}/mcp");
        let handle = std::thread::spawn(move || {
            if let Ok((mut stream, _)) = listener.accept() {
                let mut buf = [0u8; 4096];
                let _ = stream.read(&mut buf);
                let response = format!(
                    "HTTP/1.1 {status_line}\r\nContent-Type: {content_type}\r\n{extra_headers}\
                     Content-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                );
                let _ = stream.write_all(response.as_bytes());
                let _ = stream.flush();
            }
        });
        (url, handle)
    }

    #[tokio::test]
    async fn call_tool_maps_402_header_to_paid() {
        // 402 + PAYMENT-REQUIRED header → Paid, header carries the x402 challenge.
        let (url, handle) = spawn_mock_mcp(
            "402 Payment Required",
            "PAYMENT-REQUIRED: x402-challenge-abc\r\n",
            "application/json",
            r#"{"error":"payment required"}"#,
        );
        let mut client = McpClient::new(&url).unwrap();
        let out = client.call_tool("premium_tool", &json!({})).await.unwrap();
        let _ = handle.join();
        match out {
            ToolCallOutcome::Paid { header, body } => {
                assert_eq!(header, "x402-challenge-abc");
                assert!(body.contains("payment required"));
            }
            other => panic!("402 with header must map to Paid, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn call_tool_maps_402_body_challenge_to_paid() {
        // 402 with NO challenge header → header falls back to the response body.
        let (url, handle) = spawn_mock_mcp(
            "402 Payment Required",
            "",
            "application/json",
            r#"{"accepts":[{"scheme":"exact"}]}"#,
        );
        let mut client = McpClient::new(&url).unwrap();
        let out = client.call_tool("premium_tool", &json!({})).await.unwrap();
        let _ = handle.join();
        match out {
            ToolCallOutcome::Paid { header, body } => {
                assert_eq!(header, body, "no header → body is the challenge fallback");
                assert!(header.contains("accepts"));
            }
            other => panic!("402 body-only must still map to Paid, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn call_tool_maps_2xx_sse_to_free() {
        // 200 SSE stream carrying a JSON-RPC result → Free(result).
        let sse = "event: message\n\
                   data: {\"jsonrpc\":\"2.0\",\"id\":3,\"result\":{\"content\":[{\"type\":\"text\",\"text\":\"ok\"}]}}\n\n";
        let (url, handle) = spawn_mock_mcp("200 OK", "", "text/event-stream", sse);
        let mut client = McpClient::new(&url).unwrap();
        let out = client.call_tool("free_tool", &json!({})).await.unwrap();
        let _ = handle.join();
        match out {
            ToolCallOutcome::Free(result) => {
                assert_eq!(result["content"][0]["text"].as_str(), Some("ok"));
            }
            other => panic!("2xx must map to Free, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn call_tool_maps_non_402_non_2xx_to_err() {
        // 500 (neither 402 nor 2xx) → Err carrying the endpoint_unreachable token.
        let (url, handle) = spawn_mock_mcp(
            "500 Internal Server Error",
            "",
            "application/json",
            r#"{"error":"boom"}"#,
        );
        let mut client = McpClient::new(&url).unwrap();
        let err = client
            .call_tool("premium_tool", &json!({}))
            .await
            .unwrap_err();
        let _ = handle.join();
        assert!(
            err.to_string().starts_with(TOKEN_ENDPOINT_UNREACHABLE),
            "non-402/non-2xx must carry the endpoint_unreachable token: {err}"
        );
        assert!(err.to_string().contains("500"));
        // The server's 4xx/5xx body is preserved for triage (item D).
        assert!(err.to_string().contains("boom"));
    }

    #[tokio::test]
    async fn list_tools_parses_valid_catalog() {
        let body = r#"{"jsonrpc":"2.0","id":2,"result":{"tools":[{"name":"a","description":"A"},{"name":"b","inputSchema":{"type":"object"}}]}}"#;
        let (url, handle) = spawn_mock_mcp("200 OK", "", "application/json", body);
        let mut client = McpClient::new(&url).unwrap();
        let tools = client.list_tools().await.unwrap();
        let _ = handle.join();
        assert_eq!(tools.len(), 2);
        assert_eq!(tools[0].name, "a");
        assert_eq!(tools[1].name, "b");
        assert!(tools[1].input_schema.is_some());
    }

    #[tokio::test]
    async fn list_tools_tolerates_malformed_entry() {
        // A tools[] entry missing `name` degrades to "" (it must NOT fail the
        // whole catalog); a valid tool alongside it is still discovered; a
        // non-object entry is skipped entirely.
        let body = r#"{"jsonrpc":"2.0","id":2,"result":{"tools":[{"description":"no name"},{"name":"good"},"not-an-object"]}}"#;
        let (url, handle) = spawn_mock_mcp("200 OK", "", "application/json", body);
        let mut client = McpClient::new(&url).unwrap();
        let tools = client.list_tools().await.unwrap();
        let _ = handle.join();
        // Bad object degraded to name "", good tool preserved, non-object
        // skipped → 2 tools discovered rather than a whole-catalog failure.
        assert_eq!(tools.len(), 2);
        assert_eq!(tools[0].name, "");
        assert_eq!(tools[0].description.as_deref(), Some("no name"));
        assert_eq!(tools[1].name, "good");
    }
}
