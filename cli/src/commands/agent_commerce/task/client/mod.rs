//! Client 端任务命令 — 枚举定义 + 路由分发
//!
//! 按买家动作划分文件：
//! - `create.rs`       — 发布任务（场景1）
//! - `recommend.rs`    — 获取推荐卖家（场景1）
//! - `negotiate.rs`    — 协商（场景2，Agent 子 session）
//! - `accept.rs`       — 确认接单 + Fund（场景3）
//! - `complete.rs`     — 确认完成（场景5）
//! - `refuse.rs`       — 拒绝交付物（场景6）
//! - `close.rs`        — 关单（场景7）+ 领取仲裁奖金
//! - `changepublic.rs` — 设为 Public（场景8）
//! - `judge.rs`        — 评价卖家（场景9，身份系统 CLI）
//!
//! 通用：
//! - `query.rs`        — 只读查询（status、list、pay）

mod accept;
mod changepublic;
mod close;
mod complete;
mod create;
mod evidence;
mod judge;
mod negotiate;
mod query;
mod recommend;
mod refuse;
mod x402_flow;

use anyhow::Result;
use clap::Subcommand;

use crate::commands::agent_commerce::task::common::network::task_api_client::TaskApiClient;
use crate::commands::agent_commerce::task::common::PAYMENT_MODE_ESCROW;
use crate::commands::Context;

// ─── task subcommands ──────────────────────────────────────────────────────

#[derive(Subcommand)]
pub enum TaskCommand {
    /// Create a new task (Client only)
    Create {
        #[arg(long)]
        description: String,
        #[arg(long = "description-summary")]
        description_summary: Option<String>,
        #[arg(long)]
        budget: f64,
        #[arg(long = "max-budget")]
        max_budget: Option<f64>,
        #[arg(long)]
        currency: String,
        #[arg(long = "deadline-open")]
        deadline_open: String,
        #[arg(long = "deadline-submit")]
        deadline_submit: String,
        #[arg(long)]
        title: Option<String>,
    },
    /// Get recommended providers for a task
    Recommend {
        job_id: String,
        /// Show next provider (advance index) from cached list
        #[arg(long)]
        next: bool,
        /// Show current provider from cached list
        #[arg(long)]
        current: bool,
    },
    /// Get current task status
    Status {
        job_id: String,
    },
    /// List tasks
    List {
        #[arg(long)]
        role: Option<String>,
        #[arg(long)]
        status: Option<String>,
        #[arg(long, default_value = "1")]
        page: u32,
        #[arg(long, default_value = "20")]
        limit: u32,
    },
    /// Client confirms provider and stakes funds into escrow
    ConfirmAccept {
        job_id: String,
        #[arg(long)]
        provider: String,
        #[arg(long = "payment-mode", default_value = PAYMENT_MODE_ESCROW)]
        payment_mode: String,
    },
    /// Client rejects provider application
    RejectApply {
        job_id: String,
        #[arg(long)]
        provider: String,
        #[arg(long)]
        reason: String,
    },
    /// Client confirms task complete and releases payment
    Complete {
        job_id: String,
    },
    /// Client rejects deliverable
    Reject {
        job_id: String,
        #[arg(long)]
        reason: String,
    },
    /// Client closes task (only valid while Open)
    Close {
        job_id: String,
    },
    /// Client converts private task to public listing
    SetPublic {
        job_id: String,
    },
    /// Provider generates payment invoice after TASK_APPLIED
    Payment {
        job_id: String,
    },
    /// Client manually transfers payment to provider (non-escrow mode)
    Pay {
        job_id: String,
    },
    /// Client claims refund/reward after arbitration
    Claim {
        job_id: String,
    },
    /// Rate the provider after task completion
    Judge {
        job_id: String,
    },
    /// Initialize config
    Config {
        #[command(subcommand)]
        action: ConfigAction,
    },
}

#[derive(Subcommand)]
pub enum ConfigAction {
    /// Initialize configuration
    Init,
    /// Show current configuration
    Show,
}

// ─── buyer dispute subcommands ────────────────────────────────────────────

#[derive(Subcommand)]
pub enum BuyerDisputeCommand {
    /// Buyer submits evidence during dispute
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

// ─── 路由分发 ──────────────────────────────────────────────────────────────

pub async fn run_task(cmd: TaskCommand, _ctx: &Context) -> Result<()> {
    let client = TaskApiClient::new();

    match cmd {
        // ── 买家动作 ─────────────────────────────────────────────
        TaskCommand::Create { description, description_summary, budget, max_budget, currency, deadline_open, deadline_submit, title } =>
            create::handle_create(&client, description, description_summary, budget, max_budget, currency, deadline_open, deadline_submit, title).await,
        TaskCommand::Recommend { job_id, next, current } => {
            if next {
                recommend::handle_recommend_next(&job_id)
            } else if current {
                recommend::handle_recommend_current(&job_id)
            } else {
                recommend::handle_recommend(&client, &job_id).await
            }
        }
        TaskCommand::ConfirmAccept { job_id, provider, payment_mode } =>
            accept::handle_confirm_accept(&client, &job_id, &provider, &payment_mode).await,
        TaskCommand::Complete { job_id } =>
            complete::handle_complete(&client, &job_id).await,
        TaskCommand::Reject { job_id, reason } =>
            refuse::handle_reject(&client, &job_id, &reason).await,
        TaskCommand::Close { job_id } =>
            close::handle_close(&client, &job_id).await,
        TaskCommand::SetPublic { job_id } =>
            changepublic::handle_set_public(&client, &job_id).await,
        TaskCommand::Claim { job_id } =>
            close::handle_claim(&client, &job_id).await,
        TaskCommand::Judge { job_id } =>
            judge::handle_judge(&client, &job_id).await,

        // ── 只读查询 ─────────────────────────────────────────────
        TaskCommand::Status { job_id } =>
            query::handle_status(&client, &job_id).await,
        TaskCommand::List { role, status, page, limit } =>
            query::handle_list(&client, role.as_deref(), status.as_deref(), page, limit).await,
        TaskCommand::Payment { job_id } =>
            query::handle_payment(&client, &job_id).await,
        TaskCommand::Pay { job_id } =>
            query::handle_pay(&client, &job_id).await,

        // ── 占位实现 ─────────────────────────────────────────────
        TaskCommand::RejectApply { job_id, provider, reason } => {
            println!("[TODO] reject-apply {job_id} provider={provider} reason={reason} — 待确认需求");
            Ok(())
        }
        TaskCommand::Config { action } => {
            match action {
                ConfigAction::Init => println!("[stub] task config init"),
                ConfigAction::Show => println!("TASK_API_URL={}", client.base_url()),
            }
            Ok(())
        }
    }
}

pub async fn run_buyer_dispute(cmd: BuyerDisputeCommand, _ctx: &Context) -> Result<()> {
    let client = TaskApiClient::new();
    evidence::run_buyer_dispute(cmd, &client).await
}
