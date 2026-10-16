use std::collections::HashSet;
use std::time::Duration;

use anyhow::{anyhow, bail, Context, Result};
use clap::Args;
use reqwest::header::{ACCEPT, CONTENT_TYPE};
use serde::Serialize;
use serde_json::{json, Value};
use url::Url;

use crate::mcp_client::MCP_PROTOCOL_VERSION;
use crate::output;

const ENDPOINT_UNREACHABLE: &str = "ENDPOINT_UNREACHABLE";
const MAX_INPUT_SCHEMA_BYTES: usize = 16 * 1024;

#[derive(Args, Debug)]
pub struct McpToolListArgs {
    /// HTTP or MCP endpoint to inspect.
    #[arg(long)]
    pub endpoint: String,

    /// Return only tools whose name exactly matches this value.
    #[arg(long = "tool-name")]
    pub tool_name: Option<String>,

    /// MCP cursor returned as `nextCursor` by a previous `tools/list` call.
    #[arg(long, conflicts_with_all = ["all", "tool_name"])]
    pub cursor: Option<String>,

    /// Follow every `nextCursor` and return the complete tool list.
    #[arg(long, conflicts_with = "cursor")]
    pub all: bool,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct McpToolListOutput {
    r#type: &'static str,
    tools: Vec<Value>,
    #[serde(rename = "nextCursor", skip_serializing_if = "Option::is_none")]
    next_cursor: Option<String>,
    #[serde(rename = "_meta", skip_serializing_if = "Option::is_none")]
    meta: Option<Value>,
}

#[derive(Debug)]
struct ToolsPage {
    tools: Vec<Value>,
    next_cursor: Option<String>,
    meta: Option<Value>,
}

enum EndpointKind {
    Http,
    Mcp { supports_tools: bool },
}

struct McpToolListClient {
    http: reqwest::Client,
    endpoint: String,
    session_id: Option<String>,
    next_id: u64,
}

impl McpToolListClient {
    fn new(endpoint: &str) -> Result<Self> {
        validate_endpoint(endpoint)?;
        let http = reqwest::Client::builder()
            .timeout(Duration::from_secs(30))
            .build()
            .context("failed to build HTTP client")?;
        Ok(Self {
            http,
            endpoint: endpoint.to_owned(),
            session_id: None,
            next_id: 2,
        })
    }

    fn request(&self) -> reqwest::RequestBuilder {
        let mut request = self
            .http
            .post(&self.endpoint)
            .header(ACCEPT, "application/json, text/event-stream");
        if let Some(session_id) = &self.session_id {
            request = request.header("Mcp-Session-Id", session_id);
        }
        request
    }

    fn capture_session(&mut self, response: &reqwest::Response) {
        if let Some(session_id) = response
            .headers()
            .get("Mcp-Session-Id")
            .and_then(|value| value.to_str().ok())
            .filter(|value| !value.is_empty())
        {
            self.session_id = Some(session_id.to_owned());
        }
    }

    async fn detect(&mut self) -> Result<EndpointKind> {
        let payload = json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "initialize",
            "params": {
                "protocolVersion": MCP_PROTOCOL_VERSION,
                "capabilities": {},
                "clientInfo": {
                    "name": "onchainos-agent-a2mcp-tool-list",
                    "version": env!("CARGO_PKG_VERSION")
                }
            }
        });
        let response = self
            .request()
            .json(&payload)
            .send()
            .await
            .map_err(|error| anyhow!("{ENDPOINT_UNREACHABLE}: {error}"))?;
        self.capture_session(&response);
        let status = response.status();
        if !status.is_success() {
            let body = response.text().await.unwrap_or_default();
            return match status.as_u16() {
                401 | 403 | 408 | 429 | 500..=599 => Err(anyhow!(
                    "{ENDPOINT_UNREACHABLE}: initialize returned HTTP {}: {}",
                    status.as_u16(),
                    body.trim()
                )),
                _ => Ok(EndpointKind::Http),
            };
        }

