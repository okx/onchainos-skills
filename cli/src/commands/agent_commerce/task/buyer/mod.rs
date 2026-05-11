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
pub(crate) mod negotiate;
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
        max_budget: f64,
        #[arg(long)]
        currency: String,
        #[arg(long = "deadline-open")]
        deadline_open: String,
        #[arg(long = "deadline-submit")]
        deadline_submit: String,
        #[arg(long)]
        title: Option<String>,
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
    /// Set payment mode on-chain (standalone, before confirm-accept)
    SetPaymentMode {
        job_id: String,
        /// escrow / non_escrow / x402
        #[arg(long = "payment-mode")]
        payment_mode: Option<String>,
        #[arg(long = "token-symbol")]
        token_symbol: Option<String>,
        #[arg(long = "token-amount")]
        token_amount: Option<String>,
        /// x402 服务端点 URL（不指定时从 recommend 缓存或 service-list API 获取）
        #[arg(long)]
        endpoint: Option<String>,
    },
    /// Client confirms provider and executes payment (setPaymentMode must be done first)
    ConfirmAccept {
        job_id: String,
        #[arg(long = "provider-agent-id")]
        provider_agent_id: String,
        /// 不指定时自动从任务详情 paymentType 获取
        #[arg(long = "payment-mode")]
        payment_mode: Option<String>,
        /// 协商确定的支付代币符号（如 USDT），escrow 必填
        #[arg(long = "token-symbol")]
        token_symbol: Option<String>,
        /// 协商确定的支付金额（人类可读，如 "50"），escrow 必填
        #[arg(long = "token-amount")]
        token_amount: Option<String>,
    },
    /// Client confirms task complete and releases payment
    Complete {
        job_id: String,
        /// a2a_pay payment_id（卖家通过 XMTP 传递，non_escrow 必填）
        #[arg(long = "payment-id")]
        payment_id: Option<String>,
        /// 支付代币符号（non_escrow 需要，如 USDT）
        #[arg(long = "token-symbol")]
        token_symbol: Option<String>,
        /// 支付金额（non_escrow 需要，人类可读格式如 "50"）
        #[arg(long = "token-amount")]
        token_amount: Option<String>,
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
        #[arg(long = "agent-id")]
        agent_id: Option<String>,
    },
    /// Client converts private task to public listing
    SetPublic {
        job_id: String,
        #[arg(long = "agent-id")]
        agent_id: Option<String>,
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
    /// Client claims auto-refund after seller timeout (submit_expired / refuse_expired)
    ClaimAutoRefund {
        job_id: String,
    },
    /// x402 Phase 2b: direct/accept after job_payment_mode_changed + x402 endpoint interaction
    DirectAccept {
        job_id: String,
        #[arg(long = "provider-agent-id")]
        provider_agent_id: String,
        #[arg(long = "token-symbol")]
        token_symbol: Option<String>,
        #[arg(long = "token-amount")]
        token_amount: Option<String>,
    },
    /// x402 Phase 2: x402_pay signing + direct/accept + endpoint replay.
    /// Returns replay result (deliverable) and Payment Credential.
    Task402Pay {
        job_id: String,
        #[arg(long = "provider-agent-id")]
        provider_agent_id: String,
        /// JSON accepts array from the HTTP 402 response
        #[arg(long)]
        accepts: String,
        /// x402 provider endpoint URL (for replay after signing)
        #[arg(long)]
        endpoint: String,
        #[arg(long = "token-symbol")]
        token_symbol: String,
        #[arg(long = "token-amount")]
        token_amount: String,
        /// Payer address (optional, defaults to selected account)
        #[arg(long)]
        from: Option<String>,
    },
    /// Validate an x402 endpoint and extract pricing info
    X402Check {
        /// x402 provider endpoint URL
        #[arg(long)]
        endpoint: String,
    },
    /// Save negotiated payment params locally (agent calls after negotiation)
    SaveAgreed {
        job_id: String,
        #[arg(long = "provider")]
        provider_agent_id: String,
        #[arg(long = "token-symbol")]
        token_symbol: String,
        #[arg(long = "token-amount")]
        token_amount: String,
        #[arg(long = "agent-id")]
        agent_id: Option<String>,
    },
}

