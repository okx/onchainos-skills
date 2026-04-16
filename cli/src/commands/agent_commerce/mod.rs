pub mod chat;
pub mod task;

use anyhow::Result;
use clap::Subcommand;

use crate::commands::Context;

/// Top-level agent commerce subcommands.
/// Routes to task and chat modules.
#[derive(Subcommand)]
pub enum AgentCommand {
    #[command(flatten)]
    Task(task::TaskSystemCommand),

    #[command(flatten)]
    Chat(chat::ChatCommand),
}

pub async fn run(cmd: AgentCommand, ctx: &Context) -> Result<()> {
    match cmd {
        AgentCommand::Task(c) => task::run(c, ctx).await,
        AgentCommand::Chat(c) => chat::run(c, ctx).await,
    }
}
