//! mock-buyer: 买家交互式测试工具（菜单驱动）
//!
//! 用法: mock-buyer

use futures_util::{SinkExt, StreamExt};
use tokio::sync::mpsc;
use tokio_tungstenite::{connect_async, tungstenite::Message};
use ws_mock::{
    conv_id_arb, conv_id_bs, handle_inbound, lookup_role_action, register_identity_action,
    register_ws_action, send_action, MOCK_BUYER_ADDR, SERVER_URL,
};

const MY_ADDR: &str = MOCK_BUYER_ADDR;

fn parse_command(input: &str) -> Vec<serde_json::Value> {
    let parts: Vec<&str> = input.trim().splitn(3, ' ').collect();
    match parts[0] {
        "/confirm" => {
            let job_id = parts.get(1).copied().unwrap_or("unknown");
            let conv_id = conv_id_bs(job_id, MY_ADDR);
            vec![send_action(&conv_id, "TASK_CONFIRM", "验收通过，任务完成。", Some(job_id))]
        }
        "/reject" => {
            let job_id = parts.get(1).copied().unwrap_or("unknown");
            let reason = parts.get(2).copied().unwrap_or("不符合要求");
            let conv_id = conv_id_bs(job_id, MY_ADDR);
            vec![send_action(&conv_id, "TASK_REJECT", reason, Some(job_id))]
        }
        "/convid" => {
            let job_id = parts.get(1).copied().unwrap_or("jobId");
            println!("\x1b[90m买卖会话: {}\x1b[0m", conv_id_bs(job_id, MY_ADDR));
            println!("\x1b[90m仲裁会话: {}\x1b[0m", conv_id_arb(job_id, MY_ADDR));
            vec![]
        }
        "/register" => {
            println!("\x1b[90m注册身份: role=REQUESTER addr={MY_ADDR}\x1b[0m");
            vec![register_identity_action("REQUESTER", MY_ADDR)]
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

fn run_menu(tx: mpsc::UnboundedSender<String>) {
    use dialoguer::{theme::ColorfulTheme, Input, Select};
    let theme = ColorfulTheme::default();
    let mut known_tasks: Vec<String> = vec![];

    let items = [
        "/confirm   确认验收",
        "/reject    拒绝验收",
        "/convid    查看会话 ID",
        "/register  注册 ERC-8004 身份",
        "/lookup    查询角色列表",
        "send       发送自由文本",
        "quit       退出",
    ];

    loop {
        println!();
        let sel = match Select::with_theme(&theme)
            .with_prompt("\x1b[34m买家\x1b[0m 选择操作")
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
                format!("/confirm {tid}")
            }
            1 => {
                let tid = pick_job_id(&theme, &mut known_tasks);
                let reason: String = Input::with_theme(&theme)
                    .with_prompt("原因（可空）")
                    .allow_empty(true)
                    .interact_text()
                    .unwrap_or_default();
                if reason.is_empty() { format!("/reject {tid}") } else { format!("/reject {tid} {reason}") }
            }
            2 => {
                let tid = pick_job_id(&theme, &mut known_tasks);
                format!("/convid {tid}")
            }
            3 => "/register".to_string(),
            4 => {
                let role = Select::with_theme(&theme)
                    .with_prompt("查询角色")
                    .items(&["buyer (REQUESTER)", "seller (PROVIDER)", "arbitrator (EVALUATOR)"])
                    .default(0)
                    .interact()
                    .unwrap_or(0);
                ["/lookup buyer", "/lookup seller", "/lookup arbitrator"][role].to_string()
            }
            5 => {
                let conv_id: String = Input::with_theme(&theme).with_prompt("convId").interact_text().unwrap_or_default();
                let content: String = Input::with_theme(&theme).with_prompt("内容").interact_text().unwrap_or_default();
                format!("send {conv_id} {content}")
            }
            _ => { println!("再见！"); std::process::exit(0); }
        };

        let actions = parse_command(&cmd);
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
    println!("\x1b[34m[买家]\x1b[0m 钱包地址: {MY_ADDR}");
    println!("连接到 {SERVER_URL} ...");

    let (ws, _) = connect_async(SERVER_URL)
        .await
        .expect("无法连接 ws-mock server，请先启动 server");
    let (mut sink, mut stream) = ws.split();

    sink.send(Message::Text(register_ws_action(MY_ADDR).to_string().into()))
        .await
        .unwrap();
    println!("\x1b[32m✓ 已连接并注册\x1b[0m");

    let (tx, mut rx) = mpsc::unbounded_channel::<String>();

    tokio::spawn(async move {
        while let Some(msg) = rx.recv().await {
            sink.send(Message::Text(msg.into())).await.ok();
        }
    });

    tokio::spawn(async move {
        while let Some(Ok(Message::Text(text))) = stream.next().await {
            if let Ok(v) = serde_json::from_str::<serde_json::Value>(&text) {
                handle_inbound(&v, "\x1b[34m买家\x1b[0m > ", MY_ADDR);
            }
        }
    });

    tokio::task::spawn_blocking(move || run_menu(tx))
        .await
        .ok();
}
