//! HTTP wrappers for the 7 strategy endpoints.
//!
//! Thin layer over `ApiClient` (the same shared client `swap` / `signal` /
//! etc. use). Auth, base-url, retry, payment-cache, DoH are all handled by
//! `ApiClient`; this module only owns the path + body shape.
//!
//! ## Envelope handling
//!
//! Strategy uses the **raw** variants of the shared client
//! (`post_with_headers_raw` / `get_with_headers_raw`) so we receive the
//! full `{code, msg, data}` body and can read `code` as a structured
//! number for typed error classification (see `status::check_response`).
//!
//! This is necessary because some 60018 (`UPGRADE_REQUIRED`) responses
//! must trigger a transparent SD-A retry on the calling handler; the
//! non-raw `*_with_headers` methods would have collapsed `code = 60018`
//! into an `anyhow!("API error (code=60018): ...")` string, forcing a
//! brittle substring match downstream.
//!
//! Caller contract: every endpoint here returns `Result<T>` where `T` is
//! already the unwrapped DTO — `data` extraction happens here, after
//! `check_response` succeeds.
//!
//! 5 DEX endpoints under `/api/v1/dex/strategy/agentic/limitOrder/`:
//! - `create_order` / `cancel` / `get_open_order` / `open_order_detail` / `reactivate`
//!
//! 2 priapi endpoints under `/priapi/v5/wallet/agentic/strategy/`:
//! - `get_attest_doc_hex` / `register_tee_info` (used by SD-A)
//!
//! Source contracts: `.claude/strategyTrading/api/{dex,wallet}-*.md`.

use anyhow::{anyhow, Context as _, Result};
use serde_json::Value;

use crate::client::ApiClient;

use super::status::check_response;
use super::types::{
    CancelReq, CancelResp, CreateOrderReq, ListOrdersReq, ListOrdersResp, OrderListResp,
    ReactivateReq, ReactivateResp, RegisterTeeInfoReq,
};

/// Strategy endpoints require `X-Web3-Auth-Type: 1` so the BE can distinguish
/// Agentic JWT requests from generic Bearer ones. The JWT itself rides on the
/// shared `Authorization: Bearer` header that `ApiClient` already injects;
/// no separate `X-Web3-Auth` is needed.
const STRATEGY_AUTH_HEADERS: &[(&str, &str)] = &[("X-Web3-Auth-Type", "1")];

/// Per-call request/response logging — gated behind the
/// `ONCHAINOS_STRATEGY_DEBUG_LOG=1` environment variable. Off by default.
///
/// When on, every strategy API call writes a JSON file under
/// `.claude/strategyTrading/log/` containing the URL, the request body,
/// and the response (or error). HTTP headers are **never** logged so the
/// shared `Authorization: Bearer <jwt>` header injected by `ApiClient`
/// cannot leak through this channel. Sensitive payload fields
/// (`signature`, `sessionCert`, `signMsg`, `attestDocHex`, `sessionSig`,
/// any case variation of `Authorization` / `jwt`) are recursively
/// redacted with the literal string `<redacted>` before serialisation.
///
/// Intended for BE integration debugging only. Disable in production by
/// leaving the env var unset.
mod debug_log {
    use serde_json::{json, Value};
    use std::path::PathBuf;
    use std::sync::OnceLock;

    fn is_enabled() -> bool {
        static ENABLED: OnceLock<bool> = OnceLock::new();
        *ENABLED.get_or_init(|| {
            std::env::var("ONCHAINOS_STRATEGY_DEBUG_LOG")
                .map(|v| v == "1" || v.eq_ignore_ascii_case("true"))
                .unwrap_or(false)
        })
    }

    /// Recursively replace the value of any sensitive key with `<redacted>`.
    /// Match is case-insensitive on the key name.
    fn redact(value: &mut Value) {
        const SENSITIVE_KEYS: &[&str] = &[
            "signature",
            "sessionCert",
            "session_cert",
            "signMsg",
            "sign_msg",
            "attestDocHex",
            "attest_doc_hex",
            "sessionSig",
            "session_sig",
            "Authorization",
            "jwt",
            "accessToken",
            "access_token",
        ];
        match value {
            Value::Object(map) => {
                for (k, v) in map.iter_mut() {
                    if SENSITIVE_KEYS.iter().any(|s| k.eq_ignore_ascii_case(s)) {
                        *v = Value::String("<redacted>".to_string());
                    } else {
                        redact(v);
                    }
                }
            }
            Value::Array(arr) => {
                for v in arr.iter_mut() {
                    redact(v);
                }
            }
            _ => {}
        }
    }

    fn slugify(url: &str) -> String {
        url.trim_start_matches('/')
            .chars()
            .map(|c| match c {
                '/' => '_',
                '?' | '&' | '=' | ':' | ' ' => '-',
                c => c,
            })
            .collect()
    }

