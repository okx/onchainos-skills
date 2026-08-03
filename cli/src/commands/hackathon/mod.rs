//! OKX.AI trading hackathon — a time-boxed feature.
//!
//! # Decommissioning
//!
//! The hackathon runs for one event and is expected to be removed afterwards.
//! Everything it owns lives in this module, in `skills/okx-activity/` (the
//! hackathon is one activity there: `references/hackathon-*.md`), and in
//! `cli/tests/cli_hackathon.rs`. Every other touchpoint is a single grep-able
//! line:
//!
//! ```text
//! grep -rni "hackathon" --exclude-dir=target --exclude-dir=.git .
//! ```
//!
//! Case-insensitive matters: the docs write it "Hackathon". That returns the
//! rows that still name the hackathon (`CLAUDE.md`, `AGENTS.md`, `README.md`,
//! `.opencode/`, `.openclaw/`, `openclaw_template/workspace/AGENTS.md`), the
//! `Commands::Hackathon` arm in `main.rs`, the `hackathon_register` tool in
//! `mcp/mod.rs`, and the `"--uid"` / `Commands::Hackathon` entries in
//! `audit.rs`. Deleting those lines, this module, and the hackathon files under
//! `skills/okx-activity/references/` removes the feature completely; nothing
//! else depends on it.
//!
//! Not returned by that grep, and correctly so — they list only the surviving
//! hub name `okx-activity` and stay as-is: `.codex/INSTALL.md`,
//! `openclaw_template/workspace/TOOLS.md`, `package.json`, and the
//! `okx-hackathon` entry in `upgrade.rs`'s `DEPRECATED_SKILLS` (which cleans up
//! pre-rename installs and outlives the hackathon itself, as does
//! `okx-activity`, the activity hub).

mod register;

pub use register::{execute, register_via_mcp, HackathonCommand};

/// Exposed for the MCP server's tests, which assert the tool's validation step
/// without reaching the network. `register_via_mcp` is the only production
/// entry point outside this module.
#[cfg(test)]
pub use register::prepare_registration;
