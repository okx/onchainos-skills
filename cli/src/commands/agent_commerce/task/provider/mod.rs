//! Provider 端任务命令 — 枚举定义 + 路由分发
//!
//! 按卖家动作划分文件：
//! - `apply.rs`             — 申请接单
//! - `deliver.rs`           — 提交交付物
//! - `agreerefund.rs`       — 同意退款
//! - `dispute_raise.rs`     — 发起仲裁（上链）
//! - `dispute_info.rs`      — 查询争议详情
//! - `provider_claim.rs`    — submit→complete 超时领取（claimAutoComplete）
//!
//! 链下证据上传 (`dispute upload`) 由买卖双方共用，
//! 实现在 `common/dispute_upload.rs`。

mod agreerefund;
mod apply;
pub mod contact_buyer;
mod deliver;
mod dispute_info;
mod dispute_confirm;
mod dispute_raise;
pub mod find_jobs;
pub mod flow;
pub mod get_payment;
mod provider_claim;
pub mod recommend_task;

use anyhow::Result;
use clap::Subcommand;

use crate::commands::agent_commerce::task::common::dispute_upload;
use crate::commands::agent_commerce::task::common::network::task_api_client::TaskApiClient;
use crate::commands::Context;

// ─── provider subcommands ─────────────────────────────────────────────────

#[derive(Subcommand)]
pub enum ProviderCommand {
    /// Provider applies for a task (apply API → calldata → sign → broadcast)
    Apply {
        job_id: String,
        #[arg(long = "token-amount", default_value = "0")]
        token_amount: String,
        /// 任务实际币种（USDT / USDG），从任务详情读取，不要假设 USDT
        #[arg(long = "token-symbol")]
        token_symbol: String,
        #[arg(long = "agent-id")]
        agent_id: String,
    },
    /// Provider submits deliverable (submit API → sign → broadcast)
    Deliver {
        job_id: String,
        #[arg(long, default_value = "")]
        file: String,
        #[arg(long, default_value = "任务已完成，请验收")]
        message: String,
    },
    /// Provider agrees to refund (agreeRefund API → sign → broadcast)
    AgreeRefund {
        job_id: String,
    },
    /// Provider claims after submit→complete timeout (claimAutoComplete API → sign → broadcast)
    ClaimAutoComplete {
        job_id: String,
    },
}

// ─── dispute subcommands ──────────────────────────────────────────────────

#[derive(Subcommand)]
pub enum DisputeCommand {
    /// 仲裁阶段 1：调用 approve API 给 dispute 合约 token 授权（calldata → sign → broadcast）。
    /// 完成后等链上 `dispute_approved` 通知，再走 `dispute confirm` 跑阶段 2。
    Raise {
        job_id: String,
        #[arg(long)]
        reason: String,
    },
    /// 仲裁阶段 2：调用 dispute API 实际发起仲裁（calldata → sign → broadcast）。
    /// 前置必须收到 `dispute_approved` 系统通知。完成后等 `job_disputed` 通知。
    Confirm {
        job_id: String,
    },
    /// Retrieves dispute details
    Info {
        dispute_id: String,
        #[arg(long = "agent-id")]
        agent_id: Option<String>,
    },
    /// Upload offchain evidence (multipart, 1h preparation window only) — 买卖双方共用
    Upload {
        job_id: String,
        /// 调用方自己的 agentId（buyer 或 provider）。由 next-action 剧本注入，避免
        /// 客户端再做钱包-角色映射（同一钱包可能注册多个 agentId）
        #[arg(long = "agent-id")]
        agent_id: String,
        /// 文本证据（可选，text/images 至少一项）
        #[arg(long)]
        text: Option<String>,
        /// 图片路径（可重复，仅支持 jpg/jpeg/png/gif/webp）
        #[arg(long = "image")]
        images: Vec<String>,
    },
}

// ─── 路由分发 ─────────────────────────────────────────────────────────────

pub async fn run_provider(cmd: ProviderCommand, _ctx: &Context) -> Result<()> {
    let mut client = TaskApiClient::new();

    match cmd {
        ProviderCommand::Apply { job_id, token_amount, token_symbol, agent_id } =>
            apply::handle_apply(&mut client, &job_id, &token_amount, &token_symbol, &agent_id).await,
        ProviderCommand::Deliver { job_id, file, message } =>
            deliver::handle_deliver(&mut client, &job_id, &file, &message).await,
        ProviderCommand::AgreeRefund { job_id } =>
            agreerefund::handle_agree_refund(&mut client, &job_id).await,
        ProviderCommand::ClaimAutoComplete { job_id } =>
            provider_claim::handle_claim_auto_complete(&mut client, &job_id).await,
    }
}

pub async fn run_dispute(cmd: DisputeCommand, _ctx: &Context) -> Result<()> {
    let mut client = TaskApiClient::new();
    match cmd {
        DisputeCommand::Raise { job_id, reason } =>
            dispute_raise::handle_dispute_raise(&mut client, &job_id, &reason).await,
        DisputeCommand::Confirm { job_id } =>
            dispute_confirm::handle_dispute_confirm(&mut client, &job_id).await,
        DisputeCommand::Info { dispute_id, agent_id } =>
            dispute_info::handle_dispute_info(&mut client, &dispute_id, agent_id.as_deref().unwrap_or("")).await,
        DisputeCommand::Upload { job_id, agent_id, text, images } =>
            dispute_upload::handle_upload_evidence(
                &mut client, &job_id, &agent_id, text.as_deref(), &images,
            ).await,
    }
}
