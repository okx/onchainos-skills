pub mod client;
pub mod common;
pub mod evaluator;
pub mod provider;

use anyhow::Result;
use clap::Subcommand;

use super::Context;

/// Task system top-level subcommands.
#[derive(Subcommand)]
pub enum TaskSystemCommand {
    // ── Task commands (flattened, no `task` sub-group) ──────────────────────

    /// Create a new task (Client)
    #[command(name = "create-task")]
    CreateTask {
        #[arg(long)] description: String,
        #[arg(long)] budget: f64,
        #[arg(long)] currency: String,
        #[arg(long = "deadline-open")]  deadline_open: String,
        #[arg(long = "deadline-submit")] deadline_submit: String,
        #[arg(long = "quality-standards")] quality_standards: String,
        #[arg(long)] title: Option<String>,
    },

    /// Get recommended providers for a task
    Recommend { job_id: String },

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
    },

    /// Client rejects provider application
    #[command(name = "reject-apply")]
    RejectApply {
        job_id: String,
        #[arg(long)] provider: String,
        #[arg(long)] reason: String,
    },

    /// Provider confirms on-chain acceptance
    Confirm { job_id: String },

    /// Provider submits deliverable
    Deliver {
        job_id: String,
        #[arg(long)] file: String,
        #[arg(long)] message: Option<String>,
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

    /// AI-assisted deliverable quality assessment
    #[command(name = "ai-evaluate")]
    AiEvaluate { job_id: String },

    /// Task config: init | show
    Config {
        #[command(subcommand)]
        action: client::ConfigAction,
    },

    // ── Negotiation (kept as sub-group) ─────────────────────────────────────

    /// Negotiation actions: start, quote, counter, accept, reject
    #[command(subcommand)]
    Negotiate(client::NegotiateCommand),

    // ── Dispute (kept as sub-group) ──────────────────────────────────────────

    /// Dispute actions: raise, evidence, info, vote, appeal
    #[command(subcommand)]
    Dispute(client::DisputeCommand),

    // ── Common ───────────────────────────────────────────────────────────────

    /// Common queries: context lookup for AI agents
    #[command(subcommand)]
    Common(common::CommonCommand),
}

pub async fn run(cmd: TaskSystemCommand, ctx: &Context) -> Result<()> {
    use client::TaskCommand as T;

    match cmd {
        TaskSystemCommand::CreateTask { description, budget, currency, deadline_open, deadline_submit, quality_standards, title } =>
            client::run_task(T::Create { description, budget, currency, deadline_open, deadline_submit, quality_standards, title }, ctx).await,

        TaskSystemCommand::Recommend { job_id } =>
            client::run_task(T::Recommend { job_id }, ctx).await,

        TaskSystemCommand::Status { job_id } =>
            client::run_task(T::Status { job_id }, ctx).await,

        TaskSystemCommand::List { role, status, page, limit } =>
            client::run_task(T::List { role, status, page, limit }, ctx).await,

        TaskSystemCommand::ConfirmAccept { job_id, provider } =>
            client::run_task(T::ConfirmAccept { job_id, provider }, ctx).await,

        TaskSystemCommand::RejectApply { job_id, provider, reason } =>
            client::run_task(T::RejectApply { job_id, provider, reason }, ctx).await,

        TaskSystemCommand::Confirm { job_id } =>
            client::run_task(T::Confirm { job_id }, ctx).await,

        TaskSystemCommand::Deliver { job_id, file, message } =>
            client::run_task(T::Deliver { job_id, file, message }, ctx).await,

        TaskSystemCommand::Complete { job_id } =>
            client::run_task(T::Complete { job_id }, ctx).await,

        TaskSystemCommand::Reject { job_id, reason } =>
            client::run_task(T::Reject { job_id, reason }, ctx).await,

        TaskSystemCommand::Close { job_id } =>
            client::run_task(T::Close { job_id }, ctx).await,

        TaskSystemCommand::SetPublic { job_id } =>
            client::run_task(T::SetPublic { job_id }, ctx).await,

        TaskSystemCommand::AiEvaluate { job_id } =>
            client::run_task(T::AiEvaluate { job_id }, ctx).await,

        TaskSystemCommand::Config { action } =>
            client::run_task(T::Config { action }, ctx).await,

        TaskSystemCommand::Negotiate(c) =>
            client::run_negotiate(c, ctx).await,

        TaskSystemCommand::Dispute(c) =>
            client::run_dispute(c, ctx).await,

        TaskSystemCommand::Common(c) =>
            common::run(c, ctx).await,
    }
}
