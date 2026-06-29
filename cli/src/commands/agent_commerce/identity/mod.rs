//! `onchainos agent` identity commands. This module is organized by
//! responsibility; `mod.rs` is only a facade that re-exports the public
//! surface so callers keep using `identity::CreateArgs` /
//! `identity::create(...)` unchanged.
//!
//! Layout:
//! - `args`      — clap::Args for every subcommand
//! - `models`    — serde data structures + constants
//! - `utils`     — stateless helpers (HTTP client, logging, parsing)
//! - `signing`   — signing seed + Erc8004Payload + broadcast
//! - `queries`   — read-side commands (get / search / service-list /
//!   feedback-list) plus update's pre-fetch
//! - `mutations` — write-side commands (precheck / create / update / activate /
//!   deactivate / upload / feedback-submit / xmtp-sign)
//!
//! Dependency direction: `models` ← `utils` ← `signing` ← `queries` /
//! `mutations` ← `mod.rs`.

/// `eprintln!` gated behind the `debug-log` feature so production builds stay
/// quiet. Defined before the submodule declarations below so it is in scope
/// (textually) for every child module in this directory — no `use` needed.
macro_rules! debug_log {
    ($($arg:tt)*) => {
        if cfg!(feature = "debug-log") {
            eprintln!($($arg)*);
        }
    };
}

mod args;
mod models;
mod mutations;
mod queries;
mod signing;
mod socket;
mod utils;
mod validate;

// CLI `Args` structs — kept at the module root for `identity::CreateArgs`.
pub use args::{
    ActivateArgs, AgentStatusArgs, CreateArgs, FeedbackListArgs, FeedbackSubmitArgs, GetAgentsArgs,
    GetArgs, GetByAddressArgs, GetMyAgentsArgs, PrecheckArgs, SearchArgs, ServiceListArgs,
    UpdateArgs, UploadArgs, ValidateListingArgs, XmtpSignArgs,
};

// Pure-local validator — hidden CLI entry point used by the skill during QA.
pub use validate::validate_listing;

// Read-side commands.
pub use queries::{
    feedback_list, get, get_agents, get_by_address, get_my_agents, search, service_list,
};

// Write-side commands.
pub use mutations::{
    activate, create, deactivate, feedback_submit, precheck, update, upload, xmtp_sign,
};
