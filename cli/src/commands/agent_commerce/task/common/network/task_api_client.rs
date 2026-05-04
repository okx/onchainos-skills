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

/// 平台质押 & 仲裁配置（GET /priapi/v1/aieco/task/staking/config 返回结构）。
/// 后端通过 Apollo `aitask.platform.*` 配置，重启生效。
///
/// 字符串字段保留后端原始格式（OKB 金额是十进制字符串，bps 字段如 "5%"
/// 是带百分号的展示串），秒级字段已 parse 为 u64 便于计算。
#[derive(Debug, Clone)]
pub struct StakingConfig {
    pub min_cumulative_stake_okb: String,
    pub partial_unstake_min_retain_okb: String,
    pub unstake_cooldown_seconds: u64,
    pub arbitration_fee_bps: String,
    pub commit_phase_seconds: u64,
    pub reveal_phase_seconds: u64,
    pub slash_minority_bps: String,
    pub slash_timeout_bps: String,
    pub slashed_cooldown_seconds: u64,
}

impl StakingConfig {  // todo zhangxin 挪走
    /// 解质押冷却期（天，向上取整以便 UX 文案对齐"≥ N 天"语义）。
    pub fn unstake_cooldown_days(&self) -> u64 {
        self.unstake_cooldown_seconds.div_ceil(86400)
    }

    /// Commit 阶段时长（小时，整数）。
    pub fn commit_phase_hours(&self) -> u64 {
        self.commit_phase_seconds / 3600
    }

    /// Reveal 阶段时长（小时，整数）。
    pub fn reveal_phase_hours(&self) -> u64 {
        self.reveal_phase_seconds / 3600
    }
}

/// 当前登录账户的链上质押状态（GET /priapi/v1/aieco/task/staking/myStake 返回结构）。
///
/// 与"钱包余额"是两个独立概念：余额在 EOA 上、可花费；`activeStake` 已经从余额转入
/// `VoterStaking` 合约锁仓，扣过历史罚没。skill 的累计门槛判断必须用 `activeStake`，
/// 不能拿 wallet balance 顶替（参见 evaluator.md 1.5）。
///
/// 金额字段保留后端原始 wei 字符串（最小单位，OKB 18 位精度），通过 `wei_to_okb`
/// 转 UI 字符串。Unix 秒时间戳为 0 表示"不适用"。
#[derive(Debug, Clone)]
pub struct MyStake {
    pub voter_address: String,
    pub agent_id: String,
    pub active_stake_wei: String,
    pub pending_unstake_wei: String,
    pub valid_stake_wei: String,
    pub active_disputes: String,
    pub cooldown_ends_at: i64,
    pub unstake_available_at: i64,
    pub registered: bool,
}

impl MyStake {
    /// `activeStake` 转 OKB 字符串（已扣历史罚没的当前质押）。
    pub fn active_stake_okb(&self) -> String {
        wei_to_okb(&self.active_stake_wei)
    }

    /// `pendingUnstake` 转 OKB 字符串（冷却期中待解锁）。
    pub fn pending_unstake_okb(&self) -> String {
        wei_to_okb(&self.pending_unstake_wei)
    }

    /// `validStake = activeStake - pendingUnstake` 转 OKB 字符串（可被加权选取的余额）。
    pub fn valid_stake_okb(&self) -> String {
        wei_to_okb(&self.valid_stake_wei)
    }
}

/// wei → OKB 字符串（18 位小数，去尾零）。仅支持纯数字字符串；非法输入原样返回。
///
/// 例：`"100000000000000000000"` → `"100"`，`"1500000000000000000"` → `"1.5"`，
/// `"1"` → `"0.000000000000000001"`，`"0"` → `"0"`。
pub fn wei_to_okb(wei: &str) -> String {
    const DECIMALS: usize = 18;
    let s = wei.trim();
    if s.is_empty() {
        return "0".to_string();
    }
    if !s.chars().all(|c| c.is_ascii_digit()) {
        return s.to_string();
    }
    let s = s.trim_start_matches('0');
    if s.is_empty() {
        return "0".to_string();
    }
    if s.len() <= DECIMALS {
        let pad = DECIMALS - s.len();
        let frac = format!("{}{}", "0".repeat(pad), s);
        let frac_trimmed = frac.trim_end_matches('0');
        if frac_trimmed.is_empty() {
            "0".to_string()
        } else {
            format!("0.{frac_trimmed}")
        }
    } else {
        let split_at = s.len() - DECIMALS;
        let int_part = &s[..split_at];
        let frac = &s[split_at..];
        let frac_trimmed = frac.trim_end_matches('0');
        if frac_trimmed.is_empty() {
            int_part.to_string()
        } else {
            format!("{int_part}.{frac_trimmed}")
        }
    }
}

