pub mod identity;
pub mod task;

use anyhow::Result;
use clap::Subcommand;

use crate::commands::Context;

/// Shared `agent` namespace for task-system and identity commands.
#[derive(Subcommand)]
pub enum AgentCommand {
    // Identity
    /// Register a new Agent identity
    Create(identity::CreateArgs),

    /// Update Agent identity and services
    Update(identity::UpdateArgs),

    /// Query your Agents
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

    // Task system
    /// Create a new task (Client)
    #[command(name = "create-task")]
    CreateTask {
        #[arg(long)]
        description: String,
        #[arg(long)]
        budget: f64,
        #[arg(long)]
        currency: String,
        #[arg(long = "deadline-open")]
        deadline_open: String,
        #[arg(long = "deadline-submit")]
        deadline_submit: String,
        #[arg(long = "quality-standards")]
        quality_standards: String,
        #[arg(long)]
        title: Option<String>,
    },

    /// Get recommended providers for a task
    Recommend { job_id: String },

    /// Get current task status
    Status { job_id: String },

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
    #[command(name = "confirm-accept")]
    ConfirmAccept {
        job_id: String,
        #[arg(long)]
        provider: String,
    },

    /// Client rejects provider application
    #[command(name = "reject-apply")]
    RejectApply {
        job_id: String,
        #[arg(long)]
        provider: String,
        #[arg(long)]
        reason: String,
    },

    /// Provider confirms on-chain acceptance
    Confirm { job_id: String },

    /// Provider submits deliverable
    Deliver {
        job_id: String,
        #[arg(long)]
        file: String,
        #[arg(long)]
        message: Option<String>,
    },

    /// Client confirms task complete and releases payment
    Complete { job_id: String },

    /// Client rejects deliverable
    Reject {
        job_id: String,
        #[arg(long)]
        reason: String,
    },

    /// Client closes task (only valid while Open)
    Close { job_id: String },

    /// Convert private task to public listing
    #[command(name = "set-public")]
    SetPublic { job_id: String },

    /// AI-assisted deliverable quality assessment
    #[command(name = "ai-evaluate")]
    AiEvaluate { job_id: String },

    /// Task config: init | show
    Config {
        #[command(subcommand)]
        action: task::client::ConfigAction,
    },

    /// Negotiation actions: start, quote, counter, accept, reject
    #[command(subcommand)]
    Negotiate(task::client::NegotiateCommand),

    /// Dispute actions: raise, evidence, info, vote, appeal
    #[command(subcommand)]
    Dispute(task::client::DisputeCommand),

    /// Common queries: context lookup for AI agents
    #[command(subcommand)]
    Common(task::common::CommonCommand),
}

pub async fn run(cmd: AgentCommand, ctx: &Context) -> Result<()> {
    use task::client::TaskCommand as T;

    match cmd {
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

        AgentCommand::CreateTask {
            description,
            budget,
            currency,
            deadline_open,
            deadline_submit,
            quality_standards,
            title,
        } => {
            task::client::run_task(
                T::Create {
                    description,
                    budget,
                    currency,
                    deadline_open,
                    deadline_submit,
                    quality_standards,
                    title,
                },
                ctx,
            )
            .await
        }
        AgentCommand::Recommend { job_id } => {
            task::client::run_task(T::Recommend { job_id }, ctx).await
        }
        AgentCommand::Status { job_id } => task::client::run_task(T::Status { job_id }, ctx).await,
        AgentCommand::List {
            role,
            status,
            page,
            limit,
        } => {
            task::client::run_task(
                T::List {
                    role,
                    status,
                    page,
                    limit,
                },
                ctx,
            )
            .await
        }
        AgentCommand::ConfirmAccept { job_id, provider } => {
            task::client::run_task(T::ConfirmAccept { job_id, provider }, ctx).await
        }
        AgentCommand::RejectApply {
            job_id,
            provider,
            reason,
        } => {
            task::client::run_task(
                T::RejectApply {
                    job_id,
                    provider,
                    reason,
                },
                ctx,
            )
            .await
        }
        AgentCommand::Confirm { job_id } => {
            task::client::run_task(T::Confirm { job_id }, ctx).await
        }
        AgentCommand::Deliver {
            job_id,
            file,
            message,
        } => {
            task::client::run_task(
                T::Deliver {
                    job_id,
                    file,
                    message,
                },
                ctx,
            )
            .await
        }
        AgentCommand::Complete { job_id } => {
            task::client::run_task(T::Complete { job_id }, ctx).await
        }
        AgentCommand::Reject { job_id, reason } => {
            task::client::run_task(T::Reject { job_id, reason }, ctx).await
        }
        AgentCommand::Close { job_id } => task::client::run_task(T::Close { job_id }, ctx).await,
        AgentCommand::SetPublic { job_id } => {
            task::client::run_task(T::SetPublic { job_id }, ctx).await
        }
        AgentCommand::AiEvaluate { job_id } => {
            task::client::run_task(T::AiEvaluate { job_id }, ctx).await
        }
        AgentCommand::Config { action } => task::client::run_task(T::Config { action }, ctx).await,
        AgentCommand::Negotiate(c) => task::client::run_negotiate(c, ctx).await,
        AgentCommand::Dispute(c) => task::client::run_dispute(c, ctx).await,
        AgentCommand::Common(c) => task::common::run(c, ctx).await,
    }
}
