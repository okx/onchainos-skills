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
/// - `true` (default) = **keep** — each terminal arm emits "**do not call `xmtp_delete_conversation`** —
///   keep the conversation history for post-mortem review".
///   Use case: agent debugging / customer support / early-product need to replay the full task message log.
/// - `false` = **release** — each terminal arm emits "task is in a terminal state, you may call
///   `xmtp_delete_conversation` to release conversation resources".
///   Use case: large-scale production where too many sessions burden the frontend / IM bridge and need active cleanup.
pub const KEEP_CONVERSATION_ON_TERMINAL: bool = true;

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
