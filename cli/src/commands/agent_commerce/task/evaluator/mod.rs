//! Evaluator 端任务命令 — 枚举定义 + 路由分发
//!
//! 按仲裁者动作划分文件：
//! - `info.rs`            — 拉取证据（只读，含图片下载）
//! - `download.rs`        — 按 (jobId, fileKey) 单独下载一份证据字节
//! - `commit.rs`          — Commit 投票（commit-reveal 第一阶段）
//! - `reveal.rs`          — Reveal 投票（第二阶段；后端反查 vote+salt，CLI 不传 side）
//! - `claim.rs`           — account 级 pull 领取所有已结算奖励
//! - `claimable.rs`       — 查询账户待领奖励（只读）
//! - `stake.rs`           — 首次质押（身份 skill 跳转入口）
//! - `increase_stake.rs`  — 追加质押（top-up / 补齐）
//! - `unstake.rs`         — 解质押生命周期（request / claim / cancel）
//!
//! 辅助：
//! - `helpers.rs`         — disputeId 解析
//! - `flow.rs`            — 状态机提示词生成器（供 `next-action --role evaluator` 使用）

mod claim;
mod claimable;
mod commit;
mod download;
pub mod flow;
mod helpers;
mod increase_stake;
mod info;
mod my_stake;
mod reveal;
mod stake;
mod staking_config;
mod unstake;

use anyhow::Result;
use clap::Subcommand;

use crate::commands::agent_commerce::task::common::network::task_api_client::TaskApiClient;
use crate::commands::Context;

/// `--agent-id` 通用语义（所有 evaluator 子命令共享）：
///
/// 系统消息 envelope 顶层带 `agentId`（如 `{"agentId":"453", "message":{...}}`），
/// 收到事件时 agent 必须把该值原样透传给 CLI。CLI 用它在 `onchainos agent get`
/// 列表里精确定位 → 取 `ownerAddress` → 在 wallet store 中匹配本地账户来签名 +
/// 发 API。不传则退回"取当前默认钱包再反查 agentId"的旧路径（仅供手动调用兜底，
/// 多身份场景下会取错）。
#[derive(Subcommand)]
pub enum EvaluatorCommand {
    /// Fetch dispute evidence (text + images downloaded locally so multimodal agents can view them)
    Info {
        dispute_id: String,
        /// Evaluator agentId from inbound system envelope's top-level `agentId` field.
        #[arg(long = "agent-id")]
        agent_id: Option<String>,
    },
    /// Download a single evidence file by (jobId, fileKey). Useful for retry / scripted access
    /// when `info` already returned the fileKey but a particular download failed. Backend
    /// requires JWT + agenticId on this endpoint (same auth chain as `info`).
    Download {
        /// Task jobId (top-level `jobId` from `evaluator info` response or envelope).
        job_id: String,
        /// Opaque fileKey returned in `provider.images[]` / `client.images[]`.
        file_key: String,
        /// Output file path. Defaults to `$TMPDIR/onchainos-dispute/<jobId>/<fileKey-tail>`.
        #[arg(long, short = 'o')]
        output: Option<String>,
        /// Evaluator agentId from inbound system envelope's top-level `agentId` field.
        #[arg(long = "agent-id")]
        agent_id: Option<String>,
    },
    /// Commit a vote (Phase 1 of commit-reveal). vote: 0 = Approve (Client wins), 1 = Reject (Provider wins).
    /// Body sent to backend is only `{ vote }` — reason is NOT part of the API (lives in agent session memory).
    Commit {
        dispute_id: String,
        #[arg(long)]
        vote: u8,
        /// Evaluator agentId from inbound system envelope's top-level `agentId` field.
        #[arg(long = "agent-id")]
        agent_id: Option<String>,
    },
    /// Reveal a previously-committed vote (Phase 2 of commit-reveal). Driven by the
    /// `reveal_started` system event whose envelope carries `disputeId`. CLI sends an
    /// empty body `{}` — backend reads vote+salt from `task_dispute_voter` keyed by
    /// (disputeId, voter), so no `--vote` is required.
    Reveal {
        dispute_id: String,
        /// Evaluator agentId from inbound system envelope's top-level `agentId` field.
        #[arg(long = "agent-id")]
        agent_id: Option<String>,
    },
    /// Claim reward after task/dispute resolved. Account-level pull — one call drains
    /// every pending reward across all settled disputes (POST /task/claim, no jobId).
    Claim {
        /// Evaluator agentId from inbound system envelope's top-level `agentId` field.
        #[arg(long = "agent-id")]
        agent_id: Option<String>,
    },
    /// List account-level claimable rewards across all settled disputes (GET /task/claimable).
    /// Read-only; no tx. Useful when multiple disputes settle concurrently.
    Claimable {
        /// Evaluator agentId from inbound system envelope's top-level `agentId` field.
        #[arg(long = "agent-id")]
        agent_id: Option<String>,
    },
    /// First-time stake OKB to become an active evaluator (onboarding handoff from identity skill).
    /// Requires the current wallet's agentId to already be registered with evaluator role
    /// (identity=2). Backend enforces amount >= minCumulativeStakeOkb on first stake (see staking-config).
    /// For top-up / 补充质押 use `increase-stake` (backend `/staking/increaseStake`).
    Stake {
        #[arg(long)]
        amount: String,
        /// Evaluator agentId from inbound system envelope's top-level `agentId` field.
        #[arg(long = "agent-id")]
        agent_id: Option<String>,
    },
    /// Top up an existing stake (no minimum). Used to replenish slashed stake or increase
    /// selection weight. Hits a different backend endpoint than `stake`.
    IncreaseStake {
        #[arg(long)]
        amount: String,
        /// Evaluator agentId from inbound system envelope's top-level `agentId` field.
        #[arg(long = "agent-id")]
        agent_id: Option<String>,
    },
    /// Request unstake: OKB enters cooldown (period from staking-config). Partial unstake supported.
    /// Backend/contract will revert if you have active dispute participation.
    RequestUnstake {
        #[arg(long)]
        amount: String,
        /// Evaluator agentId from inbound system envelope's top-level `agentId` field.
        #[arg(long = "agent-id")]
        agent_id: Option<String>,
    },
    /// Claim unstaked OKB after the cooldown period. No parameters — contract knows the
    /// pending amount and unlock time.
    ClaimUnstake {
        /// Evaluator agentId from inbound system envelope's top-level `agentId` field.
        #[arg(long = "agent-id")]
        agent_id: Option<String>,
    },
    /// Cancel a pending unstake request within the cooldown window; OKB returns to staked state.
    CancelUnstake {
        /// Evaluator agentId from inbound system envelope's top-level `agentId` field.
        #[arg(long = "agent-id")]
        agent_id: Option<String>,
    },
    /// Read platform staking & arbitration config (Apollo-driven, JWT auth, no body).
    /// Mirrors GET /priapi/v1/aieco/task/staking/config.
    #[command(name = "staking-config", visible_alias = "stakingconfig")]
    StakingConfig {
        /// Evaluator agentId from inbound system envelope's top-level `agentId` field.
        #[arg(long = "agent-id")]
        agent_id: Option<String>,
    },
    /// Read the current account's on-chain stake state (activeStake / pendingUnstake /
    /// validStake / activeDisputes / cooldown timestamps / registered flag).
    /// Mirrors GET /priapi/v1/aieco/task/staking/myStake. JWT auth, no body, no agentId
    /// header — backend resolves from token. Use this (not wallet balance) for the
    /// cumulative-stake threshold check in evaluator.md 1.5.
    #[command(name = "my-stake", visible_alias = "mystake")]
    MyStake {
        /// Evaluator agentId from inbound system envelope's top-level `agentId` field.
        #[arg(long = "agent-id")]
        agent_id: Option<String>,
    },
}

