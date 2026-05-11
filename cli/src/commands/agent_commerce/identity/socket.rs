//! `wallet-agentic-identity` WebSocket subscription helper. Replaces the
//! old `tx-agent-status` HTTP poll for `agent create` / `agent update`:
//! after broadcasting, the caller waits up to 30 s for a push whose
//! `txHash` matches the broadcast hash.
//!
//! Lifecycle: `open_identity_subscription` connects to the full WS URL
//! the caller passes in (see `super::utils::identity_ws_url` — default
//! `WS_URL_PROD = wss://wsdex.okx.com/ws/v5/private`, or `OKX_AGENTIC_WS_URL`
//! env override). No scheme swap or path forcing happens here — the URL
//! is used verbatim. Then sends the wallet-address login op (JSON key
//! remains `"token"` per server contract; the value is the caller's
//! XLayer address, no longer a JWT), awaits `event=login,code=0`, then
//! subscribes to `wallet-agentic-identity` and awaits the subscribe ACK.
//! The caller broadcasts, then drives `wait_for_match` which streams
//! frames until a match is found or the deadline fires. Any failure here
//! is a soft failure — the surrounding command logs and falls through
//! with the `agent` field absent.

use std::time::Duration;

use anyhow::{anyhow, bail, Context as _, Result};
use futures_util::{SinkExt, StreamExt};
use serde_json::{json, Value};
use tokio::time::timeout;
use tokio_tungstenite::{connect_async, tungstenite::Message, MaybeTlsStream, WebSocketStream};

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

/// Connect → login(wallet address) → subscribe(`wallet-agentic-identity`).
///
/// `ws_url` is the full WS URL produced by `identity_ws_url()`:
/// either `WS_URL_PROD` (`wss://wsdex.okx.com/ws/v5/private`) or an
/// explicit `OKX_AGENTIC_WS_URL` override. The caller passes the URL
/// verbatim — no scheme swap or path forcing happens here.
///
/// `wallet_address` is the caller's XLayer address (the same address
/// used as `fromAddr` for the create/update broadcast). The push
/// service identifies the subscriber by this address. The wire JSON
/// key is still `"token"` for compatibility with the broader push-
/// platform login contract; only the value semantics changed (was a
/// JWT, now a wallet address). The address is public, so no redaction
/// is needed in debug logs.
///
/// The whole handshake is bounded by `OPEN_TIMEOUT` — a single budget
/// over connect, login, and subscribe — so a black-holed host cannot
/// stall the caller before broadcast. Bubbles up any failure so the
/// caller can decide whether to fall through.
pub(super) async fn open_identity_subscription(
    wallet_address: &str,
    ws_url: &str,
) -> Result<IdentitySubscription> {
    eprintln!("[agent-identity] ws connect: url={ws_url}");
    match timeout(OPEN_TIMEOUT, open_inner(wallet_address, ws_url)).await {
        Ok(Ok(sub)) => Ok(sub),
        Ok(Err(e)) => Err(e),
        Err(_) => bail!(
            "ws subscription open timed out after {}s (url={ws_url})",
            OPEN_TIMEOUT.as_secs()
        ),
    }
}

async fn open_inner(wallet_address: &str, ws_url: &str) -> Result<IdentitySubscription> {
    let (mut ws, _resp) = connect_async(ws_url)
        .await
        .with_context(|| format!("failed to connect to {ws_url}"))?;
    eprintln!("[agent-identity] ws connected");

    // ── login ─────────────────────────────────────────────────────────────
    // JSON key is "token" per push-platform contract; value is the wallet
    // address (public on-chain identifier, no redaction needed).
    let login = json!({ "op": "login", "args": [{ "token": wallet_address }] }).to_string();
    eprintln!(
        "[agent-identity] ws login request: op=login wallet_address={wallet_address}"
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
    ///
    /// Note on hash semantics: the backend's broadcast endpoint returns
    /// what it calls a "txHash" (in the AA / 4337 flow this is actually
    /// the user-operation hash, not the underlying L1 tx hash). The push
    /// payload also carries a "txHash" field. This function only relies
    /// on those two backend-named fields holding the **same value** —
    /// it does not reason about whether the value is a uop hash or an
    /// on-chain tx hash. As long as the backend is consistent across
    /// the broadcast response and the push payload, matching works.
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
                    eprintln!("[agent-identity] ws frame ignored: {text}");
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

/// Accept all three observed push shapes:
///   - `{ "arg": {..}, "data": { obj } }` (single-object data — what the
///     wallet-agentic-identity push platform actually sends)
///   - `{ "arg": {..}, "data": [ obj ] }` (legacy / theoretical array form)
///   - bare `{ obj }` with `txHash` + `agentId` at the top level
///
/// Returns the inner push object when shape is recognized; ignores
/// control frames (`event=login|subscribe|error`) that may race after the
/// initial ACK drain.
fn extract_payload(text: &str) -> Option<Value> {
    let v: Value = serde_json::from_str(text).ok()?;
    if v.get("event").is_some() {
        return None;
    }
    if let Some(data) = v.get("data") {
        if let Some(arr) = data.as_array() {
            return arr.first().cloned();
        }
        if data.is_object() {
            return Some(data.clone());
        }
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
                    "[agent-identity] ws ack skipped (non-json) while waiting for {expected}: {text_str}"
                );
                continue;
            }
        };
        match v.get("event").and_then(Value::as_str) {
            Some(e) if e == expected => {
                // `code` may arrive as string ("0") or number (0); subscribe
                // ACK in the spec has no `code` field at all. Anything else
                // (non-zero number, non-"0" string, bool / array / object) is
                // a rejection — must NOT default to success or we silently
                // accept e.g. {"event":"login","code":1,...} and then sit
                // through the 30 s push wait with no real cause surfaced.
                let code_ok = match v.get("code") {
                    None | Some(Value::Null) => true,
                    Some(Value::String(s)) => s == "0",
                    Some(Value::Number(n)) => n.as_i64() == Some(0),
                    _ => false,
                };
                if !code_ok {
                    let code_repr = v
                        .get("code")
                        .map(|c| c.to_string())
                        .unwrap_or_else(|| "<missing>".to_string());
                    let msg_field = v.get("msg").and_then(Value::as_str).unwrap_or("");
                    bail!(
                        "ws {expected} rejected: code={code_repr} msg={msg_field} raw={text_str}"
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
                    "[agent-identity] ws ack skipped (event={other:?}) while waiting for {expected}: {text_str}"
                );
                continue;
            }
        }
    }
}

// ─── Helpers ──────────────────────────────────────────────────────────────

fn normalize_hash(s: &str) -> String {
    let trimmed = s.trim();
    let no_prefix = trimmed
        .strip_prefix("0x")
        .or_else(|| trimmed.strip_prefix("0X"))
        .unwrap_or(trimmed);
    no_prefix.to_ascii_lowercase()
}
