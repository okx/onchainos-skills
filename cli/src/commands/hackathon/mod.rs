//! OKX.AI trading hackathon — a time-boxed feature.
//!
//! # Decommissioning
//!
//! The hackathon runs for one event and is expected to be removed afterwards.
//! Everything it owns lives in exactly two directories — this module and
//! `skills/okx-hackathon/` — plus `cli/tests/cli_hackathon.rs`. Every other
//! touchpoint is a single grep-able line:
//!
//! ```text
//! grep -rn "hackathon\|okx-hackathon" --exclude-dir=target --exclude-dir=.git .
//! ```
//!
//! That returns the routing rows (`CLAUDE.md`, `AGENTS.md`, `README.md`,
//! `.codex/`, `.opencode/`, `.openclaw/`, `openclaw_template/`,
//! `package.json`), the `Commands::Hackathon` arm in `main.rs`, the
//! `hackathon_register` tool in `mcp/mod.rs`, and the `"--uid"` /
//! `Commands::Hackathon` entries in `audit.rs`. Deleting those lines and the
//! two directories removes the feature completely; nothing else depends on it.

mod register;

pub use register::{execute, register_via_mcp, HackathonCommand};

/// Exposed for the MCP server's tests, which assert the tool's validation step
/// without reaching the network. `register_via_mcp` is the only production
/// entry point outside this module.
#[cfg(test)]
pub use register::prepare_registration;
