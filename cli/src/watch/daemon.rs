use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use anyhow::Result;
use base64::Engine;
use futures_util::{SinkExt, StreamExt};
use hmac::{Hmac, Mac};
use serde::Deserialize;
use sha2::Sha256;
use tokio::time::{interval, sleep, timeout};
use tokio_tungstenite::{connect_async, tungstenite::Message};

use super::store::{append_events, read_config, write_pid, write_status};
use super::types::{channel_pattern, ChannelPattern, WatchConfig, WatchEnv};

const WS_URL_PROD: &str = "wss://wsdex.okx.com/ws/v6/dex";
const WS_URL_PRE: &str = "wss://wsdexpre.okx.com:8443/ws/v6/dex";

const HEARTBEAT_SECS: u64 = 25;
const PONG_TIMEOUT_SECS: u64 = 10;
const RECONNECT_DELAY_SECS: u64 = 3;
const MAX_RECONNECT_ATTEMPTS: u32 = 20;

pub struct Credentials {
    api_key: String,
    secret_key: String,
    passphrase: String,
}

impl Credentials {
    pub fn from_watch_env(env: &WatchEnv) -> Result<Self> {
        match env {
            WatchEnv::Pre => Ok(Self {
                api_key: std::env::var("OKX_PRE_API_KEY")
                    .map_err(|_| anyhow::anyhow!("OKX_PRE_API_KEY is not set"))?,
                secret_key: std::env::var("OKX_PRE_SECRET_KEY")
                    .map_err(|_| anyhow::anyhow!("OKX_PRE_SECRET_KEY is not set"))?,
                passphrase: std::env::var("OKX_PRE_PASSPHRASE")
                    .map_err(|_| anyhow::anyhow!("OKX_PRE_PASSPHRASE is not set"))?,
            }),
            WatchEnv::Prod => Ok(Self {
                api_key: std::env::var("OKX_PROD_API_KEY")
                    .or_else(|_| std::env::var("OKX_API_KEY"))
                    .map_err(|_| anyhow::anyhow!("OKX_PROD_API_KEY or OKX_API_KEY is not set"))?,
                secret_key: std::env::var("OKX_PROD_SECRET_KEY")
                    .or_else(|_| std::env::var("OKX_SECRET_KEY"))
                    .map_err(|_| {
                        anyhow::anyhow!("OKX_PROD_SECRET_KEY or OKX_SECRET_KEY is not set")
                    })?,
                passphrase: std::env::var("OKX_PROD_PASSPHRASE")
                    .or_else(|_| std::env::var("OKX_PASSPHRASE"))
                    .map_err(|_| {
                        anyhow::anyhow!("OKX_PROD_PASSPHRASE or OKX_PASSPHRASE is not set")
                    })?,
            }),
        }
    }

    fn sign(&self, timestamp: &str) -> String {
        let prehash = format!("{}GET/users/self/verify", timestamp);
        let mut mac = Hmac::<Sha256>::new_from_slice(self.secret_key.as_bytes())
            .expect("HMAC accepts any key length");
        mac.update(prehash.as_bytes());
        base64::engine::general_purpose::STANDARD.encode(mac.finalize().into_bytes())
    }

    fn login_msg(&self) -> String {
        let ts = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0)
            .to_string();
        let sign = self.sign(&ts);
        serde_json::json!({
            "op": "login",
            "args": [{
                "apiKey": self.api_key,
                "passphrase": self.passphrase,
                "timestamp": ts,
                "sign": sign,
            }]
        })
        .to_string()
    }
}

