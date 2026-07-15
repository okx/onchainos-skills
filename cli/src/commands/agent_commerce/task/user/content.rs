//! User-side message templates — single source of truth.
//!
//! Two categories of templates:
//!
//! 1. **User-facing** — chat content shown directly to the user via `onchainos agent user-notify` /
//!    `onchainos agent pending-decisions-v2 request`. Naming suffix: `_user_notify` / `_user_prompt`.
//!    Rule: **no technical jargon** — event names (`provider_applied`/`job_*` etc.) /
//!    status names (English enums like `Open`/`accepted` are kept as doc-reserved literals) / CLI flags (`--*`) /
//!    skill names (`okx-ai` etc.) / backend method names (`claimAutoComplete` etc.).
//!    **Literals in this file are English** (aligned with the PM Review translation baseline),
//!    serving as the canonical content for sub agent localization — English users see them
//!    verbatim (after `<...>` placeholder fills); non-English users get a faithful translation
//!    that preserves all field labels, data values, and structure (see `localization_prefix`
//!    in flow.rs for the strict rules).
//!    Terminology: Job (not Task), User Agent, ASP (Agent Service Provider),
//!    escrow / x402 lowercase, agentId in camelCase for data fields.
//!    Label format: `[Label]` bracket prefix (e.g. `[Job Accepted]`).
//!    Decision prompts (❓) carry the `[Job {short_id} — you are the User Agent]` prefix.
//!    User reply instructions use descriptive phrasing (naturally translatable by the sub agent).
//!
//! 2. **Peer-facing** — agent-to-agent protocol messages sent via `okx-a2a xmtp-send`
//!    to the provider sub agent. Naming suffix: `_to_seller`.
//!    Rule: may contain protocol literals (`[intent:*]` etc.);
//!    **never instruct the peer to call CLI** (the peer has its own flow.rs and decides based on chain events;
//!    issuing commands to the peer is overreach).
//!
//! Field-value placeholders use `<...>`; the agent fills them from `common context` / session context.
//! To add copy → add a new fn; to edit copy → edit the fn body; flow.rs always calls here and never inlines literals.

// ── Platform detection ────────────────────────────────────────────

pub use crate::commands::agent_commerce::task::common::config::is_cli_mode;

// ── Event::JobCreated ──────────────────────────────────────────────

/// `Event::JobCreated` Step 0 — user notification (no designated provider).
pub fn job_created_non_designated_user_notify() -> &'static str {
    // CLI mode: drop "Waiting for ASPs to discover and apply." — passive
    // turn-end cue suppresses LLM-driven watch re-arm. See job_accepted_escrow_user_notify.
    if is_cli_mode() {
        "[Job Created]【<title>】(<short_jobId>) confirmed on-chain (public)."
    } else {
        "[Job Created]【<title>】(<short_jobId>) confirmed on-chain (public). Waiting for ASPs to discover and apply."
    }
}

/// `Event::JobCreated` Step 0 — user notification (with designated provider).
/// Used both for first-time creation and re-entry (asp_match_pick / no_asp_found designate).
pub fn job_created_designated_user_notify() -> &'static str {
    "[Connecting ASP]【<title>】(<short_jobId>) — connecting to the designated ASP (<provider_agentId>)."
}

fn designated_asp_abc_prompt(short_id: &str, dp_id: &str, job_id: &str, reason: &str) -> String {
    format!(
        "[Job {short_id} — you are the User Agent] The designated agent (agentId={dp_id}) for job `{job_id}` \
         {reason}\n\
         Please choose:\n\
         A. Designate another ASP — provide the agentId\n\
         B. Make the job public — let more ASPs discover it\n\
         C. Close the job"
    )
}

/// Prompt shown when the designated agent is not a provider or does not exist (D-Step 1.5a role gate).
pub fn not_provider_user_prompt(job_id: &str, short_id: &str, dp_id: &str) -> String {
    designated_asp_abc_prompt(
        short_id, dp_id, job_id,
        "does not exist or is not registered as an ASP (Agent Service Provider). It cannot fulfil this job.",
    )
}

/// Prompt shown when the designated ASP is offline (D-Step 1.5b).
pub fn provider_offline_user_prompt(job_id: &str, short_id: &str, dp_id: &str) -> String {
    designated_asp_abc_prompt(
        short_id, dp_id, job_id,
        "is currently offline. Negotiation requires the ASP to be online.",
    )
}

