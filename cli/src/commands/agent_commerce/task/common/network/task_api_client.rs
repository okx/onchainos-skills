//! Task backend API client.
//!
//! Internally delegates all HTTP requests to `WalletApiClient`, reusing its DoH (DNS-over-HTTPS)
//! resolution and failover retry capabilities. On top of that, it injects the task-system-specific
//! `agenticId` identity header.
//!
//! All request methods take a **path** (e.g. `/priapi/v1/aieco/task/{jobId}/apply`),
//! no longer a full URL. The return value is `body["data"]`.

use std::time::Instant;

use anyhow::{anyhow, Context, Result};
use serde_json::Value;

use crate::audit;
use crate::commands::agentic_wallet::auth::ensure_tokens_refreshed;
use crate::wallet_api::WalletApiClient;
use crate::wallet_store;

/// Write the result of a single API request to audit.jsonl.
/// Success writes no error; failure writes a truncated error. The command name is fixed as `api/<method>` for easy jq filtering.
fn log_api(
    method: &str,
    path: &str,
    agent_id: &str,
    ok: bool,
    elapsed: std::time::Duration,
    error: Option<&str>,
    extra: Option<&str>,
) {
    let mut args = vec![format!("path={path}"), format!("agentId={agent_id}")];
    if let Some(e) = extra {
        args.push(e.to_string());
    }
    audit::log(
        "cli",
        &format!("api/{method}"),
        ok,
        elapsed,
        Some(args),
        error,
    );
}

/// Task API path prefix.
const TASK_PREFIX: &str = "/priapi/v1/aieco/task";

async fn get_access_token() -> Result<String, anyhow::Error> {
    ensure_tokens_refreshed().await
}

/// Read sessionCert from the local session.
fn get_session_cert() -> Option<String> {
    wallet_store::load_session()
        .ok()
        .flatten()
        .map(|s| s.session_cert)
        .filter(|c| !c.is_empty())
}

/// Inject sessionCert into the JSON body (if the body is an Object and does not already contain that field).
fn inject_session_cert(body: &Value) -> Value {
    let mut body = body.clone();
    if let Some(obj) = body.as_object_mut() {
        if !obj.contains_key("sessionCert") {
            if let Some(cert) = get_session_cert() {
                obj.insert("sessionCert".to_string(), Value::String(cert));
            }
        }
    }
    body
}

/// Task backend API client (DoH-enabled, delegates to WalletApiClient).
pub struct TaskApiClient {
    wallet: WalletApiClient,
    pub(crate) raw_http: reqwest::Client,
    pub(crate) base_url: String,
}

impl TaskApiClient {
    pub fn new() -> Self {
        Self::build(None)
    }

    fn build(base_url_override: Option<String>) -> Self {
        // base_url resolution — keep the same precedence as WalletApiClient::with_base_url,
        // so the URL shown in eprintln matches the URL the wallet actually requests against.
        // Precedence: OKX_BASE_URL env > compile-time OKX_BASE_URL > explicit override > DEFAULT_BASE_URL.
        let base_url = std::env::var("OKX_BASE_URL")
            .ok()
            .or_else(|| option_env!("OKX_BASE_URL").map(str::to_string))
            .or(base_url_override)
            .unwrap_or_else(|| crate::client::DEFAULT_BASE_URL.to_string());

        let wallet = WalletApiClient::with_base_url(Some(base_url.as_str()))
            .expect("failed to create WalletApiClient");

        Self {
            wallet,
            raw_http: reqwest::Client::new(),
            base_url,
        }
    }

    // ─── URL / path helpers ──────────────────────────────────────────────

    /// `/priapi/v1/aieco/task/{job_id}`
    pub fn task_path(&self, job_id: &str) -> String {
        format!("{TASK_PREFIX}/{job_id}")
    }

    /// `/priapi/v1/aieco/task/{job_id}/{action}`
    pub fn endpoint(&self, job_id: &str, action: &str) -> String {
        format!("{TASK_PREFIX}/{job_id}/{action}")
    }