// ─── 路由分发 ──────────────────────────────────────────────────────────────

pub async fn run_task(cmd: TaskCommand, _ctx: &Context) -> Result<()> {
    let mut client = TaskApiClient::new();

    match cmd {
        // ── 买家动作 ─────────────────────────────────────────────
        TaskCommand::Create { description, description_summary, budget, max_budget, currency, deadline_open, deadline_submit, title, agent_id } =>
            create::handle_create(&mut client, create::CreateTaskParams {
                description, description_summary, budget, max_budget, currency,
                deadline_open, deadline_submit, title, agent_id,
            }).await,
        TaskCommand::Recommend { job_id, agent_id, next, current } => {
            if next {
                recommend::handle_recommend_next(&job_id)
            } else if current {
                recommend::handle_recommend_current(&job_id)
            } else {
                recommend::handle_recommend(&mut client, &job_id, agent_id.as_deref().unwrap_or("")).await
            }
        }
        TaskCommand::SetPaymentMode { job_id, payment_mode, token_symbol, token_amount, endpoint } =>
            accept::handle_set_payment_mode(&mut client, &job_id, payment_mode.as_deref(), token_symbol.as_deref(), token_amount.as_deref(), endpoint.as_deref()).await,
        TaskCommand::ConfirmAccept { job_id, provider_agent_id, payment_mode, token_symbol, token_amount } =>
            accept::handle_confirm_accept(&mut client, &job_id, &provider_agent_id, payment_mode.as_deref(), token_symbol.as_deref(), token_amount.as_deref()).await,
        TaskCommand::DirectAccept { job_id, provider_agent_id, token_symbol, token_amount } =>
            accept::handle_direct_accept(&mut client, &job_id, &provider_agent_id, token_symbol.as_deref(), token_amount.as_deref()).await,
        TaskCommand::Task402Pay { job_id, provider_agent_id, accepts, endpoint, token_symbol, token_amount, from } =>
            accept::handle_task_402_pay(&mut client, &job_id, &provider_agent_id, &accepts, &endpoint, &token_symbol, &token_amount, from.as_deref()).await,
        TaskCommand::X402Check { endpoint } =>
            accept::handle_x402_check(&mut client, &endpoint).await,
        TaskCommand::Complete { job_id, payment_id, token_symbol, token_amount } =>
            complete::handle_complete(&mut client, &job_id, payment_id.as_deref(), token_symbol.as_deref(), token_amount.as_deref()).await,
        TaskCommand::Reject { job_id, reason } =>
            refuse::handle_reject(&mut client, &job_id, &reason).await,
        TaskCommand::Close { job_id, agent_id } =>
            close::handle_close(&mut client, &job_id, agent_id.as_deref()).await,
        TaskCommand::SetPublic { job_id, agent_id } =>
            changepublic::handle_set_public(&mut client, &job_id, agent_id.as_deref()).await,
        TaskCommand::ClaimAutoRefund { job_id } =>
            claim_auto_refund::handle_claim_auto_refund(&mut client, &job_id).await,
        TaskCommand::SaveAgreed { job_id, provider_agent_id, token_symbol, token_amount, agent_id } => {
            negotiate::save_agreed(&mut client, &job_id, &provider_agent_id, &token_symbol, &token_amount, agent_id.as_deref()).await
        }

        // ── 只读查询 ─────────────────────────────────────────────
        TaskCommand::Payment { job_id, agent_id } =>
            query::handle_payment(&mut client, &job_id, agent_id.as_deref().unwrap_or("")).await,
        TaskCommand::Pay { job_id, agent_id } =>
            query::handle_pay(&mut client, &job_id, agent_id.as_deref().unwrap_or("")).await,

    }
}

