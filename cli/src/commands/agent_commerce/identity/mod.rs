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
//! - `mutations` — write-side commands (create / update / activate /
//!   deactivate / upload / feedback-submit / xmtp-sign)
//!
//! Dependency direction: `models` ← `utils` ← `signing` ← `queries` /
//! `mutations` ← `mod.rs`.

mod args;
mod models;
mod mutations;
mod queries;
mod signing;
mod socket;
mod utils;

// CLI `Args` structs — kept at the module root for `identity::CreateArgs`.
pub use args::{
    AgentStatusArgs, ConsentArgs, CreateArgs, FeedbackListArgs, FeedbackSubmitArgs, GetArgs,
    GetByAddressArgs, SearchArgs, ServiceListArgs, SubmitApprovalArgs, UpdateArgs, UploadArgs,
    XmtpSignArgs,
};

// Read-side commands.
pub use queries::{feedback_list, get, get_by_address, search, service_list};

// Write-side commands.
pub use mutations::{
    activate, consent, create, deactivate, feedback_submit, submit_approval, update, upload,
    xmtp_sign,
};
