// Evaluator-only CLI handlers: dispute info, commit, reveal, claim reward.
// Each handler lives in its own file; this module wires the clap surface and
// dispatches `onchainos agent evaluator <sub>` into the right handler.

mod claim;
mod commit;
mod commit_store;
mod forget;
pub mod flow;
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
    /// Commit a vote (Phase 1 of commit-reveal). side: 1 = Provider wins (Approve), 2 = Client wins (Reject).
    /// Body sent to backend is only `{ vote }` — reason is NOT part of the API (lives in agent session memory).
    Commit {
        dispute_id: String,
        #[arg(long)]
        side: u8,
    },
    /// Reveal a previously-committed vote (Phase 2 of commit-reveal).
    /// `--side` is optional: if omitted, CLI auto-resolves from `~/.onchainos/evaluator-commits.jsonl`
    /// written during the original commit. Explicit `--side` overrides the stored value (CLI warns on mismatch).
    Reveal {
        dispute_id: String,
        #[arg(long)]
        side: Option<u8>,
    },
    /// Claim reward after task/dispute resolved
    Claim { job_id: String },
    /// Delete the local commit record for a settled dispute (called on TASK_RESOLVED,
    /// idempotent). Dispute is terminal — {vote, salt} is no longer needed client-side.
    Forget { dispute_id: String },
}

pub async fn run(cmd: EvaluatorCommand, ctx: &Context) -> Result<()> {
    match cmd {
        EvaluatorCommand::Info { dispute_id } => info::run_info(dispute_id, ctx).await,
        EvaluatorCommand::Commit { dispute_id, side } =>
            commit::run_commit(dispute_id, side, ctx).await,
        EvaluatorCommand::Reveal { dispute_id, side } =>
            reveal::run_reveal(dispute_id, side, ctx).await,
        EvaluatorCommand::Claim { job_id } => claim::run_claim(job_id, ctx).await,
        EvaluatorCommand::Forget { dispute_id } => forget::run_forget(dispute_id, ctx).await,
    }
}
