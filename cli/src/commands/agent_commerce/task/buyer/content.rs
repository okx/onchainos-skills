//! Buyer-side message templates — single source of truth.
//!
//! Two categories of templates:
//!
//! 1. **User-facing** (`xmtp_dispatch_user(content)` / `xmtp_prompt_user(userContent)`)
//!    Chat content shown directly to the user. Naming suffix: `_user_notify` / `_user_prompt`.
//!    Rule: **no technical jargon** — tool names (`xmtp_*`) / event names (`provider_applied`/`job_*` etc.) /
//!    status names (English enums like `Open`/`accepted` are kept as doc-reserved literals) / CLI flags (`--*`) /
//!    skill names (`okx-agent-identity` etc.) / backend method names (`claimAutoComplete` etc.).
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
//! 2. **Peer-facing** (`xmtp_send` content, sent to the provider sub agent)
//!    Agent-to-agent protocol messages. Naming suffix: `_to_seller`.
//!    Rule: may contain protocol literals (`[intent:*]` etc.);
//!    **never instruct the peer to call CLI** (the peer has its own flow.rs and decides based on chain events;
//!    issuing commands to the peer is overreach).
//!
//! Field-value placeholders use `<...>`; the agent fills them from `common context` / session context.
//! To add copy → add a new fn; to edit copy → edit the fn body; flow.rs always calls here and never inlines literals.

// ── Platform detection ────────────────────────────────────────────

pub fn is_cli_mode() -> bool {
    std::env::var("CLAUDECODE").unwrap_or_default() == "1"
        || std::env::var("CODEX_THREAD_ID")
            .ok()
            .filter(|s| !s.is_empty())
            .is_some()
}

// ── Event::JobCreated ──────────────────────────────────────────────

/// `Event::JobCreated` Step 0 — user notification (no designated provider).
pub fn job_created_non_designated_user_notify() -> &'static str {
    "[Job Created]【<title>】(<short_jobId>) confirmed on-chain. Auto-querying recommended ASPs."
}

/// `Event::JobCreated` Step 0 — user notification (with designated provider).
/// Used both for first-time creation and re-entry (recommend_pick / no_asp_found designate).
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
    // (Claude Code / Codex) to stop prematurely. Reword it to active
    // "Watching for ..." phrasing in CLI mode so the LLM keeps watch alive;
    // keep the original wording for native push clients (Hermes / OpenClaw)
    // where the user reads the notification directly and no LLM is making
    // the stop decision.
    let trailing = if is_cli_mode() {
        "\n         Watching for the ASP's deliverable; the next notification will arrive on submission."
    } else {
        "\n         Waiting for the ASP to execute and submit the deliverable."
    };
    format!(
        "[Job Accepted] Job `{job_id}` has been accepted; execution begins.\n\
         Title: <title>\n\
         Description: <description>\n\
         Deliverable: <deliverable>\n\
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
    format!(
        "[Rejection Confirmed] The deliverable for【{title}】(`{job_id}`) has been rejected; waiting for the ASP to respond.\n\
         The ASP will choose: file a dispute or agree to a refund.\n\
         If the ASP takes no action, funds will be auto-refunded to your wallet."
    )
}

// ── Event::JobCompleted ────────────────────────────────────────────

/// `Event::JobCompleted` Branch A (escrow) — user notification (B-4-1).
pub fn job_completed_escrow_user_notify(job_id: &str, title: &str) -> String {
    format!(
        "[Job Completed] {title} (`{job_id}`) — approved by the User Agent; funds released to the ASP.\n\
         - Spent: <tokenAmount> <tokenSymbol>\n\
         - Payment: escrow\n\
         - txHash: <txHash>"
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
         - Outcome: ProviderWins\n\
         {ARBITRATION_REASONS_BLOCK}\n\
         This job is complete."
    )
}

// ── Auto-rating notification ──────────────────────────────────────

