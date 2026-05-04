//! Provider 端任务命令 — 枚举定义 + 路由分发
//!
//! 按卖家动作划分文件：
//! - `apply.rs`             — 申请接单
//! - `deliver.rs`           — 提交交付物
//! - `agreerefund.rs`       — 同意退款
//! - `dispute_raise.rs`     — 发起仲裁（上链）
//! - `provider_claim.rs`    — submit→complete 超时领取（claimAutoComplete）
//!
//! account-pull 仲裁奖励（`claim-rewards` / `claimable`）：直接在 dispatch arm
//! 里 inline 调 `common::claim`，不再为 provider 单独写薄壳——逻辑就是
//! `signing::resolve_wallet(None, None)` + `common::claim::*`，没有角色专属解析。
//!
//! 链下证据上传 (`dispute upload`) 由买卖双方共用，
//! 实现在 `common/dispute_upload.rs`。

mod agreerefund;
mod apply;
mod deliver;
mod dispute_confirm;
mod dispute_raise;
pub mod find_jobs;
pub mod flow;
pub mod get_payment;
mod provider_claim;
pub mod recommend_task;

use anyhow::Result;
use clap::Subcommand;

use anyhow::bail;

use crate::commands::agent_commerce::task::common::{
    claim as common_claim, dispute_upload, network::task_api_client::TaskApiClient,
};
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
        /// 卖家 agentId（必填）。beta 后端拒空 agenticId header → 3001 auth fail；
        /// 任务详情里的 providerAgentId 字段可能为 null，不能依赖反查。
        #[arg(long = "agent-id")]
        agent_id: String,
    },
    /// Provider agrees to refund (agreeRefund API → sign → broadcast)
    AgreeRefund {
        job_id: String,
        /// 卖家 agentId（必填）
        #[arg(long = "agent-id")]
        agent_id: String,
    },
    /// Provider claims after submit→complete timeout (claimAutoComplete API → sign → broadcast)
    ClaimAutoComplete {
        job_id: String,
        /// 卖家 agentId（必填）
        #[arg(long = "agent-id")]
        agent_id: String,
    },
    /// Get current task status (provider view)
    Status {
        job_id: String,
        #[arg(long = "agent-id")]
        agent_id: Option<String>,
    },
    /// List my tasks (provider view)
    List {
        #[arg(long)]
        status: Option<String>,
        #[arg(long, default_value = "1")]
        page: u32,
        #[arg(long, default_value = "20")]
        limit: u32,
        #[arg(long = "agent-id")]
        agent_id: Option<String>,
    },
    /// Account-pull: 查待领奖励（仲裁胜诉等场景累积的余额）
    Claimable {
        #[arg(long = "agent-id")]
        agent_id: String,
    },
    /// Account-pull: 一次性领取所有可领奖励
    ClaimRewards {
        #[arg(long = "agent-id")]
        agent_id: String,
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
        /// 卖家 agentId（必填）
        #[arg(long = "agent-id")]
        agent_id: String,
    },
    /// 仲裁阶段 2：调用 dispute API 实际发起仲裁（calldata → sign → broadcast）。
    /// 前置必须收到 `dispute_approved` 系统通知。完成后等 `job_disputed` 通知。
    Confirm {
        job_id: String,
        /// 卖家 agentId（必填）
        #[arg(long = "agent-id")]
        agent_id: String,
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
        ProviderCommand::Deliver { job_id, file, message, agent_id } =>
            deliver::handle_deliver(&mut client, &job_id, &file, &message, &agent_id).await,
        ProviderCommand::AgreeRefund { job_id, agent_id } =>
            agreerefund::handle_agree_refund(&mut client, &job_id, &agent_id).await,
        ProviderCommand::ClaimAutoComplete { job_id, agent_id } =>
            provider_claim::handle_claim_auto_complete(&mut client, &job_id, &agent_id).await,
        ProviderCommand::Status { job_id, agent_id } => {
            use crate::commands::agent_commerce::task::common::{query as common_query, AGENT_ROLE_PROVIDER};
            common_query::handle_status(&mut client, &job_id, agent_id.as_deref().unwrap_or(""), AGENT_ROLE_PROVIDER).await
        }
        ProviderCommand::List { status, page, limit, agent_id } => {
            use crate::commands::agent_commerce::task::common::{query as common_query, AGENT_ROLE_PROVIDER};
            common_query::handle_list(&mut client, status.as_deref(), page, limit, agent_id.as_deref().unwrap_or(""), AGENT_ROLE_PROVIDER).await
        }

        // account-pull claim 直接 inline 调 common::claim：
        // provider 没有 role-specific 的 wallet/agent 解析（不像 evaluator），
        // 不需要单独的 wrapper 文件。
        ProviderCommand::Claimable { agent_id } => {
            if agent_id.is_empty() {
                bail!("--agent-id 必填，传卖家自己的 agentId（beta 后端拒空 agenticId header）");
            }
            let (_account_id, address) = signing::resolve_wallet(None, None)?;
            let has_nonzero =
                common_claim::fetch_and_print_claimable(&mut client, &agent_id, &address).await?;
            if has_nonzero {
                println!("\nnext: 有可领奖励 — 跑 `onchainos agent provider-claim-rewards --agent-id {agent_id}` 一次性提走。");
            } else {
                println!("\n(当前无待领奖励)");
            }
            Ok(())
        }
        ProviderCommand::ClaimRewards { agent_id } => {
            if agent_id.is_empty() {
                bail!("--agent-id 必填，传卖家自己的 agentId（beta 后端拒空 agenticId header）");
            }
            let (account_id, address) = signing::resolve_wallet(None, None)?;
            let tx_hash =
                common_claim::submit_claim_and_broadcast(&mut client, &account_id, &address, &agent_id).await?;
            println!("✓ reward claim submitted (account={address})");
            println!("  txHash: {tx_hash}");
            println!("note: 一次性领取所有已结算争议的奖励，到账金额会在链上确认后通知。");
            Ok(())
        }
    }
}

pub async fn run_dispute(cmd: DisputeCommand, _ctx: &Context) -> Result<()> {
    let mut client = TaskApiClient::new();
    match cmd {
        DisputeCommand::Raise { job_id, reason, agent_id } =>
            dispute_raise::handle_dispute_raise(&mut client, &job_id, &reason, &agent_id).await,
        DisputeCommand::Confirm { job_id, agent_id } =>
            dispute_confirm::handle_dispute_confirm(&mut client, &job_id, &agent_id).await,
        DisputeCommand::Upload { job_id, agent_id, text, images } =>
            dispute_upload::handle_upload_evidence(
                &mut client, &job_id, &agent_id, text.as_deref(), &images,
            ).await,
    }
}
