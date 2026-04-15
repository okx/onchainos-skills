use anyhow::Result;
use clap::Subcommand;

use crate::commands::Context;

// ─── task subcommands ──────────────────────────────────────────────────────

#[derive(Subcommand)]
pub enum TaskCommand {
    /// Create a new task (Client only)
    Create {
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
    },
    /// Client rejects provider application
    RejectApply {
        job_id: String,
        #[arg(long)]
        provider: String,
        #[arg(long)]
        reason: String,
    },
    /// Provider confirms on-chain acceptance
    Confirm {
        job_id: String,
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
    /// AI-assisted deliverable quality assessment
    AiEvaluate {
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

// ─── negotiate subcommands ─────────────────────────────────────────────────

#[derive(Subcommand)]
pub enum NegotiateCommand {
    /// Client initiates negotiation with a provider
    Start {
        #[arg(long)]
        to: String,
        #[arg(long = "job-id")]
        job_id: String,
        #[arg(long)]
        message: String,
    },
    /// Provider sends a quote to client
    Quote {
        #[arg(long)]
        to: String,
        #[arg(long = "job-id")]
        job_id: String,
        #[arg(long)]
        price: f64,
        #[arg(long)]
        currency: String,
        #[arg(long = "delivery-hours")]
        delivery_hours: u32,
        #[arg(long = "skill-id")]
        skill_id: Option<String>,
        #[arg(long)]
        message: Option<String>,
    },
    /// Either party counters with a new price
    Counter {
        #[arg(long)]
        to: String,
        #[arg(long = "job-id")]
        job_id: String,
        #[arg(long)]
        price: f64,
        #[arg(long)]
        reason: Option<String>,
    },
    /// Either party accepts current terms
    Accept {
        #[arg(long)]
        to: String,
        #[arg(long = "job-id")]
        job_id: String,
        #[arg(long)]
        price: f64,
        #[arg(long = "delivery-hours")]
        delivery_hours: u32,
        #[arg(long = "payment-mode", default_value = "escrow")]
        payment_mode: String,
    },
    /// Either party rejects and ends negotiation
    Reject {
        #[arg(long)]
        to: String,
        #[arg(long = "job-id")]
        job_id: String,
        #[arg(long)]
        reason: String,
    },
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

// ─── handlers (TODO) ──────────────────────────────────────────────────────

pub async fn run_task(cmd: TaskCommand, _ctx: &Context) -> Result<()> {
    match cmd {
        TaskCommand::Create { .. } => todo!("task create: call XLayer contract + XMTP"),
        TaskCommand::Recommend { job_id } => todo!("task recommend {job_id}: query provider index"),
        TaskCommand::Status { job_id } => todo!("task status {job_id}: fetch on-chain state"),
        TaskCommand::List { .. } => todo!("task list: query task index"),
        TaskCommand::ConfirmAccept { job_id, .. } => todo!("task confirm-accept {job_id}: setProvider + stakeFund"),
        TaskCommand::RejectApply { job_id, .. } => todo!("task reject-apply {job_id}"),
        TaskCommand::Confirm { job_id } => todo!("task confirm {job_id}: provider on-chain confirm"),
        TaskCommand::Deliver { job_id, .. } => todo!("task deliver {job_id}: hash + CDN upload + on-chain + XMTP"),
        TaskCommand::Complete { job_id } => todo!("task complete {job_id}: release escrow"),
        TaskCommand::Reject { job_id, .. } => todo!("task reject {job_id}: reject deliverable"),
        TaskCommand::Close { job_id } => todo!("task close {job_id}"),
        TaskCommand::SetPublic { job_id } => todo!("task set-public {job_id}"),
        TaskCommand::AiEvaluate { job_id } => todo!("task ai-evaluate {job_id}"),
        TaskCommand::Config { action } => match action {
            ConfigAction::Init => todo!("task config init"),
            ConfigAction::Show => todo!("task config show"),
        },
    }
}

pub async fn run_negotiate(cmd: NegotiateCommand, _ctx: &Context) -> Result<()> {
    match cmd {
        NegotiateCommand::Start { .. } => todo!("negotiate start: send XMTP DM"),
        NegotiateCommand::Quote { .. } => todo!("negotiate quote: send quote via XMTP"),
        NegotiateCommand::Counter { .. } => todo!("negotiate counter: send counter via XMTP"),
        NegotiateCommand::Accept { .. } => todo!("negotiate accept: send accept + trigger on-chain confirm"),
        NegotiateCommand::Reject { .. } => todo!("negotiate reject: send reject via XMTP"),
    }
}

pub async fn run_dispute(cmd: DisputeCommand, _ctx: &Context) -> Result<()> {
    match cmd {
        DisputeCommand::Raise { .. } => todo!("dispute raise: on-chain + XMTP group notify"),
        DisputeCommand::Evidence { .. } => todo!("dispute evidence: upload file + XMTP"),
        DisputeCommand::Info { .. } => todo!("dispute info: fetch dispute state"),
        DisputeCommand::Vote { .. } => todo!("dispute vote: commit-reveal on-chain"),
        DisputeCommand::Appeal { .. } => todo!("dispute appeal: on-chain appeal"),
    }
}
