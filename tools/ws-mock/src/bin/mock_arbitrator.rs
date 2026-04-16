//! mock-arbitrator: 仲裁者交互式测试工具（菜单驱动）
//!
//! 用法: mock-arbitrator [--buyer-addr <comm_addr>] [--buyer-agent-id <agentId>]

use futures_util::{SinkExt, StreamExt};
use serde_json::json;
use tokio::sync::mpsc;
use tokio_tungstenite::{connect_async, tungstenite::Message};
use ws_mock::{
    conv_id_arb, conv_id_bs, handle_inbound, lookup_role_action,
    parse_buyer_agent_id_arg, register_identity_action, register_ws_action, send_action,
    ARB_AGENT_ID, ARB_COMM_ADDR, MOCK_BUYER_AGENT_ID, MOCK_BUYER_COMM_ADDR,
    SELLER_COMM_ADDR, SERVER_URL,
};

fn parse_command(input: &str, buyer_agent_id: &str, buyer_comm_addr: &str) -> Vec<serde_json::Value> {
    let parts: Vec<&str> = input.trim().splitn(3, ' ').collect();
    match parts[0] {
        "/resolve" => {
            let job_id = parts.get(1).copied().unwrap_or("unknown");
            let winner = parts.get(2).copied().unwrap_or("unknown");
            let arb_conv = conv_id_arb(job_id, buyer_agent_id);
            let content = format!("仲裁结果: {winner} 胜，jobId: {job_id}");
            let mut action = send_action(&arb_conv, "TASK_RESOLVE", &content, Some(job_id));
            action["payload"]["winner"] = json!(winner);
            vec![action]
        }
        "/convid" => {
            let job_id = parts.get(1).copied().unwrap_or("jobId");
            println!("\x1b[90m买卖会话: {}\x1b[0m", conv_id_bs(job_id, buyer_agent_id));
            println!("\x1b[90m仲裁会话: {}\x1b[0m", conv_id_arb(job_id, buyer_agent_id));
            vec![]
        }
        "/join" => {
            let job_id = parts.get(1).copied().unwrap_or("unknown");
            let arb_conv = conv_id_arb(job_id, buyer_agent_id);
            println!("\x1b[90m加入仲裁会话: {arb_conv}\x1b[0m");
            vec![ws_mock::join_conv_action(&arb_conv, &[buyer_comm_addr, SELLER_COMM_ADDR, ARB_COMM_ADDR])]
        }
        "/register" => {
            println!("\x1b[90m注册身份: role=EVALUATOR agent_id={ARB_AGENT_ID} comm_addr={ARB_COMM_ADDR}\x1b[0m");
            vec![register_identity_action("EVALUATOR", ARB_AGENT_ID, ARB_COMM_ADDR)]
        }
        "/lookup" => {
            let r = parts.get(1).copied().unwrap_or("");
            match r.to_uppercase().as_str() {
                "REQUESTER" | "BUYER" | "买家" => vec![lookup_role_action("REQUESTER")],
                "PROVIDER" | "SELLER" | "卖家" => vec![lookup_role_action("PROVIDER")],
                "EVALUATOR" | "ARBITRATOR" | "ARB" | "仲裁者" => vec![lookup_role_action("EVALUATOR")],
                _ => vec![],
            }
        }
        "send" => {
            let conv_id = parts.get(1).copied().unwrap_or("");
            let content = parts.get(2).map(|s| s.trim()).unwrap_or("");
            if conv_id.is_empty() || content.is_empty() { return vec![]; }
            vec![send_action(conv_id, "TEXT", content, None)]
        }
        _ => vec![],
    }
}

fn pick_job_id(
    theme: &dialoguer::theme::ColorfulTheme,
    known: &mut Vec<String>,
) -> String {
    use dialoguer::{Input, Select};
    let job_id = if known.is_empty() {
        Input::with_theme(theme).with_prompt("jobId").interact_text().unwrap_or_default()
    } else {
        let mut items: Vec<String> = known.clone();
        items.push("[ 输入新 jobId ]".to_string());
        let sel = Select::with_theme(theme)
            .with_prompt("选择 jobId")
            .items(&items)
            .default(0)
            .interact()
            .unwrap_or(items.len() - 1);
        if sel == items.len() - 1 {
            Input::with_theme(theme).with_prompt("新 jobId").interact_text().unwrap_or_default()
        } else {
            items[sel].clone()
        }
    };
    if !job_id.is_empty() && !known.contains(&job_id) {
        known.push(job_id.clone());
    }
    job_id
}