        let content_type = response_content_type(&response);
        let body = response.text().await.unwrap_or_default();
        if !is_mcp_content_type(&content_type) {
            return Ok(EndpointKind::Http);
        }
        let Some(envelope) = parse_response(&body) else {
            return Ok(EndpointKind::Http);
        };
        if envelope.get("jsonrpc").and_then(Value::as_str) != Some("2.0") {
            return Ok(EndpointKind::Http);
        }
        if let Some(error) = envelope.get("error") {
            bail!("MCP initialize rejected the request: {error}");
        }
        let Some(result) = envelope.get("result").and_then(Value::as_object) else {
            return Ok(EndpointKind::Http);
        };
        let Some(capabilities) = result.get("capabilities").and_then(Value::as_object) else {
            return Ok(EndpointKind::Http);
        };
        let supports_tools = match capabilities.get("tools") {
            Some(value) if value.is_object() => true,
            Some(_) => bail!("invalid MCP initialize result: capabilities.tools must be an object"),
            None => false,
        };
        let _ = self.send_initialized().await;
        Ok(EndpointKind::Mcp { supports_tools })
    }

    async fn send_initialized(&self) -> Result<()> {
        let response = self
            .request()
            .json(&json!({
                "jsonrpc": "2.0",
                "method": "notifications/initialized"
            }))
            .send()
            .await
            .map_err(|error| anyhow!("{ENDPOINT_UNREACHABLE}: {error}"))?;
        if !response.status().is_success() {
            bail!(
                "{ENDPOINT_UNREACHABLE}: notifications/initialized returned HTTP {}",
                response.status().as_u16()
            );
        }
        Ok(())
    }

    async fn list_page(&mut self, cursor: Option<&str>) -> Result<ToolsPage> {
        let id = self.next_id;
        self.next_id += 1;
        let params = cursor
            .map(|value| json!({ "cursor": value }))
            .unwrap_or_else(|| json!({}));
        let response = self
            .request()
            .json(&json!({
                "jsonrpc": "2.0",
                "id": id,
                "method": "tools/list",
                "params": params
            }))
            .send()
            .await
            .map_err(|error| anyhow!("{ENDPOINT_UNREACHABLE}: {error}"))?;
        self.capture_session(&response);
        let status = response.status();
        let content_type = response_content_type(&response);
        let body = response.text().await.unwrap_or_default();
        if !status.is_success() {
            bail!(
                "{ENDPOINT_UNREACHABLE}: tools/list returned HTTP {}: {}",
                status.as_u16(),
                body.trim()
            );
        }
        if !is_mcp_content_type(&content_type) {
            bail!("invalid MCP tools/list response Content-Type: {content_type}");
        }
        let envelope =
            parse_response(&body).ok_or_else(|| anyhow!("invalid MCP tools/list response"))?;
        if envelope.get("jsonrpc").and_then(Value::as_str) != Some("2.0") {
            bail!("invalid MCP tools/list response: jsonrpc must be \"2.0\"");
        }
        if let Some(error) = envelope.get("error") {
            bail!("MCP tools/list failed: {error}");
        }
        let result = envelope
            .get("result")
            .and_then(Value::as_object)
            .ok_or_else(|| anyhow!("invalid MCP tools/list response: missing result"))?;
        let tools = result
            .get("tools")
            .and_then(Value::as_array)
            .ok_or_else(|| anyhow!("invalid MCP tools/list response: missing tools array"))?
            .iter()
            .filter_map(|tool| validate_tool(tool).ok())
            .collect::<Vec<_>>();
        let next_cursor = match result.get("nextCursor") {
            Some(Value::String(value)) => Some(value.clone()),
            Some(_) => bail!("invalid MCP tools/list response: nextCursor must be a string"),
            None => None,
        };
        Ok(ToolsPage {
            tools,
            next_cursor,
            meta: result.get("_meta").cloned(),
        })
    }

    async fn list_all(&mut self) -> Result<Vec<Value>> {
        let mut tools = Vec::new();
        let mut cursor = None;
        let mut seen = HashSet::new();
        loop {
            let page = self.list_page(cursor.as_deref()).await?;
            tools.extend(page.tools);
            let Some(next_cursor) = page.next_cursor else {
                return Ok(tools);
            };
            if !seen.insert(next_cursor.clone()) {
                bail!("invalid MCP tools/list response: repeated nextCursor");
            }
            cursor = Some(next_cursor);
        }
    }
}

