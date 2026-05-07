//! Evaluator 质押 / 仲裁配置相关的领域类型 + API 封装。
//!
//! 从 `common::network::task_api_client` 迁出 — `task_api_client` 只承担
//! 底层 transport（HTTP / 鉴权头），业务级数据结构与字段解析归属于 evaluator
//! 领域模块。
//!
//! - `StakingConfig`  ← GET /priapi/v1/aieco/task/staking/config
//! - `MyStake`        ← GET /priapi/v1/aieco/task/staking/myStake（金额字段后端已是 OKB 单位）

use anyhow::{Context, Result};
use serde::{Deserialize, Deserializer};

use crate::commands::agent_commerce::task::common::network::task_api_client::TaskApiClient;

/// 把后端下发的字符串数字（如 `"604800"`）反序列化成 `u64`。
/// 后端 `*Seconds` 字段都用字符串承载，避免 JS 大整数失真。
fn de_str_u64<'de, D: Deserializer<'de>>(d: D) -> Result<u64, D::Error> {
    let raw = String::deserialize(d)?;
    raw.parse::<u64>()
        .map_err(|e| serde::de::Error::custom(format!("expected u64 string, got {raw:?}: {e}")))
}

/// 平台质押 & 仲裁配置（GET /priapi/v1/aieco/task/staking/config 返回结构）。
/// 后端通过 Apollo `aitask.platform.*` 配置，重启生效。
///
/// 示例响应（字段顺序按字母序，rename_all=camelCase 自动映射 snake_case → camelCase）：
/// ```json
/// {
///   "arbitrationFeeBps":         "5%",
///   "commitPhaseSeconds":        "64800",
///   "minCumulativeStakeOkb":     "0.001",
///   "partialUnstakeMinRetainOkb":"0.001",
///   "revealPhaseSeconds":        "21600",
///   "slashMinorityBps":          "1%",
///   "slashTimeoutBps":           "0.3%",
///   "slashedCooldownSeconds":    "86400",
///   "unstakeCooldownSeconds":    "604800"
/// }
/// ```
///
/// OKB 金额是十进制字符串、bps 字段是带 `%` 的展示串（如 `"5%"`），原样保留；
/// `*Seconds` 字段后端用字符串承载，反序列化时由 `de_str_u64` 转 `u64`。
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StakingConfig {
    pub min_cumulative_stake_okb: String,
    pub partial_unstake_min_retain_okb: String,
    #[serde(deserialize_with = "de_str_u64")]
    pub unstake_cooldown_seconds: u64,
    pub arbitration_fee_bps: String,
    #[serde(deserialize_with = "de_str_u64")]
    pub commit_phase_seconds: u64,
    #[serde(deserialize_with = "de_str_u64")]
    pub reveal_phase_seconds: u64,
    pub slash_minority_bps: String,
    pub slash_timeout_bps: String,
    #[serde(deserialize_with = "de_str_u64")]
    pub slashed_cooldown_seconds: u64,
}

impl StakingConfig {
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
/// 不能拿 wallet balance 顶替（参见 references/evaluator-staking.md §1.1）。
///
/// 示例响应：
/// ```json
/// {
///   "activeDisputes":     "0",
///   "activeStake":        "0.00196520335316019",
///   "agentId":            "548",
///   "cooldownEndsAt":     0,
///   "pendingUnstake":     "0",
///   "registered":         true,
///   "unstakeAvailableAt": 0,
///   "validStake":         "0.00196520335316019",
///   "voterAddress":       "0x9b66587f0adaf2047bf925ae196e371401e429f7"
/// }
/// ```
///
/// `activeStake` / `pendingUnstake` / `validStake` 后端已统一返回 OKB 单位（不再是 wei），
/// 这里只为本地字段名补 `_okb` 后缀做语义提示。`cooldownEndsAt` / `unstakeAvailableAt`
/// 是 JSON number（unix 秒），`0` 表示"不适用"。
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MyStake {
    pub voter_address: String,
    pub agent_id: String,
    #[serde(rename = "activeStake")]
    pub active_stake_okb: String,
    #[serde(rename = "pendingUnstake")]
    pub pending_unstake_okb: String,
    #[serde(rename = "validStake")]
    pub valid_stake_okb: String,
    pub active_disputes: String,
    #[serde(default)]
    pub cooldown_ends_at: i64,
    #[serde(default)]
    pub unstake_available_at: i64,
    #[serde(default)]
    pub registered: bool,
}

/// 拉取平台质押 & 仲裁配置（GET /priapi/v1/aieco/task/staking/config）。
///
/// 该接口需 JWT + `agenticId` 头（后端 interceptor 校验 evaluator 身份）；无 Body。
/// 返回字段含累计质押门槛、解质押冷却、仲裁押金、commit/reveal 时长、罚金比例等。
/// 所有数值都来自 Apollo 配置，后端权威，CLI 仅用于 UX 提示与本地预检（不替代合约/后端校验）。
pub async fn get_staking_config(
    client: &mut TaskApiClient,
    agent_id: &str,
) -> Result<StakingConfig> {
    let data = client
        .get_with_identity("/priapi/v1/aieco/task/staking/config", agent_id)
        .await?;
    serde_json::from_value(data).context("解析 staking config 响应失败")
}

/// 拉取当前登录账户的链上质押状态（GET /priapi/v1/aieco/task/staking/myStake）。
///
/// API doc 标注仅需 JWT,但实测纯 JWT 调用会被后端 interceptor 拒（code=3001）——
/// 与 `/staking/config` 一样要求 `agenticId` 头做 evaluator 身份校验。因此与
/// `get_staking_config` 对齐:resolve evaluator agentId 后通过 `get_with_identity` 调。
///
/// 后端已把 `activeStake` / `pendingUnstake` / `validStake` 统一以 OKB 单位的十进制
/// 字符串下发，这里直接收到字段后即可展示，无需再做 wei → OKB 换算。响应里的
/// `agentId` 字段未注册时为 `"0"`、`registered=false`,但调用本接口前必须已注册
/// evaluator(否则 interceptor 之前就会拒)。
pub async fn get_my_stake(client: &mut TaskApiClient, agent_id: &str) -> Result<MyStake> {
    let data = client
        .get_with_identity("/priapi/v1/aieco/task/staking/myStake", agent_id)
        .await?;
    serde_json::from_value(data).context("解析 myStake 响应失败")
}
