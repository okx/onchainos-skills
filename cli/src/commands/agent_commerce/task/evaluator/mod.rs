//! Evaluator 端任务命令 — 枚举定义 + 路由分发
//!
//! 按仲裁者动作划分文件：
//! - `info.rs`            — 拉取证据（只读，含图片下载）
//! - `commit.rs`          — Commit 投票（commit-reveal 第一阶段）
//! - `reveal.rs`          — Reveal 投票（第二阶段，带 canReveal 预检）
//! - `commit_store.rs`    — 本地 `{disputeId, side, salt}` 存档（~/.onchainos/evaluator-commits.jsonl）
//! - `forget.rs`          — 清理本地 commit 存档（dispute 终结后调用）
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
mod commit_store;
mod forget;
pub mod flow;
mod helpers;
mod increase_stake;
mod info;
mod reveal;
mod stake;
mod unstake;

use anyhow::Result;
use clap::Subcommand;

use crate::commands::agent_commerce::task::common::network::task_api_client::TaskApiClient;
use crate::commands::Context;

#[derive(Subcommand)]
pub enum EvaluatorCommand {
    /// Fetch dispute evidence (text + images downloaded locally so multimodal agents can view them)
    Info { dispute_id: String },
    /// Commit a vote (Phase 1 of commit-reveal). side: 1 = Provider wins (Approve), 2 = Client wins (Reject).
    /// Body sent to backend is only `{ vote }` — reason is NOT part of the API (lives in agent session memory).
    Commit {
        dispute_id: String,
        #[arg(long)]
        side: u8,
    },
    /// Reveal a previously-committed vote (Phase 2 of commit-reveal).
    /// `--side` is optional: if omitted, CLI auto-resolves from `~/.onchainos/evaluator-commits.jsonl`
    /// written during the original commit. Explicit `--side` overrides the stored value (CLI warns on mismatch).
    Reveal {
        dispute_id: String,
        #[arg(long)]
        side: Option<u8>,
    },
    /// Claim reward after task/dispute resolved. Account-level pull — one call drains
    /// every pending reward across all settled disputes (POST /task/claim, no jobId).
    Claim,
    /// List account-level claimable rewards across all settled disputes (GET /task/claimable).
    /// Read-only; no tx. Useful when multiple disputes settle concurrently.
    Claimable,
    /// Delete the local commit record for a settled dispute (called on dispute_resolved /
    /// round_failed, idempotent). Round is terminal — {vote, salt} is no longer needed client-side.
    Forget { dispute_id: String },
    /// First-time stake OKB to become an active evaluator (onboarding handoff from identity skill).
    /// Requires the current wallet's agentId to already be registered with evaluator role
    /// (identity=2). Backend enforces amount >= 100 OKB on first stake.
    /// For top-up / 补充质押 use `increase-stake` (backend `/staking/increaseStake`).
    Stake {
        #[arg(long)]
        amount: String,
    },
    /// Top up an existing stake (no minimum). Used to replenish slashed stake or increase
    /// selection weight. Hits a different backend endpoint than `stake`.
    IncreaseStake {
        #[arg(long)]
        amount: String,
    },
    /// Request unstake: OKB enters a 7-day cooldown. Partial unstake supported.
    /// Backend/contract will revert if you have active dispute participation.
    RequestUnstake {
        #[arg(long)]
        amount: String,
    },
    /// Claim unstaked OKB after the 7-day cooldown. No parameters — contract knows the
    /// pending amount and unlock time.
    ClaimUnstake,
    /// Cancel a pending unstake request within the cooldown window; OKB returns to staked state.
    CancelUnstake,
}

pub async fn run(cmd: EvaluatorCommand, _ctx: &Context) -> Result<()> {
    let mut client = TaskApiClient::new();

    match cmd {
        EvaluatorCommand::Info { dispute_id } =>
            info::handle_info(&mut client, &dispute_id).await,
        EvaluatorCommand::Commit { dispute_id, side } =>
            commit::handle_commit(&mut client, &dispute_id, side).await,
        EvaluatorCommand::Reveal { dispute_id, side } =>
            reveal::handle_reveal(&mut client, &dispute_id, side).await,
        EvaluatorCommand::Claim =>
            claim::handle_claim(&mut client).await,
        EvaluatorCommand::Claimable =>
            claimable::handle_claimable(&mut client).await,
        EvaluatorCommand::Forget { dispute_id } =>
            forget::handle_forget(&dispute_id).await,
        EvaluatorCommand::Stake { amount } =>
            stake::handle_stake(&mut client, &amount).await,
        EvaluatorCommand::IncreaseStake { amount } =>
            increase_stake::handle_increase_stake(&mut client, &amount).await,
        EvaluatorCommand::RequestUnstake { amount } =>
            unstake::handle_request_unstake(&mut client, &amount).await,
        EvaluatorCommand::ClaimUnstake =>
            unstake::handle_claim_unstake(&mut client).await,
        EvaluatorCommand::CancelUnstake =>
            unstake::handle_cancel_unstake(&mut client).await,
    }
}