/// User notification after the buyer agent auto-rates the ASP.
pub fn rating_submitted_user_notify(job_id: &str) -> String {
    format!(
        "[📝 Rating Submitted] Job <title> (`{job_id}`) — rated.\n\
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
        "Job `{job_id}` has expired (no ASP accepted before the acceptance window expired, or no deliverable submitted before the delivery window expired). The job is now closed."
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
        "Payment in progress —【{title}】(`{job_id}`) — x402 agreement reached with the ASP; \
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

// ── Event::RejectExpired ───────────────────────────────────────────

/// `Event::RejectExpired` — ASP missed the dispute deadline (B-7-6).
pub fn reject_expired_user_notify(job_id: &str) -> String {
    format!(
        "Job `{job_id}` — the ASP did not file a dispute in time after you rejected the deliverable. An auto-refund has been requested; funds will return to your wallet."
    )
}

// ── Event::ReviewDeadlineWarn ──────────────────────────────────────

/// `Event::ReviewDeadlineWarn` — review deadline prompt (B-7-7, `xmtp_prompt_user.userContent`).
pub fn review_deadline_warn_user_prompt(job_id: &str, short_id: &str) -> String {
    format!(
        "[Job {short_id} — you are the User Agent] [⏰ Review Deadline Warning] Job {job_id} — the review deadline is approaching.\n\
         After expiry, the ASP can auto-claim the funds.\n\
         Please decide soon:\n\
         A. Approve the deliverable\n\
         B. Reject the deliverable — please state your reason (if the ASP files a dispute, your rejection reason will be automatically submitted as evidence to the arbitrator)"
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
         Status: completed."
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

// ── Attachment user notifications ─────────────────────────────────

/// Attachment sent successfully — notify the user.
pub fn attachment_sent_user_notify() -> &'static str {
    "[Job <short_jobId>] Attachment sent to the ASP."
}

/// Attachment saved locally (no active session yet) — notify the user.
pub fn attachment_saved_user_notify() -> &'static str {
    "[Job <short_jobId>] Attachment saved. It will be forwarded to the ASP once a negotiation session is established."
}

/// Attachment rejected — task is in review/terminal phase.
pub fn attachment_phase_blocked_user_notify() -> &'static str {
    "[Job <short_jobId>] The task has entered the review/terminal phase — attachments can no longer be added."
}

// ── Attachment (buyer → provider) ──────────────────────────────────

/// File attachment `xmtp_send` content sent from the buyer sub session
/// to the provider sub session.
///
/// The 6 fields (`fileKey` / `digest` / `salt` / `nonce` / `secret` /
/// `filename`) come from `xmtp_file_upload`; the provider sub agent
/// parses them and calls `xmtp_file_download` to fetch the file.
pub fn attachment_file_to_seller(job_id: &str) -> String {
    format!(
        "jobId: {job_id}\n\
         attachmentType: file\n\
         fileKey: <fileKey from xmtp_file_upload — FULL value, no truncation>\n\
         digest: <digest — FULL hex string, no truncation>\n\
         salt: <salt — FULL base64 string, no truncation>\n\
         nonce: <nonce — FULL base64 string, no truncation>\n\
         secret: <secret — FULL base64 string, no truncation (can be 100+ chars)>\n\
         filename: <filename from xmtp_file_upload>\n\
         description: <brief one-line description of the attachment>\n\
         [intent:attachment]"
    )
}

// ── Escalation (preamble anomaly escalation) ───────────────────────

/// Preamble escalation hard rule 1) protocol misalignment (B-6-1).
pub fn escalation_protocol_misread_notify(job_id: &str) -> String {
    format!("[⚠️ Protocol Misalignment] Job `{job_id}` — the remote agent repeatedly sends messages that do not match the current flow. Replies have stopped. Please intervene manually to continue.")
}

// ── x402 replay result (job_payment_mode_changed) ────────────────

/// x402 replay success — deliverable received, awaiting on-chain confirmation.
pub fn x402_replay_success_user_notify(job_id: &str) -> String {
    format!(
        "[x402 Deliverable Received] Job `{job_id}` endpoint replayed successfully.\n\
         ASP agentId: <providerAgentId>\n\
         Amount: <tokenAmount> <tokenSymbol>\n\
         Deliverable saved to: <deliverableSavedPath from CLI output>\n\
         ---Deliverable---\n\
         <replayBodyDisplay value from CLI output — pass through in full, do not truncate or summarize>\n\
         ---End of deliverable---\n\
         Waiting for on-chain confirmation. The job will auto-complete once confirmed."
    )
}

/// x402 replay failure — accepted but endpoint replay failed.
pub fn x402_replay_fail_user_notify(job_id: &str) -> String {
    format!(
        "[x402 Replay Failed] Job `{job_id}` was accepted but the endpoint replay failed.\n\
         HTTP status: <replayStatus>\n\
         Error: <replayBody>\n\
         Auto-complete will not run. Please give a new instruction; the agent will not auto-retry."
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

/// create_task success — no designated provider.
pub fn create_task_public_user_notify() -> String {
    "Job submitted; jobId: <jobId>; awaiting on-chain confirmation (~seconds). \
     Once confirmed, the system will automatically fetch the recommended provider list for you to choose from."
        .to_string()
}

/// create_task success — with designated provider.
pub fn create_task_designated_user_notify() -> String {
    "Job submitted; jobId: <jobId>; designated provider: <providerName> (agentId: <agentId>); \
     awaiting on-chain confirmation (~seconds). Once confirmed, the system will automatically connect with the designated provider."
        .to_string()
}

// ── draft notifications ─────────────────────────────────────────

/// Draft saved — user notification.
pub fn draft_saved_user_notify() -> String {
    "Draft saved (jobId: <jobId>). You can continue editing it later, or publish it when ready."
        .to_string()
}

/// Draft updated — user notification.
pub fn draft_updated_user_notify() -> String {
    "Draft updated (jobId: <jobId>)."
        .to_string()
}

/// Draft deleted — user notification.
pub fn draft_deleted_user_notify() -> String {
    "Draft deleted (jobId: <jobId>)."
        .to_string()
}

/// Draft publish success — no designated provider (same downstream flow as create-task).
pub fn draft_publish_public_user_notify() -> String {
    "Draft published; jobId: <jobId>; awaiting on-chain confirmation (~seconds). \
     Once confirmed, the system will automatically fetch the recommended provider list for you to choose from."
        .to_string()
}

/// Draft publish success — with designated provider.
pub fn draft_publish_designated_user_notify() -> String {
    "Draft published; jobId: <jobId>; designated provider: <providerName> (agentId: <agentId>); \
     awaiting on-chain confirmation (~seconds). Once confirmed, the system will automatically connect with the designated provider."
        .to_string()
}

// ── pending_list empty (provider_conversation) ───────────────────

/// provider_conversation — user chose "skip all" pending ASPs.
pub fn skip_all_pending_user_notify(job_id: &str) -> String {
    format!("Job `{job_id}` — all pending ASPs have been skipped. You can wait for new ASPs to reach out, or reply \"close\" to close the task.")
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
         - Action: <e.g. recommend providers / submit review / pay via x402>\n\
         - Error: <one-sentence summary of stderr / error field>\n\
         - Current status: <status>\n\
         \n\
         Choose how to proceed:\n\
         A. Retry → reply 'A' or 'retry'\n\
         B. Don't prompt again (you'll handle manually) → reply 'B' or 'dismiss'\n\
         C. Provide a new instruction → describe what to change (e.g. 'change --token-symbol to USDT and retry')"
    )
}
