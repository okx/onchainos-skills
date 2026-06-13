//! Task module global toggles — centralized control over cross-flow behavioral differences.
//!
//! **Only edit this file** when flipping a toggle; each flow.rs / content.rs references the
//! constants here to dynamically emit prompts. After editing, run `cargo build` to regenerate
//! the binary.
//!
//! Adding a new toggle:
//! 1. Add `pub const FOO: bool = ...;` here (with a doc comment explaining what true / false mean).
//! 2. Read it at the call site via `super::super::common::config::FOO`
//!    (or `crate::commands::agent_commerce::task::common::config::FOO`).
//! 3. Typically wire it as `if config::FOO { hint_keep } else { hint_delete }` — a two-way string selector.

/// Whether terminal task states (`completed` / `refunded` / `close` / `dispute_resolved`) keep the
/// sub session history.
///
/// - `true` = **keep** — each terminal arm emits "**do not call `xmtp_delete_conversation`** —
///   keep the conversation history for post-mortem review".
///   Use case: agent debugging / customer support / early-product need to replay the full task message log.
/// - `false` (default) = **release** — each terminal arm emits "task is in a terminal state, you may call
///   `xmtp_delete_conversation` to release conversation resources".
///   Use case: large-scale production where too many sessions burden the frontend / IM bridge and need active cleanup.
///
/// Precedence: runtime `ONCHAINOS_KEEP_SESSION` env > compile-time `ONCHAINOS_KEEP_SESSION` > hardcoded default.
///
/// Pack script sets `ONCHAINOS_KEEP_SESSION=true cargo build` to bake it in;
/// runtime env var still overrides the baked-in value.
const KEEP_CONVERSATION_ON_TERMINAL_DEFAULT: bool = false;

fn parse_bool(s: &str) -> bool {
    s.eq_ignore_ascii_case("true") || s == "1"
}

pub fn keep_conversation_on_terminal() -> bool {
    std::env::var("ONCHAINOS_KEEP_SESSION")
        .map(|v| parse_bool(&v))
        .unwrap_or_else(|_| {
            option_env!("ONCHAINOS_KEEP_SESSION")
                .map(parse_bool)
                .unwrap_or(KEEP_CONVERSATION_ON_TERMINAL_DEFAULT)
        })
}

/// Detect whether the next-action / playbook output is being driven by a
/// CLI runtime (Claude Code, Codex) rather than an MCP host (OpenClaw,
/// Hermes). Used to pick between bash-style cli commands and MCP-tool-style
/// prompts in playbook generation, and to decide when Rust should run an
/// action in-process instead of emitting instructions for the LLM.
///
/// Detection: presence of the runtime-specific env var set by the host.
pub fn is_cli_mode() -> bool {
    std::env::var("CLAUDECODE").unwrap_or_default() == "1"
        || std::env::var("CODEX_THREAD_ID")
            .ok()
            .filter(|s| !s.is_empty())
            .is_some()
}

/// Task protocol version number — a single value used in both directions: it is both
/// "the version I am currently on" and "the minimum version I require the peer to be on".
///
/// - **Sender**: every `xmtp_send` puts this value into `payload.taskMinVersion`.
/// - **Receiver**: next-action reads peer's value via `--peerTaskMinVersion`;
///   if `local TASK_MIN_VERSION < peer.taskMinVersion` then the local side is stale and
///   the version_mismatch script is emitted, prompting the user to run `onchainos upgrade`.
///
/// Bump rule: only +1 when the task protocol (state machine / envelope schema / payload schema)
/// changes in a **backwards-incompatible** way; pure bug fixes / copy tweaks must not bump it.
pub const TASK_MIN_VERSION: u32 = 1;
