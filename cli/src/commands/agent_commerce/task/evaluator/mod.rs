// Evaluator-only CLI handlers: dispute info, commit, reveal, claim reward.
// Each handler lives in its own file; this module wires the clap surface and
// dispatches `onchainos agent evaluator <sub>` into the right handler.

mod claim;
mod commit;
mod helpers;
mod info;
mod reveal;

use anyhow::Result;
use clap::Subcommand;

use crate::commands::Context;

#[derive(Subcommand)]
pub enum EvaluatorCommand {
    /// Fetch dispute evidence (text + images downloaded locally so multimodal agents can view them)
    Info { dispute_id: String },
    /// Commit a vote (Phase 1 of commit-reveal). side: 1 = Provider wins, 2 = Client wins
    Commit {
        dispute_id: String,
        #[arg(long)]
        side: u8,
        #[arg(long)]
        reason: String,
    },
    /// Reveal a previously-committed vote (Phase 2 of commit-reveal)
    Reveal { dispute_id: String },
    /// Claim reward after task/dispute resolved
    Claim { job_id: String },
}

pub async fn run(cmd: EvaluatorCommand, ctx: &Context) -> Result<()> {
    match cmd {
        EvaluatorCommand::Info { dispute_id } => info::run_info(dispute_id, ctx).await,
        EvaluatorCommand::Commit { dispute_id, side, reason } =>
            commit::run_commit(dispute_id, side, reason, ctx).await,
        EvaluatorCommand::Reveal { dispute_id } => reveal::run_reveal(dispute_id, ctx).await,
        EvaluatorCommand::Claim { job_id } => claim::run_claim(job_id, ctx).await,
    }
}