fn run_menu(tx: mpsc::UnboundedSender<String>, buyer_agent_id: String, buyer_comm_addr: String) {
    use dialoguer::{theme::ColorfulTheme, Input, Select};
    let theme = ColorfulTheme::default();
    let mut known_tasks: Vec<String> = vec![];

    let items = [
        "/resolve buyer    裁定买家胜",
        "/resolve seller   裁定卖家胜",
        "/join             加入仲裁会话",
        "/convid           查看会话 ID",
        "/register         注册 ERC-8004 身份",
        "/lookup           查询角色列表",
        "send              发送自由文本",
        "quit              退出",
    ];

    loop {
        println!();
        let sel = match Select::with_theme(&theme)
            .with_prompt("\x1b[33m仲裁者\x1b[0m 选择操作")
            .items(&items)
            .default(0)
            .interact_opt()
        {
            Ok(Some(s)) => s,
            _ => break,
        };

        let cmd = match sel {
            0 => {
                let tid = pick_job_id(&theme, &mut known_tasks);
                format!("/resolve {tid} buyer")
            }
            1 => {
                let tid = pick_job_id(&theme, &mut known_tasks);
                format!("/resolve {tid} seller")
            }
            2 => {
                let tid = pick_job_id(&theme, &mut known_tasks);
                format!("/join {tid}")
            }
            3 => {
                let tid = pick_job_id(&theme, &mut known_tasks);
                format!("/convid {tid}")
            }
            4 => "/register".to_string(),
            5 => {
                let role = Select::with_theme(&theme)
                    .with_prompt("查询角色")
                    .items(&["buyer (REQUESTER)", "seller (PROVIDER)", "arbitrator (EVALUATOR)"])
                    .default(0)
                    .interact()
                    .unwrap_or(0);
                ["/lookup buyer", "/lookup seller", "/lookup arbitrator"][role].to_string()
            }
            6 => {
                let conv_id: String = Input::with_theme(&theme).with_prompt("convId").interact_text().unwrap_or_default();
                let content: String = Input::with_theme(&theme).with_prompt("内容").interact_text().unwrap_or_default();
                format!("send {conv_id} {content}")
            }
            _ => { println!("再见！"); std::process::exit(0); }
        };

        let actions = parse_command(&cmd, &buyer_agent_id, &buyer_comm_addr);
        for action in &actions {
            if action["action"].as_str() == Some("Send") {
                let conv_id = action["conversation_id"].as_str().unwrap_or("");
                let msg_type = action["payload"]["type"].as_str().unwrap_or("MSG");
                println!("\x1b[90m→ 发送 [{msg_type}] 到会话 {}\x1b[0m", &conv_id[..conv_id.len().min(30)]);
            }
            tx.send(action.to_string()).ok();
        }
    }
}

#[tokio::main]
async fn main() {
    let buyer_agent_id = parse_buyer_agent_id_arg().unwrap_or_else(|| MOCK_BUYER_AGENT_ID.to_string());

    println!("\x1b[33m[仲裁者]\x1b[0m agent_id: {ARB_AGENT_ID}  comm_addr: {ARB_COMM_ADDR}");
    println!("\x1b[90m买家 agent_id: {buyer_agent_id}\x1b[0m");
    println!("连接到 {SERVER_URL} ...");

    let (ws, _) = connect_async(SERVER_URL)
        .await
        .expect("无法连接 ws-mock server，请先启动 server");
    let (mut sink, mut stream) = ws.split();

    // 注册 WS 路由地址
    sink.send(Message::Text(register_ws_action(ARB_COMM_ADDR).to_string().into()))
        .await
        .unwrap();
    while let Some(Ok(Message::Text(text))) = stream.next().await {
        if let Ok(v) = serde_json::from_str::<serde_json::Value>(&text) {
            if v["type"].as_str() == Some("registered") { break; }
        }
    }
    println!("\x1b[32m✓ 已连接并注册\x1b[0m");

    // 通过 agentId 查买家的 comm_addr
    sink.send(Message::Text(
        serde_json::json!({ "action": "LookupAddr", "addr": buyer_agent_id }).to_string().into()
    )).await.unwrap();
    let buyer_comm_addr = {
        let mut found = MOCK_BUYER_COMM_ADDR.to_string();
        while let Some(Ok(Message::Text(text))) = stream.next().await {
            if let Ok(v) = serde_json::from_str::<serde_json::Value>(&text) {
                if v["type"].as_str() == Some("addr_lookup") {
                    if let Some(c) = v["identity"]["comm_addr"].as_str() {
                        found = c.to_string();
                        println!("\x1b[90m买家 comm_addr: {found}\x1b[0m");
                    } else {
                        println!("\x1b[33m⚠ 买家身份未注册，用默认 comm_addr: {found}\x1b[0m");
                    }
                    break;
                }
            }
        }
        found
    };

    let (tx, mut rx) = mpsc::unbounded_channel::<String>();

    tokio::spawn(async move {
        while let Some(msg) = rx.recv().await {
            sink.send(Message::Text(msg.into())).await.ok();
        }
    });

    let buyer_comm_addr_recv = buyer_comm_addr.clone();
    tokio::spawn(async move {
        while let Some(Ok(Message::Text(text))) = stream.next().await {
            if let Ok(v) = serde_json::from_str::<serde_json::Value>(&text) {
                handle_inbound(&v, "\x1b[33m仲裁者\x1b[0m > ", &buyer_comm_addr_recv);
            }
        }
    });

    tokio::task::spawn_blocking(move || run_menu(tx, buyer_agent_id, buyer_comm_addr))
        .await
        .ok();
}
