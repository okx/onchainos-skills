//! ASP-side message templates — single point of maintenance.
//!
//! Two categories of templates:
//!
//! 1. **User-facing** — chat content shown to the user via `onchainos agent user-notify` /
//!    Rule: **no technical jargon** — event names (`provider_applied`/`job_*` etc.) /
//!    status enums (`created`/`accepted` etc.) / CLI flags (`--*`) /
//!    skill names (`okx-ai` etc.) /
//!    status field names (`jobStatus`/`paymentMode`) are all banned.
//!    **The string literals in this file are English** (escrow/x402, review window
//!    expired, task completed, etc.) and serve as the source-of-truth that the sub
//!    agent localizes via LOCALIZATION_PREFIX — English users see them as-is,
//!    non-English users see equivalents produced by the sub agent (e.g. Chinese
//!    users see the equivalent of "escrow/x402, review window expired, task completed"). The no-technical-jargon
//!    rule applies to all languages, not just English.
//!
//! 2. **Peer-facing** — agent-to-agent protocol messages sent via `okx-a2a xmtp-send`
//!    to the User Agent's sub agent. Naming suffix: `_to_buyer`.
//!    Rule: protocol literals are allowed (`[intent:*]` / `fileKey`/`digest` etc.);
//!    **do NOT instruct the peer to run CLIs** — the peer has its own flow.rs and
//!    decides for itself based on chain events; giving direct CLI orders is overreach.
//!
//! Field-value placeholders use `<...>`; the agent fills them from `common context` /
//! conversation state. To add new copy → add a new fn; to change copy → edit the
//! fn body; flow.rs only ever calls into here and never embeds literals inline.

/// `Event::JobAspSelected` no-serviceId fallback — user-facing notification
/// pushed via `onchainos agent user-notify --content <text>`. The playbook does NOT
/// auto-start negotiation; it ends the turn and waits for the User Agent to re-route
/// (designate a specific service). Localize before sending.
pub fn job_asp_selected_no_service_notify(job_id: &str) -> String {
    format!(
        "[Designated Task — Skipped] Job {job_id} — the User Agent designated you as the ASP without pinning a specific service.\n\
         \x20\x20No action taken; waiting for the User Agent to re-route with a specific service."
    )
}

/// `Event::JobAspSelected` incomplete-terms guard — pushed when the inbound
/// envelope is missing `tokenAmount` and/or `tokenSymbol`. Same shape as the
/// no-service notify: user is informed; the ASP takes no on-chain action.
/// `missing_field` is interpolated (e.g. `"tokenAmount"` / `"tokenSymbol"` /
/// `"tokenAmount + tokenSymbol"`). Localize before sending.
pub fn job_asp_selected_missing_terms_notify(job_id: &str, missing_field: &str) -> String {
    format!(
        "[Designated Task — Skipped] Job {job_id} — the User Agent's designation envelope is missing `{missing_field}`; cannot determine the apply terms.\n\
         \x20\x20No action taken; waiting for the User Agent to re-send the designation with complete terms."
    )
}

/// `Event::JobUserReject` — user-facing notification pushed via
/// `onchainos agent user-notify --content <text>` when the User Agent refuses to fund /
/// confirm-accept after the ASP applied. Terminal for this round; the
/// designation is over. Localize before sending.
pub fn job_user_reject_notify(job_id: &str) -> String {
    format!(
        "[User Agent Declined Payment] Job {job_id} — the User Agent refused to fund / confirm-accept after your apply.\n\
         \x20\x20This designation is over; no further action is needed on this side."
    )
}

/// `Event::ProviderApplied` — user-facing notification pushed via
/// `onchainos agent user-notify --content <text>` after the apply has been recorded
/// on-chain (escrow path). Localize before sending.
pub fn provider_applied_user_notify(job_id: &str, agent_id: &str) -> String {
    format!(
        "[Apply Submitted] Job {job_id} — your apply has been recorded on-chain.\n\
         \x20\x20- ASP agentId: {agent_id}\n\
         \x20\x20Awaiting the User Agent's confirm-accept to fund escrow."
    )
}

