//! 共享工具：ws-mock agent 二进制公用的类型和辅助函数。

use serde_json::json;
use std::io::{self, Write};

pub const SERVER_URL: &str = "ws://127.0.0.1:9000";

// ── WS 路由地址（comm_addr，用于 Register / JoinConversation 参与者列表）──────
pub const SELLER_COMM_ADDR: &str     = "0xSeller000000000000000000000000000000001";
pub const ARB_COMM_ADDR: &str        = "0xArbitrator0000000000000000000000000001";
pub const MOCK_BUYER_COMM_ADDR: &str = "0xMockBuyer00000000000000000000000000001";

// ── Agent 逻辑身份（agentId，用于 conv_id / RegisterIdentity）────────────────
pub const SELLER_AGENT_ID: &str      = "mock-seller-agent-001";
pub const ARB_AGENT_ID: &str         = "mock-arbitrator-agent-001";
pub const MOCK_BUYER_AGENT_ID: &str  = "mock-buyer-agent-001";

/// 买卖双方会话 ID：买家 agentId 在前，卖家 agentId 在后
pub fn conv_id_bs(job_id: &str, buyer_agent_id: &str) -> String {
    format!("conv-{job_id}-{buyer_agent_id}-{SELLER_AGENT_ID}")
}

/// 三方仲裁会话 ID
pub fn conv_id_arb(job_id: &str, buyer_agent_id: &str) -> String {
    format!("conv-arb-{job_id}-{buyer_agent_id}-{SELLER_AGENT_ID}-{ARB_AGENT_ID}")
}

pub fn join_conv_action(conv_id: &str, participants: &[&str]) -> serde_json::Value {
    json!({
        "action": "JoinConversation",
        "conversation_id": conv_id,
        "participants": participants,
    })
}

pub fn send_action(
    conv_id: &str,
    msg_type: &str,
    content: &str,
    job_id: Option<&str>,
) -> serde_json::Value {
    let mut payload = json!({ "type": msg_type, "content": content });
    if let Some(jid) = job_id {
        payload["jobId"] = json!(jid);
    }
    json!({
        "action": "Send",
        "conversation_id": conv_id,
        "payload": payload,
    })
}

pub fn register_ws_action(comm_addr: &str) -> serde_json::Value {
    json!({ "action": "Register", "addr": comm_addr })
}

/// 注册 ERC-8004 身份：agent_id 是逻辑标识，comm_addr 是 WS 路由地址
pub fn register_identity_action(erc_role: &str, agent_id: &str, comm_addr: &str) -> serde_json::Value {
    json!({
        "action": "RegisterIdentity",
        "role": erc_role,
        "agent_id": agent_id,
        "comm_addr": comm_addr
    })
}

pub fn lookup_role_action(role: &str) -> serde_json::Value {
    json!({ "action": "LookupRole", "role": role })
}

/// 从命令行参数解析 `--buyer-addr` / `-b`（comm_addr）
pub fn parse_buyer_addr_arg() -> Option<String> {
    let args: Vec<String> = std::env::args().collect();
    for i in 0..args.len() {
        if args[i] == "--buyer-addr" || args[i] == "-b" {
            return args.get(i + 1).cloned();
        }
    }
    None
}

/// 从命令行参数解析 `--buyer-agent-id`（逻辑 agentId）
pub fn parse_buyer_agent_id_arg() -> Option<String> {
    let args: Vec<String> = std::env::args().collect();
    for i in 0..args.len() {
        if args[i] == "--buyer-agent-id" {
            return args.get(i + 1).cloned();
        }
    }
    None
}

/// comm_addr 到可读标签
pub fn addr_label<'a>(addr: &'a str, buyer_comm_addr: &'a str) -> &'a str {
    if addr == buyer_comm_addr       { "买家" }
    else if addr == SELLER_COMM_ADDR { "卖家" }
    else if addr == ARB_COMM_ADDR    { "仲裁者" }
    else                             { addr }
}

/// 处理一条入站 WS JSON 帧：打印内容并重新显示提示符。
/// msg_type == "registered" 时静默返回（连接确认噪音）。
pub fn handle_inbound(v: &serde_json::Value, prompt: &str, buyer_comm_addr: &str) {
    let msg_type = v["type"].as_str().unwrap_or("");
    match msg_type {
        "registered" => return,
        "conversation_joined" => {
            println!(
                "\x1b[32m✓ 已加入会话: {}\x1b[0m",
                v["conversation_id"].as_str().unwrap_or("?")
            );
        }
        "identity_registered" => {
            println!(
                "\x1b[32m✓ 身份已注册: role={} agent_id={} comm_addr={}\x1b[0m",
                v["role"].as_str().unwrap_or("?"),
                v["agent_id"].as_str().unwrap_or("?"),
                v["comm_addr"].as_str().unwrap_or("?"),
            );
        }
        "addr_lookup" => {
            let agent_id = v["agent_id"].as_str().unwrap_or("?");
            match v["identity"].as_object() {
                Some(id) => println!(
                    "\x1b[36m[身份查询] {} → role={} comm_addr={}\x1b[0m",
                    agent_id,
                    id.get("role").and_then(|r| r.as_str()).unwrap_or("?"),
                    id.get("comm_addr").and_then(|r| r.as_str()).unwrap_or("?"),
                ),
                None => println!("\x1b[33m[身份查询] {} 未注册\x1b[0m", agent_id),
            }
        }
        "identity_lookup" | "identity_list" => {
            let field = if msg_type == "identity_lookup" { "agents" } else { "identities" };
            let role_q = v["role"].as_str().unwrap_or("");
            let count = v[field].as_array().map(|a| a.len()).unwrap_or(0);
            println!("\x1b[36m[身份查询] role={role_q} 共 {count} 个:\x1b[0m");
            if let Some(arr) = v[field].as_array() {
                for a in arr {
                    println!(
                        "  agent_id={} comm_addr={}",
                        a["agent_id"].as_str().unwrap_or("?"),
                        a["comm_addr"].as_str().unwrap_or("?"),
                    );
                }
            }
        }
        "error" => {
            println!("\x1b[31m[错误] {}\x1b[0m", v["msg"].as_str().unwrap_or("unknown"));
        }
        _ => {
            let from = v["from"].as_str().unwrap_or("unknown");
            let conv_id = v["conversation_id"].as_str().unwrap_or("?");
            let n = conv_id.len().min(30);
            let payload_type = v["payload"]["type"].as_str().unwrap_or("MSG");
            let payload_str = v["payload"].to_string();
            let content = v["payload"]["content"].as_str().unwrap_or(&payload_str);
            println!(
                "\n\x1b[90m[收到]\x1b[0m \x1b[1m{}\x1b[0m \x1b[90m({}) conv:{}\x1b[0m\n  {}",
                addr_label(from, buyer_comm_addr),
                payload_type,
                &conv_id[..n],
                content,
            );
            if let Some(jid) = v["payload"]["jobId"].as_str() {
                println!("  \x1b[90mjobId: {jid}\x1b[0m");
            }
        }
    }
    print!("{prompt}");
    io::stdout().flush().ok();
}
