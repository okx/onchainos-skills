//! Integration tests for the `onchainos mcp` MCP server.
//!
//! Each test spawns the binary in MCP mode, performs a JSON-RPC 2.0 handshake
//! over stdio, then exercises one or more tools.

mod common;

use serde_json::{json, Value};
use std::io::{BufRead, BufReader, Write};
use std::process::{Child, ChildStdin, Command, Stdio};

fn onchainos_bin() -> Command {
    let cmd = assert_cmd::cargo::cargo_bin_cmd!("onchainos");
    Command::new(cmd.get_program())
}

// ── MCP client helper ──────────────────────────────────────────────────

struct McpClient {
    child: Child,
    stdin: ChildStdin,
    reader: BufReader<std::process::ChildStdout>,
    next_id: u64,
}

impl McpClient {
    fn start() -> Self {
        let mut child = onchainos_bin()
            .args(["mcp"])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .expect("failed to spawn onchainos mcp");

        let stdin = child.stdin.take().expect("no stdin");
        let stdout = child.stdout.take().expect("no stdout");
        let reader = BufReader::new(stdout);

        let mut client = Self {
            child,
            stdin,
            reader,
            next_id: 1,
        };
        client.handshake();
        client
    }

    fn send(&mut self, msg: &Value) {
        let line = serde_json::to_string(msg).unwrap();
        writeln!(self.stdin, "{line}").expect("write failed");
        self.stdin.flush().expect("flush failed");
    }

    fn recv(&mut self) -> Value {
        let mut line = String::new();
        self.reader.read_line(&mut line).expect("read failed");
        serde_json::from_str(line.trim()).unwrap_or_else(|e| {
            panic!("invalid JSON from server: {e}\nraw: {line}");
        })
    }

    fn handshake(&mut self) {
        self.send(&json!({
            "jsonrpc": "2.0",
            "id": 0,
            "method": "initialize",
            "params": {
                "protocolVersion": "2024-11-05",
                "capabilities": {},
                "clientInfo": {"name": "integration-test", "version": "0.1"}
            }
        }));
        let resp = self.recv();
        assert_eq!(resp["result"]["protocolVersion"], "2024-11-05");
        // Send initialized notification (no response expected)
        self.send(&json!({
            "jsonrpc": "2.0",
            "method": "notifications/initialized"
        }));
    }

    fn call_tool(&mut self, name: &str, args: Value) -> McpToolResult {
        let id = self.next_id;
        self.next_id += 1;
        self.send(&json!({
            "jsonrpc": "2.0",
            "id": id,
            "method": "tools/call",
            "params": {"name": name, "arguments": args}
        }));
        let resp = self.recv();
        assert!(resp.get("error").is_none(), "JSON-RPC error: {resp}");
        let result = &resp["result"];
        let content = result["content"][0]["text"]
            .as_str()
            .unwrap_or("")
            .to_string();
        let is_error = result
            .get("isError")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);
        McpToolResult { content, is_error }
    }

    fn list_tools(&mut self) -> Vec<String> {
        let id = self.next_id;
        self.next_id += 1;
        self.send(&json!({
            "jsonrpc": "2.0",
            "id": id,
            "method": "tools/list",
            "params": {}
        }));
        let resp = self.recv();
        resp["result"]["tools"]
            .as_array()
            .unwrap()
            .iter()
            .map(|t| t["name"].as_str().unwrap().to_string())
            .collect()
    }
}

impl Drop for McpClient {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

struct McpToolResult {
    content: String,
    is_error: bool,
}

impl McpToolResult {
    fn is_rate_limited(&self) -> bool {
        self.is_error && self.content.contains("Rate limited")
    }

    fn json(&self) -> Value {
        serde_json::from_str(&self.content)
            .unwrap_or_else(|e| panic!("tool response is not JSON: {e}\nraw: {}", self.content))
    }

