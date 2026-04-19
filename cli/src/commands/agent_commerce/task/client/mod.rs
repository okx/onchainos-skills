//! Client 端任务命令 — 枚举定义 + 路由分发
//!
//! 业务实现按请求类型拆分：
//! - `query.rs`     — 只读查询（无签名）
//! - `onchain.rs`   — 签名写操作（单签/双签）
//! - `dispute.rs`   — 仲裁相关
//!
//! 协商（negotiate）在子 session 中由 Agent 自然语言完成，
//! 通信模块自动转发，不需要 CLI 命令。

mod dispute;
mod query;
mod onchain;

use anyhow::Result;
use clap::Subcommand;

use crate::commands::agent_commerce::task::common::PAYMENT_MODE_ESCROW;
use crate::commands::Context;

// ─── 公共函数 ────────────────────────────────────────────────────────────

fn task_api_url() -> String {
    std::env::var("TASK_API_URL").unwrap_or_else(|_| "http://127.0.0.1:9001".to_string())
}

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
    /// Provider confirms on-chain acceptance (apply + single-sign)
    Confirm {
        job_id: String,
        /// Negotiated tokenAmount (0 = accept original price, >0 = counter-offer)
        #[arg(long = "token-amount", default_value = "0")]
        token_amount: String,
        /// Token symbol, e.g. USDT or USDG (default: read from task)
        #[arg(long = "token-symbol")]
        token_symbol: Option<String>,
        /// Provider agent ID (fallback: env AGENT_ID)
        #[arg(long = "agent-id")]
        agent_id: Option<String>,
    },
    /// Provider submits deliverable
    Deliver {
        job_id: String,
        #[arg(long)]
        file: String,
        #[arg(long)]
        message: Option<String>,
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
    /// Provider applies for a public task
    Apply {
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

// ─── dispute subcommands ───────────────────────────────────────────────────

#[derive(Subcommand)]
pub enum DisputeCommand {
    /// Provider raises a dispute after client rejects deliverable
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
    /// Evaluator retrieves dispute details
    Info {
        dispute_id: String,
    },
    /// Evaluator votes on dispute outcome
    Vote {
        dispute_id: String,
        #[arg(long)]
        side: u8,
        #[arg(long)]
        reason: String,
    },
    /// Either party appeals the arbitration result
    Appeal {
        job_id: String,
        #[arg(long)]
        reason: String,
    },
}

// ─── 路由分发 ──────────────────────────────────────────────────────────────

pub async fn run_task(cmd: TaskCommand, _ctx: &Context) -> Result<()> {
    let api = task_api_url();
    let http = reqwest::Client::new();

    match cmd {
        // ── 只读查询 → query.rs ──────────────────────────────────────
        TaskCommand::Recommend { job_id } =>
            query::handle_recommend(&http, &api, &job_id).await,
        TaskCommand::Status { job_id } =>
            query::handle_status(&http, &api, &job_id).await,
        TaskCommand::List { role, status, page, limit } =>
            query::handle_list(&http, &api, role.as_deref(), status.as_deref(), page, limit).await,
        TaskCommand::Pay { job_id } =>
            query::handle_pay(&http, &api, &job_id).await,

        // ── 签名写操作 → write.rs ────────────────────────────────────
        TaskCommand::Create { description, description_summary, budget, max_budget, currency, deadline_open, deadline_submit, title } =>
            onchain::handle_create(&http, &api, description, description_summary, budget, max_budget, currency, deadline_open, deadline_submit, title).await,
        TaskCommand::ConfirmAccept { job_id, provider, payment_mode } =>
            onchain::handle_confirm_accept(&http, &api, &job_id, &provider, &payment_mode).await,
        TaskCommand::Complete { job_id } =>
            onchain::handle_complete(&http, &api, &job_id).await,
        TaskCommand::Reject { job_id, reason } =>
            onchain::handle_reject(&http, &api, &job_id, &reason).await,
        TaskCommand::Close { job_id } =>
            onchain::handle_close(&http, &api, &job_id).await,
        TaskCommand::SetPublic { job_id } =>
            onchain::handle_set_public(&http, &api, &job_id).await,
        TaskCommand::Claim { job_id } =>
            onchain::handle_claim(&http, &api, &job_id).await,
        TaskCommand::Apply { job_id } =>
            onchain::handle_apply(&http, &api, &job_id).await,

        // ── 占位实现 ─────────────────────────────────────────────────
        // 【待确认】Scene 3 C8: Client 拒绝 Provider 接单申请
        TaskCommand::RejectApply { job_id, provider, reason } => {
            println!("[TODO] reject-apply {job_id} provider={provider} reason={reason} — 待确认需求");
            Ok(())
        }
        TaskCommand::Confirm { job_id, token_amount, token_symbol, agent_id } =>
            onchain::handle_confirm(&http, &api, &job_id, &token_amount, token_symbol.as_deref(), agent_id.as_deref()).await,
        // TODO(provider): 实现文件上传 + submit 签名流程
        TaskCommand::Deliver { job_id, file, message } => {
            println!("[TODO(provider)] deliver {job_id} file={file} msg={message:?}");
            Ok(())
        }
        TaskCommand::Config { action } => {
            match action {
                ConfigAction::Init => println!("[stub] task config init"),
                ConfigAction::Show => println!("TASK_API_URL={}", task_api_url()),
            }
            Ok(())
        }
    }
}

pub async fn run_dispute(cmd: DisputeCommand, ctx: &Context) -> Result<()> {
    dispute::run_dispute(cmd, ctx).await
}
