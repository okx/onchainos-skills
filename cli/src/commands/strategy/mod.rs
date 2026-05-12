//! `onchainos strategy` — Phase 1 limit orders (4 P0 subcommands).
//! 60018 is handled transparently via SD-A → retry once.

use anyhow::Result;
use clap::Subcommand;

use super::Context;

mod api;
mod handlers;
mod session;
mod status;
mod supported_chains;
mod trader_mode;
mod types;

#[derive(Subcommand)]
pub enum StrategyCommand {
    /// Place a price-triggered limit order.
    CreateLimit(handlers::CreateLimitArgs),
    /// Cancel a single / batch / all active limit orders.
    Cancel(handlers::CancelArgs),
    /// List open or specific limit orders.
    List(handlers::ListArgs),
    /// Re-activate suspended orders (auto-discover or by id).
    Resume(handlers::ResumeArgs),
}

pub async fn execute(ctx: &Context, cmd: StrategyCommand) -> Result<()> {
    match cmd {
        StrategyCommand::CreateLimit(args) => handlers::create_limit(ctx, args).await,
        StrategyCommand::Cancel(args) => handlers::cancel(ctx, args).await,
        StrategyCommand::List(args) => handlers::list(ctx, args).await,
        StrategyCommand::Resume(args) => handlers::resume(ctx, args).await,
    }
}
