pub mod client;
pub mod evaluator;
pub mod provider;

use anyhow::Result;
use clap::Subcommand;

use super::Context;

/// Task system top-level subcommands.
#[derive(Subcommand)]
pub enum TaskSystemCommand {
    /// Client (buyer) actions: create, confirm, complete, reject, close tasks
    #[command(subcommand)]
    Task(client::TaskCommand),

    /// Negotiation actions: start, quote, counter, accept, reject
    #[command(subcommand)]
    Negotiate(client::NegotiateCommand),

    /// Dispute actions: raise, evidence, info, vote, appeal
    #[command(subcommand)]
    Dispute(client::DisputeCommand),
}

pub async fn run(cmd: TaskSystemCommand, ctx: &Context) -> Result<()> {
    match cmd {
        TaskSystemCommand::Task(c) => client::run_task(c, ctx).await,
        TaskSystemCommand::Negotiate(c) => client::run_negotiate(c, ctx).await,
        TaskSystemCommand::Dispute(c) => client::run_dispute(c, ctx).await,
    }
}
