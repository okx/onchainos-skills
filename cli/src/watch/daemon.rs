use std::path::Path;
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
use super::types::{TradeEvent, WatchEnv};

const WS_URL_PROD: &str = "wss://wsdex.okx.com:8443/ws/v5/dex";
const WS_URL_PRE: &str = "wss://wsdexpre.okx.com:8443/ws/v6/dex";

const HEARTBEAT_SECS: u64 = 25;
const RECONNECT_DELAY_SECS: u64 = 3;
const MAX_RECONNECT_ATTEMPTS: u32 = 20;

struct Credentials {
    api_key: String,
    secret_key: String,
    passphrase: String,
}

impl Credentials {
    fn from_watch_env(env: &WatchEnv) -> Result<Self> {
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
                    .map_err(|_| anyhow::anyhow!("OKX_PROD_SECRET_KEY or OKX_SECRET_KEY is not set"))?,
                passphrase: std::env::var("OKX_PROD_PASSPHRASE")
                    .or_else(|_| std::env::var("OKX_PASSPHRASE"))
                    .map_err(|_| anyhow::anyhow!("OKX_PROD_PASSPHRASE or OKX_PASSPHRASE is not set"))?,
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
pub async fn run_daemon(_id: &str, dir: &Path) -> Result<()> {
    write_pid(dir, std::process::id())?;
    write_status(dir, "running", None)?;

    // Heartbeat writer: every 10s overwrite status so poll can detect crashes
    let dir_owned = dir.to_path_buf();
    tokio::spawn(async move {
        let mut ticker = interval(Duration::from_secs(10));
        loop {
            ticker.tick().await;
            let _ = write_status(&dir_owned, "running", None);
        }
    });

    let config = read_config(_id).unwrap_or_else(|_| super::types::WatchConfig {
        channels: super::types::ALL_CHANNELS.iter().map(|c| c.name.to_string()).collect(),
        env: WatchEnv::Prod,
        created_at: 0,
    });
    let ws_url = std::env::var("ONCHAINOS_WS_URL").unwrap_or_else(|_| match config.env {
        WatchEnv::Pre => WS_URL_PRE.to_string(),
        WatchEnv::Prod => WS_URL_PROD.to_string(),
    });
    let creds = Credentials::from_watch_env(&config.env)?;

    let mut attempts = 0u32;
    loop {
        match connect_and_stream(dir, &ws_url, &creds, &config.channels).await {
            Ok(reason) => {
                eprintln!("[watch daemon] disconnected: {}", reason);
                if reason == "stopped" {
                    write_status(dir, "stopped", None)?;
                    return Ok(());
                }
                write_status(dir, "disconnected", Some(&reason))?;
            }
            Err(e) => {
                eprintln!("[watch daemon] error: {}", e);
                write_status(dir, "disconnected", Some(&format!("error:{}", e)))?;
            }
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
async fn connect_and_stream(dir: &Path, ws_url: &str, creds: &Credentials, channels: &[String]) -> Result<String> {
    let (mut ws, _): (WsStream, _) = connect_async(ws_url).await?;

    // Login
    ws.send(Message::Text(creds.login_msg().into())).await?;
    wait_for_login_ack(&mut ws).await?;

    // Subscribe to all configured channels
    let args: Vec<_> = channels.iter()
        .map(|ch| serde_json::json!({ "channel": ch }))
        .collect();
    let sub_msg = serde_json::json!({ "op": "subscribe", "args": args });
    ws.send(Message::Text(sub_msg.to_string().into())).await?;

    // Wait for subscribe ACK
    wait_for_subscribe_ack(&mut ws).await?;
    write_status(dir, "running", None)?;

    let mut heartbeat = interval(Duration::from_secs(HEARTBEAT_SECS));
    heartbeat.tick().await; // consume immediate first tick

    loop {
        tokio::select! {
            _ = heartbeat.tick() => {
                ws.send(Message::Text("ping".to_string().into())).await?;
                match timeout(Duration::from_secs(HEARTBEAT_SECS), recv_pong(&mut ws)).await {
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

type WsStream = tokio_tungstenite::WebSocketStream<
    tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>,
>;

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

async fn wait_for_subscribe_ack(ws: &mut WsStream) -> Result<()> {
    timeout(Duration::from_secs(10), async {
        loop {
            match ws.next().await {
                Some(Ok(Message::Text(text))) => {
                    if let Ok(v) = serde_json::from_str::<serde_json::Value>(&text) {
                        match v.get("event").and_then(|e| e.as_str()) {
                            Some("subscribe") => return Ok(()),
                            Some("error") => {
                                let msg = v.get("msg").and_then(|m| m.as_str()).unwrap_or("unknown");
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

async fn recv_pong(ws: &mut WsStream) -> Result<()> {
    loop {
        match ws.next().await {
            Some(Ok(Message::Text(text))) if text.trim() == "pong" => return Ok(()),
            Some(Ok(Message::Text(text))) => {
                // Data frames arriving while waiting for pong — ignore, will come in next cycle
                let _ = text;
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
    data: Vec<TradeEvent>,
}

#[derive(Deserialize)]
struct WsPushArg {
    channel: String,
}
