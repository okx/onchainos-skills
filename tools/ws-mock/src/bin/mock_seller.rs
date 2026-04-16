//! mock-seller: 卖家交互式测试工具（菜单驱动）
//!
//! 用法: mock-seller [--buyer-addr <addr>]

use futures_util::{SinkExt, StreamExt};
use tokio::sync::mpsc;
use tokio_tungstenite::{connect_async, tungstenite::Message};
use ws_mock::{
    conv_id_arb, conv_id_bs, handle_inbound, join_conv_action, lookup_role_action,
    parse_buyer_addr_arg, register_identity_action, register_ws_action, send_action,
    ARB_ADDR, MOCK_BUYER_ADDR, SELLER_ADDR, SERVER_URL,
};

fn parse_command(input: &str, buyer_addr: &str) -> Vec<serde_json::Value> {
    let parts: Vec<&str> = input.trim().splitn(3, ' ').collect();
    match parts[0] {
        "/connect" => {
            let job_id = parts.get(1).copied().unwrap_or("unknown");
            let conv_id = conv_id_bs(job_id, buyer_addr);
            println!("\x1b[90m创建会话: {conv_id}\x1b[0m");
            vec![
                join_conv_action(&conv_id, &[buyer_addr, SELLER_ADDR]),
                send_action(&conv_id, "TASK_INQUIRE", "你好，我对这个任务感兴趣，请介绍一下详情。", Some(job_id)),
            ]
        }
        "/accept" => {
            let job_id = parts.get(1).copied().unwrap_or("unknown");
            let conv_id = conv_id_bs(job_id, buyer_addr);
            vec![send_action(&conv_id, "TASK_ACCEPT", "我接单了，任务即将开始执行。", Some(job_id))]
        }
        "/deliver" => {
            let job_id = parts.get(1).copied().unwrap_or("unknown");
            let conv_id = conv_id_bs(job_id, buyer_addr);
            vec![send_action(&conv_id, "TASK_DELIVER", "任务已完成，请买家验收。", Some(job_id))]
        }
        "/dispute" => {
            let job_id = parts.get(1).copied().unwrap_or("unknown");
            let reason = parts.get(2).copied().unwrap_or("买家拒绝验收");
            let arb_conv = conv_id_arb(job_id, buyer_addr);
            println!("\x1b[90m创建仲裁会话: {arb_conv}\x1b[0m");
            vec![
                join_conv_action(&arb_conv, &[buyer_addr, SELLER_ADDR, ARB_ADDR]),
                send_action(&arb_conv, "TASK_DISPUTE", reason, Some(job_id)),
            ]
        }
        "/convid" => {
            let job_id = parts.get(1).copied().unwrap_or("jobId");
            println!("\x1b[90m买卖会话: {}\x1b[0m", conv_id_bs(job_id, buyer_addr));
            println!("\x1b[90m仲裁会话: {}\x1b[0m", conv_id_arb(job_id, buyer_addr));
            vec![]
        }
        "/register" => {
            println!("\x1b[90m注册身份: role=PROVIDER addr={SELLER_ADDR}\x1b[0m");
            vec![register_identity_action("PROVIDER", SELLER_ADDR)]
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

/// 选择 jobId：已有的从列表点选，也可输入新的。
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

fn run_menu(tx: mpsc::UnboundedSender<String>, buyer_addr: String) {
    use dialoguer::{theme::ColorfulTheme, Input, Select};
    let theme = ColorfulTheme::default();
    let mut known_tasks: Vec<String> = vec![];

    let items = [
        "/connect   向买家发起询价",
        "/accept    接单",
        "/deliver   提交交付",
        "/dispute   发起仲裁",
        "/convid    查看会话 ID",
        "/register  注册 ERC-8004 身份",
        "/lookup    查询角色列表",
        "send       发送自由文本",
        "quit       退出",
    ];

    loop {
        println!();
        let sel = match Select::with_theme(&theme)
            .with_prompt("\x1b[32m卖家\x1b[0m 选择操作")
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
                format!("/connect {tid}")
            }
            1 => {
                let tid = pick_job_id(&theme, &mut known_tasks);
                format!("/accept {tid}")
            }
            2 => {
                let tid = pick_job_id(&theme, &mut known_tasks);
                format!("/deliver {tid}")
            }
            3 => {
                let tid = pick_job_id(&theme, &mut known_tasks);
                let reason: String = Input::with_theme(&theme)
                    .with_prompt("原因（可空）")
                    .allow_empty(true)
                    .interact_text()
                    .unwrap_or_default();
                if reason.is_empty() { format!("/dispute {tid}") } else { format!("/dispute {tid} {reason}") }
            }
            4 => {
                let tid = pick_job_id(&theme, &mut known_tasks);
                format!("/convid {tid}")
            }
            5 => "/register".to_string(),
            6 => {
                let role = Select::with_theme(&theme)
                    .with_prompt("查询角色")
                    .items(&["buyer (REQUESTER)", "seller (PROVIDER)", "arbitrator (EVALUATOR)"])
                    .default(0)
                    .interact()
                    .unwrap_or(0);
                ["/lookup buyer", "/lookup seller", "/lookup arbitrator"][role].to_string()
            }
            7 => {
                let conv_id: String = Input::with_theme(&theme).with_prompt("convId").interact_text().unwrap_or_default();
                let content: String = Input::with_theme(&theme).with_prompt("内容").interact_text().unwrap_or_default();
                format!("send {conv_id} {content}")
            }
            _ => { println!("再见！"); std::process::exit(0); }
        };

        let actions = parse_command(&cmd, &buyer_addr);
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
    let buyer_addr = parse_buyer_addr_arg().unwrap_or_else(|| MOCK_BUYER_ADDR.to_string());

    println!("\x1b[32m[卖家]\x1b[0m 钱包地址: {SELLER_ADDR}");
    println!("\x1b[90m买家地址: {buyer_addr}\x1b[0m");
    println!("连接到 {SERVER_URL} ...");

    let (ws, _) = connect_async(SERVER_URL)
        .await
        .expect("无法连接 ws-mock server，请先启动 server");
    let (mut sink, mut stream) = ws.split();

    sink.send(Message::Text(register_ws_action(SELLER_ADDR).to_string().into()))
        .await
        .unwrap();
    println!("\x1b[32m✓ 已连接并注册\x1b[0m");

    let (tx, mut rx) = mpsc::unbounded_channel::<String>();

    tokio::spawn(async move {
        while let Some(msg) = rx.recv().await {
            sink.send(Message::Text(msg.into())).await.ok();
        }
    });

    let buyer_addr_recv = buyer_addr.clone();
    tokio::spawn(async move {
        while let Some(Ok(Message::Text(text))) = stream.next().await {
            if let Ok(v) = serde_json::from_str::<serde_json::Value>(&text) {
                handle_inbound(&v, "\x1b[32m卖家\x1b[0m > ", &buyer_addr_recv);
            }
        }
    });

    tokio::task::spawn_blocking(move || run_menu(tx, buyer_addr))
        .await
        .ok();
}
