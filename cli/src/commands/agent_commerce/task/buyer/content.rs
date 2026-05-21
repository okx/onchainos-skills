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
//!    Terminology: task → Job, user → User Agent, provider → ASP, agentId in camelCase,
//!    escrow/non-escrow/x402 in lowercase, user reply instructions in plain `"..."` double quotes.
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
        "[Job {short_id} — you are the User Agent] The designated ASP (agentId={dp_id}) for job {job_id} \
         is currently offline. Negotiation requires the ASP to be online. \
         Please choose:\n\
         A. Designate another ASP — please provide the agentId\n\
         B. Make the job public — let more ASPs discover it\n\
         C. Close the job"
    )
}

// ── Event::JobAccepted ─────────────────────────────────────────────

/// `Event::JobAccepted` Branch A (escrow) — user notification that the job is accepted.
pub fn job_accepted_escrow_user_notify(job_id: &str, title: &str) -> String {
    format!(
        "\x20\x20[Job Accepted] Job `{job_id}` has been accepted; execution begins.\n\
         \x20\x20Title: {title}\n\
         \x20\x20Description: <description>\n\
         \x20\x20Deliverable: <deliverable>\n\
         \x20\x20ASP agentId: <providerAgentId>\n\
         \x20\x20Payment: escrow\n\
         \x20\x20Amount: <tokenAmount> <tokenSymbol>\n\
         \x20\x20Waiting for the ASP to execute and submit the deliverable."
    )
}

/// `Event::JobAccepted` Branch B (x402) — user notification when endpoint replay failed.
pub fn job_accepted_x402_replay_fail_user_notify(job_id: &str) -> String {
    format!(
        "\x20\x20[x402 Replay Failed] Job `{job_id}` was accepted but the endpoint replay failed.\n\
         \x20\x20HTTP status: <replayStatus>\n\
         \x20\x20Error: <replayBody>\n\
         \x20\x20The job is now in `accepted` status. Please give a new instruction; the agent will not auto-retry."
    )
}

// ── Event::JobRefused ──────────────────────────────────────────────

/// `Event::JobRefused` Step 1 — user notification that the rejection is confirmed on-chain.
pub fn job_refused_user_notify(job_id: &str, title: &str) -> String {
    format!(
        "\x20\x20\x20\x20[Rejection Confirmed] The deliverable for **{title}** (`{job_id}`) has been rejected; waiting for the ASP to respond.\n\
         \x20\x20\x20\x20The ASP has 24 hours to choose: file a dispute or agree to a refund.\n\
         \x20\x20\x20\x20If the ASP takes no action, funds will be auto-refunded to your wallet."
    )
}

// ── Event::JobDisputed ─────────────────────────────────────────────

/// `Event::JobDisputed` Step 1 — evidence collection prompt (`xmtp_prompt_user.userContent`).
pub fn job_disputed_user_evidence_prompt(short_id: &str) -> String {
    format!(
        "\x20\x20\x20\x20[Job {short_id} — you are the User Agent] The dispute is confirmed on-chain. You must submit off-chain evidence within 1 hour. Please provide:\n\
         \x20\x20\x20\x20- Text summary (required): key evidence that the deliverable failed the quality standards\n\
         \x20\x20\x20\x20- Image path (optional): local file path to screenshots, chat logs, etc.\n\
         \x20\x20\x20\x20Reply format example: \"Evidence: the deliverable is missing X/Y/Z; image: /path/to/screenshot.png\""
    )
}

// ── Event::JobCompleted ────────────────────────────────────────────

/// `Event::JobCompleted` Branch A (escrow) — user notification that the job is complete.
///
/// Ends with a prompt to rate the ASP (0–5 stars). When the user replies `rate`,
/// it is handled by the `okx-agent-identity` skill (`§Feedback Submit`);
/// does not include rating details / CLI flags here.
pub fn job_completed_escrow_user_notify(job_id: &str, title: &str) -> String {
    format!(
        "\x20\x20\x20\x20[Job Completed] {title} (`{job_id}`) — approved by the User Agent; funds released to the ASP.\n\
         \x20\x20\x20\x20  - Spent: <tokenAmount> <tokenSymbol>\n\
         \x20\x20\x20\x20  - Payment: escrow\n\
         \x20\x20\x20\x20  - txHash: <txHash>\n\
         \x20\x20\x20\x20  - Settled at: <timestamp>\n\
         \x20\x20\x20\x20\n\
         \x20\x20\x20\x20This job is complete. To rate the ASP (0–5 stars), reply \"rate\"."
    )
}

/// `Event::JobCompleted` Branch B (x402) — final summary notification to the user.
///
/// Ends with a prompt to rate the ASP (0–5 stars). When the user replies `rate`,
/// it is handled by the `okx-agent-identity` skill (`§Feedback Submit`);
/// does not include rating details / CLI flags here.
pub fn job_completed_x402_user_notify(job_id: &str, title: &str) -> String {
    format!(
        "\x20\x20\x20\x20[x402 Job Completed] {title} (`{job_id}`) — all steps complete.\n\
         \x20\x20\x20\x20  - Spent: <tokenAmount> <tokenSymbol>\n\
         \x20\x20\x20\x20  - Payment: x402\n\
         \x20\x20\x20\x20  - Settled at: <timestamp>\n\
         \x20\x20\x20\x20To rate the ASP (0–5 stars), reply \"rate\"."
    )
}

