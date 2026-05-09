//! `wallet-agentic-identity` WebSocket subscription helper. Replaces the
//! old `tx-agent-status` HTTP poll for `agent create` / `agent update`:
//! after broadcasting, the caller waits up to 30 s for a push whose
//! `txHash` matches the broadcast hash.
//!
//! Lifecycle: `open_identity_subscription` connects to
//! `<identity-base-url-as-wss>/ws/v5/private`, sends the JWT login op,
//! awaits `event=login,code=0`, then subscribes to
//! `wallet-agentic-identity` and awaits the subscribe ACK. The caller
//! broadcasts, then drives `wait_for_match` which streams frames until a
//! match is found or the deadline fires. Any failure here is a soft
//! failure — the surrounding command logs and falls through with the
//! `agent` field absent.

use std::time::Duration;

use anyhow::{anyhow, bail, Context as _, Result};
use futures_util::{SinkExt, StreamExt};
use serde_json::{json, Value};
use tokio::time::timeout;
use tokio_tungstenite::{connect_async, tungstenite::Message, MaybeTlsStream, WebSocketStream};

use super::utils::redact_token_for_debug;

const SUBSCRIBE_CHANNEL: &str = "wallet-agentic-identity";
/// Hard upper bound on the entire connect → login → subscribe handshake.
/// Bounds pre-broadcast latency so a black-holed WS host cannot stall
/// `agent create` / `agent update` (the surrounding command falls
/// through to broadcast-only on timeout).
const OPEN_TIMEOUT: Duration = Duration::from_secs(10);

type WsStream = WebSocketStream<MaybeTlsStream<tokio::net::TcpStream>>;

pub(super) struct IdentitySubscription {
    ws: WsStream,
}

/// Connect → login(JWT) → subscribe(`wallet-agentic-identity`).
///
/// `base_url` is the WS base URL produced by `identity_ws_base_url(ctx)`:
/// either an explicit `OKX_AGENTIC_WS_BASE_URL` (swim-lane / cross-host
/// envs where the push service runs on a separate domain from HTTP) or
/// the raw identity HTTP base URL as a fallback (production /
/// single-host envs). `derive_ws_url` below maps the scheme
/// (`http→ws`, `https→wss`) and forces the `/ws/v5/private` path.
///
/// Note that `WalletApiClient` may internally rewrite outgoing HTTP to
/// a DoH proxy URL; that rewrite is invisible from outside the wallet
/// module, so this WS connect cannot follow it. In DoH-proxy
/// environments the connect will hit the raw host and the caller's
/// soft-failure path kicks in (agent field absent, broadcast and
/// agent-list unaffected). Documented limitation, not a bug to chase
/// here.
///
/// The whole handshake is bounded by `OPEN_TIMEOUT` — a single budget
/// over connect, login, and subscribe — so a black-holed host cannot
/// stall the caller before broadcast. Bubbles up any failure so the
/// caller can decide whether to fall through.
pub(super) async fn open_identity_subscription(
    jwt: &str,
    base_url: &str,
) -> Result<IdentitySubscription> {
    let ws_url = derive_ws_url(base_url)?;
    eprintln!("[agent-identity] ws connect: url={ws_url}");
    match timeout(OPEN_TIMEOUT, open_inner(jwt, &ws_url)).await {
        Ok(Ok(sub)) => Ok(sub),
        Ok(Err(e)) => Err(e),
        Err(_) => bail!(
            "ws subscription open timed out after {}s (url={ws_url})",
            OPEN_TIMEOUT.as_secs()
        ),
    }
}

