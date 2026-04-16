use dashmap::DashMap;
use futures_util::{SinkExt, StreamExt};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tokio::net::TcpListener;
use tokio::sync::mpsc;
use tokio_tungstenite::accept_async;
use tokio_tungstenite::tungstenite::Message;

type Tx = mpsc::UnboundedSender<Message>;
type Registry = Arc<DashMap<String, Tx>>;
type Conversations = Arc<DashMap<String, Vec<String>>>;

/// 身份注册表：role → Vec<IdentityEntry>
/// 模拟 ERC-8004 身份系统后端，存储 REQUESTER/PROVIDER/EVALUATOR 注册信息
#[derive(Debug, Clone, Serialize, Deserialize)]
struct IdentityEntry {
    agent_id: String,
    comm_addr: String,
    role: String,
    metadata: serde_json::Value,
}

type IdentityRegistry = Arc<DashMap<String, Vec<IdentityEntry>>>;


#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "action")]
enum ClientMsg {
    Register { addr: String },
    JoinConversation { conversation_id: String, participants: Vec<String> },
    Send { conversation_id: String, payload: serde_json::Value },
    /// 注册身份：模拟 ERC-8004 身份注册
    RegisterIdentity {
        role: String,
        agent_id: String,
        comm_addr: String,
        #[serde(default)]
        metadata: Option<serde_json::Value>,
    },
    /// 按角色查询已注册的身份列表
    LookupRole { role: String },
    /// 按 agentId 查询身份
    LookupAddr { addr: String },
    /// 列出所有已注册身份
    ListIdentities {},
}

#[tokio::main]
async fn main() {
    let listener = TcpListener::bind("127.0.0.1:9000").await.unwrap();
    let registry: Registry = Arc::new(DashMap::new());
    let conversations: Conversations = Arc::new(DashMap::new());
    let identities: IdentityRegistry = Arc::new(DashMap::new());
    println!("[server] listening on ws://127.0.0.1:9000");
    while let Ok((stream, addr)) = listener.accept().await {
        println!("[server] new connection: {addr}");
        tokio::spawn(handle_connection(stream, registry.clone(), conversations.clone(), identities.clone()));
    }
}

async fn handle_connection(stream: tokio::net::TcpStream, registry: Registry, conversations: Conversations, identities: IdentityRegistry) {
    let ws = accept_async(stream).await.unwrap();
    let (mut sink, mut source) = ws.split();
    let (tx, mut rx) = mpsc::unbounded_channel::<Message>();
    // All addresses registered by this connection (may have multiple after wallet switches).
    let mut my_addrs: Vec<String> = vec![];

    let forward = tokio::spawn(async move {
        while let Some(msg) = rx.recv().await {
            if sink.send(msg).await.is_err() { break; }
        }
    });

    while let Some(Ok(msg)) = source.next().await {
        if let Message::Text(text) = msg {
            match serde_json::from_str::<ClientMsg>(&text) {
                Ok(ClientMsg::Register { addr }) => {
                    println!("[server] registered: {addr}");
                    registry.insert(addr.clone(), tx.clone());
                    if !my_addrs.contains(&addr) { my_addrs.push(addr.clone()); }
                    let ack = serde_json::json!({ "type": "registered", "addr": addr });
                    let _ = tx.send(Message::Text(ack.to_string().into()));
                }
                Ok(ClientMsg::JoinConversation { conversation_id, participants }) => {
                    println!("[server] join conv {conversation_id}: {participants:?}");
                    conversations.insert(conversation_id.clone(), participants);
                    let ack = serde_json::json!({ "type": "conversation_joined", "conversation_id": conversation_id });
                    let _ = tx.send(Message::Text(ack.to_string().into()));
                }
                Ok(ClientMsg::Send { conversation_id, payload }) => {
                    let from = my_addrs.last().cloned().unwrap_or("unknown".into());
                    let payload_str = payload.to_string();
                    let preview: String = payload_str.chars().take(120).collect();
                    println!("[server] {from} → conv:{conversation_id}: {preview}");
                    let participants = conversations.get(&conversation_id).map(|p| p.clone()).unwrap_or_default();
                    if participants.is_empty() {
                        let err = serde_json::json!({ "type": "error", "msg": format!("conversation {conversation_id} not found — call JoinConversation first") });
                        let _ = tx.send(Message::Text(err.to_string().into()));
                        continue;
                    }
                    let mut delivered = 0usize;
                    for participant in &participants {
                        if participant == &from { continue; }
                        if let Some(dest) = registry.get(participant) {
                            let envelope = serde_json::json!({ "from": from, "conversation_id": conversation_id, "payload": payload });
                            let _ = dest.send(Message::Text(envelope.to_string().into()));
                            delivered += 1;
                        }
                    }
                    if delivered == 0 {
                        let err = serde_json::json!({ "type": "error", "msg": format!("no participants online in {conversation_id}") });
                        let _ = tx.send(Message::Text(err.to_string().into()));
                    }
                }
                Ok(ClientMsg::RegisterIdentity { role, agent_id, comm_addr, metadata }) => {
                    let entry = IdentityEntry {
                        agent_id: agent_id.clone(),
                        comm_addr: comm_addr.clone(),
                        role: role.clone(),
                        metadata: metadata.unwrap_or(serde_json::Value::Null),
                    };
                    println!("[server] identity registered: role={role} agent_id={agent_id} comm_addr={comm_addr}");
                    identities.entry(role.clone()).or_default().push(entry);
                    let ack = serde_json::json!({
                        "type": "identity_registered",
                        "role": role,
                        "agent_id": agent_id,
                        "comm_addr": comm_addr
                    });
                    let _ = tx.send(Message::Text(ack.to_string().into()));
                }
                Ok(ClientMsg::LookupAddr { addr }) => {
                    // addr 字段含义：按 agent_id 查询
                    let found: Option<IdentityEntry> = identities.iter()
                        .flat_map(|e| e.value().clone())
                        .find(|e| e.agent_id == addr);
                    let ack = serde_json::json!({
                        "type": "addr_lookup",
                        "agent_id": addr,
                        "identity": found
                    });
                    let _ = tx.send(Message::Text(ack.to_string().into()));
                }
                Ok(ClientMsg::LookupRole { role }) => {
                    let agents = identities.get(&role).map(|v| v.clone()).unwrap_or_default();
                    let ack = serde_json::json!({
                        "type": "identity_lookup",
                        "role": role,
                        "agents": agents
                    });
                    let _ = tx.send(Message::Text(ack.to_string().into()));
                }
                Ok(ClientMsg::ListIdentities {}) => {
                    let all: Vec<IdentityEntry> = identities.iter()
                        .flat_map(|e| e.value().clone())
                        .collect();
                    let ack = serde_json::json!({ "type": "identity_list", "identities": all });
                    let _ = tx.send(Message::Text(ack.to_string().into()));
                }
                Err(e) => println!("[server] parse error: {e}"),
            }
        }
    }
    for addr in &my_addrs {
        registry.remove(addr);
    }
    if !my_addrs.is_empty() {
        println!("[server] disconnected, removed addrs: {:?}", my_addrs);
    }
    forward.abort();
}