    fn api_data(&self) -> Value {
        assert!(
            !self.is_error,
            "tool returned isError=true: {}",
            self.content
        );
        let v = self.json();
        if v.is_object() && v.get("data").is_some() {
            v["data"].clone()
        } else {
            v
        }
    }
}

// ── Initialize & metadata ──────────────────────────────────────────────

#[test]
fn mcp_initialize_returns_server_info() {
    let mut child = onchainos_bin()
        .args(["mcp"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .expect("spawn failed");

    let mut stdin = child.stdin.take().unwrap();
    let stdout = child.stdout.take().unwrap();
    let mut reader = BufReader::new(stdout);

    let init = json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "initialize",
        "params": {
            "protocolVersion": "2024-11-05",
            "capabilities": {},
            "clientInfo": {"name": "test", "version": "0.1"}
        }
    });
    writeln!(stdin, "{}", serde_json::to_string(&init).unwrap()).unwrap();
    stdin.flush().unwrap();

    let mut line = String::new();
    reader.read_line(&mut line).unwrap();
    let resp: Value = serde_json::from_str(line.trim()).unwrap();

    assert_eq!(resp["result"]["serverInfo"]["name"], "onchainos");
    assert!(!resp["result"]["serverInfo"]["version"]
        .as_str()
        .unwrap_or("")
        .is_empty());
    assert!(resp["result"]["capabilities"]["tools"].is_object());

    let _ = child.kill();
    let _ = child.wait();
}

#[test]
fn mcp_tools_list_returns_all_tools() {
    let mut client = McpClient::start();
    let tools = client.list_tools();

    // Verify minimum tool count (includes 5 workflow tools added in feat/workflows)
    assert!(
        tools.len() >= 53,
        "expected at least 53 tools, got {}: {:?}",
        tools.len(),
        tools
    );

    // Verify key tool categories exist
    let expected = [
        "token_search",
        "token_info",
        "market_price",
        "market_kline",
        "tracker_activities",
        "swap_quote",
        "swap_swap",
        "portfolio_total_value",
        "gateway_gas",
        "gateway_broadcast",
        "signal_list",
        "memepump_tokens",
        "leaderboard_chains",
        "leaderboard_list",
        "token_cluster_supported_chains",
        "token_cluster_overview",
        "token_cluster_top_holders",
        "token_cluster_list",
        // Workflow tools
        "workflow_token_research",
        "workflow_smart_money",
        "workflow_new_tokens",
        "workflow_wallet_analysis",
        "workflow_portfolio",
    ];
    for name in expected {
        assert!(
            tools.contains(&name.to_string()),
            "missing expected tool: {name}"
        );
    }
}

// ── Token tools ────────────────────────────────────────────────────────

#[test]
fn mcp_token_search() {
    let mut client = McpClient::start();
    let result = client.call_tool(
        "token_search",
        json!({"query": "USDC", "chains": "ethereum"}),
    );
    if result.is_rate_limited() {
        return;
    }
    let data = result.api_data();
    assert!(data.is_array(), "expected array: {data}");
    assert!(!data.as_array().unwrap().is_empty());
}

#[test]
fn mcp_token_info() {
    let mut client = McpClient::start();
    let result = client.call_tool(
        "token_info",
        json!({"address": common::tokens::ETH_USDC, "chain": "ethereum"}),
    );
    if result.is_rate_limited() {
        return;
    }
    let data = result.api_data();
    assert!(data.is_array(), "expected array: {data}");
}

// ── Market tools ──────────────────────────────────────────────────────

#[test]
fn mcp_market_price() {
    let mut client = McpClient::start();
    let result = client.call_tool(
        "market_price",
        json!({"address": common::tokens::EVM_NATIVE, "chain": "ethereum"}),
    );
    if result.is_rate_limited() {
        return;
    }
    let data = result.api_data();
    assert!(data.is_array(), "expected array: {data}");
}

#[test]
fn mcp_market_kline() {
    let mut client = McpClient::start();
    let result = client.call_tool(
        "market_kline",
        json!({
            "address": common::tokens::EVM_NATIVE,
            "chain": "ethereum",
            "bar": "1H",
            "limit": 10
        }),
    );
    if result.is_rate_limited() {
        return;
    }
    let data = result.api_data();
    assert!(data.is_array(), "expected candle data: {data}");
}

#[test]
fn mcp_tracker_activities() {
    let mut client = McpClient::start();
    let result = client.call_tool("tracker_activities", json!({"tracker_type": "smart_money"}));
    if result.is_rate_limited() {
        return;
    }
    let data = result.api_data();
    assert!(data["trades"].is_array(), "expected trades array: {data}");
}

// ── Swap tools ─────────────────────────────────────────────────────────

#[test]
fn mcp_swap_chains() {
    let mut client = McpClient::start();
    let result = client.call_tool("swap_chains", json!({}));
    if result.is_rate_limited() {
        return;
    }
    let data = result.api_data();
    assert!(data.is_array(), "expected chain list: {data}");
    assert!(!data.as_array().unwrap().is_empty());
}

#[test]
fn mcp_swap_quote() {
    let mut client = McpClient::start();
    let result = client.call_tool(
        "swap_quote",
        json!({
            "from": common::tokens::EVM_NATIVE,
            "to": common::tokens::ETH_USDC,
            "amount": "1000000000000000000",
            "chain": "ethereum"
        }),
    );
    assert!(
        !result.is_error || result.is_rate_limited(),
        "swap quote failed: {}",
        result.content
    );
}

// ── Portfolio tools ────────────────────────────────────────────────────

#[test]
fn mcp_portfolio_chains() {
    let mut client = McpClient::start();
    let result = client.call_tool("portfolio_chains", json!({}));
    if result.is_rate_limited() {
        return;
    }
    let data = result.api_data();
    assert!(data.is_array(), "expected chain list: {data}");
}

#[test]
fn mcp_portfolio_total_value() {
    let mut client = McpClient::start();
    // Vitalik's address — known to have a balance
    let result = client.call_tool(
        "portfolio_total_value",
        json!({
            "address": "0xd8dA6BF26964aF9D7eEd9e03E53415D37aA96045",
            "chains": "ethereum"
        }),
    );
    assert!(
        !result.is_error || result.is_rate_limited(),
        "portfolio query failed: {}",
        result.content
    );
}

// ── Gateway tools ──────────────────────────────────────────────────────

#[test]
fn mcp_gateway_chains() {
    let mut client = McpClient::start();
    let result = client.call_tool("gateway_chains", json!({}));
    if result.is_rate_limited() {
        return;
    }
    let data = result.api_data();
    assert!(data.is_array(), "expected chain list: {data}");
}

#[test]
fn mcp_gateway_gas() {
    let mut client = McpClient::start();
    let result = client.call_tool("gateway_gas", json!({"chain": "ethereum"}));
    assert!(
        !result.is_error || result.is_rate_limited(),
        "gas query failed: {}",
        result.content
    );
}

// ── Signal tools ───────────────────────────────────────────────────────

#[test]
fn mcp_signal_chains() {
    let mut client = McpClient::start();
    let result = client.call_tool("signal_chains", json!({}));
    if result.is_rate_limited() {
        return;
    }
    let data = result.api_data();
    assert!(data.is_array(), "expected chain list: {data}");
}

// ── Leaderboard tools ──────────────────────────────────────────────────

#[test]
fn mcp_leaderboard_chains() {
    let mut client = McpClient::start();
    let result = client.call_tool("leaderboard_chains", json!({}));
    if result.is_rate_limited() {
        return;
    }
    let data = result.api_data();
    assert!(data.is_array(), "expected chain list: {data}");
    let arr = data.as_array().unwrap();
    assert!(!arr.is_empty(), "expected at least one supported chain");
    assert!(
        arr[0].get("chainIndex").is_some(),
        "chain entry missing 'chainIndex': {}",
        arr[0]
    );
}

#[test]
fn mcp_leaderboard_list_solana_pnl() {
    let mut client = McpClient::start();
    let result = client.call_tool(
        "leaderboard_list",
        json!({"chain": "solana", "time_frame": "3", "sort_by": "1"}),
    );
    if result.is_rate_limited() {
        return;
    }
    let data = result.api_data();
    assert!(data.is_array(), "expected leaderboard array: {data}");
}

#[test]
#[should_panic(expected = "JSON-RPC error")]
fn mcp_leaderboard_list_missing_chain_fails() {
    let mut client = McpClient::start();
    client.call_tool(
        "leaderboard_list",
        json!({"time_frame": "3", "sort_by": "1"}),
    );
}

// ── Memepump tools ─────────────────────────────────────────────────────

#[test]
fn mcp_memepump_chains() {
    let mut client = McpClient::start();
    let result = client.call_tool("memepump_chains", json!({}));
    if result.is_rate_limited() {
        return;
    }
    let data = result.api_data();
    assert!(data.is_array(), "expected chain/protocol list: {data}");
}

// ── Error handling ─────────────────────────────────────────────────────

#[test]
fn mcp_tool_error_sets_is_error_flag() {
    let mut client = McpClient::start();
    // Invalid chain name triggers an API error
    let result = client.call_tool("gateway_gas", json!({"chain": "nonexistent_chain_xyz"}));
    assert!(
        result.is_error,
        "expected isError=true for invalid chain, got: {}",
        result.content
    );
}

#[test]
fn mcp_unknown_tool_returns_jsonrpc_error() {
    let mut client = McpClient::start();
    let id = client.next_id;
    client.next_id += 1;
    client.send(&json!({
        "jsonrpc": "2.0",
        "id": id,
        "method": "tools/call",
        "params": {"name": "nonexistent_tool_xyz", "arguments": {}}
    }));
    let resp = client.recv();
    assert!(
        resp.get("error").is_some(),
        "expected JSON-RPC error for unknown tool: {resp}"
    );
}

// ── Multiple sequential calls on one connection ────────────────────────

#[test]
fn mcp_multiple_calls_on_same_connection() {
    let mut client = McpClient::start();

    let mut success_count = 0;
    for tool in &["swap_chains", "portfolio_chains", "gateway_chains"] {
        let result = client.call_tool(tool, json!({}));
        if result.is_error && result.content.contains("Rate limited") {
            continue; // rate limit from sandbox — skip
        }
        assert!(!result.is_error, "{tool} failed: {}", result.content);
        assert!(result.api_data().is_array(), "{tool} did not return array");
        success_count += 1;
    }
    // At least one call should succeed (connection stays alive)
    assert!(
        success_count >= 1,
        "all calls rate-limited; cannot verify connection reuse"
    );
}

// ── Workflow tools ─────────────────────────────────────────────────────────────

#[test]
fn mcp_workflow_token_research_returns_ok() {
    let mut client = McpClient::start();
    let result = client.call_tool(
        "workflow_token_research",
        json!({ "address": common::tokens::SOL_BONK, "chain": "solana" }),
    );
    if result.is_rate_limited() {
        return;
    }
    assert!(!result.is_error, "tool returned error: {}", result.content);
}

#[test]
fn mcp_workflow_token_research_has_discriminator() {
    let mut client = McpClient::start();
    let result = client.call_tool(
        "workflow_token_research",
        json!({ "address": common::tokens::SOL_BONK, "chain": "solana" }),
    );
    if result.is_rate_limited() {
        return;
    }
    let data = result.api_data();
    assert_eq!(data["workflow"], "token-research");
}

#[test]
fn mcp_workflow_token_research_has_core_and_structure() {
    let mut client = McpClient::start();
    let result = client.call_tool(
        "workflow_token_research",
        json!({ "address": common::tokens::SOL_BONK, "chain": "solana" }),
    );
    if result.is_rate_limited() {
        return;
    }
    let data = result.api_data();
    assert!(!data["core"].is_null(), "core block missing");
    assert!(!data["structure"].is_null(), "structure block missing");
    assert!(data.get("launchpad").is_some(), "launchpad field missing");
}

#[test]
fn mcp_workflow_token_research_non_launchpad_null_launchpad() {
    // BONK is not a pump.fun token — Step 3 skipped, launchpad must be null
    let mut client = McpClient::start();
    let result = client.call_tool(
        "workflow_token_research",
        json!({ "address": common::tokens::SOL_BONK, "chain": "solana" }),
    );
    if result.is_rate_limited() {
        return;
    }
    let data = result.api_data();
    assert!(
        data["launchpad"].is_null(),
        "expected null launchpad for non-launchpad token, got: {}",
        data["launchpad"]
    );
}

#[test]
fn mcp_workflow_token_research_missing_address_is_error() {
    let mut client = McpClient::start();
    // Omitting required `address` field — server should return isError=true
    let id = client.next_id;
    client.next_id += 1;
    client.send(&json!({
        "jsonrpc": "2.0",
        "id": id,
        "method": "tools/call",
        "params": { "name": "workflow_token_research", "arguments": { "chain": "solana" } }
    }));
    let resp = client.recv();
    // Either a JSON-RPC error or isError=true in result
    let is_err =
        resp.get("error").is_some() || resp["result"]["isError"].as_bool().unwrap_or(false);
    assert!(is_err, "expected error for missing address, got: {resp}");
}

#[test]
fn mcp_workflow_smart_money_returns_ok() {
    let mut client = McpClient::start();
    let result = client.call_tool("workflow_smart_money", json!({ "chain": "solana" }));
    if result.is_rate_limited() {
        return;
    }
    assert!(!result.is_error, "tool returned error: {}", result.content);
}

#[test]
fn mcp_workflow_smart_money_has_discriminator_and_top_tokens() {
    let mut client = McpClient::start();
    let result = client.call_tool("workflow_smart_money", json!({ "chain": "solana" }));
    if result.is_rate_limited() {
        return;
    }
    let data = result.api_data();
    assert_eq!(data["workflow"], "smart-money");
    assert!(
        data["topTokens"].is_array(),
        "topTokens must be array: {data}"
    );
    assert!(data.get("rawSignals").is_some(), "rawSignals field missing");
}

#[test]
fn mcp_workflow_new_tokens_returns_ok() {
    let mut client = McpClient::start();
    let result = client.call_tool(
        "workflow_new_tokens",
        json!({ "chain": "solana", "stage": "MIGRATED" }),
    );
    if result.is_rate_limited() {
        return;
    }
    assert!(!result.is_error, "tool returned error: {}", result.content);
}

#[test]
fn mcp_workflow_new_tokens_has_discriminator_stage_enriched() {
    let mut client = McpClient::start();
    let result = client.call_tool(
        "workflow_new_tokens",
        json!({ "chain": "solana", "stage": "MIGRATED" }),
    );
    if result.is_rate_limited() {
        return;
    }
    let data = result.api_data();
    assert_eq!(data["workflow"], "new-tokens");
    assert_eq!(data["stage"], "MIGRATED");
    assert!(
        data["enriched"].is_array(),
        "enriched must be array: {data}"
    );
}

#[test]
fn mcp_workflow_wallet_analysis_returns_ok() {
    let mut client = McpClient::start();
    let result = client.call_tool(
        "workflow_wallet_analysis",
        json!({ "address": common::tokens::ETH_VITALIK, "chain": "ethereum" }),
    );
    if result.is_rate_limited() {
        return;
    }
    assert!(!result.is_error, "tool returned error: {}", result.content);
}

#[test]
fn mcp_workflow_wallet_analysis_has_discriminator_and_performance() {
    let mut client = McpClient::start();
    let result = client.call_tool(
        "workflow_wallet_analysis",
        json!({ "address": common::tokens::ETH_VITALIK, "chain": "ethereum" }),
    );
    if result.is_rate_limited() {
        return;
    }
    let data = result.api_data();
    assert_eq!(data["workflow"], "wallet-analysis");
    assert_eq!(data["address"], common::tokens::ETH_VITALIK);
    assert!(
        data["performance"].get("7d").is_some(),
        "performance.7d missing"
    );
    assert!(
        data["performance"].get("30d").is_some(),
        "performance.30d missing"
    );
    assert!(data.get("balances").is_some(), "balances field missing");
    assert!(data.get("recentPnl").is_some(), "recentPnl field missing");
    assert!(data.get("activities").is_some(), "activities field missing");
}

#[test]
fn mcp_workflow_portfolio_returns_ok() {
    let mut client = McpClient::start();
    let result = client.call_tool(
        "workflow_portfolio",
        json!({ "address": common::tokens::ETH_VITALIK, "chains": "ethereum" }),
    );
    if result.is_rate_limited() {
        return;
    }
    assert!(!result.is_error, "tool returned error: {}", result.content);
}

#[test]
fn mcp_workflow_portfolio_has_discriminator_and_required_fields() {
    let mut client = McpClient::start();
    let result = client.call_tool(
        "workflow_portfolio",
        json!({ "address": common::tokens::ETH_VITALIK, "chains": "ethereum" }),
    );
    if result.is_rate_limited() {
        return;
    }
    let data = result.api_data();
    assert_eq!(data["workflow"], "portfolio");
    assert_eq!(data["address"], common::tokens::ETH_VITALIK);
    assert!(data.get("balances").is_some(), "balances field missing");
    assert!(data.get("totalValue").is_some(), "totalValue field missing");
    assert!(data.get("overview").is_some(), "overview field missing");
}
