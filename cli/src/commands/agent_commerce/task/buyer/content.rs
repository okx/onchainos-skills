//! Buyer-side message templates — single source of truth.
//!
//! Two categories of templates:
//!
//! 1. **User-facing** (`xmtp_dispatch_user(content)` / `xmtp_prompt_user(userContent)`)
//!    Chat content shown directly to the user. Naming suffix: `_user_notify` / `_user_prompt`.
//!    Rule: **no technical jargon** — tool names (`xmtp_*`) / event names (`provider_applied`/`job_*` etc.) /
//!    status names (English enums like `Open`/`accepted` are kept as doc-reserved literals) / CLI flags (`--*`) /
//!    skill names (`okx-agent-identity` etc.) / backend method names (`claimAutoComplete` etc.).
//!    **Literals in this file are English** (aligned with the PM Review translation baseline;
//!    source: `https://okg-block.sg.larksuite.com/docx/YSHcdZaWmo2KofxaHRuloeBYgme` §1),
//!    serving as the source-of-truth for sub agent LOCALIZATION_PREFIX translation — English users
//!    see them as-is; non-English users get an equivalent conversational translation from the sub agent.
//!    Terminology: Job (not Task), User Agent, ASP (Agent Service Provider),
//!    escrow / x402 lowercase, agentId in camelCase for data fields.
//!    Label format: `[Label]` bracket prefix (e.g. `[Job Accepted]`).
//!    Decision prompts (❓) carry the `[Job {short_id} — you are the User Agent]` prefix.
//!    User reply instructions use descriptive phrasing (naturally translatable by the sub agent).
//!
//! 2. **Peer-facing** (`xmtp_send` content, sent to the provider sub agent)
//!    Agent-to-agent protocol messages. Naming suffix: `_to_seller`.
//!    Rule: may contain protocol literals (`[intent:*]` etc.);
//!    **never instruct the peer to call CLI** (the peer has its own flow.rs and decides based on chain events;
//!    issuing commands to the peer is overreach).
//!
//! Field-value placeholders use `<...>`; the agent fills them from `common context` / session context.
//! To add copy → add a new fn; to edit copy → edit the fn body; flow.rs always calls here and never inlines literals.

// ── Event::JobCreated ──────────────────────────────────────────────

/// `Event::JobCreated` Step 0 — user notification that the job is confirmed on-chain.
pub fn job_created_user_notify(job_id: &str, notify_text: &str) -> String {
    format!("Job `{job_id}` confirmed on-chain (status: Open). {notify_text}")
}

/// Prompt shown when the designated ASP is offline (D-Step 1.5).
pub fn provider_offline_user_prompt(job_id: &str, short_id: &str, dp_id: &str) -> String {
    format!(
        "[Job {short_id} — you are the User Agent] The designated ASP (agentId={dp_id}) for job `{job_id}` \
         is currently offline. Negotiation requires the ASP to be online.\n\
         Please choose:\n\
         A. Designate another ASP — provide the agentId\n\
         B. Make the job public — let more ASPs discover it\n\
         C. Close the job"
    )
}

// ── Event::JobAccepted ─────────────────────────────────────────────

