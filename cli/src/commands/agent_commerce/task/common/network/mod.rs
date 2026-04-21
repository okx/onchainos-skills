//! 底层 HTTP 客户端
//!
//! 通用的 JSON API 请求封装，不包含任何任务业务逻辑。

pub mod task_api_client;

use anyhow::{bail, Result};
use serde_json::Value;

/// 通用 JSON API 客户端
pub struct ApiClient {
    http: reqwest::Client,
    base_url: String,
}

impl ApiClient {
    pub fn new(base_url: String) -> Self {
        Self {
            http: reqwest::Client::new(),
            base_url,
        }
    }

    pub fn http(&self) -> &reqwest::Client {
        &self.http
    }

    pub fn base_url(&self) -> &str {
        &self.base_url
    }

    /// GET JSON — 返回 `{ "code": 0, "data": ... }` 格式响应，校验 code
    pub async fn get(&self, url: &str) -> Result<Value> {
        let resp: Value = self
            .http
            .get(url)
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

    /// POST JSON — 发送 body，校验响应 code
    pub async fn post(&self, url: &str, body: &Value) -> Result<Value> {
        let resp: Value = self
            .http
            .post(url)
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