// ── Event::JobAccepted ─────────────────────────────────────────────

/// `Event::JobAccepted` Branch A (escrow) — user notification that the job is accepted (B-2-1).
pub fn job_accepted_escrow_user_notify(job_id: &str, _title: &str) -> String {
    // The trailing "Waiting for the ASP to ..." sentence reads like a
    // "conversation ending" cue and can cause LLM-driven watch loops
    // (Claude Code / Codex) to stop prematurely. An earlier attempt to
    // reword it to active "Watching for ..." phrasing failed in practice —
    // when the sub agent localized the notification to Chinese, "Watching"
    // was translated back to "等待", reintroducing the passive cue. Drop
    // the sentence entirely in CLI mode (the metadata above is sufficient,
    // and the watch loop continues without any natural-language nudge).
    // Keep the original wording for native push clients (Hermes / OpenClaw)
    // where the user reads the notification directly and no LLM is making
    // the stop decision.
    let trailing = if is_cli_mode() {
        ""
    } else {
        "\n         Waiting for the ASP to execute and submit the deliverable."
    };
    format!(
        "[Job Accepted] Job `{job_id}` has been accepted; execution begins.\n\
         Title: <title>\n\
         Description: <description>\n\
         ASP agentId: <providerAgentId>\n\
         Payment: escrow\n\
         Amount: <tokenAmount> <tokenSymbol>{trailing}"
    )
}

/// `Event::JobAccepted` Branch B (x402) — user notification when endpoint replay failed (B-2-4).
pub fn job_accepted_x402_replay_fail_user_notify(job_id: &str) -> String {
    format!(
        "[x402 Replay Failed] Job `{job_id}` was accepted but the endpoint replay failed.\n\
         HTTP status: <replayStatus>\n\
         Error: <replayBody>\n\
         The job is now in `accepted` status. Please give a new instruction; the agent will not auto-retry."
    )
}

// ── Event::JobRejected ─────────────────────────────────────────────

/// `Event::JobRejected` Step 1 — user notification that the rejection is confirmed on-chain.
pub fn job_rejected_user_notify(job_id: &str, title: &str) -> String {
    // CLI mode: drop "; waiting for the ASP to respond" — passive turn-end cue.
    let lead = if is_cli_mode() {
        format!("[Rejection Confirmed] The deliverable for【{title}】(`{job_id}`) has been rejected.")
    } else {
        format!("[Rejection Confirmed] The deliverable for【{title}】(`{job_id}`) has been rejected; waiting for the ASP to respond.")
    };
    format!(
        "{lead}\n\
         The ASP will choose: file a dispute or agree to a refund.\n\
         If the ASP takes no action, funds will be auto-refunded to your wallet."
    )
}

// ── Event::JobCompleted ────────────────────────────────────────────

/// `Event::JobCompleted` Branch A (escrow) — user notification (B-4-1).
pub fn job_completed_escrow_user_notify(job_id: &str, title: &str, token_amount: &str, token_symbol: &str) -> String {
    format!(
        "[Job Completed] {title} (`{job_id}`) — approved by the User Agent; funds released to the ASP.\n\
         - Spent: {token_amount} {token_symbol}\n\
         - Payment: escrow"
    )
}

/// `Event::JobCompleted` Branch B (x402) — final summary notification (B-4-3).
pub fn job_completed_x402_user_notify(job_id: &str, title: &str) -> String {
    format!(
        "[x402 Job Completed] {title} (`{job_id}`) — all steps complete.\n\
         - Spent: <tokenAmount> <tokenSymbol>\n\
         - Payment: x402\n\
         - Deliverable saved to: <deliverableSavedPath from task-402-pay output; if not in context, omit this line>\n\
         - Deliverable summary: <one-line summary of the replayBodyDisplay content from task-402-pay; if not in context, omit this line>"
    )
}

// ── Event::DisputeResolved ─────────────────────────────────────────

