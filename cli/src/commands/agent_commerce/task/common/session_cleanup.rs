//! Terminal-state session cleanup: cancel pending decisions + output
//! xmtp_delete_conversation instructions.
//!
//! Replaces the multi-step manual cleanup in terminal playbooks with a
//! single CLI command. The LLM still needs to call `session_status` and
//! `xmtp_delete_conversation` (MCP tools), but the pending-decisions
//! bookkeeping is handled automatically.

use anyhow::Result;

use super::pending_v2;

pub fn handle_session_cleanup(job_id: &str, role: &str) -> Result<()> {
    let cancelled = pending_v2::cancel_all_for_job(job_id).unwrap_or(0);
    if cancelled > 0 {
        eprintln!("[session-cleanup] cancelled {cancelled} pending decision(s) for job {job_id}");
    }

    let is_buyer = role.eq_ignore_ascii_case("buyer");

    let mut out = String::new();
    out.push_str(&format!(
        "✓ session-cleanup: {cancelled} pending decision(s) cleared for job {job_id}.\n\n"
    ));

    if super::config::keep_conversation_on_terminal() {
        out.push_str("ℹ️ KEEP_SESSION=true — conversation history retained. No further action needed.\n");
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