/// `Event::JobAspSelected` APPLY failure — pushed when the on-chain `apply`
/// command returns non-zero. `error_summary` is interpolated directly (caller
/// passes either the stderr / one-line error message, or a placeholder for the
/// LLM to fill). Localize before sending.
pub fn job_asp_selected_apply_failed_notify(job_id: &str, error_summary: &str) -> String {
    format!(
        "[Designated Task — Apply Failed] Job {job_id} — the on-chain apply did not go through.\n\
         \x20\x20- Error: {error_summary}\n\
         \x20\x20The designated assignment was NOT recorded; please retry or contact the User Agent."
    )
}

/// `Event::JobAspSelected` REJECT path — user-facing notification pushed via
/// `onchainos agent user-notify --content <text>` after the off-chain `asp-reject`.
/// `reason` is interpolated directly (caller passes either a fixed string for
/// code-determined rejections — `"designated service not registered"` /
/// `"price below registered floor"` — or the literal `<reason>` placeholder
/// when the LLM picks the wording). Localize the full string before sending.
pub fn job_asp_selected_rejected_notify(job_id: &str, reason: &str) -> String {
    format!(
        "[Designated Task Declined] Job {job_id} — the designated assignment was declined.\n\
         \x20\x20- Reason: {reason}\n\
         \x20\x20The User Agent can now re-route to another ASP."
    )
}

pub(super) const L10N_DISPATCH_SHORT: &str = "\
🌐🛑 **MUST translate** the content below to the user's language before passing to `onchainos agent user-notify` (rule 5: non-English → faithful translation; rule 4: English → verbatim). Sending English content to a Chinese user is a violation.";

/// `Event::JobAccepted` Step 1 — job-accepted notice pushed to the user.
///
/// Each line is prefixed with 4 spaces of indentation to align with other step
/// content blocks in flow.rs. (Rust string line-continuation swallows whitespace
/// after the newline, so indentation must be expressed via explicit `\x20` escapes.)
pub fn job_accepted_user_notify(job_id: &str, agent_id: &str) -> String {
    format!(
        "\x20\x20\x20\x20[Job Accepted] Job {job_id} has been accepted.\n\
         \x20\x20\x20\x20- Title: <title>\n\
         \x20\x20\x20\x20- Description: <description>\n\
         \x20\x20\x20\x20- Negotiated price: <tokenAmount> <tokenSymbol>\n\
         \x20\x20\x20\x20- Payment: <escrow>\n\
         \x20\x20\x20\x20- ASP: {agent_id}\n\
         \x20\x20\x20\x20Funds are now escrowed; the ASP has started execution."
    )
}

/// `Event::JobAccepted` — x402 / A2MCP variant. Different from the escrow
/// version: there is no negotiation (price is fixed by service registration),
/// funds were paid up-front via the A2MCP endpoint (not escrowed), and the
/// deliverable was already returned at request time. The agent fills in the
/// `<title>` / `<description>` / `<tokenAmount>` / `<tokenSymbol>` placeholders from
/// the prefetched task context. Localize before sending.
pub fn job_accepted_user_notify_a2mcp(job_id: &str, agent_id: &str) -> String {
    format!(
        "\x20\x20\x20\x20[Service Request Received] Job {job_id} — request received and paid via the A2MCP endpoint.\n\
         \x20\x20\x20\x20- Title: <title>\n\
         \x20\x20\x20\x20- Description: <description>\n\
         \x20\x20\x20\x20- Price: <tokenAmount> <tokenSymbol>\n\
         \x20\x20\x20\x20- Payment: A2MCP (paid at request time)\n\
         \x20\x20\x20\x20- ASP: {agent_id}\n\
         \x20\x20\x20\x20Deliverable was returned by the service endpoint at request time; awaiting on-chain completion receipt."
    )
}

