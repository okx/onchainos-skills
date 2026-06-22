//! Terminal-state session cleanup: cancel pending decisions + close
//! conversations.

use anyhow::Result;

use super::okx_a2a;
use super::pending_v2;
use super::DEBUG_LOG;

pub fn handle_session_cleanup(job_id: &str) -> Result<()> {
    let cancelled = pending_v2::cancel_all_for_job(job_id).unwrap_or(0);
    if cancelled > 0 && DEBUG_LOG {
        eprintln!("[session-cleanup] cancelled {cancelled} pending decision(s) for job {job_id}");
    }

    let mut out = String::new();
    if super::config::keep_conversation_on_terminal() {
        out.push_str("ℹ️ KEEP_SESSION=true — conversation history retained. No further action needed.\n");
    } else {
        match okx_a2a::session_delete(job_id, None) {
            Ok(()) => out.push_str(&"OK".to_string()),
            Err(e) => out.push_str(&format!("⚠️ sub session delete failed: {e}\n")),
        }
        out.push_str("\n✅ All session cleanup steps completed by Rust. End the turn.\n");
    }

    print!("{out}");
    Ok(())
}
