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
mod claim_auto_refund;
mod close;
mod complete;
mod create;
pub mod flow;
mod judge;
mod negotiate;
mod query;
mod recommend;
mod refuse;
mod x402_flow;

use anyhow::Result;
use clap::Subcommand;

use crate::commands::agent_commerce::task::common::network::task_api_client::TaskApiClient;
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
        /// 支付方式: escrow(担保) / non_escrow(非担保) / x402（不指定则为"未设置"）
        #[arg(long = "payment-mode")]
        payment_mode: Option<String>,
        /// Buyer agent ID（多 buyer 时必传，单 buyer 时自动选择）
        #[arg(long = "agent-id")]
        agent_id: Option<String>,
    },
    /// Get recommended providers for a task
    Recommend {
        job_id: String,
        /// Agent identity (agenticId header)
        #[arg(long = "agent-id")]
        agent_id: Option<String>,
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
        #[arg(long = "agent-id")]
        agent_id: Option<String>,
    },
    /// List my tasks
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
    /// Client confirms provider and stakes funds into escrow
    ConfirmAccept {
        job_id: String,
        #[arg(long)]
        provider: String,
        /// 不指定时自动从任务详情 paymentType 获取
        #[arg(long = "payment-mode")]
        payment_mode: Option<String>,
        /// a2a_pay payment_id（卖家通过 XMTP 传递，non_escrow 必填；escrow 不需要）
        #[arg(long = "payment-id")]
        payment_id: Option<String>,
        /// 协商确定的支付代币符号（如 USDT），escrow 必填
        #[arg(long = "token-symbol")]
        token_symbol: Option<String>,
        /// 协商确定的支付金额（人类可读，如 "50"），escrow 必填
        #[arg(long = "token-amount")]
        token_amount: Option<String>,
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
    /// Provider generates payment invoice after provider_applied
    Payment {
        job_id: String,
        #[arg(long = "agent-id")]
        agent_id: Option<String>,
    },
    /// Client manually transfers payment to provider (non-escrow mode)
    Pay {
        job_id: String,
        #[arg(long = "agent-id")]
        agent_id: Option<String>,
    },
    /// Client claims refund/reward after arbitration
    Claim {
        job_id: String,
    },
    /// Client claims auto-refund after seller timeout (submit_expired / refuse_expired)
    ClaimAutoRefund {
        job_id: String,
    },
    /// Save negotiated payment params locally (agent calls after negotiation)
    SaveAgreed {
        job_id: String,
        #[arg(long = "token-symbol")]
        token_symbol: String,
        #[arg(long = "token-amount")]
        token_amount: String,
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

// ─── 路由分发 ──────────────────────────────────────────────────────────────

pub async fn run_task(cmd: TaskCommand, _ctx: &Context) -> Result<()> {
    let mut client = TaskApiClient::new();

    match cmd {
        // ── 买家动作 ─────────────────────────────────────────────
        TaskCommand::Create { description, description_summary, budget, max_budget, currency, deadline_open, deadline_submit, title, payment_mode, agent_id } =>
            create::handle_create(&mut client, description, description_summary, budget, max_budget, currency, deadline_open, deadline_submit, title, payment_mode, agent_id).await,
        TaskCommand::Recommend { job_id, agent_id, next, current } => {
            if next {
                recommend::handle_recommend_next(&job_id)
            } else if current {
                recommend::handle_recommend_current(&job_id)
            } else {
                recommend::handle_recommend(&mut client, &job_id, agent_id.as_deref().unwrap_or("")).await
            }
        }
        TaskCommand::ConfirmAccept { job_id, provider, payment_mode, payment_id, token_symbol, token_amount } =>
            accept::handle_confirm_accept(&mut client, &job_id, &provider, payment_mode.as_deref(), payment_id.as_deref(), token_symbol.as_deref(), token_amount.as_deref()).await,
        TaskCommand::Complete { job_id } =>
            complete::handle_complete(&mut client, &job_id).await,
        TaskCommand::Reject { job_id, reason } =>
            refuse::handle_reject(&mut client, &job_id, &reason).await,
        TaskCommand::Close { job_id } =>
            close::handle_close(&mut client, &job_id).await,
        TaskCommand::SetPublic { job_id } =>
            changepublic::handle_set_public(&mut client, &job_id).await,
        TaskCommand::Claim { job_id } =>
            close::handle_claim(&mut client, &job_id).await,
        TaskCommand::ClaimAutoRefund { job_id } =>
            claim_auto_refund::handle_claim_auto_refund(&mut client, &job_id).await,
        TaskCommand::SaveAgreed { job_id, token_symbol, token_amount } => {
            negotiate::save_agreed(&job_id, &token_symbol, &token_amount)
        }
        TaskCommand::Judge { job_id } =>
            judge::handle_judge(&mut client, &job_id).await,

        // ── 只读查询 ─────────────────────────────────────────────
        TaskCommand::Status { job_id, agent_id } =>
            query::handle_status(&mut client, &job_id, agent_id.as_deref().unwrap_or("")).await,
        TaskCommand::List { status, page, limit, agent_id } =>
            query::handle_list(&mut client, status.as_deref(), page, limit, agent_id.as_deref().unwrap_or("")).await,
        TaskCommand::Payment { job_id, agent_id } =>
            query::handle_payment(&mut client, &job_id, agent_id.as_deref().unwrap_or("")).await,
        TaskCommand::Pay { job_id, agent_id } =>
            query::handle_pay(&mut client, &job_id, agent_id.as_deref().unwrap_or("")).await,

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

