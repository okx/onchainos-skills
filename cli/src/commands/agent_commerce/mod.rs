pub mod identity;
pub mod mock_identity;
pub mod task;

use anyhow::Result;
use clap::Subcommand;

use crate::commands::Context;

/// Shared `agent` namespace for identity + task-system commands.
#[derive(Subcommand)]
pub enum AgentCommand {
    // ── Identity ────────────────────────────────────────────────────────────
    /// Register a new Agent identity
    Create(identity::CreateArgs),

    /// Update Agent identity and services
    Update(identity::UpdateArgs),

    /// Query your Agents / ws-mock identity (legacy)
    Get(identity::GetArgs),

    /// Activate an Agent
    Activate(identity::AgentStatusArgs),

    /// Deactivate an Agent
    Deactivate(identity::AgentStatusArgs),

    /// Upload an Agent avatar image
    Upload(identity::UploadArgs),

    /// Search public Agents
    Search(identity::SearchArgs),

    /// Query an Agent's services
    #[command(name = "service-list")]
    ServiceList(identity::ServiceListArgs),

    /// Submit an Agent review
    #[command(name = "feedback-submit", visible_alias = "feedbacksubmit")]
    FeedbackSubmit(identity::FeedbackSubmitArgs),

    /// Query Agent reviews
    #[command(name = "feedback-list")]
    FeedbackList(identity::FeedbackListArgs),

    /// 用 keyUuid + signing_seed 代签任意 message（xmtp 等场景），不走广播
    #[command(name = "xmtp-sign")]
    XmtpSign(identity::XmtpSignArgs),

    // ── Task system (Client) ────────────────────────────────────────────────
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

    /// Provider generates payment invoice after TASK_APPLIED
    Payment { job_id: String },

    /// Client manually transfers payment to provider (non-escrow mode)
    Pay { job_id: String },

    /// Client claims refund/reward after arbitration
    Claim { job_id: String },

    // ── Task system (Provider) ──────────────────────────────────────────────
    /// Provider fetches recommended Public tasks matching their skill
    #[command(name = "recommend-task")]
    RecommendTask {
        #[arg(long = "agent-id")]
        agent_id: Option<String>,
    },

    /// 开始接单：调 `agent get` 拉所有在线 provider agent，对每个循环 recommend-task
    #[command(name = "find-jobs")]
    FindJobs,

    /// Provider initiates contact with a buyer (xmtp, placeholder)
    #[command(name = "contact-buyer")]
    ContactBuyer {
        #[arg(long = "to")]
        to_agent_id: String,
        #[arg(long = "job-id")]
        job_id: String,
        #[arg(long)]
        message: Option<String>,
    },

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
        #[arg(long, default_value = "")] file: String,
        #[arg(long, default_value = "任务已完成，请验收")] message: String,
    },

    /// Provider agrees to refund (agreeRefund API → sign → broadcast)
    #[command(name = "agree-refund")]
    AgreeRefund { job_id: String },

    /// Provider fetches on-chain payment pre-info after TASK_APPLIED
    #[command(name = "get-payment")]
    GetPayment {
        job_id: String,
        #[arg(long = "token-symbol", default_value = "USDT")]
        token_symbol: String,
    },

    // ── Task system (sub-groups) ────────────────────────────────────────────
    /// Task config: init | show
    Config {
        #[command(subcommand)]
        action: task::client::ConfigAction,
    },

    /// Dispute actions (provider): raise, evidence, info, upload
    #[command(subcommand)]
    Dispute(task::provider::DisputeCommand),

    /// Dispute actions (buyer): evidence, info
    #[command(name = "buyer-dispute", subcommand)]
    BuyerDispute(task::client::BuyerDisputeCommand),

    /// Common queries: context lookup for AI agents
    #[command(subcommand)]
    Common(task::common::CommonCommand),

    /// Get next-step instruction prompt for current job state
    #[command(name = "next-action")]
    NextAction {
        #[arg(long = "jobid")] job_id: String,
        #[arg(long = "jobStatus")] job_status: String,
        #[arg(long = "agentId")] agent_id: String,
        #[arg(long)] role: String,
    },
}

