//! 任务后端 API 客户端
//!
//! 内部委托 `WalletApiClient` 完成所有 HTTP 请求，复用其 DoH（DNS-over-HTTPS）
//! 解析和 failover retry 能力。在此基础上，额外注入任务系统特有的
//! `X-Agent-Id` / `X-Wallet-Address` 身份头。
//!
//! 所有请求方法接收 **path**（如 `/priapi/v1/aieco/task/{jobId}/apply`），
//! 不再接收完整 URL。返回值为 `body["data"]`。

use anyhow::Result;
use serde_json::Value;

use crate::commands::agentic_wallet::auth::ensure_tokens_refreshed;
use crate::wallet_api::WalletApiClient;

/// 任务 API 路径前缀
const TASK_PREFIX: &str = "/priapi/v1/aieco/task";

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

        let base_url = base_url_override.unwrap_or_else(|| {
            std::env::var("OKX_BASE_URL")
                .ok()
                .or_else(|| option_env!("OKX_BASE_URL").map(|s| s.to_string()))
                .unwrap_or_else(|| crate::client::DEFAULT_BASE_URL.to_string())
        });

        Self { wallet, raw_http: reqwest::Client::new(), base_url }
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

    /// GET + JWT → 返回 data
    pub async fn get(&mut self, path: &str) -> Result<Value> {
        let token = get_access_token().await;
        self.wallet.get_authed(path, &token, &[]).await
    }

    /// POST JSON + JWT → 返回 data
    pub async fn post(&mut self, path: &str, body: &Value) -> Result<Value> {
        let token = get_access_token().await;
        self.wallet.post_authed(path, &token, body).await
    }

    /// POST JSON + JWT + 身份头 → 返回 data
    pub async fn post_with_identity(
        &mut self,
        path: &str,
        body: &Value,
        agent_id: &str,
        address: &str,
    ) -> Result<Value> {
        let token = get_access_token().await;
        let headers = [("X-Agent-Id", agent_id), ("X-Wallet-Address", address)];
        self.wallet
            .post_authed_with_headers(path, &token, body, Some(&headers))
            .await
    }

    /// POST multipart/form-data + JWT + 身份头（X-Agent-Id / X-Wallet-Address）
    pub async fn multipart_post_with_identity(
        &mut self,
        path: &str,
        form: reqwest::multipart::Form,
        agent_id: &str,
        address: &str,
    ) -> Result<Value> {
        let token = get_access_token().await;
        let headers = [("X-Agent-Id", agent_id), ("X-Wallet-Address", address)];
        self.wallet
            .post_authed_multipart_with_headers(path, &token, form, Some(&headers))
            .await
    }
}