pub async fn run(cmd: EvaluatorCommand, _ctx: &Context) -> Result<()> {
    let mut client = TaskApiClient::new();

    match cmd {
        EvaluatorCommand::Info { dispute_id, agent_id } =>
            info::handle_info(&mut client, &dispute_id, agent_id.as_deref()).await,
        EvaluatorCommand::Download { job_id, file_key, output, agent_id } =>
            download::handle_download(&client, &job_id, &file_key, output.as_deref(), agent_id.as_deref()).await,
        EvaluatorCommand::Commit { dispute_id, vote, agent_id } =>
            commit::handle_commit(&mut client, &dispute_id, vote, agent_id.as_deref()).await,
        EvaluatorCommand::Reveal { dispute_id, agent_id } =>
            reveal::handle_reveal(&mut client, &dispute_id, agent_id.as_deref()).await,
        EvaluatorCommand::Claim { agent_id } =>
            claim::handle_claim(&mut client, agent_id.as_deref()).await,
        EvaluatorCommand::Claimable { agent_id } =>
            claimable::handle_claimable(&mut client, agent_id.as_deref()).await,
        EvaluatorCommand::Stake { amount, agent_id } =>
            stake::handle_stake(&mut client, &amount, agent_id.as_deref()).await,
        EvaluatorCommand::IncreaseStake { amount, agent_id } =>
            increase_stake::handle_increase_stake(&mut client, &amount, agent_id.as_deref()).await,
        EvaluatorCommand::RequestUnstake { amount, agent_id } =>
            unstake::handle_request_unstake(&mut client, &amount, agent_id.as_deref()).await,
        EvaluatorCommand::ClaimUnstake { agent_id } =>
            unstake::handle_claim_unstake(&mut client, agent_id.as_deref()).await,
        EvaluatorCommand::CancelUnstake { agent_id } =>
            unstake::handle_cancel_unstake(&mut client, agent_id.as_deref()).await,
        EvaluatorCommand::StakingConfig { agent_id } =>
            staking_config::handle_staking_config(&mut client, agent_id.as_deref()).await,
        EvaluatorCommand::MyStake { agent_id } =>
            my_stake::handle_my_stake(&mut client, agent_id.as_deref()).await,
    }
}
