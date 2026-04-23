//! 任务后端 API 客户端
//!
//! 按 identity 模块模式：每次请求前调 `ensure_tokens_refreshed` 获取有效 JWT，
//! 不在构造时烘焙到 default_headers。Base URL 统一读 `OKX_BASE_URL`。

use anyhow::Result;
use reqwest::RequestBuilder;
use serde_json::Value;

use crate::client::DEFAULT_BASE_URL;
use crate::commands::agentic_wallet::auth::{ensure_tokens_refreshed, format_api_error};
use crate::wallet_api::ApiCodeError;

/// 获取有效 access_token，失败则回退到 keyring 中的静态值
async fn get_access_token() -> String {
    ensure_tokens_refreshed()
        .await
        .unwrap_or_else(|_| crate::keyring_store::get_opt("access_token").unwrap_or_default())
}

/// 校验响应 code，非 0 时抛出 `ApiCodeError`（可被上层 `format_api_error` 格式化）
fn check_response(resp: &Value) -> Result<()> {
    let code = resp["code"].as_i64().unwrap_or(-1);
    if code != 0 {
        let msg = resp["msg"].as_str().unwrap_or("unknown error").to_string();
        Err(ApiCodeError { code: code.to_string(), msg }.into())
    } else {
        Ok(())
    }
}

/// 任务后端 API 客户端
pub struct TaskApiClient {
    http: reqwest::Client,
    base_url: String,
    broadcast_url: String,
}

impl TaskApiClient {
    pub fn new() -> Self {
        let base = std::env::var("OKX_BASE_URL")
            .unwrap_or_else(|_| DEFAULT_BASE_URL.to_string());
        Self::with_base_url(base)
    }

    pub fn with_base_url(base_url: String) -> Self {
        let broadcast_url = format!("{base_url}/priapi/v1/aieco/task/broadcast");
        Self {
            http: reqwest::Client::new(),
            base_url,
            broadcast_url,
        }
    }

    /// 获取裸 reqwest::Client（不含 JWT，用于外部端点如 x402）
    pub fn http(&self) -> &reqwest::Client {
        &self.http
    }

    pub fn base_url(&self) -> &str {
        &self.base_url
    }

    /// `{base}/priapi/v1/aieco/task/broadcast`
    pub fn broadcast_url(&self) -> &str {
        &self.broadcast_url
    }

    /// `{base}/priapi/v1/aieco/task/{job_id}/{action}`
    pub fn endpoint(&self, job_id: &str, action: &str) -> String {
        format!(
            "{}/priapi/v1/aieco/task/{}/{}",
            self.base_url, job_id, action
        )
    }

    // ─── 公开请求方法 ────────────────────────────────────────────────────

    /// GET JSON + JWT + code 校验
    pub async fn get(&self, url: &str) -> Result<Value> {
        self.send(self.http.get(url), "", "").await
    }

    /// POST JSON + JWT + code 校验（无身份头，用于 broadcast 等端点）
    pub async fn post(&self, url: &str, body: &Value) -> Result<Value> {
        self.send(self.http.post(url).json(body), "", "").await
    }

    /// GET JSON + JWT + 身份头（X-Agent-Id / X-Wallet-Address）+ code 校验
    pub async fn get_with_identity(
        &self,
        url: &str,
        agent_id: &str,
        address: &str,
    ) -> Result<Value> {
        self.send(self.http.get(url), agent_id, address).await
    }

    /// POST JSON + JWT + 身份头（X-Agent-Id / X-Wallet-Address）+ code 校验
    pub async fn post_with_identity(
        &self,
        url: &str,
        body: &Value,
        agent_id: &str,
        address: &str,
    ) -> Result<Value> {
        self.send(self.http.post(url).json(body), agent_id, address).await
    }

    /// POST multipart/form-data + JWT + 身份头 + code 校验
    pub async fn multipart_post_with_identity(
        &self,
        url: &str,
        form: reqwest::multipart::Form,
        agent_id: &str,
        address: &str,
    ) -> Result<Value> {
        self.send(self.http.post(url).multipart(form), agent_id, address).await
    }

    // ─── 内部 ────────────────────────────────────────────────────────────

    /// 统一发送：注入 JWT + 可选身份头 → send → 解析 JSON → check code
    async fn send(
        &self,
        mut req: RequestBuilder,
        agent_id: &str,
        address: &str,
    ) -> Result<Value> {
        let token = get_access_token().await;
        req = req.header("Authorization", format!("Bearer {token}"));
        if !agent_id.is_empty() {
            req = req.header("X-Agent-Id", agent_id);
        }
        if !address.is_empty() {
            req = req.header("X-Wallet-Address", address);
        }
        let resp: Value = req
            .send()
            .await
            .map_err(|e| anyhow::anyhow!("HTTP 请求失败: {e}"))?
            .json()
            .await?;
        check_response(&resp).map_err(format_api_error)?;
        Ok(resp)
    }
}