fn validate_endpoint(endpoint: &str) -> Result<()> {
    let url = Url::parse(endpoint).map_err(|error| anyhow!("invalid endpoint URL: {error}"))?;
    let Some(host) = url.host_str() else {
        bail!("invalid endpoint URL: expected an absolute http or https URL");
    };
    if !matches!(url.scheme(), "http" | "https") {
        bail!("invalid endpoint URL: expected an absolute http or https URL");
    }
    let host = host.trim_end_matches('.');
    if host.eq_ignore_ascii_case("localhost") || host.to_ascii_lowercase().ends_with(".localhost") {
        bail!("invalid endpoint URL: localhost is not allowed");
    }
    Ok(())
}

fn response_content_type(response: &reqwest::Response) -> String {
    response
        .headers()
        .get(CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .unwrap_or("")
        .to_ascii_lowercase()
}

fn is_mcp_content_type(content_type: &str) -> bool {
    content_type.contains("application/json") || content_type.contains("text/event-stream")
}

fn parse_response(body: &str) -> Option<Value> {
    if let Ok(value) = serde_json::from_str::<Value>(body) {
        return (value.get("result").is_some() || value.get("error").is_some()).then_some(value);
    }

    let normalized = body.replace("\r\n", "\n");
    for event in normalized.split("\n\n") {
        let data = event
            .lines()
            .filter_map(|line| line.strip_prefix("data:").map(str::trim_start))
            .collect::<Vec<_>>()
            .join("\n");
        if data.is_empty() {
            continue;
        }
        if let Ok(value) = serde_json::from_str::<Value>(&data) {
            if value.get("result").is_some() || value.get("error").is_some() {
                return Some(value);
            }
        }
    }
    None
}

fn validate_tool(tool: &Value) -> Result<Value> {
    let object = tool
        .as_object()
        .ok_or_else(|| anyhow!("invalid MCP tool: expected an object"))?;
    let name = object
        .get("name")
        .and_then(Value::as_str)
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| anyhow!("invalid MCP tool: missing or blank name"))?;
    if name.is_empty() {
        bail!("invalid MCP tool: missing name");
    }
    let mut filtered = serde_json::Map::new();
    filtered.insert("name".to_owned(), Value::String(name.to_string()));
    if let Some(value) = object.get("description").and_then(Value::as_str) {
        filtered.insert("description".to_owned(), Value::String(value.to_string()));
    }
    if let Some(value) = object.get("inputSchema").filter(|value| value.is_object()) {
        if serde_json::to_vec(value)
            .map_err(|error| anyhow!("invalid MCP tool inputSchema: {error}"))?
            .len()
            > MAX_INPUT_SCHEMA_BYTES
        {
            bail!("invalid MCP tool: inputSchema exceeds {MAX_INPUT_SCHEMA_BYTES} bytes");
        }
        filtered.insert("inputSchema".to_owned(), value.clone());
    }
    if let Some(value) = object.get("title").and_then(Value::as_str) {
        filtered.insert("title".to_owned(), Value::String(value.to_string()));
    }
    Ok(Value::Object(filtered))
}

fn select_tools(tools: Vec<Value>, tool_name: Option<&str>) -> Vec<Value> {
    match tool_name {
        Some(name) => tools
            .into_iter()
            .filter(|tool| tool.get("name").and_then(Value::as_str) == Some(name))
            .collect(),
        None => tools,
    }
}

