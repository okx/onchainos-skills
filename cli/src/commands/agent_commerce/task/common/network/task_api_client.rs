//! 任务后端 API 客户端
//!
//! 内部委托 `WalletApiClient` 完成所有 HTTP 请求，复用其 DoH（DNS-over-HTTPS）
//! 解析和 failover retry 能力。在此基础上，额外注入任务系统特有的
//! `agenticId` 身份头。
//!
//! 所有请求方法接收 **path**（如 `/priapi/v1/aieco/task/{jobId}/apply`），
//! 不再接收完整 URL。返回值为 `body["data"]`。

use anyhow::{anyhow, Context, Result};
use serde_json::Value;

use crate::commands::agentic_wallet::auth::ensure_tokens_refreshed;
use crate::wallet_api::WalletApiClient;
use crate::wallet_store;

/// 任务 API 路径前缀
const TASK_PREFIX: &str = "/priapi/v1/aieco/task";

/// 获取有效 access_token，失败则回退到 keyring 中的静态值
async fn get_access_token() -> String {
    ensure_tokens_refreshed()
        .await
        .unwrap_or_else(|_| crate::keyring_store::get_opt("access_token").unwrap_or_default())
}

/// 从本地 session 读取 sessionCert
fn get_session_cert() -> Option<String> {
    wallet_store::load_session()
        .ok()
        .flatten()
        .map(|s| s.session_cert)
        .filter(|c| !c.is_empty())
}

/// 将 sessionCert 注入到 JSON body 中（如果 body 是 Object 且尚未包含该字段）
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

/// 任务后端 API 客户端（DoH-enabled，委托 WalletApiClient）
pub struct TaskApiClient {
    wallet: WalletApiClient,
    raw_http: reqwest::Client,
    base_url: String,
}

impl TaskApiClient {
    pub fn new() -> Self {
        Self::build(None)
    }

    /// 指定自定义 base URL（最高优先级，盖过 env / 常量）
    pub fn with_base_url(base_url: String) -> Self {
        Self::build(Some(base_url))
    }