/// Per-arbiter verdict rationales block shared by both `DisputeResolved` outcomes.
/// Source field: `message.voteReportSummaries[*].voterReportSummary` from the system envelope.
const ARBITRATION_REASONS_BLOCK: &str = concat!(
    "- Arbitration reasons:\n",
    "    Arbiter 1: <voterReportSummary from message.voteReportSummaries[0]>\n",
    "    Arbiter 2: <voterReportSummary from message.voteReportSummaries[1]>\n",
    "    ... (one line per entry; first skip entries whose voterReportSummary is missing / empty / whitespace, then number the kept entries consecutively starting at 1 in array order — do NOT preserve gaps from the original index; omit this whole `- Arbitration reasons:` section if voteReportSummaries is missing, not an array, empty, or every entry would be skipped — do NOT print a header with no body, do NOT fabricate filler text)",
);

/// `Event::DisputeResolved` — user wins (B-5-4).
pub fn dispute_won_user_notify(job_id: &str, title: &str) -> String {
    format!(
        "[Dispute Won] {title} (`{job_id}`) — dispute resolved; User Agent wins.\n\
         - Refund: <tokenAmount> <tokenSymbol>\n\
         - Outcome: ClientWins\n\
         {ARBITRATION_REASONS_BLOCK}\n\
         This job is complete."
    )
}

/// `Event::DisputeResolved` — user loses (B-5-5).
pub fn dispute_lost_user_notify(job_id: &str, title: &str) -> String {
    format!(
        "[Dispute Lost] {title} (`{job_id}`) — dispute resolved; ASP wins.\n\
         - Loss: <tokenAmount> <tokenSymbol> (funds released to the ASP)\n\
         - Outcome: ASPWins\n\
         {ARBITRATION_REASONS_BLOCK}\n\
         This job is complete."
    )
}

// ── Auto-rating notification ──────────────────────────────────────

/// User notification after the user agent auto-rates the ASP.
pub fn rating_submitted_user_notify(job_id: &str, title: &str) -> String {
    format!(
        "[📝 Rating Submitted] {title} (`{job_id}`) — rated.\n\
         Score: <score> / 5.00\n\
         💬 Comment: <description>"
    )
}

// ── Event::JobRefunded ─────────────────────────────────────────────

/// `Event::JobRefunded` — refund settled (B-5-1).
pub fn job_refunded_user_notify(job_id: &str) -> String {
    format!("[Refund Settled] Job `{job_id}` — refund confirmed on-chain; funds returned to your wallet. This job is complete.")
}

// ── Event::JobAutoRefunded ─────────────────────────────────────────

/// `Event::JobAutoRefunded` — auto-refund settled (B-5-2).
pub fn job_auto_refunded_user_notify(job_id: &str, title: &str) -> String {
    format!("[Auto-Refund Settled] {title} (`{job_id}`) — escrowed funds returned to your wallet. This job is complete.")
}

// ── Event::JobExpired ──────────────────────────────────────────────

/// `Event::JobExpired` — job expired (B-7-1).
pub fn job_expired_user_notify(job_id: &str) -> String {
    format!(
        "[Job Expired] Job `{job_id}` has expired (no ASP accepted before the accept deadline, or no deliverable submitted before the submit deadline). The job is now closed."
    )
}

// ── Event::JobClosed ───────────────────────────────────────────────

/// `Event::JobClosed` — job closed (B-7-2).
pub fn job_closed_user_notify(job_id: &str, title: &str) -> String {
    format!("[Job Closed] {title} (`{job_id}`) has been closed; funds have been returned.")
}

// ── Event::JobVisibilityChanged ────────────────────────────────────

/// `Event::JobVisibilityChanged` visibility=0 — public (B-7-3).
pub fn visibility_public_user_notify(job_id: &str, title: &str) -> String {
    // CLI mode: drop "Waiting for ASPs to reach out." — passive turn-end cue.
    if is_cli_mode() {
        format!("[Visibility Changed] {title} (`{job_id}`) is now public.")
    } else {
        format!("[Visibility Changed] {title} (`{job_id}`) is now public. Waiting for ASPs to reach out.")
    }
}

/// `Event::JobVisibilityChanged` visibility=1 — private (B-7-4).
pub fn visibility_private_user_notify(job_id: &str, title: &str) -> String {
    format!("[Visibility Changed] {title} (`{job_id}`) is now private.")
}

// ── Event::JobPaymentModeChanged ───────────────────────────────────

/// `Event::JobPaymentModeChanged` escrow branch — user notification (B-2-5).
pub fn payment_mode_escrow_user_notify(job_id: &str, title: &str) -> String {
    format!("[Payment Mode Set] {title} (`{job_id}`) — payment mode updated successfully; ASP <providerName> (<providerAgentId>) is accepting...")
}