pub async fn mcp_tool_list(args: McpToolListArgs) -> Result<()> {
    let mut client = McpToolListClient::new(&args.endpoint)?;
    let EndpointKind::Mcp { supports_tools } = client.detect().await? else {
        output::success(McpToolListOutput {
            r#type: "http",
            tools: Vec::new(),
            next_cursor: None,
            meta: None,
        });
        return Ok(());
    };
    if !supports_tools {
        output::success(McpToolListOutput {
            r#type: "mcp",
            tools: Vec::new(),
            next_cursor: None,
            meta: None,
        });
        return Ok(());
    }

    let fetch_all = args.all || args.tool_name.is_some();
    let (tools, next_cursor, meta) = if fetch_all {
        (client.list_all().await?, None, None)
    } else {
        let page = client.list_page(args.cursor.as_deref()).await?;
        (page.tools, page.next_cursor, page.meta)
    };
    output::success(McpToolListOutput {
        r#type: "mcp",
        tools: select_tools(tools, args.tool_name.as_deref()),
        next_cursor,
        meta,
    });
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Read, Write};
    use std::net::{TcpListener, TcpStream};

    fn read_request(stream: &mut TcpStream) -> String {
        let mut request = Vec::new();
        let mut buffer = [0_u8; 4096];
        loop {
            let read = stream.read(&mut buffer).unwrap();
            request.extend_from_slice(&buffer[..read]);
            let Some(header_end) = request.windows(4).position(|window| window == b"\r\n\r\n")
            else {
                continue;
            };
            let headers = String::from_utf8_lossy(&request[..header_end]);
            let content_length = headers
                .lines()
                .find_map(|line| {
                    line.to_ascii_lowercase()
                        .strip_prefix("content-length:")
                        .and_then(|value| value.trim().parse::<usize>().ok())
                })
                .unwrap_or(0);
            if request.len() >= header_end + 4 + content_length {
                return String::from_utf8(request).unwrap();
            }
        }
    }

    fn write_response(stream: &mut TcpStream, status: &str, headers: &str, body: &str) {
        write!(
            stream,
            "HTTP/1.1 {status}\r\nContent-Length: {}\r\n{headers}Connection: close\r\n\r\n{body}",
            body.len()
        )
        .unwrap();
    }

    #[test]
    fn parses_json_and_sse_responses_without_matching_request_id() {
        let json = r#"{"jsonrpc":"2.0","id":2,"result":{}}"#;
        assert!(parse_response(json).is_some());
        let sse = "event: message\r\ndata: {\"jsonrpc\":\"2.0\",\"id\":2,\"result\":{}}\r\n\r\n";
        assert!(parse_response(sse).is_some());
    }

    #[test]
    fn exact_name_filter_preserves_allowed_tool_fields() {
        let tools = vec![
            json!({"name":"one","description":"First tool","inputSchema":{"type":"object"}}),
            json!({"name":"two","title":"Two","description":"Second tool","inputSchema":{"type":"object"}}),
        ];
        let selected = select_tools(tools, Some("two"));
        assert_eq!(selected.len(), 1);
        assert_eq!(selected[0]["title"], "Two");
    }

    #[test]
    fn requires_only_a_tool_name() {
        assert!(validate_tool(&json!({"name":"ok"})).is_ok());
        assert!(validate_tool(&json!({"description":"missing name"})).is_err());
    }

    #[test]
    fn preserves_tool_text_longer_than_512_characters() {
        let long_text = "x".repeat(513);
        let tool = validate_tool(&json!({
            "name": long_text,
            "title": long_text,
            "description": long_text,
        }))
        .unwrap();

        assert_eq!(tool["name"].as_str().unwrap().chars().count(), 513);
        assert_eq!(tool["title"].as_str().unwrap().chars().count(), 513);
        assert_eq!(tool["description"].as_str().unwrap().chars().count(), 513);
    }

    #[test]
    fn tool_output_contains_only_allowed_fields() {
        let tool = validate_tool(&json!({
            "name": "quote",
            "title": "Get quote",
            "description": "Returns a quote",
            "inputSchema": {"type": "object"},
            "outputSchema": {"type": "object"},
            "annotations": {"readOnlyHint": true},
            "_meta": {"provider": "example"}
        }))
        .unwrap();

        assert_eq!(
            tool,
            json!({
                "description": "Returns a quote",
                "name": "quote",
                "inputSchema": {"type": "object"},
                "title": "Get quote"
            })
        );
    }

    #[test]
    fn tool_output_omits_malformed_optional_fields() {
        let tool = validate_tool(&json!({
            "name": "quote",
            "description": 1,
            "inputSchema": "not an object",
            "title": false
        }))
        .unwrap();

        assert_eq!(tool, json!({"name": "quote"}));
    }

    #[test]
    fn rejects_localhost_endpoints() {
        assert!(validate_endpoint("http://localhost:3000/mcp").is_err());
        assert!(validate_endpoint("https://LOCALHOST/mcp").is_err());
        assert!(validate_endpoint("https://tools.localhost/mcp").is_err());
        assert!(validate_endpoint("https://localhost./mcp").is_err());
    }

    #[tokio::test]
    async fn ordinary_json_api_is_http() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let server = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let request = read_request(&mut stream);
            assert!(request.contains("\"method\":\"initialize\""));
            write_response(
                &mut stream,
                "200 OK",
                "Content-Type: application/json\r\n",
                r#"{"price":"1.23"}"#,
            );
        });

        let mut client = McpToolListClient::new(&format!("http://{address}/prices")).unwrap();
        assert!(matches!(client.detect().await.unwrap(), EndpointKind::Http));
        server.join().unwrap();
    }

    #[tokio::test]
    async fn mcp_detection_and_cursor_traversal_accept_minimal_server_metadata() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let server = std::thread::spawn(move || {
            for step in 0..4 {
                let (mut stream, _) = listener.accept().unwrap();
                let request = read_request(&mut stream);
                match step {
                    0 => {
                        assert!(request.contains("\"method\":\"initialize\""));
                        assert!(!request
                            .to_ascii_lowercase()
                            .contains("mcp-protocol-version:"));
                        write_response(
                            &mut stream,
                            "200 OK",
                            "Content-Type: application/json\r\nMcp-Session-Id: session-1\r\n",
                            r#"{"jsonrpc":"2.0","id":1,"result":{"capabilities":{"tools":{}}}}"#,
                        );
                    }
                    1 => {
                        assert!(request.contains("notifications/initialized"));
                        assert!(!request
                            .to_ascii_lowercase()
                            .contains("mcp-protocol-version:"));
                        assert!(request
                            .to_ascii_lowercase()
                            .contains("mcp-session-id: session-1"));
                        write_response(&mut stream, "500 Internal Server Error", "", "");
                    }
                    2 => {
                        assert!(request.contains("\"method\":\"tools/list\""));
                        assert!(!request.contains("\"cursor\""));
                        write_response(
                            &mut stream,
                            "200 OK",
                            "Content-Type: text/event-stream\r\n",
                            "event: message\ndata: {\"jsonrpc\":\"2.0\",\"id\":2,\"result\":{\"tools\":[{\"name\":\"one\",\"description\":\"First tool\",\"inputSchema\":{}},\"not-a-tool\"],\"nextCursor\":\"page-2\"}}\n\n",
                        );
                    }
                    _ => {
                        assert!(request.contains("\"cursor\":\"page-2\""));
                        write_response(
                            &mut stream,
                            "200 OK",
                            "Content-Type: application/json\r\n",
                            r#"{"jsonrpc":"2.0","id":3,"result":{"tools":[{"name":"two","title":"Two","description":"Second tool","inputSchema":{}}]}}"#,
                        );
                    }
                }
            }
        });

        let mut client = McpToolListClient::new(&format!("http://{address}/mcp")).unwrap();
        assert!(matches!(
            client.detect().await.unwrap(),
            EndpointKind::Mcp {
                supports_tools: true
            }
        ));
        let tools = client.list_all().await.unwrap();
        server.join().unwrap();
        assert_eq!(tools.len(), 2);
        assert_eq!(tools[0]["name"], "one");
        assert_eq!(tools[1]["title"], "Two");
    }
}
