//! `onchainos strategy` — limit-order strategy trading on Agentic Wallet.
//!
//! Phase 1 scope (see `.claude/strategyTrading/tech-design.md`):
//! 4 P0 subcommands — `create-limit`, `cancel`, `list`, `resume`.
//! Activation/upgrade is normally transparent via
//! `trader_mode::activate` (code 60018 → SD-A → retry once).
//!
//! ## Module layout
//! - `handlers` — 4 subcommand handlers + their clap Args
//! - `api`      — HTTP wrappers for 7 endpoints (5 dex + 2 wallet)
//! - `types`    — DTOs
//! - `status`   — OrderStatus enum + StrategyError code map + check_response
//! - `trader_mode` — intent / sign / SD-A activate / 60018 retry / output formatters
//! - `session`  — wallet-session loader (cert / seed / accountId / addresses)

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