async fn open_inner(jwt: &str, ws_url: &str) -> Result<IdentitySubscription> {
    let (mut ws, _resp) = connect_async(ws_url)
        .await
        .with_context(|| format!("failed to connect to {ws_url}"))?;
    eprintln!("[agent-identity] ws connected");

    // ── login ─────────────────────────────────────────────────────────────
    let login = json!({ "op": "login", "args": [{ "token": jwt }] }).to_string();
    eprintln!(
        "[agent-identity] ws login request: op=login token_len={} token_prefix={}",
        jwt.len(),
        redact_token_for_debug(jwt),
    );
    ws.send(Message::Text(login.into()))
        .await
        .context("ws login send failed")?;
    let login_resp = wait_for_event(&mut ws, "login").await?;
    eprintln!("[agent-identity] ws login response: {login_resp}");

    // ── subscribe ─────────────────────────────────────────────────────────
    let sub = json!({
        "op": "subscribe",
        "args": [{ "channel": SUBSCRIBE_CHANNEL }],
    })
    .to_string();
    eprintln!("[agent-identity] ws subscribe request: {sub}");
    ws.send(Message::Text(sub.into()))
        .await
        .context("ws subscribe send failed")?;
    let sub_resp = wait_for_event(&mut ws, "subscribe").await?;
    eprintln!("[agent-identity] ws subscribe response: {sub_resp}");

    eprintln!("[agent-identity] ws subscribed: channel={SUBSCRIBE_CHANNEL}");
    Ok(IdentitySubscription { ws })
}

impl IdentitySubscription {
    /// Read frames until one carries a push whose `txHash` matches
    /// `tx_hash` (lowercase, ignoring optional `0x` prefix on either
    /// side). Returns `Ok(None)` on timeout. Non-matching frames and
    /// unrecognized shapes are logged and skipped.
    pub(super) async fn wait_for_match(
        mut self,
        tx_hash: &str,
        wait: Duration,
    ) -> Result<Option<Value>> {
        let target = normalize_hash(tx_hash);
        let outcome = timeout(wait, async {
            loop {
                let msg = match self.ws.next().await {
                    Some(Ok(m)) => m,
                    Some(Err(e)) => return Err(anyhow!("ws read error: {e}")),
                    None => return Err(anyhow!("ws closed before match")),
                };
                let text = match msg {
                    Message::Text(t) => t.to_string(),
                    Message::Binary(b) => match String::from_utf8(b.to_vec()) {
                        Ok(s) => s,
                        Err(_) => continue,
                    },
                    Message::Close(_) => return Err(anyhow!("ws closed by server")),
                    Message::Ping(_) | Message::Pong(_) | Message::Frame(_) => continue,
                };
                if let Some(payload) = extract_payload(&text) {
                    let push_hash = payload
                        .get("txHash")
                        .and_then(Value::as_str)
                        .map(normalize_hash)
                        .unwrap_or_default();
                    if !push_hash.is_empty() && push_hash == target {
                        return Ok(Some(payload));
                    }
                    eprintln!(
                        "[agent-identity] ws push skipped: txHash={push_hash} target={target}"
                    );
                } else {
                    eprintln!(
                        "[agent-identity] ws frame ignored: {}",
                        truncate(&text, 200)
                    );
                }
            }
        })
        .await;

        // Best-effort close — ignore errors; caller does not care once we have a verdict.
        let _ = SinkExt::close(&mut self.ws).await;

        match outcome {
            Ok(Ok(payload)) => Ok(payload),
            Ok(Err(e)) => Err(e),
            Err(_) => {
                eprintln!(
                    "[agent-identity] ws wait timed out after {}s",
                    wait.as_secs()
                );
                Ok(None)
            }
        }
    }
}

// ─── Frame parsing ────────────────────────────────────────────────────────

/// Accept both `{ "arg": {..}, "data": [obj] }` and bare `{ obj }`.
/// Returns the inner push object when shape is recognized; ignores
/// control frames (`event=login|subscribe|error`) that may race after the
/// initial ACK drain.
fn extract_payload(text: &str) -> Option<Value> {
    let v: Value = serde_json::from_str(text).ok()?;
    if v.get("event").is_some() {
        return None;
    }
    if let Some(arr) = v.get("data").and_then(Value::as_array) {
        return arr.first().cloned();
    }
    if v.get("txHash").is_some() && v.get("agentId").is_some() {
        return Some(v);
    }
    None
}

