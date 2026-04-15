/// mock-agent: 任务系统交互式测试工具
///
/// 用法:
///   mock-agent             # 启动后交互式选择角色
///   mock-agent --role buyer
///
/// 命令:
///   send <convId> <内容>      发消息到指定会话
///   /connect <task_id>       快捷：卖家发起会话（创建+发起询问）
///   /accept  <task_id>       快捷：卖家接单
///   /deliver <task_id>       快捷：卖家交付
///   /dispute <task_id> [原因] 快捷：卖家发起仲裁（拉群）
///   /confirm <task_id>       快捷：买家确认验收
///   /reject  <task_id> [原因] 快捷：买家拒绝验收
///   /resolve <task_id> <winner> 快捷：仲裁裁决
///   /convid  <task_id>       显示该任务的会话 ID
///   help                     查看命令帮助
///   quit                     退出

use futures_util::{SinkExt, StreamExt};
use serde_json::json;
use std::io::{self, Write};
use tokio::sync::mpsc;
use tokio_tungstenite::{connect_async, tungstenite::Message};

const SERVER_URL: &str = "ws://127.0.0.1:9000";

#[derive(Debug, Clone, PartialEq)]
enum Role {
    Buyer,
    Seller,
    Arbitrator,
    System,
}

impl Role {
    fn from_str(s: &str) -> Option<Self> {
        match s.to_lowercase().as_str() {
            "buyer" | "买家" | "1" => Some(Role::Buyer),
            "seller" | "卖家" | "2" => Some(Role::Seller),
            "arbitrator" | "仲裁者" | "arb" | "3" => Some(Role::Arbitrator),
            "system" | "系统" | "4" => Some(Role::System),
            _ => None,
        }
    }

    fn wallet_addr(&self) -> &str {
        match self {
            Role::Buyer => "0xBuyer0000000000000000000000000000000001",
            Role::Seller => "0xSeller000000000000000000000000000000001",
            Role::Arbitrator => "0xArbitrator0000000000000000000000000001",
            Role::System => "0xSystem000000000000000000000000000000001",
        }
    }

    fn label(&self) -> &str {
        match self {
            Role::Buyer => "买家",
            Role::Seller => "卖家",
            Role::Arbitrator => "仲裁者",
            Role::System => "系统",
        }
    }

    fn color_label(&self) -> String {
        match self {
            Role::Buyer => "\x1b[34m[买家]\x1b[0m".to_string(),
            Role::Seller => "\x1b[32m[卖家]\x1b[0m".to_string(),
            Role::Arbitrator => "\x1b[33m[仲裁者]\x1b[0m".to_string(),
            Role::System => "\x1b[35m[系统]\x1b[0m".to_string(),
        }
    }
}

fn parse_role_arg() -> Option<Role> {
    let args: Vec<String> = std::env::args().collect();
    for i in 0..args.len() {
        if args[i] == "--role" || args[i] == "-r" {
            return args.get(i + 1).and_then(|s| Role::from_str(s));
        }
    }
    None
}

fn parse_buyer_addr_arg() -> Option<String> {
    let args: Vec<String> = std::env::args().collect();
    for i in 0..args.len() {
        if args[i] == "--buyer-addr" || args[i] == "-b" {
            return args.get(i + 1).cloned();
        }
    }
    None
}

fn prompt_role() -> Role {
    println!("\x1b[1m=== 任务系统 Mock Agent ===\x1b[0m");
    println!("选择角色:");
    println!("  1. \x1b[34m买家\x1b[0m     (Buyer)");
    println!("  2. \x1b[32m卖家\x1b[0m     (Seller)");
    println!("  3. \x1b[33m仲裁者\x1b[0m   (Arbitrator)");
    println!("  4. \x1b[35m系统通知\x1b[0m (System)");
    print!("\n请输入角色 [1-4]: ");
    io::stdout().flush().unwrap();
    loop {
        let mut input = String::new();
        io::stdin().read_line(&mut input).unwrap();
        if let Some(role) = Role::from_str(input.trim()) {
            return role;
        }
        print!("无效输入，请输入 1-4: ");
        io::stdout().flush().unwrap();
    }
}