/// `Event::JobRejected` Step 1 — decision prompt shown to the user.
///
/// The short jobId prefix lets the user tell tasks apart at a glance when
/// multiple prompts are in flight concurrently.
pub fn job_rejected_user_decision_prompt(short_id: &str, expire_time: Option<i64>) -> String {
    // FR-4: append the decision-deadline reminder after the refund option. `None`
    // (no expireTime, or not representable) ⇒ empty string, card unchanged (FR-5).
    use super::super::common::deadline::{self, DeadlineKind};
    let decision_deadline_line = deadline::deadline_reminder_line(
        expire_time,
        chrono::Local::now().timestamp(),
        DeadlineKind::Decision,
    )
    .map(|l| format!("\n\x20\x20\x20\x20{l}"))
    .unwrap_or_default();
    format!(
        "\x20\x20\x20\x20[Job {short_id} — you are the ASP] The User Agent rejected the deliverable. Choose:\n\
         \x20\x20\x20\x20A. File a dispute → reply 'file dispute, reason: <reason>'\n\
         \x20\x20\x20\x20B. Agree to refund → reply 'agree to refund'{decision_deadline_line}"
    )
}

/// `Event::JobSubmitted` — notify the user (ASP's owner) that the deliverable
/// is on-chain (deliver tx confirmed) and the User Agent's review window has begun.
/// ASP has no further peer-side action; this is a milestone status update
/// only. Localize before sending.
pub fn job_submitted_user_notify(job_id: &str) -> String {
    format!(
        "[Deliverable Submitted] Job {job_id} — your deliverable is on-chain (submit tx confirmed).\n\
         \x20\x20Waiting for the User Agent's review (approve or reject)."
    )
}

/// `Event::JobCompleted` Step 2 — task-completed notice pushed to the user.
pub fn job_completed_user_notify(job_id: &str) -> String {
    format!(
        "\x20\x20\x20\x20[💰 Job Completed] Job {job_id} (<title>) — approved by the User Agent; funds received.\n\
         \x20\x20\x20\x20  - Income: <tokenAmount> <tokenSymbol>\n\
         \x20\x20\x20\x20  - User Agent: <buyerAgentId>\n\
         \x20\x20\x20\x20\n\
         \x20\x20\x20\x20This job is complete."
    )
}

/// Per-evaluator verdict rationales block shared by all three `DisputeResolved` outcomes.
/// Source field: `message.voteReportSummaries[*].voterReportSummary` from the system envelope.
/// Indentation matches the ASP's 6-space bullet style (header at 6 spaces, entries at 10).
const EVALUATION_REASONS_BLOCK: &str = "\x20\x20\x20\x20\x20\x20- Evaluation reasons:\n\
\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20Evaluator 1: <voterReportSummary from message.voteReportSummaries[0]>\n\
\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20Evaluator 2: <voterReportSummary from message.voteReportSummaries[1]>\n\
\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20... (one line per entry; first skip entries whose voterReportSummary is missing / empty / whitespace, then number the kept entries consecutively starting at 1 in array order — do NOT preserve gaps from the original index; omit this whole `- Evaluation reasons:` section if voteReportSummaries is missing, not an array, empty, or every entry would be skipped — do NOT print a header with no body, do NOT fabricate filler text)";

/// `Event::DisputeResolved` branch A (ASP wins) — user notify emitted when the
/// agent actually claims a non-zero reward in A-Step 2.
pub fn dispute_won_with_claim_user_notify(job_id: &str) -> String {
    format!(
        "\x20\x20\x20\x20[⚖️💰 Dispute Won] Job {job_id} (<title>) — dispute resolved; ASP wins.\n\
         \x20\x20\x20\x20  - Outcome: ASPWins\n\
         \x20\x20\x20\x20  - Job income: <tokenAmount> <tokenSymbol>\n\
         \x20\x20\x20\x20  - Auto-claimed account reward: <claimed amount> <symbol> (txHash=<hash>)\n\
         \x20\x20\x20\x20  - User Agent: <buyerAgentId>\n\
         {EVALUATION_REASONS_BLOCK}\n\
         \x20\x20\x20\x20  \n\
         \x20\x20\x20\x20  This job is complete."
    )
}