/// x402 set-payment-mode confirmed on-chain; transition notification before task-402-pay.
pub fn x402_paying_user_notify(job_id: &str, title: &str) -> String {
    format!(
        "Payment in progress —【{title}】(`{job_id}`) — x402 agreement reached with the ASP; \
         fee: <tokenAmount> <tokenSymbol>. Paying and fetching the deliverable..."
    )
}

// ── Pseudo events (close / set_public) ─────────────────────────────

/// User notification after closing a job (B-7-11).
pub fn close_user_notify(job_id: &str) -> String {
    format!("[Job Closed] Job `{job_id}` has been closed.")
}

/// User notification after switching a job to public (B-7-12).
pub fn set_public_user_notify(job_id: &str) -> String {
    // CLI mode: drop "Waiting for ASPs to apply." — passive turn-end cue.
    if is_cli_mode() {
        format!("Job `{job_id}` is now public.")
    } else {
        format!("Job `{job_id}` is now public. Waiting for ASPs to apply.")
    }
}

// ── Event::SubmitExpired ───────────────────────────────────────────

/// `Event::SubmitExpired` — ASP missed the submit deadline (B-7-5).
pub fn submit_expired_user_notify(job_id: &str) -> String {
    if is_cli_mode() {
        format!(
            "Job `{job_id}` — the ASP did not submit the deliverable before the deadline. An auto-refund is in progress; funds will return to your wallet and a final refund-settled notice will follow shortly."
        )
    } else {
        format!(
            "Job `{job_id}` — the ASP did not submit the deliverable before the deadline. An auto-refund has been requested; funds will return to your wallet."
        )
    }
}

// ── Event::RejectExpired ───────────────────────────────────────────

/// `Event::RejectExpired` — ASP missed the dispute deadline (B-7-6).
pub fn reject_expired_user_notify(job_id: &str) -> String {
    if is_cli_mode() {
        format!(
            "Job `{job_id}` — the ASP did not file a dispute in time after you rejected the deliverable. An auto-refund is in progress; funds will return to your wallet and a final refund-settled notice will follow shortly."
        )
    } else {
        format!(
            "Job `{job_id}` — the ASP did not file a dispute in time after you rejected the deliverable. An auto-refund has been requested; funds will return to your wallet."
        )
    }
}

// ── Event::ReviewDeadlineWarn ──────────────────────────────────────

/// `Event::ReviewDeadlineWarn` — review deadline prompt (B-7-7).
pub fn review_deadline_warn_user_prompt(job_id: &str, short_id: &str) -> String {
    format!(
        "[Job {short_id} — you are the User Agent] [⏰ Review Deadline Warning] Job {job_id} — the review deadline is approaching.\n\
         After expiry, the ASP can auto-claim the funds.\n\
         Please decide soon:\n\
         A. Approve the deliverable\n\
         B. Reject the deliverable — please state your reason (if the ASP files a dispute, your rejection reason will be automatically submitted as evidence to the arbitrator)"
    )
}

// ── Event::RewardClaimed ───────────────────────────────────────────

/// `Event::RewardClaimed` — reward claimed (B-7-10).
pub fn reward_claimed_user_notify(job_id: &str, title: &str) -> String {
    format!("[Reward Claimed] {title} (`{job_id}`) — reward / refund successfully claimed to your wallet.")
}

// ── Event::WakeupNotify ────────────────────────────────────────────

/// `Event::WakeupNotify` — resume notification (B-7-15).
pub fn wakeup_resume_user_notify(job_id: &str) -> String {
    format!("[Resumed] Job `{job_id}` is back online. Please continue when ready.")
}

// ── provider_conversation — no more ASPs ───────────────────────────

/// `provider_conversation` B-Step 4 — no more ASPs pending (B-7-14).
pub fn no_more_sellers_user_notify(job_id: &str) -> String {
    format!("[Job `{job_id}` — you are the User Agent] All pending ASPs have been contacted; none remaining. Choose next step:")
}

// ── Attachment user notifications ─────────────────────────────────

/// Attachment sent successfully — notify the user.
pub fn attachment_sent_user_notify() -> &'static str {
    "[Job <short_jobId>] Attachment sent to the ASP."
}