/// Entry point for the daemon process. Runs until stopped.
pub async fn run_daemon(id: &str, dir: &Path) -> Result<()> {
    write_pid(dir, std::process::id())?;
    write_status(dir, "running", None)?;

    let config = read_config(id).unwrap_or_else(|_| WatchConfig {
        channels: super::types::DEFAULT_CHANNELS
            .iter()
            .map(|c| c.to_string())
            .collect(),
        wallet_addresses: vec![],
        token_pairs: vec![],
        chain_indexes: vec![],
        env: WatchEnv::Prod,
        created_at: 0,
        idle_timeout_ms: 30 * 60 * 1000,
    });

    // Heartbeat writer: every 10s overwrite status so poll can detect crashes.
    // Also checks idle timeout — signals main loop to exit gracefully.
    let heartbeat_active = Arc::new(AtomicBool::new(true));
    let heartbeat_active_clone = Arc::clone(&heartbeat_active);
    let idle_expired = Arc::new(AtomicBool::new(false));
    let idle_expired_clone = Arc::clone(&idle_expired);
    let dir_owned = dir.to_path_buf();
    let idle_timeout_ms = config.idle_timeout_ms;
    let created_at = config.created_at;
    tokio::spawn(async move {
        let mut ticker = interval(Duration::from_secs(10));
        loop {
            ticker.tick().await;
            if heartbeat_active_clone.load(Ordering::Relaxed) {
                let _ = write_status(&dir_owned, "running", None);
            }
            // Idle timeout check — signal main loop instead of process::exit
            if idle_timeout_ms > 0 {
                let last_activity = super::store::last_poll_time(&dir_owned).unwrap_or(created_at);
                if super::store::now_ms().saturating_sub(last_activity) > idle_timeout_ms {
                    let _ = write_status(&dir_owned, "stopped", Some("idle_timeout"));
                    idle_expired_clone.store(true, Ordering::Relaxed);
                    break;
                }
            }
        }
    });
    let ws_url = std::env::var("ONCHAINOS_WS_URL").unwrap_or_else(|_| match config.env {
        WatchEnv::Pre => WS_URL_PRE.to_string(),
        WatchEnv::Prod => WS_URL_PROD.to_string(),
    });
    let creds = match Credentials::from_watch_env(&config.env) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("[watch daemon] credentials error: {}", e);
            write_status(dir, "stopped", Some(&format!("credentials:{}", e)))?;
            return Err(e);
        }
    };

    let mut attempts = 0u32;
    loop {
        heartbeat_active.store(true, Ordering::Relaxed);
        match connect_and_stream(dir, &ws_url, &creds, &config, &idle_expired).await {
            Ok(reason) => {
                heartbeat_active.store(false, Ordering::Relaxed);
                attempts = 0; // reset after successful session
                eprintln!("[watch daemon] disconnected: {}", reason);
                if reason == "stopped" || reason == "idle_timeout" {
                    write_status(dir, "stopped", Some(&reason))?;
                    return Ok(());
                }
                write_status(dir, "disconnected", Some(&reason))?;
            }
            Err(e) => {
                heartbeat_active.store(false, Ordering::Relaxed);
                eprintln!("[watch daemon] error: {}", e);
                write_status(dir, "disconnected", Some(&format!("error:{}", e)))?;
            }
        }

        // Check idle timeout signal before reconnecting
        if idle_expired.load(Ordering::Relaxed) {
            eprintln!("[watch daemon] idle timeout reached, shutting down");
            return Ok(());
        }

        attempts += 1;
        if attempts >= MAX_RECONNECT_ATTEMPTS {
            write_status(dir, "stopped", Some("max_reconnect_reached"))?;
            return Ok(());
        }

        write_status(dir, "reconnecting", None)?;
        sleep(Duration::from_secs(RECONNECT_DELAY_SECS)).await;
    }
}

/// Connect to WS, login, subscribe, stream events. Returns a reason string on clean exit.
async fn connect_and_stream(
    dir: &Path,
    ws_url: &str,
    creds: &Credentials,
    config: &WatchConfig,
    idle_expired: &AtomicBool,
) -> Result<String> {
    let (mut ws, _): (WsStream, _) = connect_async(ws_url).await?;

    // Login
    ws.send(Message::Text(creds.login_msg().into())).await?;
    wait_for_login_ack(&mut ws).await?;

    // Build subscribe args per channel pattern
    let args: Vec<serde_json::Value> = config
        .channels
        .iter()
        .flat_map(|ch| -> Vec<serde_json::Value> {
            match channel_pattern(ch) {
                ChannelPattern::Global => {
                    vec![serde_json::json!({ "channel": ch })]
                }
                ChannelPattern::PerWallet => config
                    .wallet_addresses
                    .iter()
                    .map(|addr| serde_json::json!({ "channel": ch, "walletAddress": addr }))
                    .collect(),
                ChannelPattern::PerToken => config
                    .token_pairs
                    .iter()
                    .map(|tp| {
                        serde_json::json!({
                            "channel": ch,
                            "chainIndex": tp.chain_index,
                            "tokenContractAddress": tp.token_contract_address
                        })
                    })
                    .collect(),
                ChannelPattern::PerChain => config
                    .chain_indexes
                    .iter()
                    .map(|ci| serde_json::json!({ "channel": ch, "chainIndex": ci }))
                    .collect(),
            }
        })
        .collect();

    let ack_count = args.len();
    let sub_msg = serde_json::json!({ "op": "subscribe", "args": args });
    ws.send(Message::Text(sub_msg.to_string().into())).await?;

    // Wait for one ACK per subscription arg
    wait_for_subscribe_acks(&mut ws, ack_count).await?;
    write_status(dir, "running", None)?;

    let mut heartbeat = interval(Duration::from_secs(HEARTBEAT_SECS));
    heartbeat.tick().await; // consume immediate first tick

    loop {
        tokio::select! {
            _ = heartbeat.tick() => {
                if idle_expired.load(Ordering::Relaxed) {
                    return Ok("idle_timeout".to_string());
                }
                ws.send(Message::Text("ping".to_string().into())).await?;
                match timeout(Duration::from_secs(PONG_TIMEOUT_SECS), recv_pong(&mut ws, dir)).await {
                    Ok(Ok(_)) => {}
                    _ => return Err(anyhow::anyhow!("ping_timeout")),
                }
            }

            msg = ws.next() => {
                match msg {
                    None => return Err(anyhow::anyhow!("connection_closed")),
                    Some(Err(e)) => return Err(e.into()),
                    Some(Ok(Message::Text(text))) => {
                        if text.trim() == "pong" {
                            continue;
                        }
                        if let Some(reason) = check_notice(&text) {
                            return Ok(reason);
                        }
                        if let Ok(push) = serde_json::from_str::<WsPush>(&text) {
                            append_events(dir, &push.arg.channel, &push.data)?;
                        }
                    }
                    Some(Ok(Message::Close(_))) => {
                        return Err(anyhow::anyhow!("server_closed"));
                    }
                    Some(Ok(_)) => {}
                }
            }
        }
    }
}