/// `Event::DisputeResolved` branch A (ASP wins) — user notify emitted when
/// A-Step 1 `claimable` returns all zeros (nothing to claim).
pub fn dispute_won_no_claim_user_notify(job_id: &str) -> String {
    format!(
        "\x20\x20\x20\x20[⚖️💰 Dispute Won] Job {job_id} (<title>) — dispute resolved; ASP wins.\n\
         \x20\x20\x20\x20  - Outcome: ASPWins\n\
         \x20\x20\x20\x20  - Job income: <tokenAmount> <tokenSymbol>\n\
         \x20\x20\x20\x20  - Account-level pending reward: none (checked)\n\
         \x20\x20\x20\x20  - User Agent: <buyerAgentId>\n\
         {EVALUATION_REASONS_BLOCK}\n\
         \x20\x20\x20\x20  \n\
         \x20\x20\x20\x20  This job is complete."
    )
}

/// `Event::RewardClaimed` Step 1 — failure notice pushed to the user when
/// code != 0 (reward-claim tx failed).
pub fn reward_claim_failed_user_notify(job_id: &str) -> String {
    format!("[Reward Claim Failed] Job {job_id} — the reward-claim transaction failed. Please review and retry manually; the agent will not auto-retry.")
}

/// `Event::RewardClaimed` Step 2 — success notice pushed to the user when the
/// reward has been settled to their wallet.
pub fn reward_claimed_user_notify(job_id: &str) -> String {
    format!("[Reward Claimed] Job {job_id} — reward successfully claimed to your wallet.")
}

/// Preamble exception-escalation hard rule 1) protocol misalignment — content template.
pub fn escalation_protocol_misread_notify(job_id: &str) -> String {
    format!("[⚠️ Protocol Misalignment] Job {job_id} — repeated clarifications on the same flow, and the remote agent still repeats. Replies have stopped. Please intervene or give a new instruction.")
}

/// Preamble exception-escalation hard rule 2) execution error — content template.
pub fn escalation_cli_failed_notify(job_id: &str) -> String {
    format!(
        "[⚠️ Operation Failed] Job {job_id}\n\
         - Action: <e.g. submit deliverable / accept job / fetch paymentId>\n\
         - Error: <one-sentence summary of stderr / error field>\n\
         - Current status: <status>\n\
         \n\
         Choose how to proceed:\n\
         A. Retry → reply 'A' or 'retry'\n\
         B. Don't prompt again (you'll handle manually) → reply 'B' or 'dismiss'\n\
         C. Provide a new instruction → describe what to change (e.g. 'change --token-symbol to USDT and retry')"
    )
}

/// `Event::SubmitDeadlineWarn` — decision prompt shown to the user.
///
/// The short jobId prefix lets the user tell tasks apart at a glance (same as
/// `job_rejected_user_decision_prompt`). If the user replies `submit now` →
/// the user-session relays the decision back to the sub, which runs the delivery
/// flow; if they stay silent → the sub waits for `submit_expired` to trigger a refund.
pub fn submit_deadline_warn_user_prompt(short_id: &str) -> String {
    format!(
        "\x20\x20\x20\x20[⏰ Deadline Warning — Job {short_id}, you are the ASP] The submit deadline is approaching.\n\
         \x20\x20\x20\x20If the deliverable is ready, reply 'submit now' and I will run the delivery flow immediately.\n\
         \x20\x20\x20\x20If it is not ready, you may stay silent — after expiry the User Agent can claim an auto-refund, escrowed funds return to the User Agent, and this job is void."
    )
}

