//! 任务后端 API 客户端
//!
//! 基于 `network::ApiClient` 封装任务系统的端点构建和身份认证。

use anyhow::{bail, Result};
use serde_json::Value;

use super::ApiClient;

/// 从环境变量读取任务 API 基础 URL
fn task_api_url() -> String {
    std::env::var("TASK_API_URL").unwrap_or_else(|_| "http://127.0.0.1:9001".to_string())
}

/// 任务后端 API 客户端
pub struct TaskApiClient {
    api: ApiClient,
}

impl TaskApiClient {
    pub fn new() -> Self {
        Self::with_base_url(task_api_url())
    }

    pub fn with_base_url(base_url: String) -> Self {
        Self {
            api: ApiClient::new(base_url),
        }
    }

    pub fn http(&self) -> &reqwest::Client {
        self.api.http()
    }

    pub fn base_url(&self) -> &str {
        self.api.base_url()
    }

    /// `{base}/priapi/v1/aieco/task/broadcast`
    pub fn broadcast_url(&self) -> String {
        format!("{}/priapi/v1/aieco/task/broadcast", self.api.base_url())
    }

    /// `{base}/priapi/v1/aieco/task/{job_id}/{action}`
    pub fn endpoint(&self, job_id: &str, action: &str) -> String {
        format!(
            "{}/priapi/v1/aieco/task/{}/{}",
            self.api.base_url(),
            job_id,
            action
        )
    }

    /// GET JSON + code 校验
    pub async fn get(&self, url: &str) -> Result<Value> {
        self.api.get(url).await
    }

    /// POST JSON + code 校验（无身份头，用于 broadcast 等无身份端点）
    pub async fn post(&self, url: &str, body: &Value) -> Result<Value> {
        self.api.post(url, body).await
    }

    /// GET JSON + 身份头（X-Agent-Id / X-Wallet-Address）+ code 校验
    pub async fn get_with_identity(
        &self,
        url: &str,
        agent_id: &str,
        address: &str,
    ) -> Result<Value> {
        let resp: Value = self
            .api
            .http()
            .get(url)
            .header("X-Agent-Id", agent_id)
            .header("X-Wallet-Address", address)
            .send()
            .await
            .map_err(|e| anyhow::anyhow!("HTTP GET 失败: {e}"))?
            .json()
            .await?;
        if resp["code"] != 0 {
            bail!(
                "请求失败: {}",
                resp["msg"].as_str().unwrap_or("unknown error")
            );
        }
        Ok(resp)
    }

    /// POST JSON + 身份头（X-Agent-Id / X-Wallet-Address）+ code 校验
    pub async fn post_with_identity(
        &self,
        url: &str,
        body: &Value,
        agent_id: &str,
        address: &str,
    ) -> Result<Value> {
        let resp: Value = self
            .api
            .http()
            .post(url)
            .header("X-Agent-Id", agent_id)
            .header("X-Wallet-Address", address)
            .json(body)
            .send()
            .await
            .map_err(|e| anyhow::anyhow!("HTTP POST 失败: {e}"))?
            .json()
            .await?;
        if resp["code"] != 0 {
            bail!(
                "请求失败: {}",
                resp["msg"].as_str().unwrap_or("unknown error")
            );
        }
        Ok(resp)
    }
}