/// `Event::JobAccepted` Branch A (escrow) — user notification that the job is accepted (B-2-1).
pub fn job_accepted_escrow_user_notify(job_id: &str, _title: &str) -> String {
    format!(
        "[Job Accepted] Job `{job_id}` has been accepted; execution begins.\n\
         Title: <title>\n\
         Description: <description>\n\
         Deliverable: <deliverable>\n\
         ASP agentId: <providerAgentId>\n\
         Payment: escrow\n\
         Amount: <tokenAmount> <tokenSymbol>\n\
         Waiting for the ASP to execute and submit the deliverable."
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

// ── Event::JobRefused ──────────────────────────────────────────────

/// `Event::JobRefused` Step 1 — user notification that the rejection is confirmed on-chain.
pub fn job_refused_user_notify(job_id: &str, title: &str) -> String {
    format!(
        "[Rejection Confirmed] The deliverable for **{title}** (`{job_id}`) has been rejected; waiting for the ASP to respond.\n\
         The ASP has 24 hours to choose: file a dispute or agree to a refund.\n\
         If the ASP takes no action, funds will be auto-refunded to your wallet."
    )
}

// ── Event::JobDisputed ─────────────────────────────────────────────

/// `Event::JobDisputed` Step 1 — evidence collection prompt (B-5-3, `xmtp_prompt_user.userContent`).
pub fn job_disputed_user_evidence_prompt(short_id: &str) -> String {
    format!(
        "[Job {short_id} — you are the User Agent] The dispute is confirmed on-chain. You must submit off-chain evidence within 1 hour. Please provide:\n\
         - Text summary (required): key evidence that the deliverable failed the quality standards\n\
         - Image path (optional): local file path to screenshots, chat logs, etc.\n\
         Reply format example: \"Evidence: the deliverable is missing X/Y/Z; image: /path/to/screenshot.png\""
    )
}

// ── Event::JobCompleted ────────────────────────────────────────────

/// `Event::JobCompleted` Branch A (escrow) — user notification (B-4-1).
pub fn job_completed_escrow_user_notify(job_id: &str, title: &str) -> String {
    format!(
        "[Job Completed] {title} (`{job_id}`) — approved by the User Agent; funds released to the ASP.\n\
         - Spent: <tokenAmount> <tokenSymbol>\n\
         - Payment: escrow\n\
         - txHash: <txHash>\n\
         - Settled at: <timestamp>\n\
         This job is complete."
    )
}

/// `Event::JobCompleted` Branch B (x402) — final summary notification (B-4-3).
pub fn job_completed_x402_user_notify(job_id: &str, title: &str) -> String {
    format!(
        "[x402 Job Completed] {title} (`{job_id}`) — all steps complete.\n\
         - Spent: <tokenAmount> <tokenSymbol>\n\
         - Payment: x402\n\
         - Settled at: <timestamp>\n\
         To rate the ASP, reply with your rating."
    )
}

// ── Event::DisputeResolved ─────────────────────────────────────────

/// `Event::DisputeResolved` — user wins (B-5-4).
pub fn dispute_won_user_notify(job_id: &str, title: &str) -> String {
    format!(
        "[Dispute Won] {title} (`{job_id}`) — dispute resolved; User Agent wins.\n\
         - Refund: <tokenAmount> <tokenSymbol>\n\
         - Outcome: ClientWins\n\
         This job is complete. To rate the ASP, reply with your rating."
    )
}

/// `Event::DisputeResolved` — user loses (B-5-5).
pub fn dispute_lost_user_notify(job_id: &str, title: &str) -> String {
    format!(
        "[Dispute Lost] {title} (`{job_id}`) — dispute resolved; ASP wins.\n\
         - Loss: <tokenAmount> <tokenSymbol> (funds released to the ASP)\n\
         - Outcome: ProviderWins\n\
         This job is complete. To rate the ASP, reply with your rating."
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
        "Job `{job_id}` has expired (no ASP accepted before the accept deadline, or no deliverable submitted before the submit deadline). The job is now closed."
    )
}

// ── Event::JobClosed ───────────────────────────────────────────────

/// `Event::JobClosed` — job closed (B-7-2).
pub fn job_closed_user_notify(job_id: &str, title: &str) -> String {
    format!("{title} (`{job_id}`) has been closed; funds have been returned.")
}

// ── Event::JobVisibilityChanged ────────────────────────────────────

/// `Event::JobVisibilityChanged` visibility=0 — public (B-7-3).
pub fn visibility_public_user_notify(job_id: &str, title: &str) -> String {
    format!("[Visibility Changed] {title} (`{job_id}`) is now public. Waiting for ASPs to reach out.")
}

/// `Event::JobVisibilityChanged` visibility=1 — private (B-7-4).
pub fn visibility_private_user_notify(job_id: &str, title: &str) -> String {
    format!("[Visibility Changed] {title} (`{job_id}`) is now private.")
}

// ── Event::JobPaymentModeChanged ───────────────────────────────────

/// `Event::JobPaymentModeChanged` escrow branch — user notification (B-2-5).
pub fn payment_mode_escrow_user_notify(job_id: &str, title: &str) -> String {
    format!("{title} (`{job_id}`) — payment mode updated successfully; ASP <providerName> (<providerAgentId>) is accepting...")
}

/// x402 set-payment-mode confirmed on-chain; transition notification before task-402-pay.
pub fn x402_paying_user_notify(job_id: &str, title: &str) -> String {
    format!(
        "Payment in progress — **{title}** (`{job_id}`) — x402 agreement reached with the ASP; \
         fee: <tokenAmount> <tokenSymbol>. Paying and fetching the deliverable..."
    )
}

// ── Event::NegotiateReply (over budget) ────────────────────────────

/// `Event::NegotiateReply` — decision prompt when the ASP's quote exceeds max_budget.
pub fn over_budget_user_prompt(short_id: &str) -> String {
    format!(
        "[Job {short_id} — you are the User Agent] The ASP's quote exceeds the maximum budget; negotiation terminated. Choose next step:\n\
         A. View recommended ASP list\n\
         B. Designate another ASP — provide the agentId\n\
         C. Close the job"
    )
}

// ── Pseudo events (close / set_public) ─────────────────────────────

/// User notification after closing a job (B-7-11).
pub fn close_user_notify(job_id: &str) -> String {
    format!("Job `{job_id}` has been closed.")
}

/// User notification after switching a job to public (B-7-12).
pub fn set_public_user_notify(job_id: &str) -> String {
    format!("Job `{job_id}` is now public. Waiting for ASPs to apply.")
}

// ── Event::SubmitExpired ───────────────────────────────────────────

/// `Event::SubmitExpired` — ASP missed the submit deadline (B-7-5).
pub fn submit_expired_user_notify(job_id: &str) -> String {
    format!(
        "Job `{job_id}` — the ASP did not submit the deliverable before the deadline. An auto-refund has been requested; funds will return to your wallet."
    )
}

// ── Event::RefuseExpired ───────────────────────────────────────────

/// `Event::RefuseExpired` — ASP missed the dispute deadline (B-7-6).
pub fn refuse_expired_user_notify(job_id: &str) -> String {
    format!(
        "Job `{job_id}` — the ASP did not file a dispute in time after you rejected the deliverable. An auto-refund has been requested; funds will return to your wallet."
    )
}

// ── Event::ReviewDeadlineWarn ──────────────────────────────────────

/// `Event::ReviewDeadlineWarn` — review deadline prompt (B-7-7, `xmtp_prompt_user.userContent`).
pub fn review_deadline_warn_user_prompt(job_id: &str, short_id: &str) -> String {
    format!(
        "[Job {short_id} — you are the User Agent] [⏰ Review Deadline Warning] Job `{job_id}` — the review deadline is approaching.\n\
         After expiry, the ASP can auto-claim the funds.\n\
         Please decide soon:\n\
         A. Approve the deliverable\n\
         B. Reject the deliverable — please state your reason"
    )
}

// ── Event::ReviewExpired ───────────────────────────────────────────

/// `Event::ReviewExpired` — review window expired (B-7-8).
pub fn review_expired_user_notify(job_id: &str) -> String {
    format!(
        "[Review Expired] Job `{job_id}` — the review window has expired; you did not decide before the deadline.\n\
         The ASP can now claim the funds automatically. Waiting for the ASP's action..."
    )
}

// ── Event::JobAutoCompleted ────────────────────────────────────────

/// `Event::JobAutoCompleted` — job auto-completed (B-7-9).
pub fn job_auto_completed_user_notify(job_id: &str, title: &str) -> String {
    format!(
        "[Job Auto-Completed] {title} (`{job_id}`) — the review window expired and the ASP has claimed the funds.\n\
         Status: completed. This job is complete."
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
    format!("Job `{job_id}` is back online. Please continue your decision in the user session.")
}

// ── provider_conversation — no more ASPs ───────────────────────────

/// `provider_conversation` B-Step 4 — no more ASPs pending (B-7-14).
pub fn no_more_sellers_user_notify(job_id: &str) -> String {
    format!("[Job `{job_id}` — you are the User Agent] All pending ASPs have been contacted; none remaining. Choose next step:")
}

// ── Escalation (preamble anomaly escalation) ───────────────────────

/// Preamble escalation hard rule 1) protocol misalignment (B-6-1).
pub fn escalation_protocol_misread_notify(job_id: &str) -> String {
    format!("[⚠️ Protocol Misalignment] Job `{job_id}` — the remote agent repeatedly sends messages that do not match the current flow. Replies have stopped. Please intervene manually to continue.")
}

/// Preamble escalation hard rule 2) CLI execution error (B-6-2).
pub fn escalation_cli_failed_notify(job_id: &str) -> String {
    format!(
        "[⚠️ Operation Failed] Task `{job_id}`\n\
         - Action: <e.g. recommend providers / submit review / pay via x402>\n\
         - Error: <one-sentence summary of stderr / error field>\n\
         - Current status: <status>\n\
         \n\
         Choose how to proceed:\n\
         A. Retry → reply `A` or `retry`\n\
         B. Don't prompt again (you'll handle manually) → reply `B` or `dismiss`\n\
         C. Provide a new instruction → describe what to change (e.g. `change --token-symbol to USDT and retry`)"
    )
}
