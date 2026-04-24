//! 任务后端 API 客户端
//!
//! 内部委托 `WalletApiClient` 完成所有 HTTP 请求，复用其 DoH（DNS-over-HTTPS）
//! 解析和 failover retry 能力。在此基础上，额外注入任务系统特有的
//! `X-Agent-Id` / `X-Wallet-Address` 身份头。
//!
//! 返回值为 `body["data"]`（与 WalletApiClient 一致），调用方直接
//! 访问 `resp["xxx"]` 而非 `resp["data"]["xxx"]`。

use anyhow::Result;
use serde_json::Value;

use crate::commands::agentic_wallet::auth::ensure_tokens_refreshed;
use crate::wallet_api::WalletApiClient;

/// 获取有效 access_token，失败则回退到 keyring 中的静态值
async fn get_access_token() -> String {
    ensure_tokens_refreshed()
        .await
        .unwrap_or_else(|_| crate::keyring_store::get_opt("access_token").unwrap_or_default())
}

/// 任务后端 API 客户端（DoH-enabled，委托 WalletApiClient）
pub struct TaskApiClient {
    wallet: WalletApiClient,
    raw_http: reqwest::Client,
    base_url: String,
    broadcast_path: String,
}

impl TaskApiClient {
    pub fn new() -> Self {
        Self::build(None)
    }

    /// 指定自定义 base URL（用于 mock-api 等场景）
    pub fn with_base_url(base_url: String) -> Self {
        Self::build(Some(base_url))
    }

    fn build(base_url_override: Option<String>) -> Self {
        let wallet = match &base_url_override {
            Some(url) => WalletApiClient::with_base_url(Some(url)),
            None => WalletApiClient::new(),
        }
        .expect("failed to create WalletApiClient");

        // base_url 与 WalletApiClient 内部保持一致的逻辑
        let base_url = base_url_override.unwrap_or_else(|| {
            std::env::var("OKX_BASE_URL")
                .ok()
                .or_else(|| option_env!("OKX_BASE_URL").map(|s| s.to_string()))
                .unwrap_or_else(|| crate::client::DEFAULT_BASE_URL.to_string())
        });

        Self {
            wallet,
            raw_http: reqwest::Client::new(),
            base_url,
            broadcast_path: "/priapi/v1/aieco/task/broadcast".to_string(),
        }
    }

    /// 获取裸 reqwest::Client（不含 DoH，用于外部端点如 x402）
    pub fn http(&self) -> &reqwest::Client {
        &self.raw_http
    }

    pub fn base_url(&self) -> &str {
        &self.base_url
    }

    /// `{base}/priapi/v1/aieco/task/broadcast` — 返回完整 URL
    pub fn broadcast_url(&self) -> String {
        format!("{}{}", self.base_url, self.broadcast_path)
    }

    /// `{base}/priapi/v1/aieco/task/{job_id}/{action}` — 返回完整 URL
    pub fn endpoint(&self, job_id: &str, action: &str) -> String {
        format!(
            "{}/priapi/v1/aieco/task/{}/{}",
            self.base_url, job_id, action
        )
    }

    // ─── 公开请求方法 ────────────────────────────────────────────────────

    /// GET + JWT + code 校验 → 返回 data
    pub async fn get(&mut self, url: &str) -> Result<Value> {
        let token = get_access_token().await;
        let path = self.to_path(url);
        self.wallet.get_authed(&path, &token, &[]).await
    }

    /// POST JSON + JWT + code 校验 → 返回 data
    pub async fn post(&mut self, url: &str, body: &Value) -> Result<Value> {
        let token = get_access_token().await;
        let path = self.to_path(url);
        self.wallet.post_authed(&path, &token, body).await
    }

    /// GET + JWT + 身份头 → 返回 data
    ///
    /// 注意：WalletApiClient 的 get_authed 不支持 extra_headers，
    /// 因此 agent_id / address 暂未注入到 GET 请求中。
    /// 目前唯一调用方为 common/context（mock-api 日志场景），功能不受影响。
    pub async fn get_with_identity(
        &mut self,
        url: &str,
        _agent_id: &str,
        _address: &str,
    ) -> Result<Value> {
        let token = get_access_token().await;
        let path = self.to_path(url);
        self.wallet.get_authed(&path, &token, &[]).await
    }

    /// POST JSON + JWT + 身份头 → 返回 data
    pub async fn post_with_identity(
        &mut self,
        url: &str,
        body: &Value,
        agent_id: &str,
        address: &str,
    ) -> Result<Value> {
        let token = get_access_token().await;
        let path = self.to_path(url);
        let headers = Self::identity_headers(agent_id, address);
        let header_refs: Vec<(&str, &str)> = headers.iter().map(|(k, v)| (k.as_str(), v.as_str())).collect();
        self.wallet
            .post_authed_with_headers(&path, &token, body, Some(&header_refs))
            .await
    }

    /// POST multipart/form-data + JWT + code 校验 → 返回 data
    ///
    /// 注意：WalletApiClient 的 post_authed_multipart 不支持 extra_headers，
    /// 因此 agent_id / address 参数暂未注入。后续如需身份头，需扩展 WalletApiClient。
    pub async fn multipart_post_with_identity(
        &mut self,
        url: &str,
        form: reqwest::multipart::Form,
        _agent_id: &str,
        _address: &str,
    ) -> Result<Value> {
        let token = get_access_token().await;
        let path = self.to_path(url);
        self.wallet
            .post_authed_multipart(&path, &token, form)
            .await
    }

    // ─── 内部 ────────────────────────────────────────────────────────────

    /// 从完整 URL 提取 path 部分（去掉 base_url 前缀）
    fn to_path(&self, url: &str) -> String {
        url.strip_prefix(&self.base_url)
            .unwrap_or(url)
            .to_string()
    }

    /// 构建身份头对 (X-Agent-Id, X-Wallet-Address)
    fn identity_headers(agent_id: &str, address: &str) -> Vec<(String, String)> {
        let mut headers = Vec::new();
        if !agent_id.is_empty() {
            headers.push(("X-Agent-Id".to_string(), agent_id.to_string()));
        }
        if !address.is_empty() {
            headers.push(("X-Wallet-Address".to_string(), address.to_string()));
        }
        headers
    }
}