type WsStream =
    tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>;

async fn wait_for_login_ack(ws: &mut WsStream) -> Result<()> {
    timeout(Duration::from_secs(10), async {
        loop {
            match ws.next().await {
                Some(Ok(Message::Text(text))) => {
                    if let Ok(v) = serde_json::from_str::<serde_json::Value>(&text) {
                        if v.get("event").and_then(|e| e.as_str()) == Some("login") {
                            let code = v.get("code").and_then(|c| c.as_str()).unwrap_or("-1");
                            if code == "0" {
                                return Ok(());
                            }
                            let msg = v.get("msg").and_then(|m| m.as_str()).unwrap_or("unknown");
                            return Err(anyhow::anyhow!("login error: {}", msg));
                        }
                    }
                }
                Some(Err(e)) => return Err(e.into()),
                None => return Err(anyhow::anyhow!("connection closed during login")),
                _ => {}
            }
        }
    })
    .await
    .unwrap_or(Err(anyhow::anyhow!("login ack timeout")))
}

/// Wait for `count` subscribe ACKs (one per subscription arg).
async fn wait_for_subscribe_acks(ws: &mut WsStream, count: usize) -> Result<()> {
    if count == 0 {
        return Ok(());
    }
    timeout(Duration::from_secs(10), async {
        let mut acked = 0usize;
        loop {
            match ws.next().await {
                Some(Ok(Message::Text(text))) => {
                    if let Ok(v) = serde_json::from_str::<serde_json::Value>(&text) {
                        match v.get("event").and_then(|e| e.as_str()) {
                            Some("subscribe") => {
                                acked += 1;
                                if acked >= count {
                                    return Ok(());
                                }
                            }
                            Some("error") => {
                                let msg =
                                    v.get("msg").and_then(|m| m.as_str()).unwrap_or("unknown");
                                return Err(anyhow::anyhow!("subscribe error: {}", msg));
                            }
                            _ => {}
                        }
                    }
                }
                Some(Err(e)) => return Err(e.into()),
                None => return Err(anyhow::anyhow!("connection closed during subscribe")),
                _ => {}
            }
        }
    })
    .await
    .unwrap_or(Err(anyhow::anyhow!("subscribe ack timeout")))
}

/// Wait for the server's "pong" reply. Any push data frames received while
/// waiting are processed normally so they are not lost.
async fn recv_pong(ws: &mut WsStream, dir: &Path) -> Result<()> {
    loop {
        match ws.next().await {
            Some(Ok(Message::Text(text))) if text.trim() == "pong" => return Ok(()),
            Some(Ok(Message::Text(text))) => {
                // Push data arrived while waiting for pong — process it to avoid data loss.
                if let Ok(push) = serde_json::from_str::<WsPush>(&text) {
                    append_events(dir, &push.arg.channel, &push.data)?;
                }
            }
            Some(Err(e)) => return Err(e.into()),
            None => return Err(anyhow::anyhow!("connection closed")),
            _ => {}
        }
    }
}

fn check_notice(text: &str) -> Option<String> {
    let v: serde_json::Value = serde_json::from_str(text).ok()?;
    if v.get("event")?.as_str()? == "notice" {
        Some("service_upgrade".to_string())
    } else {
        None
    }
}

#[derive(Deserialize)]
struct WsPush {
    arg: WsPushArg,
    data: Vec<serde_json::Value>,
}

#[derive(Deserialize)]
struct WsPushArg {
    channel: String,
}