// ── Event::DisputeResolved ─────────────────────────────────────────

/// `Event::DisputeResolved` — user notification when the user wins the dispute.
///
/// Ends with a prompt to rate the ASP (0–5 stars). When the user replies `rate`,
/// it is handled by the `okx-agent-identity` skill (`§Feedback Submit`).
pub fn dispute_won_user_notify(job_id: &str, title: &str) -> String {
    format!(
        "\x20\x20\x20\x20[Dispute Won] {title} (`{job_id}`) — dispute resolved; User Agent wins.\n\
         \x20\x20\x20\x20  - Refund: <tokenAmount> <tokenSymbol>\n\
         \x20\x20\x20\x20  - Outcome: ClientWins\n\
         \x20\x20\x20\x20This job is complete. To rate the ASP (0–5 stars), reply \"rate\"."
    )
}

/// `Event::DisputeResolved` — user notification when the user loses the dispute.
///
/// Ends with a prompt to rate the ASP (0–5 stars). When the user replies `rate`,
/// it is handled by the `okx-agent-identity` skill (`§Feedback Submit`).
pub fn dispute_lost_user_notify(job_id: &str, title: &str) -> String {
    format!(
        "\x20\x20\x20\x20[Dispute Lost] {title} (`{job_id}`) — dispute resolved; ASP wins.\n\
         \x20\x20\x20\x20  - Loss: <tokenAmount> <tokenSymbol> (funds released to the ASP)\n\
         \x20\x20\x20\x20  - Outcome: ProviderWins\n\
         \x20\x20\x20\x20This job is complete. To rate the ASP (0–5 stars), reply \"rate\"."
    )
}

// ── Event::JobRefunded ─────────────────────────────────────────────

/// `Event::JobRefunded` — user notification that the refund is settled.
pub fn job_refunded_user_notify(job_id: &str) -> String {
    format!(
        "\x20\x20\x20\x20[Refund Settled] Job `{job_id}` — refund confirmed on-chain; funds returned to your wallet. This job is complete."
    )
}

// ── Event::JobAutoRefunded ─────────────────────────────────────────

/// `Event::JobAutoRefunded` — user notification that the auto-refund succeeded.
pub fn job_auto_refunded_user_notify(job_id: &str, title: &str) -> String {
    format!(
        "\x20\x20\x20\x20[Auto-Refund Settled] {title} (`{job_id}`) — escrowed funds returned to your wallet. This job is complete."
    )
}

// ── Event::JobExpired ──────────────────────────────────────────────

/// `Event::JobExpired` — user notification that the job has expired.
pub fn job_expired_user_notify(job_id: &str) -> String {
    format!(
        "Job `{job_id}` has expired (no ASP accepted before the accept deadline, or no deliverable submitted before the submit deadline). The job is now closed."
    )
}

// ── Event::JobClosed ───────────────────────────────────────────────

/// `Event::JobClosed` — user notification that the job is closed.
pub fn job_closed_user_notify(job_id: &str, title: &str) -> String {
    format!("{title} (`{job_id}`) has been closed; funds have been returned.")
}

// ── Event::JobVisibilityChanged ────────────────────────────────────

/// `Event::JobVisibilityChanged` visibility=0 — public notification.
pub fn visibility_public_user_notify(job_id: &str, title: &str) -> String {
    format!("[Visibility Changed] {title} (`{job_id}`) is now public. Waiting for ASPs to reach out.")
}

/// `Event::JobVisibilityChanged` visibility=1 — private notification.
pub fn visibility_private_user_notify(job_id: &str, title: &str) -> String {
    format!("[Visibility Changed] {title} (`{job_id}`) is now private.")
}

// ── Event::JobPaymentModeChanged ───────────────────────────────────

/// `Event::JobPaymentModeChanged` escrow branch Step 4 — user notification.
pub fn payment_mode_escrow_user_notify(job_id: &str, title: &str) -> String {
    format!("{title} (`{job_id}`) — payment mode updated successfully; ASP <providerName> (`<providerAgentId>`) is accepting...")
}

/// x402 set-payment-mode confirmed on-chain; transition notification before task-402-pay.
pub fn x402_paying_user_notify(job_id: &str, title: &str) -> String {
    format!(
        "[x402 Payment In Progress] Job **{title}** (`{job_id}`) — x402 agreement reached with ASP (<providerAgentId>); \
         fee: <tokenAmount> <tokenSymbol>. Paying and fetching the deliverable..."
    )
}


// ── Event::NegotiateReply (over budget) ────────────────────────────