/// User notification after the ASP agent auto-rates the User Agent.
pub fn rating_submitted_user_notify(job_id: &str) -> String {
    format!(
        "\x20\x20\x20\x20[📝 Rating Submitted] Job <title> (`{job_id}`) — rated.\n\
         \x20\x20\x20\x20Score: <score> / 5.00\n\
         \x20\x20\x20\x20💬 Comment: <description>"
    )
}

/// `Event::DisputeResolved` branch B (ASP loses) — B-Step 1 user notify.
pub fn dispute_lost_user_notify(job_id: &str) -> String {
    format!(
        "\x20\x20\x20\x20[⚖️⚠️ Dispute Lost] Job {job_id} (<title>) — dispute resolved; User Agent wins.\n\
         \x20\x20\x20\x20  - Outcome: ClientWins\n\
         \x20\x20\x20\x20  - Loss: <tokenAmount> <tokenSymbol> (funds returned to the User Agent)\n\
         \x20\x20\x20\x20  - User Agent: <buyerAgentId>\n\
         {EVALUATION_REASONS_BLOCK}\n\
         \x20\x20\x20\x20  \n\
         \x20\x20\x20\x20  This job is complete."
    )
}

/// `Event::JobAccepted` Step 3 branch A (escrow text deliverable) — peer message sent to the User Agent.
///
/// **Do not direct** the peer's CLI — once the User Agent's sub agent receives
/// this, it follows its own `Event::JobSubmitted` script.
///
/// NOTE: No longer called from flow.rs — deliver.rs now uses `build_text_deliver_message`
/// with actual values. Kept as protocol format reference.
#[allow(dead_code)]
pub fn deliver_text_to_user(job_id: &str) -> String {
    format!(
        "jobId: {job_id}\n\
         deliverableType: text\n\
         - - -\n\
         <paste the deliverable text here>\n\
         - - -\n\
         [intent:deliver]"
    )
}

/// `Event::JobAccepted` Step 3 branch A (escrow file deliverable) — peer message sent to the User Agent.
///
/// The 5 decryption-metadata fields (`fileKey` / `digest` / `salt` / `nonce` /
/// `secret` / `filename`) are protocol literals; the User Agent's sub agent
/// parses them and downloads the local file via the file-attachment flow.
/// **Do not direct** the peer's CLI.
///
/// NOTE: No longer called from flow.rs — deliver.rs now uses `build_file_deliver_message`
/// with actual upload metadata. Kept as protocol format reference.
#[allow(dead_code)]
pub fn deliver_file_to_user(job_id: &str) -> String {
    format!(
        "jobId: {job_id}\n\
         deliverableType: file\n\
         fileKey: <full fileKey string returned from A-Step 1>\n\
         digest: <digest returned from A-Step 1>\n\
         salt: <salt returned from A-Step 1>\n\
         nonce: <nonce returned from A-Step 1>\n\
         secret: <secret returned from A-Step 1>\n\
         filename: <filename returned from A-Step 1>\n\
         [intent:deliver]"
    )
}

/// Build the actual text-deliver XMTP message with real content (used by deliver.rs).
pub fn build_text_deliver_message(job_id: &str, text: &str) -> String {
    format!(
        "jobId: {job_id}\n\
         deliverableType: text\n\
         - - -\n\
         {text}\n\
         - - -\n\
         [intent:deliver]"
    )
}

/// Build the actual file-deliver XMTP message with real upload metadata (used by deliver.rs).
pub fn build_file_deliver_message(
    job_id: &str,
    upload: &crate::commands::agent_commerce::task::common::okx_a2a::FileUploadResult,
) -> String {
    format!(
        "jobId: {job_id}\n\
         deliverableType: file\n\
         fileKey: {}\n\
         digest: {}\n\
         salt: {}\n\
         nonce: {}\n\
         secret: {}\n\
         filename: {}\n\
         [intent:deliver]",
        upload.file_key, upload.digest, upload.salt, upload.nonce, upload.secret, upload.filename,
    )
}