// 确定性生成 buyer-seller 会话 ID
fn conv_id_bs(task_id: &str, buyer_addr: &str) -> String {
    let seller = Role::Seller.wallet_addr();
    let (a, b) = if buyer_addr <= seller { (buyer_addr, seller) } else { (seller, buyer_addr) };
    format!("conv-{task_id}-{a}-{b}")
}

// 确定性生成仲裁三方会话 ID
fn conv_id_arb(task_id: &str, buyer_addr: &str) -> String {
    let seller = Role::Seller.wallet_addr();
    let arb = Role::Arbitrator.wallet_addr();
    let mut addrs = [buyer_addr, seller, arb];
    addrs.sort();
    format!("conv-arb-{task_id}-{}-{}-{}", addrs[0], addrs[1], addrs[2])
}

fn print_help(role: &Role) {
    println!("\n\x1b[1m命令帮助\x1b[0m (当前角色: {})", role.color_label());
    println!("─────────────────────────────────────────");
    println!("  \x1b[36msend <convId> <内容>\x1b[0m   发消息到指定会话");
    println!("  \x1b[36m/convid <task_id>\x1b[0m      显示该任务的会话 ID");
    println!();
    println!("  \x1b[1m快捷命令:\x1b[0m");
    match role {
        Role::Buyer => {
            println!("  \x1b[32m/confirm <task_id>\x1b[0m            确认验收");
            println!("  \x1b[32m/reject  <task_id> [原因]\x1b[0m      拒绝验收");
        }
        Role::Seller => {
            println!("  \x1b[32m/connect <task_id>\x1b[0m             发起会话（创建+询问）");
            println!("  \x1b[32m/accept  <task_id>\x1b[0m             接单");
            println!("  \x1b[32m/deliver <task_id>\x1b[0m             提交交付");
            println!("  \x1b[32m/dispute <task_id> [原因]\x1b[0m      发起仲裁");
        }
        Role::Arbitrator => {
            println!("  \x1b[32m/resolve <task_id> buyer\x1b[0m       裁定买家胜");
            println!("  \x1b[32m/resolve <task_id> seller\x1b[0m      裁定卖家胜");
        }
        Role::System => {
            println!("  \x1b[32m/chain <tx_hash>\x1b[0m               模拟链上确认（打印，不发消息）");
            println!("  \x1b[32m/lock  <task_id> <amount>\x1b[0m      模拟资金锁定（广播到买卖双方会话）");
        }
    }
    println!();
    println!("  \x1b[1m身份系统:\x1b[0m");
    println!("  \x1b[36m/register [role]\x1b[0m              注册当前 Agent 身份（模拟 ERC-8004）");
    println!("  \x1b[36m/lookup <role>\x1b[0m                查询角色对应的 Agent 列表");
    println!();
    println!("  \x1b[36mhelp\x1b[0m   显示帮助");
    println!("  \x1b[36mquit\x1b[0m   退出");
    println!("─────────────────────────────────────────\n");
}

fn addr_label(addr: &str) -> &str {
    match addr {
        a if a == Role::Buyer.wallet_addr() => "买家",
        a if a == Role::Seller.wallet_addr() => "卖家",
        a if a == Role::Arbitrator.wallet_addr() => "仲裁者",
        a if a == Role::System.wallet_addr() => "系统",
        _ => addr,
    }
}

fn join_conv_action(conv_id: &str, participants: Vec<&str>) -> serde_json::Value {
    json!({
        "action": "JoinConversation",
        "conversation_id": conv_id,
        "participants": participants
    })
}

fn send_action(conv_id: &str, msg_type: &str, content: &str, task_id: Option<&str>) -> serde_json::Value {
    let mut payload = json!({ "type": msg_type, "content": content });
    if let Some(tid) = task_id {
        payload["task_id"] = json!(tid);
    }
    json!({
        "action": "Send",
        "conversation_id": conv_id,
        "payload": payload
    })
}

