//! 共享工具：ws-mock agent 二进制公用的类型和辅助函数。

use serde_json::json;
use std::io::{self, Write};

pub const SERVER_URL: &str = "ws://127.0.0.1:9000";

/// 固定 mock 钱包地址
pub const SELLER_ADDR: &str    = "0xSeller000000000000000000000000000000001";
pub const ARB_ADDR: &str       = "0xArbitrator0000000000000000000000000001";
pub const MOCK_BUYER_ADDR: &str = "0xMockBuyer00000000000000000000000000001";

/// 确定性买卖双方会话 ID（两地址排序后拼接）
pub fn conv_id_bs(job_id: &str, buyer_addr: &str) -> String {
    format!("conv-{job_id}-{buyer_addr}-{SELLER_ADDR}")
}

/// 确定性三方仲裁会话 ID（三地址排序后拼接）
pub fn conv_id_arb(job_id: &str, buyer_addr: &str) -> String {
    let mut addrs = [buyer_addr, SELLER_ADDR, ARB_ADDR];
    addrs.sort();
    format!("conv-arb-{job_id}-{}-{}-{}", addrs[0], addrs[1], addrs[2])
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

pub fn register_ws_action(addr: &str) -> serde_json::Value {
    json!({ "action": "Register", "addr": addr })
}

pub fn register_identity_action(erc_role: &str, addr: &str) -> serde_json::Value {
    json!({ "action": "RegisterIdentity", "role": erc_role, "addr": addr })
}

pub fn lookup_role_action(role: &str) -> serde_json::Value {
    json!({ "action": "LookupRole", "role": role })
}

/// 从命令行参数解析 `--buyer-addr` / `-b`
pub fn parse_buyer_addr_arg() -> Option<String> {
    let args: Vec<String> = std::env::args().collect();
    for i in 0..args.len() {
        if args[i] == "--buyer-addr" || args[i] == "-b" {
            return args.get(i + 1).cloned();
        }
    }
    None
}

/// 地址到可读标签（buyer_addr 在运行时确定）
pub fn addr_label<'a>(addr: &'a str, buyer_addr: &'a str) -> &'a str {
    if addr == buyer_addr        { "买家" }
    else if addr == SELLER_ADDR  { "卖家" }
    else if addr == ARB_ADDR     { "仲裁者" }
    else                         { addr }
}

/// 处理一条入站 WS JSON 帧：打印内容并重新显示提示符。
/// msg_type == "registered" 时静默返回（连接确认噪音）。
pub fn handle_inbound(v: &serde_json::Value, prompt: &str, buyer_addr: &str) {
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
                "\x1b[32m✓ 身份已注册: role={} addr={}\x1b[0m",
                v["role"].as_str().unwrap_or("?"),
                v["addr"].as_str().unwrap_or("?"),
            );
        }
        "addr_lookup" => {
            let addr = v["addr"].as_str().unwrap_or("?");
            match v["identity"].as_object() {
                Some(id) => println!(
                    "\x1b[36m[地址查询] {} → role={}\x1b[0m",
                    addr,
                    id.get("role").and_then(|r| r.as_str()).unwrap_or("?")
                ),
                None => println!("\x1b[33m[地址查询] {} 未注册\x1b[0m", addr),
            }
        }
        "identity_lookup" | "identity_list" => {
            let field = if msg_type == "identity_lookup" { "agents" } else { "identities" };
            let role_q = v["role"].as_str().unwrap_or("");
            let count = v[field].as_array().map(|a| a.len()).unwrap_or(0);
            println!("\x1b[36m[身份查询] role={role_q} 共 {count} 个:\x1b[0m");
            if let Some(arr) = v[field].as_array() {
                for a in arr {
                    println!("  addr={}", a["addr"].as_str().unwrap_or("?"));
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
                addr_label(from, buyer_addr),
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