/// 把 JSON 里的字符串字段拷出来。
fn take_str_field(data: &Value, key: &str) -> Result<String> {
    data.get(key)
        .and_then(Value::as_str)
        .map(str::to_string)
        .ok_or_else(|| anyhow!("staking config 缺少字段 {key} 或类型非 string"))
}

/// 把 JSON 里的字符串字段 parse 成 u64（后端 seconds 字段都是字符串）。
fn take_u64_field(data: &Value, key: &str) -> Result<u64> {
    let raw = take_str_field(data, key)?;
    raw.parse::<u64>()
        .with_context(|| format!("staking config 字段 {key}={raw} 不是合法 u64"))
}

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

    /// 拉取平台质押 & 仲裁配置（GET /priapi/v1/aieco/task/staking/config）。
    ///
    /// 该接口需 JWT + `agenticId` 头（后端 interceptor 校验 evaluator 身份）；无 Body。
    /// 返回字段含累计质押门槛、解质押冷却、仲裁押金、commit/reveal 时长、罚金比例等。
    /// 所有数值都来自 Apollo 配置，后端权威，CLI 仅用于 UX 提示与本地预检（不替代合约/后端校验）。
    /// // todo zhangxin 挪走
    pub async fn get_staking_config(&mut self, agent_id: &str) -> Result<StakingConfig> {
        let data = self
            .get_with_identity("/priapi/v1/aieco/task/staking/config", agent_id)
            .await?;
        Ok(StakingConfig {
            min_cumulative_stake_okb: take_str_field(&data, "minCumulativeStakeOkb")?,
            partial_unstake_min_retain_okb: take_str_field(&data, "partialUnstakeMinRetainOkb")?,
            unstake_cooldown_seconds: take_u64_field(&data, "unstakeCooldownSeconds")?,
            arbitration_fee_bps: take_str_field(&data, "arbitrationFeeBps")?,
            commit_phase_seconds: take_u64_field(&data, "commitPhaseSeconds")?,
            reveal_phase_seconds: take_u64_field(&data, "revealPhaseSeconds")?,
            slash_minority_bps: take_str_field(&data, "slashMinorityBps")?,
            slash_timeout_bps: take_str_field(&data, "slashTimeoutBps")?,
            slashed_cooldown_seconds: take_u64_field(&data, "slashedCooldownSeconds")?,
        })
    }

    /// 拉取当前登录账户的链上质押状态（GET /priapi/v1/aieco/task/staking/myStake）。
    ///
    /// API doc 标注仅需 JWT,但实测纯 JWT 调用会被后端 interceptor 拒（code=3001）——
    /// 与 `/staking/config` 一样要求 `agenticId` 头做 evaluator 身份校验。因此与
    /// `get_staking_config` 对齐:resolve evaluator agentId 后通过 `get_with_identity` 调。
    ///
    /// 返回的金额字段都是 wei（最小单位字符串）；UI 用 `MyStake::active_stake_okb()` 等
    /// 方法做 OKB 换算。响应里的 `agentId` 字段未注册时为 `"0"`、`registered=false`,
    /// 但调用本接口前必须已注册 evaluator(否则 interceptor 之前就会拒)。
    pub async fn get_my_stake(&mut self, agent_id: &str) -> Result<MyStake> {
        let data = self
            .get_with_identity("/priapi/v1/aieco/task/staking/myStake", agent_id)
            .await?;
        Ok(MyStake {
            voter_address: take_str_field(&data, "voterAddress")?,
            agent_id: take_str_field(&data, "agentId")?,
            active_stake_wei: take_str_field(&data, "activeStake")?,
            pending_unstake_wei: take_str_field(&data, "pendingUnstake")?,
            valid_stake_wei: take_str_field(&data, "validStake")?,
            active_disputes: take_str_field(&data, "activeDisputes")?,
            cooldown_ends_at: data["cooldownEndsAt"].as_i64().unwrap_or(0),
            unstake_available_at: data["unstakeAvailableAt"].as_i64().unwrap_or(0),
            registered: data["registered"].as_bool().unwrap_or(false),
        })
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