// ─── ACK reader ───────────────────────────────────────────────────────────

/// Drain frames until the expected `event` ACK arrives, returning the
/// raw response text so callers can log it. Non-matching text frames
/// (heartbeats, race-arriving pushes, malformed json) are surfaced via
/// stderr with an `[agent-identity] ws ack skipped` prefix and dropped.
/// Bounded by the outer `OPEN_TIMEOUT` wrapper in
/// `open_identity_subscription` — no per-step timeout needed here.
async fn wait_for_event(ws: &mut WsStream, expected: &str) -> Result<String> {
    loop {
        let msg = match ws.next().await {
            Some(Ok(m)) => m,
            Some(Err(e)) => bail!("ws ack read error: {e}"),
            None => bail!("ws closed during {expected} ack"),
        };
        let Message::Text(text) = msg else {
            continue;
        };
        let text_str = text.to_string();
        let v: Value = match serde_json::from_str(&text_str) {
            Ok(v) => v,
            Err(_) => {
                eprintln!(
                    "[agent-identity] ws ack skipped (non-json) while waiting for {expected}: {}",
                    truncate(&text_str, 200)
                );
                continue;
            }
        };
        match v.get("event").and_then(Value::as_str) {
            Some(e) if e == expected => {
                let code = v.get("code").and_then(Value::as_str).unwrap_or("0");
                if code != "0" {
                    let msg_field = v.get("msg").and_then(Value::as_str).unwrap_or("");
                    bail!(
                        "ws {expected} rejected: code={code} msg={msg_field} raw={text_str}"
                    );
                }
                return Ok(text_str);
            }
            Some("error") => {
                let msg_field = v.get("msg").and_then(Value::as_str).unwrap_or("unknown");
                bail!("ws error during {expected}: {msg_field} raw={text_str}");
            }
            other => {
                eprintln!(
                    "[agent-identity] ws ack skipped (event={:?}) while waiting for {expected}: {}",
                    other,
                    truncate(&text_str, 200)
                );
                continue;
            }
        }
    }
}

// ─── Helpers ──────────────────────────────────────────────────────────────

/// Swap the scheme on the identity HTTP base URL to its WS equivalent
/// (`https`↔`wss`, `http`↔`ws`) and force the path to `/ws/v5/private`.
/// Preserving TLS-vs-plain matches what HTTP actually uses, so local /
/// non-TLS dev (`--base-url http://127.0.0.1:...`) goes to `ws://...`
/// instead of failing a TLS handshake. Any path/query in `base_url` is
/// discarded — the WS endpoint owns the path.
fn derive_ws_url(base_url: &str) -> Result<String> {
    let trimmed = base_url.trim();
    let (ws_scheme, rest) = if let Some(r) = trimmed.strip_prefix("https://") {
        ("wss", r)
    } else if let Some(r) = trimmed.strip_prefix("http://") {
        ("ws", r)
    } else if let Some(r) = trimmed.strip_prefix("wss://") {
        ("wss", r)
    } else if let Some(r) = trimmed.strip_prefix("ws://") {
        ("ws", r)
    } else {
        bail!("base_url is missing a scheme: {trimmed}");
    };
    let host = rest.split('/').next().unwrap_or(rest);
    if host.is_empty() {
        bail!("base_url has an empty host: {trimmed}");
    }
    Ok(format!("{ws_scheme}://{host}/ws/v5/private"))
}

fn normalize_hash(s: &str) -> String {
    let trimmed = s.trim();
    let no_prefix = trimmed
        .strip_prefix("0x")
        .or_else(|| trimmed.strip_prefix("0X"))
        .unwrap_or(trimmed);
    no_prefix.to_ascii_lowercase()
}

fn truncate(s: &str, max: usize) -> String {
    if s.len() <= max {
        return s.to_string();
    }
    let mut end = max;
    while end > 0 && !s.is_char_boundary(end) {
        end -= 1;
    }
    format!("{}...", &s[..end])
}
