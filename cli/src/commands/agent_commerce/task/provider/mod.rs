//! Provider 端任务命令 — 枚举定义 + 路由分发
//!
//! 按卖家动作划分文件：
//! - `apply.rs`       — 申请接单
//! - `deliver.rs`     — 提交交付物
//! - `agreerefund.rs` — 同意退款
//! - `evidence.rs`    — 仲裁：发起仲裁、提交证据、查询争议

mod agreerefund;
mod apply;
mod deliver;
mod evidence;

use anyhow::Result;
use clap::Subcommand;

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
        #[arg(long = "token-symbol", default_value = "USDT")]
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
}

// ─── dispute subcommands ──────────────────────────────────────────────────

#[derive(Subcommand)]
pub enum DisputeCommand {
    /// Raise a dispute (dispute API → calldata → sign → broadcast)
    Raise {
        job_id: String,
        #[arg(long)]
        reason: String,
    },
    /// Either party submits evidence during dispute
    Evidence {
        job_id: String,
        #[arg(long)]
        summary: String,
        #[arg(long)]
        file: Option<String>,
        #[arg(long = "type")]
        evidence_type: Option<String>,
    },
    /// Retrieves dispute details
    Info {
        dispute_id: String,
    },
}

// ─── 路由分发 ─────────────────────────────────────────────────────────────

pub async fn run_provider(cmd: ProviderCommand, _ctx: &Context) -> Result<()> {
    let client = TaskApiClient::new();

    match cmd {
        ProviderCommand::Apply { job_id, token_amount, token_symbol, agent_id } =>
            apply::handle_apply(&client, &job_id, &token_amount, &token_symbol, &agent_id).await,
        ProviderCommand::Deliver { job_id, file, message } =>
            deliver::handle_deliver(&client, &job_id, &file, &message).await,
        ProviderCommand::AgreeRefund { job_id } =>
            agreerefund::handle_agree_refund(&client, &job_id).await,
    }
}

pub async fn run_dispute(cmd: DisputeCommand, _ctx: &Context) -> Result<()> {
    let client = TaskApiClient::new();
    evidence::run_evidence(cmd, &client).await
}
