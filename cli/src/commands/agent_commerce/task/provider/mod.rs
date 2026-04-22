//! Provider 端任务命令 — 枚举定义 + 路由分发
//!
//! 按卖家动作划分文件：
//! - `apply.rs`             — 申请接单
//! - `deliver.rs`           — 提交交付物
//! - `agreerefund.rs`       — 同意退款
//! - `dispute_raise.rs`     — 发起仲裁（上链）
//! - `dispute_evidence.rs`  — 提交证据（上链）
//! - `dispute_info.rs`      — 查询争议详情
//!
//! 链下证据上传 (`dispute upload`) 由买卖双方共用，
//! 实现在 `common/dispute_upload.rs`。

mod agreerefund;
mod apply;
mod deliver;
mod dispute_evidence;
mod dispute_info;
mod dispute_raise;

use anyhow::Result;
use clap::Subcommand;

use crate::commands::agent_commerce::task::common::dispute_upload;
use crate::commands::agent_commerce::task::common::network::task_api_client::TaskApiClient;
use crate::commands::agent_commerce::task::signing;
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
    /// Upload offchain evidence (multipart, 1h preparation window only) — 买卖双方共用
    Upload {
        job_id: String,
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
    match cmd {
        DisputeCommand::Raise { job_id, reason } =>
            dispute_raise::handle_dispute_raise(&client, &job_id, &reason).await,
        DisputeCommand::Evidence { job_id, summary, .. } =>
            dispute_evidence::handle_dispute_evidence(&client, &job_id, &summary).await,
        DisputeCommand::Info { dispute_id } =>
            dispute_info::handle_dispute_info(&client, &dispute_id).await,
        DisputeCommand::Upload { job_id, text, images } => {
            // 自动识别角色：比对当前钱包地址和 task 的 buyer/provider 地址
            let (_, address) = signing::resolve_wallet(None, None)?;
            let url = format!("{}/priapi/v1/aieco/task/{}", client.base_url(), &job_id);
            let resp = client.get(&url).await?;
            let task = &resp["data"]["task"];
            let buyer_addr = task["buyerAgentAddress"].as_str().unwrap_or("");
            let provider_addr = task["providerAgentAddress"].as_str().unwrap_or("");
            let agent_id = if address.eq_ignore_ascii_case(buyer_addr) {
                task["buyerAgentId"].as_str().unwrap_or("").to_string()
            } else if address.eq_ignore_ascii_case(provider_addr) {
                task["providerAgentId"].as_str().unwrap_or("").to_string()
            } else {
                anyhow::bail!("当前钱包 {address} 不是任务 {job_id} 的买家或卖家")
            };
            dispute_upload::handle_upload_evidence(
                &client, &job_id, &agent_id, &address, text.as_deref(), &images,
            ).await
        }
    }
}
