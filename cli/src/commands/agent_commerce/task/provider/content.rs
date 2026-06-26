//! ASP-side message templates — single point of maintenance.
//!
//! Two categories of templates:
//!
//! 1. **User-facing** — chat content shown to the user via `onchainos agent user-notify` /
//!    Rule: **no technical jargon** — event names (`provider_applied`/`job_*` etc.) /
//!    status enums (`created`/`accepted` etc.) / CLI flags (`--*`) /
//!    skill names (`okx-agent-identity` etc.) /
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
/// (designate a specific service / list the task publicly). Localize before sending.
pub fn job_asp_selected_no_service_notify(job_id: &str) -> String {
    format!(
        "[Designated Task — Skipped] Job {job_id} — the User Agent designated you as the ASP without pinning a specific service.\n\
         \x20\x20No action taken; waiting for the User Agent to re-route with a specific service or list the task publicly."
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
/// confirm-accept after the provider applied. Terminal for this round; the
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
         \x20\x20The User Agent can now re-route to another ASP or list the task publicly."
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
pub fn job_rejected_user_decision_prompt(short_id: &str) -> String {
    format!(
        "\x20\x20\x20\x20[Job {short_id} — you are the ASP] The User Agent rejected the deliverable. Choose:\n\
         \x20\x20\x20\x20A. File a dispute → reply 'file dispute, reason: <reason>'\n\
         \x20\x20\x20\x20B. Agree to refund → reply 'agree to refund'"
    )
}

/// `Event::JobSubmitted` — notify the user (ASP's owner) that the deliverable
/// is on-chain (deliver tx confirmed) and the User Agent's review window has begun.
/// Provider has no further peer-side action; this is a milestone status update
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

/// Per-arbiter verdict rationales block shared by all three `DisputeResolved` outcomes.
/// Source field: `message.voteReportSummaries[*].voterReportSummary` from the system envelope.
/// Indentation matches the provider's 6-space bullet style (header at 6 spaces, entries at 10).
const ARBITRATION_REASONS_BLOCK: &str = "\x20\x20\x20\x20\x20\x20- Arbitration reasons:\n\
\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20Arbiter 1: <voterReportSummary from message.voteReportSummaries[0]>\n\
\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20Arbiter 2: <voterReportSummary from message.voteReportSummaries[1]>\n\
\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20... (one line per entry; first skip entries whose voterReportSummary is missing / empty / whitespace, then number the kept entries consecutively starting at 1 in array order — do NOT preserve gaps from the original index; omit this whole `- Arbitration reasons:` section if voteReportSummaries is missing, not an array, empty, or every entry would be skipped — do NOT print a header with no body, do NOT fabricate filler text)";

/// `Event::DisputeResolved` branch A (ASP wins) — user notify emitted when the
/// agent actually claims a non-zero reward in A-Step 2.
pub fn dispute_won_with_claim_user_notify(job_id: &str) -> String {
    format!(
        "\x20\x20\x20\x20[⚖️💰 Dispute Won] Job {job_id} (<title>) — dispute resolved; ASP wins.\n\
         \x20\x20\x20\x20  - Outcome: ProviderWins\n\
         \x20\x20\x20\x20  - Job income: <tokenAmount> <tokenSymbol>\n\
         \x20\x20\x20\x20  - Auto-claimed account reward: <claimed amount> <symbol> (txHash=<hash>)\n\
         \x20\x20\x20\x20  - User Agent: <buyerAgentId>\n\
         {ARBITRATION_REASONS_BLOCK}\n\
         \x20\x20\x20\x20  \n\
         \x20\x20\x20\x20  This job is complete."
    )
}

/// `Event::DisputeResolved` branch A (ASP wins) — user notify emitted when
/// A-Step 1 `claimable` returns all zeros (nothing to claim).
pub fn dispute_won_no_claim_user_notify(job_id: &str) -> String {
    format!(
        "\x20\x20\x20\x20[⚖️💰 Dispute Won] Job {job_id} (<title>) — dispute resolved; ASP wins.\n\
         \x20\x20\x20\x20  - Outcome: ProviderWins\n\
         \x20\x20\x20\x20  - Job income: <tokenAmount> <tokenSymbol>\n\
         \x20\x20\x20\x20  - Account-level pending reward: none (checked)\n\
         \x20\x20\x20\x20  - User Agent: <buyerAgentId>\n\
         {ARBITRATION_REASONS_BLOCK}\n\
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

/// User notification after the provider agent auto-rates the User Agent.
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
         {ARBITRATION_REASONS_BLOCK}\n\
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
        upload.file_key, upload.digest, upload.salt,
        upload.nonce, upload.secret, upload.filename,
    )
}

/// User Agent attachment received — notify the provider's user.
pub fn user_attachment_received_user_notify(job_id: &str) -> String {
    format!("[Job `{job_id}`] The User Agent sent an attachment (reference material for this task). File downloaded and saved locally.")
}