    pub fn dump(url: &str, request: &Value, response: Option<&Value>, error: Option<&str>) {
        if !is_enabled() {
            return;
        }
        let dir = PathBuf::from(".claude/strategyTrading/log");
        if std::fs::create_dir_all(&dir).is_err() {
            return;
        }
        let now = chrono::Utc::now();
        let ts_file = now.format("%Y-%m-%d-%H%M%S-%3f").to_string();
        let slug = slugify(url);
        let path = dir.join(format!("{ts_file}-{slug}.json"));

        let mut req_redacted = request.clone();
        redact(&mut req_redacted);
        let mut resp_redacted = response.cloned();
        if let Some(r) = resp_redacted.as_mut() {
            redact(r);
        }

        let body = json!({
            "url": url,
            "request": req_redacted,
            "response": resp_redacted,
            "error": error,
            "ts": now.to_rfc3339_opts(chrono::SecondsFormat::Millis, true),
        });
        let _ = std::fs::write(path, serde_json::to_string_pretty(&body).unwrap_or_default());
    }
}

/// POST wrapper that captures request/response under the debug-log
/// env-var gate. When the gate is off, this is a zero-overhead pass-through
/// to `client.post_with_headers_raw`.
async fn post_logged(client: &mut ApiClient, url: &str, body: &Value) -> Result<Value> {
    match client
        .post_with_headers_raw(url, body, Some(STRATEGY_AUTH_HEADERS))
        .await
    {
        Ok(v) => {
            debug_log::dump(url, body, Some(&v), None);
            Ok(v)
        }
        Err(e) => {
            let msg = format!("{e:#}");
            debug_log::dump(url, body, None, Some(&msg));
            Err(e)
        }
    }
}

/// GET wrapper — same gate as `post_logged`. The querystring is dumped as
/// the `request` payload so reviewers can see what parameters went out.
async fn get_logged(
    client: &mut ApiClient,
    url: &str,
    query: &[(&str, &str)],
) -> Result<Value> {
    let req_repr: Value = Value::Object(
        query
            .iter()
            .map(|(k, v)| ((*k).to_string(), Value::String((*v).to_string())))
            .collect(),
    );
    match client
        .get_with_headers_raw(url, query, Some(STRATEGY_AUTH_HEADERS))
        .await
    {
        Ok(v) => {
            debug_log::dump(url, &req_repr, Some(&v), None);
            Ok(v)
        }
        Err(e) => {
            let msg = format!("{e:#}");
            debug_log::dump(url, &req_repr, None, Some(&msg));
            Err(e)
        }
    }
}

/// Extract `body.data` after typed envelope check. Returns `Value::Null` for
/// missing `data` (some endpoints legitimately respond with no payload —
/// e.g. cancel may return only `data: null`).
fn data_field(body: Value) -> Value {
    match body {
        Value::Object(mut map) => map.remove("data").unwrap_or(Value::Null),
        // Defensive: BE shouldn't return a bare array for strategy
        // endpoints, but keep the body intact rather than discard it.
        other => other,
    }
}

// ── DEX limit-order endpoints ─────────────────────────────────────────

/// POST `/api/v1/dex/strategy/agentic/limitOrder/createOrder`
///
/// Returns the freshly-created `OrderListResp`. Caller drives SD-A retry
/// inline (60018 → trader_mode::activate → retry once); see handlers.rs.
pub async fn create_order(
    client: &mut ApiClient,
    req: &CreateOrderReq,
) -> Result<OrderListResp> {
    let body = serde_json::to_value(req).context("createOrder: serialise request")?;
    let resp = post_logged(
        client,
        "/api/v1/dex/strategy/agentic/limitOrder/createOrder",
        &body,
    )
    .await?;
    check_response(&resp)?;
    serde_json::from_value(data_field(resp))
        .context("createOrder: data shape did not match OrderListResp")
}

/// POST `/api/v1/dex/strategy/agentic/limitOrder/cancel`
pub async fn cancel(client: &mut ApiClient, req: &CancelReq) -> Result<CancelResp> {
    let body = serde_json::to_value(req).context("cancel: serialise request")?;
    let resp = post_logged(
        client,
        "/api/v1/dex/strategy/agentic/limitOrder/cancel",
        &body,
    )
    .await?;
    check_response(&resp)?;
    let data = data_field(resp);
    if data.is_null() {
        return Ok(CancelResp::default());
    }
    serde_json::from_value(data).context("cancel: data shape did not match CancelResp")
}

