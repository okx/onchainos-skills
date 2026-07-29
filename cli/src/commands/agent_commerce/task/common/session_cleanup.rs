//! Terminal-state session cleanup: cancel pending decisions + close
//! conversations.

use anyhow::Result;

use super::okx_a2a;
use super::pending_v2;
use super::prefilled_notify;
use super::prefilled_rating;
use super::DEBUG_LOG;

/// `print_output = true` writes the human-readable summary to stdout — for
/// CLI entry handlers whose stdout is consumed directly by a human / shell.
/// In-process callers inside playbook handlers (e.g. `next-action` event
/// dispatch) must pass `false`, otherwise the summary would prepend the
/// playbook returned to the LLM.
pub fn handle_session_cleanup(job_id: &str, print_output: bool) -> Result<()> {
    let cancelled = pending_v2::cancel_all_for_job(job_id).unwrap_or(0);
    if cancelled > 0 && DEBUG_LOG {
        eprintln!("[session-cleanup] cancelled {cancelled} pending decision(s) for job {job_id}");
    }

    let _ = prefilled_notify::clear(job_id);
    let _ = prefilled_rating::clear(job_id);

    let mut out = String::new();
    if super::config::keep_conversation_on_terminal() {
        out.push_str("ℹ️ KEEP_SESSION=true — conversation history retained. No further action needed.\n");
    } else {
        match okx_a2a::session_delete(job_id, None) {
            Ok(()) => out.push_str(&"OK".to_string()),
            Err(e) => out.push_str(&format!("⚠️ sub session delete failed: {e}\n")),
        }
    }

    if print_output {
        print!("{out}");
    }
    Ok(())
}