pub async fn run(cmd: AgentCommand, ctx: &Context) -> Result<()> {
    use task::client::TaskCommand as T;

    match cmd {
        // ── Identity ────────────────────────────────────────────────
        AgentCommand::Create(args) => identity::create(args, ctx).await,
        AgentCommand::Update(args) => identity::update(args, ctx).await,
        AgentCommand::Get(args) => identity::get(args, ctx).await,
        AgentCommand::Activate(args) => identity::activate(args, ctx).await,
        AgentCommand::Deactivate(args) => identity::deactivate(args, ctx).await,
        AgentCommand::Upload(args) => identity::upload(args, ctx).await,
        AgentCommand::Search(args) => identity::search(args, ctx).await,
        AgentCommand::ServiceList(args) => identity::service_list(args, ctx).await,
        AgentCommand::FeedbackSubmit(args) => identity::feedback_submit(args, ctx).await,
        AgentCommand::FeedbackList(args) => identity::feedback_list(args, ctx).await,
        AgentCommand::XmtpSign(args) => identity::xmtp_sign(args, ctx).await,

        // ── Client (buyer) task commands ────────────────────────────
        AgentCommand::CreateTask {
            description, description_summary, budget, max_budget, currency,
            deadline_open, deadline_submit, title,
        } => task::client::run_task(
            T::Create {
                description, description_summary, budget, max_budget, currency,
                deadline_open, deadline_submit, title,
            }, ctx,
        ).await,

        AgentCommand::Recommend { job_id, next, current } =>
            task::client::run_task(T::Recommend { job_id, next, current }, ctx).await,

        AgentCommand::Status { job_id } =>
            task::client::run_task(T::Status { job_id }, ctx).await,

        AgentCommand::List { role, status, page, limit } =>
            task::client::run_task(T::List { role, status, page, limit }, ctx).await,

        AgentCommand::ConfirmAccept { job_id, provider, payment_mode } =>
            task::client::run_task(T::ConfirmAccept { job_id, provider, payment_mode }, ctx).await,

        AgentCommand::RejectApply { job_id, provider, reason } =>
            task::client::run_task(T::RejectApply { job_id, provider, reason }, ctx).await,

        AgentCommand::Complete { job_id } =>
            task::client::run_task(T::Complete { job_id }, ctx).await,

        AgentCommand::Reject { job_id, reason } =>
            task::client::run_task(T::Reject { job_id, reason }, ctx).await,

        AgentCommand::Close { job_id } =>
            task::client::run_task(T::Close { job_id }, ctx).await,

        AgentCommand::SetPublic { job_id } =>
            task::client::run_task(T::SetPublic { job_id }, ctx).await,

        AgentCommand::Payment { job_id } =>
            task::client::run_task(T::Payment { job_id }, ctx).await,

        AgentCommand::Pay { job_id } =>
            task::client::run_task(T::Pay { job_id }, ctx).await,

        AgentCommand::Claim { job_id } =>
            task::client::run_task(T::Claim { job_id }, ctx).await,

        // ── Provider task commands ──────────────────────────────────
        AgentCommand::RecommendTask { agent_id } => {
            let c = task::common::network::task_api_client::TaskApiClient::new();
            task::provider::recommend_task::handle_recommend_task(&c, agent_id.as_deref()).await
        }

        AgentCommand::FindJobs =>
            task::provider::find_jobs::handle_find_jobs().await,

        AgentCommand::ContactBuyer { to_agent_id, job_id, message } =>
            task::provider::contact_buyer::handle_contact_buyer(&to_agent_id, &job_id, message.as_deref()).await,

        AgentCommand::Apply { job_id, token_amount, token_symbol, agent_id } =>
            task::provider::run_provider(
                task::provider::ProviderCommand::Apply { job_id, token_amount, token_symbol, agent_id },
                ctx,
            ).await,

        AgentCommand::Deliver { job_id, file, message } =>
            task::provider::run_provider(
                task::provider::ProviderCommand::Deliver { job_id, file, message }, ctx,
            ).await,

        AgentCommand::AgreeRefund { job_id } =>
            task::provider::run_provider(
                task::provider::ProviderCommand::AgreeRefund { job_id }, ctx,
            ).await,

        AgentCommand::GetPayment { job_id, token_symbol } => {
            let c = task::common::network::task_api_client::TaskApiClient::new();
            task::provider::get_payment::handle_get_payment(&c, &job_id, &token_symbol).await
        }

        // ── Sub-groups ──────────────────────────────────────────────
        AgentCommand::Config { action } =>
            task::client::run_task(T::Config { action }, ctx).await,

        AgentCommand::Dispute(c) =>
            task::provider::run_dispute(c, ctx).await,

        AgentCommand::BuyerDispute(c) =>
            task::client::run_buyer_dispute(c, ctx).await,

        AgentCommand::Common(c) =>
            task::common::run(c, ctx).await,

        AgentCommand::NextAction { job_id, job_status, agent_id, role } => {
            let prompt = match role.as_str() {
                "provider" | "seller" =>
                    task::provider::flow::generate_next_action(&job_id, &job_status, &agent_id),
                "buyer" | "client" =>
                    task::client::flow::generate_next_action(&job_id, &job_status, &agent_id),
                other => anyhow::bail!("--role 必须是 provider/seller/buyer/client，当前: {other}"),
            };
            println!("{prompt}");
            Ok(())
        }
    }
}