/// User Agent attachment received — notify the ASP's user.
pub fn user_attachment_received_user_notify(job_id: &str) -> String {
    format!("[Job `{job_id}`] The User Agent sent an attachment (reference material for this task). File downloaded and saved locally.")
}

/// Append the FR-2 `autotrade: <canonical json>` line to a delivery message.
///
/// Empty `autotrade_line` ⇒ the message is returned byte-for-byte unchanged
/// (ordinary delivery). The buyer sub parses this trailing line to run the
/// auto-trade pipeline.
pub fn with_autotrade_line(message: String, autotrade_line: &str) -> String {
    if autotrade_line.is_empty() {
        message
    } else {
        format!("{message}\nautotrade: {autotrade_line}")
    }
}

// ── Subscription notifications (display-class) ─────────────────────

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

// The service name can be unresolvable (envelope omitted jobTitle AND the task prefetch
// failed or returned an empty title). Degrade by omitting the quoted-name subclause —
// a literal `<title>` placeholder must never reach the user-visible notification body.
fn service_name_clause(preposition: &str, service_name: Option<&str>) -> String {
    match service_name {
        Some(s) if !s.is_empty() => format!("{preposition} \"{s}\""),
        _ => String::new(),
    }
}

pub fn sub_asp_selected_asp_notify(
    service_name: Option<&str>,
    buyer_agent_id: Option<&str>,
    job_id: &str,
    token_amount: Option<&str>,
    token_symbol: Option<&str>,
    period_start: Option<i64>,
    period_end: Option<i64>,
) -> String {
    let svc = service_name_clause(" for", service_name);
    let mut out = format!("[New Subscription] You have a new subscriber{svc}.");
    if let Some(buyer) = buyer_agent_id {
        out.push_str(&format!(" Buyer: {buyer}."));
    }
    out.push_str(&format!(" Job {job_id}"));
    if let (Some(s), Some(e)) = (fmt_epoch(period_start), fmt_epoch(period_end)) {
        out.push_str(&format!(", current period {s}–{e}"));
    }
    match (token_amount, token_symbol) {
        (Some(amt), Some(sym)) => out.push_str(&format!(", payment received: {amt} {sym}")),
        (Some(amt), None) => out.push_str(&format!(", payment received: {amt}")),
        _ => {}
    }
    out.push('.');
    out.push_str(" Please begin delivering the service.");
    out
}

/// `sub_asp_selected` with `trialType=1` — the subscriber is on a free trial, so nothing
/// has been charged yet; the ASP must NOT be told a payment was received (the real payment
/// is announced on conversion via `sub_trial_into_active`). Mirrors the buyer-side
/// `sub_created_trial_user_notify` trial variant.
pub fn sub_asp_selected_trial_asp_notify(
    service_name: Option<&str>,
    buyer_agent_id: Option<&str>,
    job_id: &str,
    token_amount: Option<&str>,
    token_symbol: Option<&str>,
    trial_start: Option<i64>,
    trial_end: Option<i64>,
) -> String {
    let svc = service_name_clause(" for", service_name);
    let mut out = format!("[New Trial Subscriber] You have a new subscriber{svc} on a free trial");
    if let (Some(s), Some(e)) = (fmt_epoch(trial_start), fmt_epoch(trial_end)) {
        out.push_str(&format!(" ({s}\u{2013}{e})"));
    }
    out.push('.');
    if let Some(buyer) = buyer_agent_id {
        out.push_str(&format!(" Buyer: {buyer}."));
    }
    out.push_str(&format!(" Job {job_id}. No payment during the trial"));
    if let Some(amt) = token_amount {
        match token_symbol {
            Some(sym) => out.push_str(&format!("; {amt} {sym} will be charged on conversion")),
            None => out.push_str(&format!("; {amt} will be charged on conversion")),
        }
        if let Some(e) = fmt_epoch(trial_end) {
            out.push_str(&format!(" at {e}"));
        }
    }
    out.push('.');
    out.push_str(" Please begin delivering the service.");
    out
}

