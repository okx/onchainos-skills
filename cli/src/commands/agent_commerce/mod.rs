pub mod chat;
pub mod identity;
pub mod task;

use anyhow::Result;
use clap::Subcommand;

use crate::commands::Context;

/// Top-level agent commerce subcommands.
/// Flattens task and chat sub-enums; inlines identity commands
/// (identity exposes per-op Args structs instead of a single command enum).
#[derive(Subcommand)]
pub enum AgentCommand {
    #[command(flatten)]
    Task(task::TaskSystemCommand),

    #[command(flatten)]
    Chat(chat::ChatCommand),

    // ── Identity ────────────────────────────────────────────────────────
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

    /// 用 keyUuid + signing_seed 代签任意 message（xmtp 等场景），不走广播
    #[command(name = "xmtp-sign")]
    XmtpSign(identity::XmtpSignArgs),
}

pub async fn run(cmd: AgentCommand, ctx: &Context) -> Result<()> {
    match cmd {
        AgentCommand::Task(c) => task::run(c, ctx).await,
        AgentCommand::Chat(c) => chat::run(c, ctx).await,
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
    }
}