fn parse_command(input: &str, role: &Role, buyer_addr: &str) -> Vec<serde_json::Value> {
    let parts: Vec<&str> = input.trim().splitn(3, ' ').collect();
    if parts.is_empty() {
        return vec![];
    }

    match parts[0] {
        "send" => {
            let conv_id = parts.get(1).unwrap_or(&"");
            let content = parts.get(2).unwrap_or(&"").trim();
            if conv_id.is_empty() || content.is_empty() {
                println!("用法: send <convId> <内容>");
                return vec![];
            }
            vec![send_action(conv_id, "TEXT", content, None)]
        }

        "/convid" => {
            let task_id = parts.get(1).unwrap_or(&"task_id");
            let bs = conv_id_bs(task_id, buyer_addr);
            let arb = conv_id_arb(task_id, buyer_addr);
            println!("\x1b[90m买卖会话: {bs}\x1b[0m");
            println!("\x1b[90m仲裁会话: {arb}\x1b[0m");
            vec![]
        }

        // 卖家：发起会话
        "/connect" => {
            let task_id = parts.get(1).unwrap_or(&"unknown");
            let conv_id = conv_id_bs(task_id, buyer_addr);
            let seller = Role::Seller.wallet_addr();
            println!("\x1b[90m创建会话: {conv_id}\x1b[0m");
            vec![
                join_conv_action(&conv_id, vec![buyer_addr, seller]),
                send_action(&conv_id, "TASK_INQUIRE", "你好，我对这个任务感兴趣，请介绍一下详情。", Some(task_id)),
            ]
        }

        // 卖家：接单
        "/accept" => {
            let task_id = parts.get(1).unwrap_or(&"unknown");
            let conv_id = conv_id_bs(task_id, buyer_addr);
            vec![send_action(&conv_id, "TASK_ACCEPT", "我接单了，任务即将开始执行。", Some(task_id))]
        }

        // 卖家：交付
        "/deliver" => {
            let task_id = parts.get(1).unwrap_or(&"unknown");
            let conv_id = conv_id_bs(task_id, buyer_addr);
            vec![send_action(&conv_id, "TASK_DELIVER", "任务已完成，请买家验收。", Some(task_id))]
        }

        // 卖家：发起仲裁
        "/dispute" => {
            let task_id = parts.get(1).unwrap_or(&"unknown");
            let reason = parts.get(2).unwrap_or(&"买家拒绝验收");
            let arb_conv = conv_id_arb(task_id, buyer_addr);
            let seller = Role::Seller.wallet_addr();
            let arb = Role::Arbitrator.wallet_addr();
            println!("\x1b[90m创建仲裁会话: {arb_conv}\x1b[0m");
            vec![
                join_conv_action(&arb_conv, vec![buyer_addr, seller, arb]),
                send_action(&arb_conv, "TASK_DISPUTE", reason, Some(task_id)),
            ]
        }

        // 买家：确认验收
        "/confirm" => {
            let task_id = parts.get(1).unwrap_or(&"unknown");
            let conv_id = conv_id_bs(task_id, buyer_addr);
            vec![send_action(&conv_id, "TASK_CONFIRM", "验收通过，任务完成。", Some(task_id))]
        }

        // 买家：拒绝验收
        "/reject" => {
            let task_id = parts.get(1).unwrap_or(&"unknown");
            let reason = parts.get(2).unwrap_or(&"不符合要求");
            let conv_id = conv_id_bs(task_id, buyer_addr);
            vec![send_action(&conv_id, "TASK_REJECT", reason, Some(task_id))]
        }

        // 仲裁者：裁决
        "/resolve" => {
            let task_id = parts.get(1).unwrap_or(&"unknown");
            let winner = parts.get(2).unwrap_or(&"unknown");
            let arb_conv = conv_id_arb(task_id, buyer_addr);
            let content = format!("仲裁结果: {winner} 胜，task_id: {task_id}");
            let mut action = send_action(&arb_conv, "TASK_RESOLVE", &content, Some(task_id));
            action["payload"]["winner"] = json!(winner);
            vec![action]
        }

        // 注册身份（模拟 ERC-8004 身份系统）
        "/register" => {
            let role_arg = parts.get(1).unwrap_or(&"");
            let normalized_role = match role_arg.to_uppercase().as_str() {
                "REQUESTER" | "BUYER" | "买家" => "REQUESTER",
                "PROVIDER" | "SELLER" | "卖家" => "PROVIDER",
                "EVALUATOR" | "ARBITRATOR" | "ARB" | "仲裁者" => "EVALUATOR",
                "" => match role {
                    Role::Buyer => "REQUESTER",
                    Role::Seller => "PROVIDER",
                    Role::Arbitrator => "EVALUATOR",
                    Role::System => "REQUESTER",
                },
                other => {
                    println!("未知角色: {other}，支持: REQUESTER/PROVIDER/EVALUATOR（或 buyer/seller/arbitrator）");
                    return vec![];
                }
            };
            let addr = role.wallet_addr();
            println!("\x1b[90m注册身份: role={normalized_role} addr={addr}\x1b[0m");
            vec![serde_json::json!({
                "action": "RegisterIdentity",
                "role": normalized_role,
                "addr": addr
            })]
        }

        // 查询角色对应的 Agent 列表
        "/lookup" => {
            let role_arg = parts.get(1).unwrap_or(&"");
            let normalized_role = match role_arg.to_uppercase().as_str() {
                "REQUESTER" | "BUYER" | "买家" => "REQUESTER",
                "PROVIDER" | "SELLER" | "卖家" => "PROVIDER",
                "EVALUATOR" | "ARBITRATOR" | "ARB" | "仲裁者" => "EVALUATOR",
                other => {
                    println!("未知角色: {other}");
                    return vec![];
                }
            };
            vec![serde_json::json!({ "action": "LookupRole", "role": normalized_role })]
        }

        // 系统：链上确认（仅打印，无 WS 消息）
        "/chain" => {
            let tx_hash = parts.get(1).unwrap_or(&"0xtxhash");
            println!("\x1b[35m[链上确认] tx_hash: {tx_hash}\x1b[0m");
            vec![]
        }

        // 系统：资金锁定（发到买卖双方会话）
        "/lock" => {
            let task_id = parts.get(1).unwrap_or(&"unknown");
            let amount = parts.get(2).unwrap_or(&"0");
            let conv_id = conv_id_bs(task_id, buyer_addr);
            let content = format!("资金已锁定: {amount} USDC");
            let mut action = send_action(&conv_id, "SYSTEM_NOTIFY", &content, Some(task_id));
            action["payload"]["event"] = json!("payment_locked");
            action["payload"]["amount"] = json!(amount);
            vec![action]
        }

        "help" | "h" | "?" => {
            print_help(role);
            vec![]
        }
        "quit" | "exit" | "q" => {
            println!("再见！");
            std::process::exit(0);
        }
        _ => {
            println!("未知命令: {}，输入 help 查看帮助", parts[0]);
            vec![]
        }
    }
}