/// POST `/api/v1/dex/strategy/agentic/limitOrder/getOpenOrder`
///
/// BE returns `{cursor, dataList, hasNext}` nested under `data`.
pub async fn get_open_order(
    client: &mut ApiClient,
    req: &ListOrdersReq,
) -> Result<ListOrdersResp> {
    let body = serde_json::to_value(req).context("getOpenOrder: serialise request")?;
    let resp = post_logged(
        client,
        "/api/v1/dex/strategy/agentic/limitOrder/getOpenOrder",
        &body,
    )
    .await?;
    check_response(&resp)?;
    let data = data_field(resp);
    if data.is_null() {
        return Ok(ListOrdersResp::default());
    }
    serde_json::from_value(data).context("getOpenOrder: response did not match ListOrdersResp")
}

/// GET `/api/v1/dex/strategy/agentic/limitOrder/openOrderDetail`
///
/// `order_id` is `Long` per the contract — pass as string to avoid
/// JS-style precision loss.
pub async fn open_order_detail(
    client: &mut ApiClient,
    account_id: &str,
    order_id: &str,
    strategy_mode: i32,
) -> Result<OrderListResp> {
    let mode_str = strategy_mode.to_string();
    let query: [(&str, &str); 3] = [
        ("accountId", account_id),
        ("orderId", order_id),
        ("strategyMode", mode_str.as_str()),
    ];
    let resp = get_logged(
        client,
        "/api/v1/dex/strategy/agentic/limitOrder/openOrderDetail",
        &query,
    )
    .await?;
    check_response(&resp)?;
    serde_json::from_value(data_field(resp))
        .context("openOrderDetail: data shape did not match OrderListResp")
}

/// POST `/api/v1/dex/strategy/agentic/limitOrder/reactivate`
///
/// Per api/dex-reactivate.md (Notes section), response shape is uncertain — BE may
/// return `{successIds, failIds}` or a `cancel`-style `{updateNum}`. We
/// accept both: explicit lists win; otherwise synthesise from `updateNum`
/// against the submitted ids.
pub async fn reactivate(
    client: &mut ApiClient,
    req: &ReactivateReq,
) -> Result<ReactivateResp> {
    let body = serde_json::to_value(req).context("reactivate: serialise request")?;
    let resp = post_logged(
        client,
        "/api/v1/dex/strategy/agentic/limitOrder/reactivate",
        &body,
    )
    .await?;
    check_response(&resp)?;
    let data = data_field(resp);

    if let Some(obj) = data.as_object() {
        if obj.contains_key("successIds") || obj.contains_key("failIds") {
            return serde_json::from_value(data)
                .context("reactivate: data shape did not match ReactivateResp");
        }

        // Cancel-style fallback. TEMPORARY shim until BE confirms contract.
        if let Some(update_num) = obj.get("updateNum").and_then(|v| v.as_i64()) {
            return Ok(ReactivateResp {
                success_ids: if update_num > 0 {
                    req.order_ids.clone()
                } else {
                    Vec::new()
                },
                fail_ids: if update_num > 0 {
                    Vec::new()
                } else {
                    req.order_ids.clone()
                },
            });
        }
    }

    Ok(ReactivateResp::default())
}

// ── Wallet priapi endpoints (used by SD-A) ────────────────────────────

/// GET `/priapi/v5/wallet/agentic/strategy/getAttestDocHex`
///
/// Documented shape: `{code: 0, data: ["hex..."]}`. Phase 1 returns the
/// first element; the per-element semantics (per coinType?) are TBC with BE.
pub async fn get_attest_doc_hex(client: &mut ApiClient) -> Result<String> {
    let resp = get_logged(
        client,
        "/priapi/v5/wallet/agentic/strategy/getAttestDocHex",
        &[],
    )
    .await?;
    check_response(&resp)?;
    let data = data_field(resp);
    let arr = data.as_array().ok_or_else(|| {
        anyhow!(
            "getAttestDocHex: response not an array — got: {}",
            serde_json::to_string(&data).unwrap_or_default()
        )
    })?;
    let first = arr
        .first()
        .and_then(|v| v.as_str())
        .ok_or_else(|| anyhow!("getAttestDocHex: array is empty"))?;
    Ok(first.to_string())
}

/// POST `/priapi/v5/wallet/agentic/strategy/registerTeeInfo`
///
/// Marker call for SD-A Phase 2 — failure is fatal (no auto-retry, per
/// tech-design §5.1). Success path returns `()`; the `data` field is not
/// consumed.
pub async fn register_tee_info(
    client: &mut ApiClient,
    req: &RegisterTeeInfoReq,
) -> Result<()> {
    let body: Value =
        serde_json::to_value(req).context("registerTeeInfo: serialise request")?;
    let resp = post_logged(
        client,
        "/priapi/v5/wallet/agentic/strategy/registerTeeInfo",
        &body,
    )
    .await?;
    check_response(&resp)?;
    Ok(())
}