    /// `/priapi/v1/aieco/task/broadcast`
    pub fn broadcast_path(&self) -> &'static str {
        const PATH: &str = "/priapi/v1/aieco/task/broadcast";
        PATH
    }

    // ─── Request methods (take a path, not a full URL) ───────────────────

    /// GET + JWT + agenticId header (no sessionCert injection) -> returns data.
    /// Used by query endpoints (e.g. providerConfirmStatus) that need JWT + agenticId but not sessionCert.
    pub async fn get_with_agent_id(&mut self, path: &str, agent_id: &str) -> Result<Value> {
        let url = format!("{}{}", self.base_url, path);
        let token = get_access_token().await?;
        let query: Vec<(&str, &str)> = vec![];
        let headers: Vec<(&str, &str)> = vec![("agenticId", agent_id)];
        if cfg!(feature = "debug-log") {
            eprintln!("[TaskAPI] GET(jwt+agenticId) {url} | headers: Authorization=Bearer(len={}), agenticId={agent_id}", token.len());
        }
        let started = Instant::now();
        let result = self.wallet.get_authed_with_headers(path, &token, &query, Some(&headers)).await;
        let elapsed = started.elapsed();
        match &result {
            Ok(data) => {
                if cfg!(feature = "debug-log") {
                    eprintln!("[TaskAPI] GET(jwt+agenticId) {url} ← {data}");
                }
                log_api("get", path, agent_id, true, elapsed, None, None);
            }
            Err(e) => {
                let err_msg = format!("{e:#}");
                if cfg!(feature = "debug-log") {
                    eprintln!("[TaskAPI] GET(jwt+agenticId) {url} ← ERROR: {err_msg}");
                }
                log_api("get", path, agent_id, false, elapsed, Some(&err_msg), None);
            }
        }
        result
    }

    /// GET + JWT + identity header (agenticId) -> returns data (sessionCert is auto-injected as a query param).
    pub async fn get_with_identity(
        &mut self,
        path: &str,
        agent_id: &str,
    ) -> Result<Value> {
        let url = format!("{}{}", self.base_url, path);
        let token = get_access_token().await?;
        let cert = get_session_cert();
        let query: Vec<(&str, &str)> = cert.as_deref()
            .map(|c| vec![("sessionCert", c)])
            .unwrap_or_default();
        if cfg!(feature = "debug-log") {
            eprintln!("[TaskAPI] GET {url} | headers: Authorization=Bearer(len={}), agenticId={agent_id}", token.len());
        }
        let headers = [("agenticId", agent_id)];
        let started = Instant::now();
        let result = self.wallet
            .get_authed_with_headers(path, &token, &query, Some(&headers))
            .await;
        let elapsed = started.elapsed();
        match &result {
            Ok(data) => {
                if cfg!(feature = "debug-log") {
                    eprintln!("[TaskAPI] GET {url} ← {data}");
                }
                log_api("get", path, agent_id, true, elapsed, None, None);
            }
            Err(e) => {
                let err_msg = format!("{e:#}");
                if cfg!(feature = "debug-log") {
                    eprintln!("[TaskAPI] GET {url} ← ERROR: {err_msg}");
                }
                log_api("get", path, agent_id, false, elapsed, Some(&err_msg), None);
            }
        }
        result
    }

    /// GET binary (non-JSON endpoints such as evidence download) + JWT + agenticId header.
    /// Uses raw `raw_http` (bypasses wallet's JSON `handle_response`, no DoH failover) and returns raw bytes.
    /// The backend also enforces auth on evidence/download, so Bearer is required.
    pub async fn get_bytes_with_identity(
        &self,
        path: &str,
        query: &[(&str, &str)],
        agent_id: &str,
    ) -> Result<Vec<u8>> {
        let token = get_access_token().await?;
        let url = format!("{}{}", self.base_url.trim_end_matches('/'), path);
        let mut headers = crate::client::ApiClient::jwt_headers(&token);
        if let (Ok(name), Ok(val)) = (
            reqwest::header::HeaderName::from_bytes(b"agenticId"),
            reqwest::header::HeaderValue::from_str(agent_id),
        ) {
            headers.insert(name, val);
        }
        if cfg!(feature = "debug-log") {
            eprintln!(
                "[TaskAPI] GET(bytes) {url} | headers: Authorization=Bearer(len={}), agenticId={agent_id}",
                token.len()
            );
        }
        let query_summary = query
            .iter()
            .map(|(k, v)| format!("{k}={v}"))
            .collect::<Vec<_>>()
            .join("&");
        let query_extra = if query_summary.is_empty() {
            None
        } else {
            Some(format!("query={query_summary}"))
        };
        let started = Instant::now();
        let resp = match self
            .raw_http
            .get(&url)
            .headers(headers)
            .query(query)
            .send()
            .await
            .context("evidence download request failed")
        {
            Ok(r) => r,
            Err(e) => {
                let err_msg = format!("{e:#}");
                log_api(
                    "get_bytes",
                    path,
                    agent_id,
                    false,
                    started.elapsed(),
                    Some(&err_msg),
                    query_extra.as_deref(),
                );
                return Err(e);
            }
        };
        let status = resp.status();
        if !status.is_success() {
            let body = resp.text().await.unwrap_or_default();
            let err_msg = format!("evidence download failed ({status}): {url}; body={body}");
            log_api(
                "get_bytes",
                path,
                agent_id,
                false,
                started.elapsed(),
                Some(&err_msg),
                query_extra.as_deref(),
            );
            return Err(anyhow!("{err_msg}"));
        }
        let bytes = match resp.bytes().await {
            Ok(b) => b,
            Err(e) => {
                let err_msg = format!("{e:#}");
                log_api(
                    "get_bytes",
                    path,
                    agent_id,
                    false,
                    started.elapsed(),
                    Some(&err_msg),
                    query_extra.as_deref(),
                );
                return Err(e.into());
            }
        };
        log_api(
            "get_bytes",
            path,
            agent_id,
            true,
            started.elapsed(),
            None,
            query_extra.as_deref(),
        );
        Ok(bytes.to_vec())
    }

    /// POST JSON + JWT + identity header -> returns data (sessionCert auto-injected).
    pub async fn post_with_identity(
        &mut self,
        path: &str,
        body: &Value,
        agent_id: &str,
    ) -> Result<Value> {
        let body = inject_session_cert(body);
        let url = format!("{}{}", self.base_url, path);
        let token = get_access_token().await?;
        if cfg!(feature = "debug-log") {
            eprintln!("[TaskAPI] POST {url} | headers: Authorization=Bearer(len={}), agenticId={agent_id} | body: {body}", token.len());
        }
        let headers = [("agenticId", agent_id)];
        let started = Instant::now();
        let result = self.wallet
            .post_authed_with_headers(path, &token, &body, Some(&headers))
            .await;
        let elapsed = started.elapsed();
        match &result {
            Ok(data) => {
                if cfg!(feature = "debug-log") {
                    eprintln!("[TaskAPI] POST {url} ← {data}");
                }
                log_api("post", path, agent_id, true, elapsed, None, None);
            }
            Err(e) => {
                let err_msg = format!("{e:#}");
                if cfg!(feature = "debug-log") {
                    eprintln!("[TaskAPI] POST {url} ← ERROR: {err_msg}");
                }
                log_api("post", path, agent_id, false, elapsed, Some(&err_msg), None);
            }
        }
        result
    }

    /// POST raw body + custom Content-Type + JWT + identity header (agenticId).
    ///
    /// Used for scenarios that need precise control over the wire format, such as hand-rolled multipart bodies (curl-compatible).
    /// Callers write the body bytes themselves and provide the Content-Type (including the boundary);
    /// this is more controllable than reqwest's built-in `multipart::Form` builder and avoids chunked-transfer / part-header issues.
    pub async fn raw_post_with_identity(
        &mut self,
        path: &str,
        body: Vec<u8>,
        content_type: &str,
        agent_id: &str,
    ) -> Result<Value> {
        let url = format!("{}{}", self.base_url, path);
        let token = get_access_token().await?;
        let content_len = body.len();
        if cfg!(feature = "debug-log") {
            eprintln!(
                "[TaskAPI] POST(raw) {url} | headers: Authorization=Bearer(len={}), agenticId={agent_id}, Content-Type={content_type}, Content-Length={content_len}",
                token.len(),
            );
        }
        let extra = [("agenticId", agent_id)];
        let extra_meta = format!("contentType={content_type}; contentLength={content_len}");
        let started = Instant::now();
        let result = self.wallet
            .post_authed_raw_with_headers(path, &token, body, content_type, Some(&extra))
            .await;
        let elapsed = started.elapsed();
        match &result {
            Ok(data) => {
                if cfg!(feature = "debug-log") {
                    eprintln!("[TaskAPI] POST(raw) {url} ← {data}");
                }
                log_api("post_raw", path, agent_id, true, elapsed, None, Some(&extra_meta));
            }
            Err(e) => {
                let err_msg = format!("{e:#}");
                if cfg!(feature = "debug-log") {
                    eprintln!("[TaskAPI] POST(raw) {url} ← ERROR: {err_msg}");
                }
                log_api("post_raw", path, agent_id, false, elapsed, Some(&err_msg), Some(&extra_meta));
            }
        }
        result
    }
}
