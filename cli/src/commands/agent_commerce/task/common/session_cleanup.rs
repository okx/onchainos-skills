//! Terminal-state session cleanup: cancel pending decisions + close
//! conversations.
//!
//! Two paths:
//! - **CLI mode** (Claude Code / Codex driving via bash): Rust spawns
//!   `okx-a2a session status` + `okx-a2a session delete` in-process and
//!   closes both the sub session and (for buyers) the backup session. The
//!   LLM gets a "done" summary and ends the turn.
//! - **Non-CLI mode** (MCP host): emit instructions for the LLM to call
//!   `session_status` + `xmtp_delete_conversation` MCP tools, since those
//!   tools are the correct entrypoint when an MCP host is present.

use anyhow::Result;

use super::config::is_cli_mode;
use super::okx_a2a;
use super::pending_v2;
use super::DEBUG_LOG;

pub fn handle_session_cleanup(job_id: &str, role: &str) -> Result<()> {
    let cancelled = pending_v2::cancel_all_for_job(job_id).unwrap_or(0);
    if cancelled > 0 && DEBUG_LOG {
        eprintln!("[session-cleanup] cancelled {cancelled} pending decision(s) for job {job_id}");
    }

    let is_buyer = role.eq_ignore_ascii_case("buyer");

    let mut out = String::new();
    out.push_str(&format!(
        "✓ session-cleanup: {cancelled} pending decision(s) cleared for job {job_id}.\n\n"
    ));

    if super::config::keep_conversation_on_terminal() {
        out.push_str("ℹ️ KEEP_SESSION=true — conversation history retained. No further action needed.\n");
    } else if is_cli_mode() {
        // CLI mode: Rust spawns `okx-a2a session delete` directly so the
        // LLM doesn't need to call MCP `xmtp_delete_conversation` (which
        // isn't available in cli-driver runtimes). Both failures are
        // best-effort — log and continue, do not bail.
        match okx_a2a::session_status() {
            Ok(Some(sub_key)) => match okx_a2a::session_delete(&sub_key) {
                Ok(()) => out.push_str(&format!("✓ sub session deleted: {sub_key}\n")),
                Err(e) => out.push_str(&format!("⚠️ sub session delete failed (best-effort, continuing): {e}\n")),
            },
            Ok(None) => out.push_str("ℹ️ no active sub session reported by `session status` — skipping sub delete.\n"),
            Err(e) => out.push_str(&format!("⚠️ session_status failed (best-effort, continuing): {e}\n")),
        }
        if is_buyer {
            let backup_key = format!("backup:{job_id}");
            match okx_a2a::session_delete(&backup_key) {
                Ok(()) => out.push_str(&format!("✓ backup session deleted: {backup_key}\n")),
                Err(e) => out.push_str(&format!("⚠️ backup session delete failed (best-effort, continuing): {e}\n")),
            }
        }
        out.push_str("\n✅ All session cleanup steps completed by Rust. End the turn.\n");
    } else {
        out.push_str("Now close the XMTP conversations to release resources:\n\n");
        out.push_str("1. Call `session_status` to get the current sub session's `sessionKey`.\n");
        out.push_str("2. Call `xmtp_delete_conversation` with `sessionKey=<sessionKey from step 1>` to close the sub session.\n");
        if is_buyer {
            out.push_str(&format!(
                "3. Call `xmtp_delete_conversation` with `sessionKey=backup:{}` to close the backup session.\n",
                job_id
            ));
        }
    }

    print!("{out}");
    Ok(())
}