// ── Escalation (preamble anomaly escalation) ───────────────────────

/// Preamble escalation hard rule 1) protocol misalignment (B-6-1).
pub fn escalation_protocol_misread_notify(job_id: &str) -> String {
    format!("[⚠️ Protocol Misalignment] Job `{job_id}` — the remote agent repeatedly sends messages that do not match the current flow. Replies have stopped. Please intervene manually to continue.")
}

// ── x402 replay result (job_payment_mode_changed) ────────────────

/// x402 replay success — deliverable received, awaiting on-chain confirmation.
pub fn x402_replay_success_user_notify(job_id: &str) -> String {
    let trailing = if is_cli_mode() {
        "\n         On-chain confirmation is in progress. The job will auto-complete and a final completion notice will follow shortly."
    } else {
        "\n         Waiting for on-chain confirmation. The job will auto-complete once confirmed."
    };
    format!(
        "[x402 Deliverable Received] Job `{job_id}` endpoint replayed successfully.\n\
         ASP agentId: <providerAgentId>\n\
         Amount: <tokenAmount> <tokenSymbol>\n\n\
         If CLI output contains `deliverableSavedPath`:\n\
         \x20\x20Deliverable saved to: <deliverableSavedPath>\n\n\
         If CLI output does NOT contain `deliverableSavedPath` (save failed):\n\
         \x20\x20---Deliverable---\n\
         \x20\x20<replayBodyDisplay in full>\n\
         \x20\x20---End of deliverable---{trailing}"
    )
}

// ── complete failure (job_accepted x402 branch) ──────────────────

/// x402 complete command failed — notify user with retry command.
pub fn complete_failed_user_notify(job_id: &str) -> String {
    format!(
        "[⚠️ Complete Failed] Job `{job_id}` — the completion step failed. \
         Please retry later or reply with a new instruction."
    )
}

// ── create_task notification ─────────────────────────────────────

/// create_task success — no designated provider (public task).
pub fn create_task_public_user_notify() -> String {
    "Job submitted (public); jobId: <jobId>; awaiting on-chain confirmation (~seconds). \
     Once confirmed, ASPs will be able to discover and apply for this task."
        .to_string()
}

/// create_task success — with designated provider.
pub fn create_task_designated_user_notify() -> String {
    "Job submitted; jobId: <jobId>; designated provider: <providerName> (agentId: <agentId>); \
     awaiting on-chain confirmation (~seconds). Once confirmed, the system will automatically connect with the designated provider."
        .to_string()
}

// ── provider_conversation — single ASP accept/reject card ────────

/// Canonical user-facing card for a single ASP accept/reject decision.
/// Placeholders are pre-filled; the LLM only needs to translate.
pub fn provider_pending_single_user_card(
    short_job_id: &str,
    title: &str,
    agent_id: &str,
    name: &str,
) -> String {
    let name_line = if name.is_empty() {
        String::new()
    } else {
        format!("Name: {name}\n")
    };
    format!(
        "[Job {short_job_id}] 「{title}」\n\
         \n\
         A provider wants to work on your task:\n\
         {name_line}\
         Agent ID: {agent_id}\n\
         \n\
         Accept this provider?\n\
         1. Accept\n\
         2. Reject"
    )
}

/// provider_conversation — pending list is empty; no ASPs to contact.
pub fn pending_list_empty_user_notify() -> String {
    "There are no ASPs to contact right now. You can wait for new ASPs to reach out, or reply \"close\" to close the task."
        .to_string()
}

// ── Escalation (preamble anomaly escalation) ───────────────────────

/// Preamble escalation hard rule 2) CLI execution error (B-6-2).
pub fn escalation_cli_failed_notify(job_id: &str) -> String {
    format!(
        "[⚠️ Operation Failed] Job `{job_id}`\n\
         - Action: <e.g. match ASPs / submit review / pay via x402>\n\
         - Error: <one-sentence summary of stderr / error field>\n\
         - Current status: <describe in plain language, e.g. waiting for provider / under review / payment pending>\n\
         \n\
         Choose how to proceed:\n\
         A. Retry → reply 'A' or 'retry'\n\
         B. Don't prompt again (you'll handle manually) → reply 'B' or 'dismiss'\n\
         C. Provide a new instruction → describe what to change (e.g. 'change --token-symbol to USDT and retry')"
    )
}
