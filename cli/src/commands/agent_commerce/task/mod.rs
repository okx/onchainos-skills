pub mod client;
pub mod common;
pub mod evaluator;
pub mod provider;
pub mod signing;

use anyhow::Result;
use clap::Subcommand;

use crate::commands::Context;

/// Task system top-level subcommands.
#[derive(Subcommand)]
pub enum TaskSystemCommand {
    // ── Task commands (flattened, no `task` sub-group) ──────────────────────

    /// Create a new task (Client)
    #[command(name = "create-task")]
    CreateTask {
        #[arg(long)] description: String,
        #[arg(long = "description-summary")] description_summary: Option<String>,
        #[arg(long)] budget: f64,
        #[arg(long = "max-budget")] max_budget: Option<f64>,
        #[arg(long)] currency: String,
        #[arg(long = "deadline-open")]  deadline_open: String,
        #[arg(long = "deadline-submit")] deadline_submit: String,
        #[arg(long)] title: Option<String>,
    },

    /// Get recommended providers for a task
    Recommend {
        job_id: String,
        #[arg(long)] next: bool,
        #[arg(long)] current: bool,
    },

    /// Provider fetches recommended Public tasks matching their skill
    #[command(name = "recommend-task")]
    RecommendTask {
        /// Provider agent id（可选，默认从钱包身份推导）
        #[arg(long = "agent-id")]
        agent_id: Option<String>,
    },

    /// Provider initiates contact with a buyer (xmtp, placeholder)
    #[command(name = "contact-buyer")]
    ContactBuyer {
        /// 买家 agentId
        #[arg(long = "to")]
        to_agent_id: String,
        /// 任务 ID
        #[arg(long = "job-id")]
        job_id: String,
        /// 自定义消息（可选，默认询问详情）
        #[arg(long)]
        message: Option<String>,
    },

    /// Get current task status
    Status { job_id: String },

    /// List tasks
    List {
        #[arg(long)] role: Option<String>,
        #[arg(long)] status: Option<String>,
        #[arg(long, default_value = "1")]  page: u32,
        #[arg(long, default_value = "20")] limit: u32,
    },

    /// Client confirms provider and stakes funds into escrow
    #[command(name = "confirm-accept")]
    ConfirmAccept {
        job_id: String,
        #[arg(long)] provider: String,
        #[arg(long = "payment-mode", default_value = "escrow")] payment_mode: String,
    },

    /// Client rejects provider application
    #[command(name = "reject-apply")]
    RejectApply {
        job_id: String,
        #[arg(long)] provider: String,
        #[arg(long)] reason: String,
    },


    /// Client confirms task complete and releases payment
    Complete { job_id: String },

    /// Client rejects deliverable
    Reject {
        job_id: String,
        #[arg(long)] reason: String,
    },

    /// Client closes task (only valid while Open)
    Close { job_id: String },

    /// Convert private task to public listing
    #[command(name = "set-public")]
    SetPublic { job_id: String },


    /// Provider applies for a task (apply API → sign → broadcast)
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
    #[command(name = "agree-refund")]
    AgreeRefund { job_id: String },

    /// Provider generates payment invoice after TASK_APPLIED
    Payment { job_id: String },

    /// Provider fetches on-chain payment pre-info after TASK_APPLIED (to build invoice for buyer)
    #[command(name = "get-payment")]
    GetPayment {
        job_id: String,
        /// 支付代币（USDT 或 USDG），默认 USDT
        #[arg(long = "token-symbol", default_value = "USDT")]
        token_symbol: String,
    },

    /// Client manually transfers payment to provider (non-escrow mode)
    Pay { job_id: String },

    /// Client claims refund/reward after arbitration
    Claim { job_id: String },

    /// Client claims auto-refund after seller timeout
    #[command(name = "claim-auto-refund")]
    ClaimAutoRefund { job_id: String },

    /// Task config: init | show
    Config {
        #[command(subcommand)]
        action: client::ConfigAction,
    },

    // ── Dispute (kept as sub-group) ──────────────────────────────────────────

    /// Dispute actions (provider): raise, evidence, info
    #[command(subcommand)]
    Dispute(provider::DisputeCommand),

    /// Dispute actions (buyer): evidence, info
    #[command(name = "buyer-dispute", subcommand)]
    BuyerDispute(client::BuyerDisputeCommand),

    // ── Common ───────────────────────────────────────────────────────────────

    /// Common queries: context lookup for AI agents
    #[command(subcommand)]
    Common(common::CommonCommand),

    /// Get current agent identity from ws-mock identity system (ERC-8004)
    Get {
        /// ws-mock server 地址（默认 ws://127.0.0.1:9000）
        #[arg(long, default_value = "ws://127.0.0.1:9000")]
        ws_url: String,

        /// 查询指定地址（不传则读 ~/.openclaw/ws-mock-addresses.json 中的 default）
        #[arg(long)]
        addr: Option<String>,
    },