/// ASP terminal notice: subscription completed all scheduled renewals (status Completed).
/// `period_end` clause is omitted when the field is absent (graceful degradation).
pub fn sub_complete_notify_asp_notify(
    service_name: Option<&str>,
    job_id: &str,
    period_end: Option<i64>,
) -> String {
    let svc = service_name_clause(" to", service_name);
    let mut out = format!(
        "[Subscription Complete] The user's subscription{svc} has completed all scheduled renewals. Job {job_id} status: Completed; service ends normally"
    );
    if let Some(e) = fmt_epoch(period_end) {
        out.push_str(&format!(" at {e}"));
    }
    out.push_str(" — no further delivery is required.");
    out
}

/// ASP terminal notice: subscription ended because the renewal charge failed during grace (Closed).
pub fn sub_close_notify_asp_notify(service_name: Option<&str>, job_id: &str) -> String {
    let svc = service_name_clause(" to", service_name);
    format!(
        "[Subscription Ended] The user's subscription{svc} has ended because the renewal charge failed during the grace period. Job {job_id} status: Closed — please stop delivering the service."
    )
}

/// ASP terminal notice: free trial failed to convert to a paid subscription (Closed).
/// `reason` clause is omitted when the field is absent (graceful degradation).
pub fn sub_failed_notify_asp_notify(
    service_name: Option<&str>,
    job_id: &str,
    reason: Option<&str>,
) -> String {
    let svc = service_name_clause(" for", service_name);
    let reason_clause = match reason {
        Some(r) if !r.is_empty() => format!(" (reason: {r})"),
        _ => String::new(),
    };
    format!(
        "[Trial Not Converted] The user's free trial{svc} failed to convert to a paid subscription{reason_clause}. Job {job_id} status: Closed — no further delivery is required."
    )
}

