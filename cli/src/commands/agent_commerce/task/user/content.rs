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
         B. Close the job"
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
        short_id,
        dp_id,
        job_id,
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
    // was translated back to the Chinese word for "waiting", reintroducing the passive cue. Drop
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
        format!(
            "[Rejection Confirmed] The deliverable for【{title}】(`{job_id}`) has been rejected."
        )
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
pub fn job_completed_escrow_user_notify(
    job_id: &str,
    title: &str,
    token_amount: &str,
    token_symbol: &str,
) -> String {
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

/// Per-evaluator verdict rationales block shared by both `DisputeResolved` outcomes.
/// Source field: `message.voteReportSummaries[*].voterReportSummary` from the system envelope.
const EVALUATION_REASONS_BLOCK: &str = concat!(
    "- Evaluation reasons:\n",
    "    Evaluator 1: <voterReportSummary from message.voteReportSummaries[0]>\n",
    "    Evaluator 2: <voterReportSummary from message.voteReportSummaries[1]>\n",
    "    ... (one line per entry; first skip entries whose voterReportSummary is missing / empty / whitespace, then number the kept entries consecutively starting at 1 in array order — do NOT preserve gaps from the original index; omit this whole `- Evaluation reasons:` section if voteReportSummaries is missing, not an array, empty, or every entry would be skipped — do NOT print a header with no body, do NOT fabricate filler text)",
);

/// `Event::DisputeResolved` — user wins (B-5-4).
pub fn dispute_won_user_notify(job_id: &str, title: &str) -> String {
    format!(
        "[Dispute Won] {title} (`{job_id}`) — dispute resolved; User Agent wins.\n\
         - Refund: <tokenAmount> <tokenSymbol>\n\
         - Outcome: ClientWins\n\
         {EVALUATION_REASONS_BLOCK}\n\
         This job is complete."
    )
}

/// `Event::DisputeResolved` — user loses (B-5-5).
pub fn dispute_lost_user_notify(job_id: &str, title: &str) -> String {
    format!(
        "[Dispute Lost] {title} (`{job_id}`) — dispute resolved; ASP wins.\n\
         - Loss: <tokenAmount> <tokenSymbol> (funds released to the ASP)\n\
         - Outcome: ASPWins\n\
         {EVALUATION_REASONS_BLOCK}\n\
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

// ── Pseudo events (close) ──────────────────────────────────────────

/// User notification after closing a job (B-7-11).
pub fn close_user_notify(job_id: &str) -> String {
    format!("[Job Closed] Job `{job_id}` has been closed.")
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
         B. Reject the deliverable — please state your reason (if the ASP files a dispute, your rejection reason will be automatically submitted as evidence to the Evaluator)"
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

/// create_task success — with designated provider.
pub fn create_task_designated_user_notify() -> String {
    "Job submitted; jobId: <jobId>; designated provider: <providerName> (agentId: <agentId>); \
     awaiting on-chain confirmation (~seconds). Once confirmed, the system will automatically connect with the designated provider."
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

// ── Subscription notifications (display-class) ─────────────────────
// Variables map to the event body fields
// (serviceName→jobTitle, amount→tokenAmount, periodStart/End→subStartTime/subEndTime,
// trialEndsAt→trialEndTime (legacy fallback trailEndTime), graceEndsAt→subBufferEndTime). The next-charge date
// (`nextChargeAt`) = the current period's `periodEnd` = `subEndTime`, so it is rendered
// directly from `subEndTime` (not a separate/fabricated field); `nextPeriodEnd` is likewise
// superseded by the rendered period range (subStartTime/subEndTime). The remaining forward-compatible
// field NOT in the event body — `rejectWindowEndsAt` — is still conditional-rendered: its clause
// appears only when present and is omitted gracefully otherwise (no fabrication).

/// Format an epoch-seconds timestamp (`subStartTime` / `subEndTime` / trial window / buffer) as
/// `YYYY-MM-DD HH:MM UTC`. Returns `None` for an absent (`0` / negative) or unrepresentable value
/// so callers omit the clause.
fn fmt_epoch(ts: Option<i64>) -> Option<String> {
    let ts = ts.filter(|t| *t > 0)?;
    // Backend timestamps are contract-seconds; tolerate a millisecond-scale value
    // (>= 1e12 could only be year 33658+ as seconds) so a unit drift upstream
    // renders a sane date instead of a five-digit year.
    let ts = if ts >= 1_000_000_000_000 {
        ts / 1000
    } else {
        ts
    };
    chrono::DateTime::<chrono::Utc>::from_timestamp(ts, 0)
        .map(|dt| dt.format("%Y-%m-%d %H:%M UTC").to_string())
}

/// `sub_created` — subscription confirmed, first charge taken (user).
pub fn sub_created_user_notify(
    job_id: &str,
    service_name: &str,
    token_amount: Option<&str>,
    token_symbol: Option<&str>,
    period_start: Option<i64>,
    period_end: Option<i64>,
    auto_renew: bool,
) -> String {
    let mut out = format!(
        "[Subscribed] Job {job_id} (subscribing to {service_name}) is on-chain, status: Active"
    );
    if let (Some(s), Some(e)) = (fmt_epoch(period_start), fmt_epoch(period_end)) {
        out.push_str(&format!(", current period {s}–{e}"));
    }
    out.push('.');
    if let Some(amt) = token_amount {
        match token_symbol {
            Some(sym) => out.push_str(&format!(" First charge of {amt} {sym} completed.")),
            None => out.push_str(&format!(" First charge of {amt} completed.")),
        }
    }
    if auto_renew {
        out.push_str(" Auto-renew is on");
        if let Some(nc) = fmt_epoch(period_end) {
            out.push_str(&format!("; next charge date: {nc}"));
        }
    } else {
        out.push_str(" Auto-renew is off");
    }
    out.push('.');
    out
}

/// `sub_created` with `trialType=1` — free trial started, nothing charged yet (user).
/// Renders the trial-start copy: the trial window is charge-free, so the
/// immediate-first-charge copy from `sub_created_user_notify` must never be shown
/// for a trial order (the real first charge is announced by `sub_trial_into_active`).
/// The duration label slot (`{trialDisplay}`) has no envelope source, so only the
/// date range renders; the charge sentence needs an amount and degrades away without one.
pub fn sub_created_trial_user_notify(
    token_amount: Option<&str>,
    token_symbol: Option<&str>,
    trial_start: Option<i64>,
    trial_end: Option<i64>,
) -> String {
    let mut out = String::from("[Trial Started] Your free trial is active");
    if let (Some(s), Some(e)) = (fmt_epoch(trial_start), fmt_epoch(trial_end)) {
        out.push_str(&format!(" ({s}\u{2013}{e})"));
    }
    out.push('.');
    if let Some(amt) = token_amount {
        match token_symbol {
            Some(sym) => out.push_str(&format!(" After it ends, {amt} {sym} will be auto-charged")),
            None => out.push_str(&format!(" After it ends, {amt} will be auto-charged")),
        }
        if let Some(e) = fmt_epoch(trial_end) {
            out.push_str(&format!(" on {e}"));
        }
        out.push_str(
            " to convert to a paid subscription (attempted once, within the final hour before the trial ends \u{2014} it will not retry if missed).",
        );
    }
    out
}

/// `sub_trial_into_active` — free trial ended, first real charge taken (user).
pub fn sub_trial_into_active_user_notify(
    job_id: &str,
    service_name: &str,
    token_amount: Option<&str>,
    token_symbol: Option<&str>,
    period_start: Option<i64>,
    period_end: Option<i64>,
) -> String {
    let mut out = String::from("[Trial Converted] Your free trial has ended;");
    match (token_amount, token_symbol) {
        (Some(amt), Some(sym)) => out.push_str(&format!(
            " the first charge of {amt} {sym} for \"{service_name}\" is complete"
        )),
        (Some(amt), None) => out.push_str(&format!(
            " the first charge of {amt} for \"{service_name}\" is complete"
        )),
        _ => out.push_str(&format!(
            " the first charge for \"{service_name}\" is complete"
        )),
    }
    // Append the current period range (periodStart/End = subStartTime/subEndTime).
    if let (Some(s), Some(e)) = (fmt_epoch(period_start), fmt_epoch(period_end)) {
        out.push_str(&format!(", current period {s}–{e}"));
    }
    out.push('.');
    out.push_str(&format!(" Job {job_id} status: Active."));
    // `nextChargeAt` = `periodEnd` = `subEndTime`; render from `period_end`, omit if absent.
    if let Some(nc) = fmt_epoch(period_end) {
        out.push_str(&format!(" Next charge date: {nc}."));
    }
    out
}

/// `sub_renew` — renewal outcome (user). `renew_result == "fail"` → grace-period failure
/// copy with `failReason` verbatim (sub_renew carries a `renewResult=fail` branch;
/// see the joint-testing flag in the feedback — confirm in joint testing which event carries
/// renewal failure); otherwise the success copy.
#[allow(clippy::too_many_arguments)] // display renderer threads the mapped event body fields
pub fn sub_renew_user_notify(
    renew_result: Option<&str>,
    fail_reason: Option<&str>,
    service_name: &str,
    job_id: &str,
    token_amount: Option<&str>,
    token_symbol: Option<&str>,
    _period_start: Option<i64>,
    period_end: Option<i64>,
    grace_ends_at: Option<i64>,
) -> String {
    if renew_result == Some("fail") {
        let mut out =
            format!("[⚠️ Renewal Failed] \"{service_name}\" — this cycle's charge failed");
        if let Some(reason) = fail_reason {
            out.push_str(&format!(": {reason}"));
        }
        out.push_str(". A grace period is in effect");
        if let Some(g) = fmt_epoch(grace_ends_at) {
            out.push_str(&format!(" (until {g})"));
        }
        out.push_str(
            "; service continues normally and the system will keep retrying. Please add funding / increase allowance as soon as possible.",
        );
        out
    } else {
        let mut out = format!("[Renewed] \"{service_name}\" —");
        match (token_amount, token_symbol) {
            (Some(amt), Some(sym)) => out.push_str(&format!(
                " this cycle's renewal of {amt} {sym} is complete."
            )),
            (Some(amt), None) => {
                out.push_str(&format!(" this cycle's renewal of {amt} is complete."))
            }
            _ => out.push_str(" this cycle's renewal is complete."),
        }
        // Renewal keeps the same billing cycle; the period range is intentionally not repeated here.
        out.push_str(&format!(" Job {job_id} status: Active"));
        // `nextChargeAt` = `periodEnd` = `subEndTime`; render from `period_end`, omit if absent.
        if let Some(nc) = fmt_epoch(period_end) {
            out.push_str(&format!(". Next charge date: {nc}."));
        } else {
            out.push('.');
        }
        out
    }
}

/// `sub_user_reject` (user side) — the user's rejection for the current period was
/// submitted; the ASP must respond or the period is auto-refunded.
pub fn sub_user_reject_user_notify(
    service_name: &str,
    period_start: Option<i64>,
    period_end: Option<i64>,
    reject_window_ends_at: Option<i64>,
    token_amount: Option<&str>,
    token_symbol: Option<&str>,
) -> String {
    let mut out =
        format!("[Rejection Submitted] Your rejection for \"{service_name}\"'s current period");
    if let (Some(s), Some(e)) = (fmt_epoch(period_start), fmt_epoch(period_end)) {
        out.push_str(&format!(" ({s}–{e})"));
    }
    out.push_str(" has been submitted. The ASP must respond");
    // `rejectWindowEndsAt` is NOT in the event body — backend enhancement pending. Render the
    // clause only when a real epoch is present; never fabricate a value.
    if let Some(w) = fmt_epoch(reject_window_ends_at) {
        out.push_str(&format!(" by {w}"));
    }
    out.push_str(", or a full refund");
    match (token_amount, token_symbol) {
        (Some(amt), Some(sym)) => out.push_str(&format!(" of {amt} {sym}")),
        (Some(amt), None) => out.push_str(&format!(" of {amt}")),
        _ => {}
    }
    out.push_str(" will be issued automatically.");
    out
}

/// `sub_asp_dispute` (user side) — the ASP disputed the user's rejection; evaluation
/// opened (non-terminal). Added the current-period range (subStartTime/subEndTime).
pub fn sub_asp_dispute_user_notify(
    service_name: &str,
    job_id: &str,
    period_start: Option<i64>,
    period_end: Option<i64>,
) -> String {
    let mut out =
        format!("[Dispute Filed] The ASP has disputed your rejection of \"{service_name}\"");
    if let (Some(s), Some(e)) = (fmt_epoch(period_start), fmt_epoch(period_end)) {
        out.push_str(&format!("'s current period ({s}–{e})"));
    }
    out.push_str(&format!(
        " and escalated to evaluation. Job {job_id} status: Disputed."
    ));
    out
}

/// `sub_cancel` — cancellation outcome (user). Terminal-ness is decided by the caller from
/// `cancelResult` + `trialType` (see `flow_lifecycle::subscription::sub_cancel`); this function only builds the copy:
/// - `cancel_result == "fail"` (either branch) → the free-text `failReason` is shown verbatim.
/// - success + `trialType == 1` (trial cancel) → auto-conversion cancelled; the trial
///   continues unaffected until `trialEndsAt` (`trialEndTime`, legacy fallback `trailEndTime`), no charge after it ends (terminal).
/// - success + `trialType == 0` (formal-period cancel) → auto-renew cancelled; the
///   current period stays active until `periodEnd` (`subEndTime`), then the job moves to Completed
///   (non-terminal). An absent `trialType` falls into this non-terminal branch
///   (safer default — never claims the subscription ended when it may still be live).
pub fn sub_cancel_user_notify(
    cancel_result: Option<&str>,
    fail_reason: Option<&str>,
    trial_type: Option<i64>,
    service_name: &str,
    job_id: &str,
    trial_ends_at: Option<i64>,
    sub_end: Option<i64>,
) -> String {
    if cancel_result == Some("fail") {
        let mut out = String::from(
            "[Subscription Cancellation Failed] Your subscription could not be cancelled.",
        );
        if let Some(reason) = fail_reason {
            out.push_str(&format!("\n         Reason: {reason}"));
        }
        out
    } else if trial_type == Some(1) {
        let mut out = format!(
            "[Cancelled] Auto-conversion for the \"{service_name}\" free trial has been cancelled. This trial continues unaffected"
        );
        if let Some(t) = fmt_epoch(trial_ends_at) {
            out.push_str(&format!(" until {t}"));
        }
        out.push_str("; no charge will occur after it ends.");
        out
    } else {
        let mut out =
            format!("[Auto-Renew Cancelled] Auto-renew for \"{service_name}\" has been cancelled. Current service continues");
        match fmt_epoch(sub_end) {
            Some(e) => out.push_str(&format!(" until {e}")),
            None => out.push_str(" for the remainder of the current period"),
        }
        out.push_str(&format!("; job {job_id} will then move to Completed."));
        out
    }
}

/// `sub_asp_agree` (user side) — the ASP acknowledged the issue; full refund sent and
/// auto-renew turned off (terminal). Added the current-period range (subStartTime/subEndTime).
pub fn sub_asp_agree_user_notify(
    service_name: &str,
    token_amount: Option<&str>,
    token_symbol: Option<&str>,
    period_start: Option<i64>,
    period_end: Option<i64>,
) -> String {
    let mut out = format!(
        "[Refund Complete] The ASP has acknowledged the issue with \"{service_name}\"'s current period"
    );
    if let (Some(s), Some(e)) = (fmt_epoch(period_start), fmt_epoch(period_end)) {
        out.push_str(&format!(" ({s}–{e})"));
    }
    out.push_str(". A full refund");
    match (token_amount, token_symbol) {
        (Some(amt), Some(sym)) => out.push_str(&format!(" of {amt} {sym}")),
        (Some(amt), None) => out.push_str(&format!(" of {amt}")),
        _ => {}
    }
    out.push_str(" has been sent directly to your wallet, and auto-renew has been turned off.");
    out
}

/// `sub_complete_notify` (user side) — all scheduled renewals completed; normal end
/// (terminal).
pub fn sub_complete_notify_user_notify(
    service_name: &str,
    job_id: &str,
    period_end: Option<i64>,
) -> String {
    let mut out = format!(
        "[Subscription Complete] \"{service_name}\" has completed all scheduled renewals. Job {job_id} status: Completed; service ends normally"
    );
    if let Some(e) = fmt_epoch(period_end) {
        out.push_str(&format!(" at {e}"));
    }
    out.push_str(" with no further renewal.");
    out
}

/// `sub_close_notify` (user side) — current period ended; service closed (terminal).
pub fn sub_close_notify_user_notify(
    service_name: &str,
    job_id: &str,
    period_start: Option<i64>,
    period_end: Option<i64>,
) -> String {
    let mut out = format!("[Service Closed] \"{service_name}\"");
    if let (Some(s), Some(e)) = (fmt_epoch(period_start), fmt_epoch(period_end)) {
        out.push_str(&format!("'s current period ({s}–{e})"));
    }
    out.push_str(&format!(
        " has ended. Job {job_id} status: Closed. The system will automatically complete the rating for this job."
    ));
    out
}

/// `sub_failed_notify` (user side, terminal). Two variants selected by `trial_type`:
/// trial-conversion fail (`trialType == 1`) shows the verbatim `reason`; renewal terminal
/// fail (otherwise) appends the service-ended date from `grace_ends_at` (`subBufferEndTime`)
/// when present. `reason` is the verbatim `failReason` (`failReasopn` fallback), shown as-is.
pub fn sub_failed_notify_user_notify(
    service_name: &str,
    trial_type: Option<i64>,
    reason: Option<&str>,
    job_id: &str,
    grace_ends_at: Option<i64>,
) -> String {
    if trial_type == Some(1) {
        // Trial conversion charge failed before the paid period began.
        let mut out = format!(
            "[Trial Ended] \"{service_name}\" — the conversion charge could not be completed before the trial ended"
        );
        if let Some(r) = reason {
            out.push_str(&format!(" ({r})"));
        }
        out.push_str(&format!(
            "; conversion failed with no retry. Job {job_id} status: Closed. Subscribe again to continue."
        ));
        out
    } else {
        // Renewal charge kept failing through the grace period.
        let mut out = format!(
            "[Subscription Ended] \"{service_name}\" — the charge still failed after the grace period"
        );
        if let Some(g) = fmt_epoch(grace_ends_at) {
            out.push_str(&format!("; the service ended at {g}"));
        }
        out.push_str(&format!(
            ". Job {job_id} status: Closed. Subscribe again to continue."
        ));
        out
    }
}

/// `sub_expire_warn` — renewal reminder (base text; autoRenew hint added by handler).
pub fn sub_expire_warn_user_notify(job_id: &str) -> String {
    format!(
        "[Renewal Reminder] Job `{job_id}` — your subscription's current period \
         is ending soon. It will auto-renew on expiry. \
         Cancel in advance via subscription management if you don't want this."
    )
}

/// `sub_reject_refund_notify` — ASP missed rejection response window; user can claim refund.
pub fn sub_reject_refund_notify_user(
    service_name: &str,
    period_start: Option<i64>,
    period_end: Option<i64>,
    reject_window_ends_at: Option<i64>,
    amount: Option<&str>,
    token_symbol: Option<&str>,
) -> String {
    let mut out = format!("[Auto-Refund] Your rejection request for \"{service_name}\"");
    if let (Some(s), Some(e)) = (fmt_epoch(period_start), fmt_epoch(period_end)) {
        out.push_str(&format!("'s period ({s}\u{2013}{e})"));
    }
    out.push_str(" went unanswered past the ASP's response deadline");
    if let Some(d) = fmt_epoch(reject_window_ends_at) {
        out.push_str(&format!(" ({d})"));
    }
    out.push('.');
    match (amount, token_symbol) {
        (Some(a), Some(sym)) => out.push_str(&format!(
            " The system has automatically issued a full refund of {a} {sym} to your wallet."
        )),
        (Some(a), None) => out.push_str(&format!(
            " The system has automatically issued a full refund of {a} to your wallet."
        )),
        _ => out.push_str(" The system has automatically issued a full refund to your wallet."),
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    // Non-ASCII fixtures are written as unicode escapes so the source file stays free of CJK
    // bytes (the "no Chinese in source" gate scans the whole file, tests included) while still
    // proving that free-text `failReason` runtime data is echoed verbatim.
    const CN_INSUFFICIENT_BALANCE: &str = "\u{4f59}\u{989d}\u{4e0d}\u{8db3}"; // yu-e-bu-zu = "insufficient balance"
    const CN_NETWORK_ERROR: &str = "\u{7f51}\u{7edc}\u{9519}\u{8bef}"; // wang-luo-cuo-wu = "network error"

    #[test]
    fn sub_reject_refund_notify_user_full_and_degraded() {
        // Full slots → service name / period / deadline / amount all render.
        let full = sub_reject_refund_notify_user(
            "My Sub",
            Some(1_700_000_000),
            Some(1_700_500_000),
            Some(1_700_600_000),
            Some("0.0005"),
            Some("USDT"),
        );
        assert!(full.starts_with("[Auto-Refund]"), "{full}");
        assert!(full.contains("for \"My Sub\""), "service name: {full}");
        assert!(full.contains("'s period ("), "period slot: {full}");
        assert!(
            full.contains("automatically issued a full refund of 0.0005 USDT to your wallet."),
            "amount: {full}"
        );
        // Degrade #1: no period/deadline/amount → core sentence only, no empty slots, no panic.
        let bare = sub_reject_refund_notify_user("My Sub", None, None, None, None, None);
        assert!(
            bare.contains("for \"My Sub\" went unanswered"),
            "period clause omitted: {bare}"
        );
        assert!(
            bare.contains("automatically issued a full refund to your wallet."),
            "amount clause degrades: {bare}"
        );
        assert!(!bare.contains("()"), "no empty slot: {bare}");
        // Degrade #2: amount without symbol → amount-only clause.
        let amt_only =
            sub_reject_refund_notify_user("My Sub", None, None, None, Some("0.0005"), None);
        assert!(
            amt_only.contains("automatically issued a full refund of 0.0005 to your wallet."),
            "{amt_only}"
        );
    }

    #[test]
    fn sub_created_renders_active_and_first_charge_verbatim() {
        let out = sub_created_user_notify(
            "job-1",
            "My Sub",
            Some("1.500000"),
            Some("USDT"),
            Some(1_700_000_000),
            Some(1_700_500_000),
            true,
        );
        assert!(out.starts_with("[Subscribed]"), "canonical prefix: {out}");
        assert!(out.contains("Job job-1"));
        assert!(out.contains("subscribing to My Sub"));
        assert!(out.contains("status: Active"));
        assert!(out.contains("current period"));
        assert!(
            out.contains("First charge of 1.500000 USDT completed"),
            "amount rendered verbatim: {out}"
        );
        assert!(out.contains("Auto-renew is on"));
        assert!(
            out.contains("next charge date:"),
            "nextChargeAt = subEndTime → clause present: {out}"
        );
    }

    #[test]
    fn sub_created_auto_renew_off() {
        let out = sub_created_user_notify(
            "job-1",
            "My Sub",
            Some("1.5"),
            Some("USDT"),
            Some(1_700_000_000),
            Some(1_700_500_000),
            false,
        );
        assert!(
            out.contains("Auto-renew is off"),
            "off when autoRenew=0: {out}"
        );
        assert!(
            !out.contains("next charge date"),
            "no next charge when off: {out}"
        );
    }

    #[test]
    fn sub_created_conditional_next_charge_and_degrades() {
        let with = sub_created_user_notify(
            "job-1",
            "My Sub",
            Some("1.5"),
            Some("USDT"),
            Some(1_700_000_000),
            Some(1_700_500_000),
            true,
        );
        assert!(with.contains("next charge date:"), "clause present: {with}");
        let bare = sub_created_user_notify("job-1", "My Sub", None, None, None, None, false);
        assert!(bare.contains("Job job-1"));
        assert!(!bare.contains("First charge"));
        assert!(!bare.contains("current period"));
        assert!(
            !bare.contains("next charge date"),
            "subEndTime absent → omitted"
        );
        assert!(bare.contains("Auto-renew is off"));
    }

    #[test]
    fn sub_created_trial_renders_trial_started_no_charge() {
        let out = sub_created_trial_user_notify(
            Some("1.500000"),
            Some("USDT"),
            Some(1_700_000_000),
            Some(1_700_500_000),
        );
        assert!(out.starts_with("[Trial Started]"), "trial prefix: {out}");
        assert!(
            out.contains("Your free trial is active ("),
            "date range rendered: {out}"
        );
        assert!(
            out.contains("After it ends, 1.500000 USDT will be auto-charged on"),
            "conversion charge announced with amount + date: {out}"
        );
        assert!(
            out.contains("it will not retry if missed"),
            "one-attempt caveat: {out}"
        );
        assert!(
            !out.contains("First charge") && !out.contains("completed"),
            "trial start must not claim a completed charge: {out}"
        );
    }

    #[test]
    fn sub_created_trial_degrades_without_amount_or_dates() {
        let bare = sub_created_trial_user_notify(None, None, None, None);
        assert_eq!(
            bare, "[Trial Started] Your free trial is active.",
            "no amount → whole conversion sentence omitted; no dates → no range"
        );
        let no_dates = sub_created_trial_user_notify(Some("1.5"), None, None, None);
        assert!(
            no_dates.contains("After it ends, 1.5 will be auto-charged to convert"),
            "amount without symbol/date still announces conversion: {no_dates}"
        );
    }

    #[test]
    fn sub_trial_into_active_renders_first_charge_and_active() {
        let out = sub_trial_into_active_user_notify(
            "job-1",
            "My Sub",
            Some("2.00"),
            Some("USDT"),
            Some(1_700_000_000),
            Some(1_700_500_000),
        );
        assert!(out.starts_with("[Trial Converted]"));
        assert!(out.contains("first charge of 2.00 USDT for \"My Sub\" is complete"));
        // Current period range + next charge date (= subEndTime) rendered.
        assert!(
            out.contains("current period"),
            "period range present: {out}"
        );
        assert!(out.contains("Job job-1 status: Active."));
        assert!(
            out.contains("Next charge date:"),
            "nextChargeAt = subEndTime: {out}"
        );
        // Period fields absent → period + next-charge clauses omitted, core copy intact.
        let bare = sub_trial_into_active_user_notify(
            "job-1",
            "My Sub",
            Some("2.00"),
            Some("USDT"),
            None,
            None,
        );
        assert!(bare.contains("first charge of 2.00 USDT for \"My Sub\" is complete."));
        assert!(!bare.contains("current period"));
        assert!(!bare.contains("Next charge date"));
        assert!(bare.contains("Job job-1 status: Active."));
    }

    #[test]
    fn sub_renew_success_renders_active_and_conditional_next() {
        // Period fields absent → no New period / Next charge date clause.
        let bare = sub_renew_user_notify(
            Some("success"),
            None,
            "My Sub",
            "job-1",
            Some("1.00"),
            Some("USDT"),
            None,
            None,
            None,
        );
        assert!(bare.starts_with("[Renewed]"));
        assert!(bare.contains("this cycle's renewal of 1.00 USDT is complete"));
        assert!(bare.contains("Job job-1 status: Active."));
        assert!(!bare.contains("New period"), "period absent → omitted");
        assert!(
            !bare.contains("Next charge date"),
            "subEndTime absent → omitted"
        );
        // period present → new period range + next charge date (= subEndTime) render.
        let with = sub_renew_user_notify(
            Some("success"),
            None,
            "My Sub",
            "job-1",
            Some("1.00"),
            Some("USDT"),
            Some(1_700_000_000),
            Some(1_700_600_000),
            None,
        );
        assert!(
            !with.contains("New period"),
            "period range not repeated on renewal: {with}"
        );
        assert!(
            with.contains("Next charge date:"),
            "nextChargeAt = subEndTime: {with}"
        );
    }

    #[test]
    fn sub_renew_fail_renders_reason_verbatim_and_grace() {
        let out = sub_renew_user_notify(
            Some("fail"),
            Some(CN_INSUFFICIENT_BALANCE),
            "My Sub",
            "job-1",
            None,
            None,
            None,
            None,
            Some(1_700_600_000),
        );
        assert!(
            out.starts_with("[\u{26a0}\u{fe0f} Renewal Failed]"),
            "warning prefix: {out}"
        );
        assert!(
            out.contains(CN_INSUFFICIENT_BALANCE),
            "failReason echoed verbatim: {out}"
        );
        assert!(
            out.contains("(until "),
            "grace end rendered when subBufferEndTime>0"
        );
        assert!(out.contains("keep retrying"));
    }

    #[test]
    fn sub_renew_fail_degrades_without_grace() {
        let out = sub_renew_user_notify(
            Some("fail"),
            Some("insufficient balance"),
            "My Sub",
            "job-1",
            None,
            None,
            None,
            None,
            None,
        );
        assert!(out.contains("insufficient balance"));
        assert!(
            out.contains("A grace period is in effect;"),
            "no (until ...) clause: {out}"
        );
        assert!(!out.contains("(until "));
    }

    #[test]
    fn sub_user_reject_renders_period_and_conditional_window() {
        let out = sub_user_reject_user_notify(
            "My Sub",
            Some(1_700_000_000),
            Some(1_700_500_000),
            None,
            Some("5.00"),
            Some("USDT"),
        );
        assert!(out.starts_with("[Rejection Submitted]"));
        assert!(out.contains("\"My Sub\"'s current period"));
        assert!(out.contains("full refund of 5.00 USDT will be issued automatically"));
        assert!(
            !out.contains(" by "),
            "rejectWindowEndsAt absent → clause omitted"
        );
        // rejectWindowEndsAt present → the response-deadline clause renders.
        let with =
            sub_user_reject_user_notify("My Sub", None, None, Some(1_700_600_000), None, None);
        assert!(
            with.contains("respond by "),
            "window clause present: {with}"
        );
    }

    #[test]
    fn sub_asp_dispute_renders_disputed_status() {
        let out = sub_asp_dispute_user_notify(
            "My Sub",
            "job-1",
            Some(1_700_000_000),
            Some(1_700_500_000),
        );
        assert!(out.starts_with("[Dispute Filed]"));
        assert!(out.contains("disputed your rejection of \"My Sub\""));
        // Current-period range included.
        assert!(
            out.contains("current period ("),
            "period range present: {out}"
        );
        assert!(out.contains("Job job-1 status: Disputed."));
        // Period absent → range omitted, core copy intact.
        let bare = sub_asp_dispute_user_notify("My Sub", "job-1", None, None);
        assert!(bare.contains("disputed your rejection of \"My Sub\" and escalated to evaluation"));
        assert!(!bare.contains("current period"));
    }

    #[test]
    fn sub_cancel_fail_renders_reason_verbatim() {
        // fail copy is trialType-independent; failReason echoed verbatim.
        let out = sub_cancel_user_notify(
            Some("fail"),
            Some(CN_NETWORK_ERROR),
            Some(0),
            "My Sub",
            "job-1",
            None,
            None,
        );
        assert!(out.contains(CN_NETWORK_ERROR));
        assert!(out.contains("could not be cancelled"));
        assert!(!out.contains("Auto-Renew Cancelled"));
        assert!(!out.contains("Auto-conversion"));
    }

    #[test]
    fn sub_cancel_trial_renders_trial_unaffected_copy() {
        // trialType == 1 (trial cancel) → auto-conversion-cancelled copy with trialEndsAt.
        let out = sub_cancel_user_notify(
            Some("success"),
            None,
            Some(1),
            "My Sub",
            "job-1",
            Some(1_700_600_000),
            None,
        );
        assert!(out.starts_with("[Cancelled]"));
        assert!(
            out.contains("Auto-conversion for the \"My Sub\" free trial has been cancelled"),
            "trial cancel copy: {out}"
        );
        assert!(out.contains("continues unaffected until"));
        assert!(out.contains("no charge will occur after it ends"));
        assert!(!out.contains("Auto-Renew Cancelled"));
    }

    #[test]
    fn sub_cancel_formal_renders_autorenew_cancelled_with_period() {
        // trialType == 0 (formal-period cancel) → auto-renew-cancelled copy with periodEnd.
        let out = sub_cancel_user_notify(
            Some("success"),
            None,
            Some(0),
            "My Sub",
            "job-1",
            None,
            Some(1_700_600_000),
        );
        assert!(out.starts_with("[Auto-Renew Cancelled]"));
        assert!(out.contains("Auto-renew for \"My Sub\" has been cancelled"));
        assert!(
            out.contains("Current service continues until"),
            "renders subEndTime: {out}"
        );
        assert!(out.contains("job job-1 will then move to Completed"));
        assert!(!out.contains("free trial"));
    }

    #[test]
    fn sub_cancel_formal_degrades_without_sub_end() {
        // Absent subEndTime → drop the "until <date>" clause but keep the non-terminal copy.
        let out = sub_cancel_user_notify(
            Some("success"),
            None,
            Some(0),
            "My Sub",
            "job-1",
            None,
            None,
        );
        assert!(out.starts_with("[Auto-Renew Cancelled]"));
        assert!(!out.contains("continues until"));
        assert!(out.contains("remainder of the current period"));
        assert!(out.contains("move to Completed"));
    }

    #[test]
    fn sub_cancel_absent_trial_type_defaults_non_terminal() {
        // Missing trialType → safer non-terminal (auto-renew-cancelled) copy, never the trial one.
        let out = sub_cancel_user_notify(
            Some("success"),
            None,
            None,
            "My Sub",
            "job-1",
            None,
            Some(1_700_600_000),
        );
        assert!(out.starts_with("[Auto-Renew Cancelled]"));
        assert!(!out.contains("free trial"));
    }

    #[test]
    fn sub_asp_agree_renders_refund_and_amount_verbatim() {
        let out = sub_asp_agree_user_notify(
            "My Sub",
            Some("5.00"),
            Some("USDT"),
            Some(1_700_000_000),
            Some(1_700_500_000),
        );
        assert!(out.starts_with("[Refund Complete]"));
        assert!(out.contains("acknowledged the issue with \"My Sub\"'s current period"));
        // Current-period range included.
        assert!(
            out.contains("current period ("),
            "period range present: {out}"
        );
        assert!(out.contains("A full refund of 5.00 USDT has been sent directly to your wallet"));
        assert!(out.contains("auto-renew has been turned off"));
        // Period absent → range omitted, refund copy intact.
        let bare = sub_asp_agree_user_notify("My Sub", Some("5.00"), Some("USDT"), None, None);
        assert!(bare.contains("current period. A full refund of 5.00 USDT"));
    }

    #[test]
    fn sub_complete_notify_renders_completed_with_period() {
        let out = sub_complete_notify_user_notify("My Sub", "job-1", Some(1_700_600_000));
        assert!(out.starts_with("[Subscription Complete]"));
        assert!(out.contains("\"My Sub\" has completed all scheduled renewals"));
        assert!(out.contains("Job job-1 status: Completed"));
        assert!(out.contains("service ends normally at"));
    }

    #[test]
    fn sub_close_notify_renders_closed_and_rating_note() {
        let out = sub_close_notify_user_notify(
            "My Sub",
            "job-1",
            Some(1_700_000_000),
            Some(1_700_600_000),
        );
        assert!(out.starts_with("[Service Closed]"));
        assert!(
            out.contains("current period ("),
            "period range rendered: {out}"
        );
        assert!(out.contains("has ended"));
        assert!(out.contains("Job job-1 status: Closed"));
        assert!(out.contains("automatically complete the rating"));
        // period absent → degrade, no period clause.
        let bare = sub_close_notify_user_notify("My Sub", "job-1", None, None);
        assert!(
            bare.contains("[Service Closed] \"My Sub\" has ended."),
            "degrade without period: {bare}"
        );
    }

    #[test]
    fn sub_failed_notify_renders_reason_verbatim() {
        // Trial-conversion fail (trialType=1) → [Trial Ended] with verbatim reason clause.
        let trial = sub_failed_notify_user_notify(
            "My Sub",
            Some(1),
            Some(CN_INSUFFICIENT_BALANCE),
            "job-1",
            None,
        );
        assert!(trial.starts_with("[Trial Ended]"), "trial label: {trial}");
        assert!(
            trial.contains(CN_INSUFFICIENT_BALANCE),
            "reason verbatim: {trial}"
        );
        assert!(trial.contains("conversion failed with no retry"));
        assert!(trial.contains("Job job-1 status: Closed"));
        assert!(trial.contains("Subscribe again to continue"));
        // Renewal terminal fail (trialType absent) + grace present → [Subscription Ended] + ended date.
        let grace =
            sub_failed_notify_user_notify("My Sub", None, None, "job-1", Some(1_700_600_000));
        assert!(
            grace.starts_with("[Subscription Ended]"),
            "sub-ended label: {grace}"
        );
        assert!(
            grace.contains("after the grace period; the service ended at "),
            "grace end appended: {grace}"
        );
        assert!(grace.contains("Job job-1 status: Closed"));
        assert!(grace.contains("Subscribe again to continue"));
        // grace absent → no service-ended clause.
        let bare = sub_failed_notify_user_notify("My Sub", None, None, "job-1", None);
        assert!(bare.starts_with("[Subscription Ended]"));
        assert!(
            !bare.contains("the service ended at"),
            "no grace end when absent: {bare}"
        );
    }

    #[test]
    fn fmt_epoch_tolerates_millisecond_timestamps() {
        // A millisecond-scale grace deadline must render a sane date, not a five-digit year.
        let out = sub_renew_user_notify(
            Some("fail"),
            Some("balance"),
            "Svc",
            "job-1",
            None,
            None,
            None,
            None,
            Some(1_790_000_000_000i64),
        );
        assert!(
            out.contains("(until 2026-"),
            "ms grace rendered as seconds date: {out}"
        );
        assert!(!out.contains("+58692"), "no five-digit year: {out}");
    }
}