    /// Get next-step instruction prompt for current job state (role-specific)
    #[command(name = "next-action")]
    NextAction {
        /// 任务 ID
        #[arg(long = "jobid")]
        job_id: String,

        /// 当前收到的系统通知类型（TASK_APPLIED / TASK_ACCEPTED / TASK_SUBMITTED / TASK_REFUSED / TASK_COMPLETED / TASK_DISPUTED / TASK_REJECTED / DISPUTE_RAISE / AGREE_REFUND）
        #[arg(long = "jobStatus")]
        job_status: String,

        /// 你的 agentId
        #[arg(long = "agentId")]
        agent_id: String,

        /// 角色：provider | buyer | evaluator
        #[arg(long)]
        role: String,
    },
}

pub async fn run(cmd: TaskSystemCommand, ctx: &Context) -> Result<()> {
    use client::TaskCommand as T;

    match cmd {
        TaskSystemCommand::CreateTask { description, description_summary, budget, max_budget, currency, deadline_open, deadline_submit, title } =>
            client::run_task(T::Create { description, description_summary, budget, max_budget, currency, deadline_open, deadline_submit, title }, ctx).await,

        TaskSystemCommand::Recommend { job_id, next, current } =>
            client::run_task(T::Recommend { job_id, next, current }, ctx).await,

        TaskSystemCommand::RecommendTask { agent_id } => {
            let c = common::network::task_api_client::TaskApiClient::new();
            provider::recommend_task::handle_recommend_task(&c, agent_id.as_deref()).await
        }

        TaskSystemCommand::ContactBuyer { to_agent_id, job_id, message } =>
            provider::contact_buyer::handle_contact_buyer(&to_agent_id, &job_id, message.as_deref()).await,

        TaskSystemCommand::Status { job_id } =>
            client::run_task(T::Status { job_id }, ctx).await,

        TaskSystemCommand::List { role, status, page, limit } =>
            client::run_task(T::List { role, status, page, limit }, ctx).await,

        TaskSystemCommand::ConfirmAccept { job_id, provider, payment_mode } =>
            client::run_task(T::ConfirmAccept { job_id, provider, payment_mode }, ctx).await,

        TaskSystemCommand::RejectApply { job_id, provider, reason } =>
            client::run_task(T::RejectApply { job_id, provider, reason }, ctx).await,


        TaskSystemCommand::Complete { job_id } =>
            client::run_task(T::Complete { job_id }, ctx).await,

        TaskSystemCommand::Reject { job_id, reason } =>
            client::run_task(T::Reject { job_id, reason }, ctx).await,

        TaskSystemCommand::Close { job_id } =>
            client::run_task(T::Close { job_id }, ctx).await,

        TaskSystemCommand::SetPublic { job_id } =>
            client::run_task(T::SetPublic { job_id }, ctx).await,


        TaskSystemCommand::Apply { job_id, token_amount, token_symbol, agent_id } =>
            provider::run_provider(provider::ProviderCommand::Apply { job_id, token_amount, token_symbol, agent_id }, ctx).await,

        TaskSystemCommand::Deliver { job_id, file, message } =>
            provider::run_provider(provider::ProviderCommand::Deliver { job_id, file, message }, ctx).await,

        TaskSystemCommand::AgreeRefund { job_id } =>
            provider::run_provider(provider::ProviderCommand::AgreeRefund { job_id }, ctx).await,

        TaskSystemCommand::Payment { job_id } =>
            client::run_task(T::Payment { job_id }, ctx).await,

        TaskSystemCommand::GetPayment { job_id, token_symbol } => {
            let c = common::network::task_api_client::TaskApiClient::new();
            provider::get_payment::handle_get_payment(&c, &job_id, &token_symbol).await
        }

        TaskSystemCommand::Pay { job_id } =>
            client::run_task(T::Pay { job_id }, ctx).await,

        TaskSystemCommand::Claim { job_id } =>
            client::run_task(T::Claim { job_id }, ctx).await,

        TaskSystemCommand::ClaimAutoRefund { job_id } =>
            client::run_task(T::ClaimAutoRefund { job_id }, ctx).await,


        TaskSystemCommand::Config { action } =>
            client::run_task(T::Config { action }, ctx).await,

        TaskSystemCommand::Dispute(c) =>
            provider::run_dispute(c, ctx).await,

        TaskSystemCommand::BuyerDispute(c) =>
            client::run_buyer_dispute(c, ctx).await,

        TaskSystemCommand::Common(c) =>
            common::run(c, ctx).await,

        TaskSystemCommand::Get { ws_url, addr } =>
            common::run(common::CommonCommand::Get { ws_url, addr }, ctx).await,

        TaskSystemCommand::NextAction { job_id, job_status, agent_id, role } => {
            let prompt = match role.as_str() {
                "provider" | "seller" => provider::flow::generate_next_action(&job_id, &job_status, &agent_id),
                "buyer" | "client" => client::flow::generate_next_action(&job_id, &job_status, &agent_id),
                other => anyhow::bail!("--role 暂只支持 provider/seller/buyer/client，当前: {other}"),
            };
            println!("{prompt}");
            Ok(())
        }
    }
}