    fn build(base_url_override: Option<String>) -> Self {
        // base_url 解析 —— 与 WalletApiClient::with_base_url 保持一致的优先级，
        // 这样 eprintln 里展示的 URL 跟 wallet 实际发请求的 URL 不会撕裂。
        // 优先级：OKX_BASE_URL env > 编译时 OKX_BASE_URL > 显式 override > DEFAULT_BASE_URL
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

    // ─── URL / path 辅助 ─────────────────────────────────────────────────

    /// 获取裸 reqwest::Client（不含 DoH，用于外部端点如 x402）
    pub fn http(&self) -> &reqwest::Client {
        &self.raw_http
    }

    pub fn base_url(&self) -> &str {
        &self.base_url
    }

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

    // ─── 请求方法（接收 path，非完整 URL）────────────────────────────────

    /// GET + JWT → 返回 data（自动注入 sessionCert query param）// todo liyun 删掉
    pub async fn get(&mut self, path: &str) -> Result<Value> {
        let url = format!("{}{}", self.base_url, path);
        let token = get_access_token().await;
        let cert = get_session_cert();
        let query: Vec<(&str, &str)> = cert.as_deref()
            .map(|c| vec![("sessionCert", c)])
            .unwrap_or_default();
        eprintln!("[TaskAPI] GET {url} | headers: Authorization=Bearer(len={})", token.len());
        let result = self.wallet.get_authed(path, &token, &query).await;
        match &result {
            Ok(data) => eprintln!("[TaskAPI] GET {url} ← {data}"),
            Err(e) => eprintln!("[TaskAPI] GET {url} ← ERROR: {e}"),
        }
        result
    }

    /// GET + JWT + agenticId header（不注入 sessionCert）→ 返回 data。
    /// 用于查询接口（如 providerConfirmStatus）需要 JWT + agenticId 但不需要 sessionCert 的场景。
    pub async fn get_with_agent_id(&mut self, path: &str, agent_id: &str) -> Result<Value> {
        let url = format!("{}{}", self.base_url, path);
        let token = get_access_token().await;
        let query: Vec<(&str, &str)> = vec![];
        let headers: Vec<(&str, &str)> = vec![("agenticId", agent_id)];
        eprintln!("[TaskAPI] GET(jwt+agenticId) {url} | headers: Authorization=Bearer(len={}), agenticId={agent_id}", token.len());
        let result = self.wallet.get_authed_with_headers(path, &token, &query, Some(&headers)).await;
        match &result {
            Ok(data) => eprintln!("[TaskAPI] GET(jwt+agenticId) {url} ← {data}"),
            Err(e) => eprintln!("[TaskAPI] GET(jwt+agenticId) {url} ← ERROR: {e}"),
        }
        result
    }

    /// GET + JWT + 身份头（agenticId）→ 返回 data（自动注入 sessionCert query param）。
    pub async fn get_with_identity(
        &mut self,
        path: &str,
        agent_id: &str,
    ) -> Result<Value> {
        let url = format!("{}{}", self.base_url, path);
        let token = get_access_token().await;
        let cert = get_session_cert();
        let query: Vec<(&str, &str)> = cert.as_deref()
            .map(|c| vec![("sessionCert", c)])
            .unwrap_or_default();
        eprintln!("[TaskAPI] GET {url} | headers: Authorization=Bearer(len={}), agenticId={agent_id}", token.len());
        let headers = [("agenticId", agent_id)];
        let result = self.wallet
            .get_authed_with_headers(path, &token, &query, Some(&headers))
            .await;
        match &result {
            Ok(data) => eprintln!("[TaskAPI] GET {url} ← {data}"),
            Err(e) => eprintln!("[TaskAPI] GET {url} ← ERROR: {e}"),
        }
        result
    }

    /// GET 二进制（证据下载等非 JSON 端点）+ JWT + agenticId header。
    /// 走裸 `raw_http`（不经 wallet 的 JSON `handle_response`，无 DoH failover），
    /// 返回原始字节。后端对 evidence/download 也强制鉴权，必须带 Bearer。
    pub async fn get_bytes_with_identity(
        &self,
        path: &str,
        query: &[(&str, &str)],
        agent_id: &str,
    ) -> Result<Vec<u8>> {
        let token = get_access_token().await;
        let url = format!("{}{}", self.base_url.trim_end_matches('/'), path);
        let mut headers = crate::client::ApiClient::jwt_headers(&token);
        if let (Ok(name), Ok(val)) = (
            reqwest::header::HeaderName::from_bytes(b"agenticId"),
            reqwest::header::HeaderValue::from_str(agent_id),
        ) {
            headers.insert(name, val);
        }
        eprintln!(
            "[TaskAPI] GET(bytes) {url} | headers: Authorization=Bearer(len={}), agenticId={agent_id}",
            token.len()
        );
        let resp = self
            .raw_http
            .get(&url)
            .headers(headers)
            .query(query)
            .send()
            .await
            .context("evidence download request failed")?;
        let status = resp.status();
        if !status.is_success() {
            let body = resp.text().await.unwrap_or_default();
            return Err(anyhow!("evidence download failed ({status}): {url}; body={body}"));
        }
        Ok(resp.bytes().await?.to_vec())
    }

    /// POST JSON + JWT → 返回 data（自动注入 sessionCert）// todo liyun 删除
    pub async fn post(&mut self, path: &str, body: &Value) -> Result<Value> {
        let body = inject_session_cert(body);
        let url = format!("{}{}", self.base_url, path);
        let token = get_access_token().await;
        eprintln!("[TaskAPI] POST {url} | headers: Authorization=Bearer(len={}) | body: {body}", token.len());
        let result = self.wallet.post_authed(path, &token, &body).await;
        match &result {
            Ok(data) => eprintln!("[TaskAPI] POST {url} ← {data}"),
            Err(e) => eprintln!("[TaskAPI] POST {url} ← ERROR: {e}"),
        }
        result
    }

    /// POST JSON + JWT + 身份头 → 返回 data（自动注入 sessionCert）
    pub async fn post_with_identity(
        &mut self,
        path: &str,
        body: &Value,
        agent_id: &str,
    ) -> Result<Value> {
        let body = inject_session_cert(body);
        let url = format!("{}{}", self.base_url, path);
        let token = get_access_token().await;
        eprintln!("[TaskAPI] POST {url} | headers: Authorization=Bearer(len={}), agenticId={agent_id} | body: {body}", token.len());
        let headers = [("agenticId", agent_id)];
        let result = self.wallet
            .post_authed_with_headers(path, &token, &body, Some(&headers))
            .await;
        match &result {
            Ok(data) => eprintln!("[TaskAPI] POST {url} ← {data}"),
            Err(e) => eprintln!("[TaskAPI] POST {url} ← ERROR: {e}"),
        }
        result
    }

    /// POST 原始 body + 自定义 Content-Type + JWT + 身份头（agenticId）
    ///
    /// 用于手写 multipart body 等需要精确控制 wire 格式的场景（curl 兼容）。
    /// 调用方手写 body bytes 并提供 Content-Type（含 boundary）；
    /// 比 reqwest 自带的 `multipart::Form` builder 更可控，避免 chunked 传输 / part 头不可控问题。
    pub async fn raw_post_with_identity(
        &mut self,
        path: &str,
        body: Vec<u8>,
        content_type: &str,
        agent_id: &str,
    ) -> Result<Value> {
        let url = format!("{}{}", self.base_url, path);
        let token = get_access_token().await;
        eprintln!(
            "[TaskAPI] POST(raw) {url} | headers: Authorization=Bearer(len={}), agenticId={agent_id}, Content-Type={content_type}, Content-Length={}",
            token.len(),
            body.len(),
        );
        let extra = [("agenticId", agent_id)];
        let result = self.wallet
            .post_authed_raw_with_headers(path, &token, body, content_type, Some(&extra))
            .await;
        match &result {
            Ok(data) => eprintln!("[TaskAPI] POST(raw) {url} ← {data}"),
            Err(e) => eprintln!("[TaskAPI] POST(raw) {url} ← ERROR: {e}"),
        }
        result
    }
}