/// `Event::NegotiateReply` — decision prompt when the ASP's quote exceeds max_budget.
pub fn over_budget_user_prompt(short_id: &str) -> String {
    format!(
        "\x20\x20\x20\x20[Task {short_id}] The ASP's quote exceeds the maximum budget; negotiation terminated. Choose next step:\n\
         \x20\x20\x20\x20\x20\x20A. View recommended ASP list\n\
         \x20\x20\x20\x20\x20\x20B. Designate another ASP (provide the agentId)\n\
         \x20\x20\x20\x20\x20\x20C. Close the job"
    )
}

// ── Pseudo events (close / set_public) ─────────────────────────────

/// User notification after closing a job.
pub fn close_user_notify(job_id: &str) -> String {
    format!("Job `{job_id}` has been closed.")
}

/// User notification after switching a job to public.
pub fn set_public_user_notify(job_id: &str) -> String {
    format!("Job `{job_id}` is now public. Waiting for ASPs to apply.")
}

// ── Event::SubmitExpired ───────────────────────────────────────────

/// `Event::SubmitExpired` — user notification that the ASP missed the submit deadline.
pub fn submit_expired_user_notify(job_id: &str) -> String {
    format!(
        "Job `{job_id}` — the ASP did not submit the deliverable before the deadline. An auto-refund has been requested; funds will return to your wallet."
    )
}

// ── Event::RefuseExpired ───────────────────────────────────────────

/// `Event::RefuseExpired` — user notification that the ASP missed the dispute deadline.
pub fn refuse_expired_user_notify(job_id: &str) -> String {
    format!(
        "Job `{job_id}` — the ASP did not file a dispute in time after you rejected the deliverable. An auto-refund has been requested; funds will return to your wallet."
    )
}

// ── Event::ReviewDeadlineWarn ──────────────────────────────────────

/// `Event::ReviewDeadlineWarn` — review deadline prompt (`xmtp_prompt_user.userContent`).
pub fn review_deadline_warn_user_prompt(job_id: &str) -> String {
    format!(
        "\x20\x20[⏰ Review Deadline Warning] Job `{job_id}` — the review deadline is approaching.\n\
         \x20\x20After expiry, the ASP can auto-claim the funds.\n\
         \x20\x20Please decide soon:\n\
         \x20\x20A. Approve → reply \"approve\"\n\
         \x20\x20B. Reject → reply \"reject\" and provide  {{reason}}"
    )
}

// ── Event::ReviewExpired ───────────────────────────────────────────

/// `Event::ReviewExpired` — user notification that the review window has expired.
pub fn review_expired_user_notify(job_id: &str) -> String {
    format!(
        "\x20\x20[Review Expired] Job `{job_id}` — the review window has expired; you did not decide before the deadline.\n\
         \x20\x20The ASP can now claim the funds automatically. Waiting for the ASP's action..."
    )
}

// ── Event::JobAutoCompleted ────────────────────────────────────────

/// `Event::JobAutoCompleted` — user notification that the job was auto-completed.
pub fn job_auto_completed_user_notify(job_id: &str, title: &str) -> String {
    format!(
        "\x20\x20[Job Auto-Completed] {title} (`{job_id}`) — the review window expired and the ASP has claimed the funds.\n\
         \x20\x20Status: completed. This job is complete."
    )
}

// ── Event::RewardClaimed ───────────────────────────────────────────

/// `Event::RewardClaimed` — user notification that the reward has been claimed.
pub fn reward_claimed_user_notify(job_id: &str, title: &str) -> String {
    format!("[Reward Claimed] {title} (`{job_id}`) — reward / refund successfully claimed to your wallet.")
}

// ── Event::WakeupNotify ────────────────────────────────────────────

/// `Event::WakeupNotify` — user notification to resume when pending decisions exist.
pub fn wakeup_resume_user_notify(job_id: &str) -> String {
    format!("Job `{job_id}` is back online. Please continue your decision in the user session.")
}

// ── provider_conversation — no more ASPs ───────────────────────────

/// `provider_conversation` B-Step 4 — notification when no more ASPs are pending.
pub fn no_more_sellers_user_notify(job_id: &str) -> String {
    format!("Job `{job_id}` — no more pending ASPs. Wait for new ASPs to reach out, or adjust the job description.")
}

// ── Escalation (preamble anomaly escalation) ───────────────────────

/// Preamble escalation hard rule 1) protocol misalignment — content template.
pub fn escalation_protocol_misread_notify(job_id: &str) -> String {
    format!("[⚠️ Protocol Misalignment] Job `{job_id}` — the remote agent repeatedly sends messages that do not match the current flow. Replies have stopped. Please intervene manually to continue.")
}

/// Preamble escalation hard rule 2) CLI execution error — content template.
pub fn escalation_cli_failed_notify(job_id: &str) -> String {
    format!("[⚠️ CLI Error] Job `{job_id}` <action summary, e.g. \"confirm accept\" / \"approve deliverable\" / \"submit evidence\"> failed. Please review and give a new instruction; the agent will not auto-retry.")
}