/// `sub_user_reject` ASP-side decision copy: the buyer rejected the current period; the ASP
/// must confirm the refund or file a dispute before the response deadline, else a full refund
/// is issued automatically. Rendered as the canonical body pushed through the pending-decisions
/// relay (A/B decision included). Slots degrade per sibling pattern; a missing deadline falls
/// back to the approximate "within about 1 day" window rather than an empty slot.
pub fn sub_user_reject_asp_decision_copy(
    service_name: &str,
    period_start: Option<i64>,
    period_end: Option<i64>,
    reject_window_ends_at: Option<i64>,
    amount: Option<&str>,
    token_symbol: Option<&str>,
) -> String {
    let mut out = format!(
        "[Action Needed: User Rejection] The user has rejected \"{service_name}\"'s current period"
    );
    if let (Some(s), Some(e)) = (fmt_epoch(period_start), fmt_epoch(period_end)) {
        out.push_str(&format!(" ({s}\u{2013}{e})"));
    }
    out.push('.');
    match fmt_epoch(reject_window_ends_at) {
        Some(d) => out.push_str(&format!(
            " Please confirm the refund or file a dispute by {d}"
        )),
        None => out.push_str(" Please confirm the refund or file a dispute within about 1 day"),
    }
    out.push_str(" — otherwise a full refund");
    match (amount, token_symbol) {
        (Some(a), Some(sym)) => out.push_str(&format!(" of {a} {sym}")),
        (Some(a), None) => out.push_str(&format!(" of {a}")),
        _ => {}
    }
    out.push_str(" will be issued to the user automatically.\n");
    out.push_str("  A. File a dispute for evaluation.\n");
    out.push_str("  B. Confirm the refund for this period.");
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── job_rejected_user_decision_prompt decision-deadline reminder (FR-4) ──

    #[test]
    fn rejected_prompt_appends_decision_line_when_expire_present() {
        let now = chrono::Local::now().timestamp();
        let out = job_rejected_user_decision_prompt("0xabc", Some(now + 86_400));
        assert!(
            out.contains("\u{23f0} Decision deadline: 1 day(s)"),
            "prompt should append the Decision reminder line; got:\n{out}"
        );
    }

    #[test]
    fn rejected_prompt_no_reminder_when_expire_none() {
        let out = job_rejected_user_decision_prompt("0xabc", None);
        assert!(
            !out.contains('\u{23f0}'),
            "no reminder when expire_time is None; got:\n{out}"
        );
        assert!(
            out.ends_with("agree to refund'"),
            "card unchanged when None; got:\n{out}"
        );
    }

    // ── sub_asp_selected canonical copy ──

    #[test]
    fn sub_asp_selected_renders_buyer_period_and_payment_verbatim() {
        let out = sub_asp_selected_asp_notify(
            Some("My Sub"),
            Some("agent-buyer-1"),
            "job-1",
            Some("3.00"),
            Some("USDT"),
            Some(1_700_000_000),
            Some(1_700_500_000),
        );
        assert!(
            out.starts_with("[New Subscription]"),
            "canonical prefix: {out}"
        );
        assert!(out.contains("new subscriber for \"My Sub\""));
        assert!(out.contains("Buyer: agent-buyer-1."));
        assert!(out.contains("Job job-1"));
        assert!(out.contains("current period"));
        assert!(
            out.contains("payment received: 3.00 USDT"),
            "amount rendered verbatim: {out}"
        );
        assert!(out.contains("Please begin delivering the service."));
    }

    #[test]
    fn sub_asp_selected_degrades_when_optional_fields_absent() {
        // No buyer / period / amount → still renders the core notice grammatically.
        let out =
            sub_asp_selected_asp_notify(Some("My Sub"), None, "job-1", None, None, None, None);
        assert!(out.contains("new subscriber for \"My Sub\""));
        assert!(!out.contains("Buyer:"));
        assert!(!out.contains("current period"));
        assert!(!out.contains("payment received"));
        assert!(out.contains("Job job-1. Please begin delivering the service."));
    }

    #[test]
    fn sub_asp_notices_omit_service_clause_when_title_unresolvable() {
        // Envelope title AND prefetch both missing → the quoted-name subclause degrades
        // away; a literal `<title>` placeholder must never appear in the body.
        let selected = sub_asp_selected_asp_notify(None, None, "job-1", None, None, None, None);
        let complete = sub_complete_notify_asp_notify(None, "job-1", None);
        let closed = sub_close_notify_asp_notify(None, "job-1");
        let failed = sub_failed_notify_asp_notify(None, "job-1", None);
        for out in [&selected, &complete, &closed, &failed] {
            assert!(!out.contains("<title>"), "no literal placeholder: {out}");
            assert!(!out.contains("\"\""), "no empty quoted name: {out}");
        }
        assert!(selected.contains("You have a new subscriber."));
        assert!(complete.contains("The user's subscription has completed all scheduled renewals"));
        assert!(
            closed.contains("The user's subscription has ended because the renewal charge failed")
        );
        assert!(failed.contains("The user's free trial failed to convert to a paid subscription"));
    }

    #[test]
    fn sub_asp_selected_trial_branch_omits_payment_received() {
        // trialType=1: nothing is charged on selection, so the ASP must NOT be told a
        // payment was received; the amount is framed as a future conversion charge.
        let out = sub_asp_selected_trial_asp_notify(
            Some("My Sub"),
            Some("agent-buyer-1"),
            "job-1",
            Some("0.0005"),
            Some("USDT"),
            Some(1_700_000_000),
            Some(1_700_500_000),
        );
        assert!(
            out.starts_with("[New Trial Subscriber]"),
            "trial prefix: {out}"
        );
        assert!(
            !out.contains("payment received"),
            "no false payment claim: {out}"
        );
        assert!(
            out.contains("No payment during the trial"),
            "states no charge: {out}"
        );
        assert!(
            out.contains("0.0005 USDT will be charged on conversion"),
            "future charge framed: {out}"
        );
        assert!(out.contains("Please begin delivering the service."));
    }
}