#[tokio::main]
async fn main() {
    let role = parse_role_arg().unwrap_or_else(prompt_role);
    let buyer_addr = parse_buyer_addr_arg()
        .unwrap_or_else(|| Role::Buyer.wallet_addr().to_string());

    println!(
        "\n{} 已选择角色 {} | 钱包地址: {}",
        role.color_label(),
        role.label(),
        role.wallet_addr()
    );
    println!("\x1b[90m买家地址: {buyer_addr}\x1b[0m");
    println!("连接到 {} ...", SERVER_URL);

    let (ws, _) = connect_async(SERVER_URL)
        .await
        .expect("无法连接到 ws-mock server，请先启动 server");

    let (mut sink, mut stream) = ws.split();

    let register_msg = json!({ "action": "Register", "addr": role.wallet_addr() });
    sink.send(Message::Text(register_msg.to_string().into())).await.unwrap();

    println!("\x1b[32m✓ 已连接并注册\x1b[0m");
    print_help(&role);

    let (tx, mut rx) = mpsc::unbounded_channel::<String>();

    tokio::spawn(async move {
        while let Some(msg) = rx.recv().await {
            sink.send(Message::Text(msg.into())).await.ok();
        }
    });

    let role_for_recv = role.clone();
    tokio::spawn(async move {
        while let Some(Ok(Message::Text(text))) = stream.next().await {
            if let Ok(v) = serde_json::from_str::<serde_json::Value>(&text) {
                let msg_type = v["type"].as_str().unwrap_or("");
                if msg_type == "registered" {
                    continue;
                }
                if msg_type == "conversation_joined" {
                    println!("\x1b[32m✓ 已加入会话: {}\x1b[0m", v["conversation_id"].as_str().unwrap_or("?"));
                    print!("{} > ", role_for_recv.label());
                    io::stdout().flush().ok();
                    continue;
                }
                if msg_type == "identity_registered" {
                    println!("\x1b[32m✓ 身份已注册: role={} addr={}\x1b[0m",
                        v["role"].as_str().unwrap_or("?"),
                        v["addr"].as_str().unwrap_or("?"));
                    print!("{} > ", role_for_recv.label());
                    io::stdout().flush().ok();
                    continue;
                }
                if msg_type == "addr_lookup" {
                    let addr = v["addr"].as_str().unwrap_or("?");
                    match v["identity"].as_object() {
                        Some(id) => println!("\x1b[36m[地址查询] {} → role={}\x1b[0m",
                            addr, id.get("role").and_then(|r| r.as_str()).unwrap_or("?")),
                        None => println!("\x1b[33m[地址查询] {} 未注册\x1b[0m", addr),
                    }
                    print!("{} > ", role_for_recv.label());
                    io::stdout().flush().ok();
                    continue;
                }
                if msg_type == "identity_lookup" {
                    let role_q = v["role"].as_str().unwrap_or("?");
                    let agents = v["agents"].as_array().map(|a| a.len()).unwrap_or(0);
                    println!("\x1b[36m[身份查询] role={role_q} 共 {agents} 个 Agent:\x1b[0m");
                    if let Some(arr) = v["agents"].as_array() {
                        for a in arr {
                            println!("  addr={}", a["addr"].as_str().unwrap_or("?"));
                        }
                    }
                    print!("{} > ", role_for_recv.label());
                    io::stdout().flush().ok();
                    continue;
                }
                if msg_type == "identity_list" {
                    let all = v["identities"].as_array().map(|a| a.len()).unwrap_or(0);
                    println!("\x1b[36m[所有身份] 共 {all} 个:\x1b[0m");
                    if let Some(arr) = v["identities"].as_array() {
                        for e in arr {
                            println!("  role={} addr={}", e["role"].as_str().unwrap_or("?"), e["addr"].as_str().unwrap_or("?"));
                        }
                    }
                    print!("{} > ", role_for_recv.label());
                    io::stdout().flush().ok();
                    continue;
                }
                if msg_type == "error" {
                    println!("\x1b[31m[错误] {}\x1b[0m", v["msg"].as_str().unwrap_or("unknown"));
                    print!("{} > ", role_for_recv.label());
                    io::stdout().flush().ok();
                    continue;
                }
                let from = v["from"].as_str().unwrap_or("unknown");
                let conv_id = v["conversation_id"].as_str().unwrap_or("?");
                let msg_type = v["payload"]["type"].as_str().unwrap_or("MSG");
                let payload_str = v["payload"].to_string();
                let content = v["payload"]["content"].as_str().unwrap_or(&payload_str);
                println!(
                    "\n\x1b[90m[收到]\x1b[0m \x1b[1m{}\x1b[0m \x1b[90m({}) conv:{}\x1b[0m\n  {}",
                    addr_label(from), msg_type, &conv_id[..conv_id.len().min(30)], content
                );
                if let Some(task_id) = v["payload"]["task_id"].as_str() {
                    println!("  \x1b[90mtask_id: {}\x1b[0m", task_id);
                }
                print!("{} > ", role_for_recv.label());
                io::stdout().flush().ok();
            }
        }
    });

    let prompt = format!("{} > ", role.label());
    loop {
        print!("{}", prompt);
        io::stdout().flush().unwrap();

        let mut input = String::new();
        if io::stdin().read_line(&mut input).is_err() {
            break;
        }
        let input = input.trim();
        if input.is_empty() {
            continue;
        }

        let actions = parse_command(input, &role, &buyer_addr);
        for action in &actions {
            let conv_id = action["conversation_id"].as_str().unwrap_or("");
            let action_type = action["action"].as_str().unwrap_or("");
            if action_type == "Send" {
                let msg_type = action["payload"]["type"].as_str().unwrap_or("MSG");
                println!("\x1b[90m→ 发送 [{}] 到会话 {}\x1b[0m", msg_type, &conv_id[..conv_id.len().min(30)]);
            }
            tx.send(action.to_string()).ok();
        }
    }
}
