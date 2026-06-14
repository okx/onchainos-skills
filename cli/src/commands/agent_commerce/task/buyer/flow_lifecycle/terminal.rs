//! Terminal states, timeouts, auto-completion, and fallback prompt generators.

use super::super::flow::FlowContext;

pub(crate) fn job_refunded(ctx: &FlowContext<'_>) -> String {
    let l10n_short = super::super::flow::L10N_DISPATCH_SHORT;
    let job_id = ctx.job_id;
    let terminal_session_hint = &ctx.terminal_session_hint;

    let refunded_notify = super::super::content::job_refunded_user_notify(job_id);
    format!(
    "[Current Status] job_refunded (funds refunded to the user)\n\
     [Role] User (User Agent)\n\n\
     🛑 **You MUST call `xmtp_dispatch_user` to notify the user that the refund completed; do not produce a plain text reply inside the sub session** (see Hard Rule 9).\n\n\
     [Your next actions (strict order)]\n\n\
     **Step 1 -- Call xmtp_dispatch_user to notify the user the refund completed** ({l10n_short}):\n\n\
     content:\n\
     {refunded_notify}\n\n\
     **Step 2 -- Terminal wrap-up (keep the sub session):**\n\
     {terminal_session_hint}\n\
     Refund flow fully complete.\n\n\
     [OUTPUT_TEMPLATE]\n\
     Your entire response for this event MUST be exactly:\n\
     1. One `xmtp_dispatch_user` call with the content above\n\
     No other text or tool calls. End turn after the call completes.\n"
    )
}

pub(crate) fn job_auto_refunded(ctx: &FlowContext<'_>) -> String {
    let l10n_short = super::super::flow::L10N_DISPATCH_SHORT;
    let job_id = ctx.job_id;
    let title_display = ctx.title_display;
    let title_query_hint = ctx.title_query_hint;
    let terminal_session_hint = &ctx.terminal_session_hint;

    let auto_refunded_notify = super::super::content::job_auto_refunded_user_notify(job_id, title_display);
    format!(
    "[System Notification] job_auto_refunded (claimAutoRefund tx receipt)\n\
     [Role] User (User Agent)\n\n\
     🛑 **You MUST call `xmtp_dispatch_user` to notify the user the refund has arrived; do not produce a plain text reply inside the sub session** (see Hard Rule 9).\n\n\
     [Your next actions (strict order)]\n\n\
     {title_query_hint}\
     **Step 1 -- Call xmtp_dispatch_user to notify the user the refund has arrived** ({l10n_short}):\n\n\
     content:\n\
     {auto_refunded_notify}\n\n\
     **Step 2 -- Terminal wrap-up (keep the sub session):**\n\
     {terminal_session_hint}\n\
     Refund flow fully complete.\n\n\
     [OUTPUT_TEMPLATE]\n\
     Your entire response for this event MUST be exactly:\n\
     1. One `xmtp_dispatch_user` call with the content above\n\
     No other text or tool calls. End turn after the call completes.\n"
    )
}

pub(crate) fn job_expired(ctx: &FlowContext<'_>) -> String {
    let l10n_short = super::super::flow::L10N_DISPATCH_SHORT;
    let job_id = ctx.job_id;

    let expired_notify = super::super::content::job_expired_user_notify(job_id);
    format!(
    "[Current Status] job_expired (task expired; no ASP accepted or no submission)\n\
     [Role] User (User Agent)\n\n\
     [Your next actions]\n\n\
     **Step 1 -- Call xmtp_dispatch_user to notify the user the task expired** ({l10n_short}):\n\
     \x20\x20content: {expired_notify}\n\n\
     This task reached a terminal state; the flow ends.\n\n\
     [OUTPUT_TEMPLATE]\n\
     Your entire response for this event MUST be exactly:\n\
     1. One `xmtp_dispatch_user` call with the content above\n\
     No other text or tool calls. End turn after the call completes.\n"
    )
}

pub(crate) fn job_closed(ctx: &FlowContext<'_>) -> String {
    let l10n_short = super::super::flow::L10N_DISPATCH_SHORT;
    let job_id = ctx.job_id;
    let title_display = ctx.title_display;
    let title_query_hint = ctx.title_query_hint;
    let terminal_session_hint = &ctx.terminal_session_hint;

    let closed_notify = super::super::content::job_closed_user_notify(job_id, title_display);
    format!(
    "[Current Status] job_closed (close tx result notification)\n\
     [Role] User (User Agent)\n\n\
     [Your next actions]\n\n\
     {title_query_hint}\
     **Step 1 -- Call xmtp_dispatch_user to notify the user** ({l10n_short}):\n\
     \x20\x20content: {closed_notify}\n\n\
     **Terminal wrap-up (keep the sub session):**\n\
     {terminal_session_hint}\n\
     Close flow ends.\n\n\
     [OUTPUT_TEMPLATE]\n\
     Your entire response for this event MUST be exactly:\n\
     1. One `xmtp_dispatch_user` call with the content above\n\
     No other text or tool calls. End turn after the call completes.\n"
    )
}

// --- Timeouts / auto-completion ---------------------------------------

pub(crate) async fn submit_expired(ctx: &FlowContext<'_>) -> String {
    use crate::commands::agent_commerce::task::common::network::task_api_client::TaskApiClient;
    let l10n_short = super::super::flow::L10N_DISPATCH_SHORT;
    let job_id = ctx.job_id;

    let submit_expired = super::super::content::submit_expired_user_notify(job_id);

    // Rust in-process claim-auto-refund — symmetric to approve_review /
    // reject_review (each broadcasts a tx in-process and tells the LLM to
    // just notify the user). Failure → cli_failed bail.
    let mut client = TaskApiClient::new();
    match super::super::claim_auto_refund::handle_claim_auto_refund(&mut client, job_id).await {
        Ok(()) => format!(
            "🛑 **You MUST call `xmtp_dispatch_user` to notify the user; do not produce a plain text reply inside the sub session** (see Hard Rule 9).\n\n\
             **Call xmtp_dispatch_user to notify the user** ({l10n_short}):\n\
             content: \"{submit_expired}\"\n"
        ),
        Err(e) => format!(
            "[submit_expired] ❌ `onchainos agent claim-auto-refund {job_id}` failed in-process: {e}\n\n\
             Push a `cli_failed` decision to the user via `pending-decisions-v2 request` (see SKILL.md §Exception Escalation 5-substep protocol). Do NOT retry blindly.\n"
        ),
    }
}

pub(crate) async fn reject_expired(ctx: &FlowContext<'_>) -> String {
    use crate::commands::agent_commerce::task::common::network::task_api_client::TaskApiClient;
    let l10n_short = super::super::flow::L10N_DISPATCH_SHORT;
    let job_id = ctx.job_id;

    let reject_expired = super::super::content::reject_expired_user_notify(job_id);

    let mut client = TaskApiClient::new();
    match super::super::claim_auto_refund::handle_claim_auto_refund(&mut client, job_id).await {
        Ok(()) => format!(
            "🛑 **You MUST call `xmtp_dispatch_user` to notify the user; do not produce a plain text reply inside the sub session** (see Hard Rule 9).\n\n\
             **Call xmtp_dispatch_user to notify the user** ({l10n_short}):\n\
             content: \"{reject_expired}\"\n"
        ),
        Err(e) => format!(
            "[reject_expired] ❌ `onchainos agent claim-auto-refund {job_id}` failed in-process: {e}\n\n\
             Push a `cli_failed` decision to the user via `pending-decisions-v2 request` (see SKILL.md §Exception Escalation 5-substep protocol). Do NOT retry blindly.\n"
        ),
    }
}

pub(crate) fn review_deadline_warn(ctx: &FlowContext<'_>) -> String {
    let job_id = ctx.job_id;
    let agent_id = ctx.agent_id;
    let short_id = ctx.short_id;
    let title_display = ctx.title_display;
    let review_deadline_prompt = super::super::content::review_deadline_warn_user_prompt(job_id, short_id);
    let request_block = crate::commands::agent_commerce::task::common::pending_v2::request_command_block(
        job_id,
        "buyer",
        agent_id,
        &review_deadline_prompt,
        &format!("[Decision {short_id}] {title_display} acceptance decision (deadline soon)"),
        "review_deadline_warn",
    );
    format!(
    "[System Notification] review_deadline_warn (review deadline approaching)\n\
     [Role] User (User Agent)\n\n\
     🛑 **CRITICAL -- this event MUST push the review decision to the user via `pending-decisions-v2 request` (NOT a plain text reply, NOT just `xmtp_dispatch_user`).**\n\
     Review deadline = user funds safety red line — if the user is not notified, funds auto-release to the ASP on timeout, irreversibly.\n\
     ❌ Do not substitute a plain text reply for the `pending-decisions-v2 request` call.\n\
     ❌ Do not substitute `xmtp_dispatch_user` for the `pending-decisions-v2 request` (the user must make a review decision; dispatch_user cannot relay).\n\n\
     **Push the review decision to the user (5-substep protocol; read ALL 5 before running any command)**:\n\n\
     {request_block}",
    )
}

pub(crate) fn review_expired(ctx: &FlowContext<'_>) -> String {
    let l10n_short = super::super::flow::L10N_DISPATCH_SHORT;
    let job_id = ctx.job_id;

    let review_expired = super::super::content::review_expired_user_notify(job_id);
    format!(
    "[System Notification] review_expired (review window expired; task is still submitted)\n\
     [Role] User (User Agent)\n\n\
     🛑 **You MUST call `xmtp_dispatch_user` to notify the user the review window expired; do not produce a plain text reply inside the sub session** (see Hard Rule 9).\n\n\
     [Your next actions]\n\n\
     **Step 1 -- Call xmtp_dispatch_user to notify the user the review window expired** ({l10n_short}):\n\
     \x20\x20content:\n\
     {review_expired}\n\n\
     **Step 2** -- Wait for the `job_auto_completed` system notification and then wrap up.\n\n\
     [OUTPUT_TEMPLATE]\n\
     Your entire response for this event MUST be exactly:\n\
     1. One `xmtp_dispatch_user` call with the content above\n\
     No other text or tool calls. End turn after the call completes.\n"
    )
}

pub(crate) fn job_auto_completed(ctx: &FlowContext<'_>) -> String {
    let l10n_short = super::super::flow::L10N_DISPATCH_SHORT;
    let job_id = ctx.job_id;
    let agent_id = ctx.agent_id;
    let terminal_session_hint = &ctx.terminal_session_hint;

    let rating_notify = super::super::content::rating_submitted_user_notify(job_id);

    // job_auto_completed fires on the claimAutoComplete tx receipt — the
    // chain has settled to Completed and a provider is guaranteed to exist.
    // Anything else (no prefetched / missing provider) is a data anomaly —
    // bail to a cli_failed instruction instead of running a half-blind
    // playbook that asks the LLM to chase down providerAgentId via
    // `common context`.
    let p = match ctx.prefetched {
        Some(p) => p,
        None => return format!(
            "[job_auto_completed] ❌ no prefetched task context for job {job_id}; auto-rate cannot run.\n\n\
             Push a `cli_failed` decision to the user via `pending-decisions-v2 request`.\n"
        ),
    };
    let provider_id = match p.provider_agent_id.as_deref().filter(|s| !s.is_empty()) {
        Some(s) => s,
        None => return format!(
            "[job_auto_completed] ❌ prefetched.provider_agent_id missing for job {job_id}; auto-rate cannot run.\n\n\
             Push a `cli_failed` decision to the user via `pending-decisions-v2 request`.\n"
        ),
    };

    // prefetched.title is authoritative — use it directly instead of
    // ctx.title_display (which falls back to `<title>` placeholder when the
    // envelope lacks jobTitle and would force the LLM to query).
    let auto_completed_notify = super::super::content::job_auto_completed_user_notify(job_id, &p.title);

    format!(
    "[System Notification] job_auto_completed (claimAutoComplete tx receipt)\n\
     [Role] User (User Agent)\n\n\
     🛑 **You MUST call `xmtp_dispatch_user` to notify the user the task auto-completed; do not produce a plain text reply inside the sub session** (see Hard Rule 9).\n\n\
     [Your next actions]\n\n\
     **Step 1 -- Call xmtp_dispatch_user to notify the user the task auto-completed** ({l10n_short}):\n\
     \x20\x20content:\n\
     {auto_completed_notify}\n\n\
     🛑 Do NOT end this turn — Step 3 (auto-rate) and Step 3.5 (notify rating) below are MANDATORY.\n\n\
     **Step 2 — Task fields (pre-fetched; do NOT call `common context`):**\n\
     \x20\x20- title: {title}\n\
     \x20\x20- tokenAmount: {amt} | tokenSymbol: {sym}\n\
     \x20\x20- providerAgentId: {provider_id}\n\n\
     **Step 3 -- 🛑 Auto-rate the ASP (MANDATORY):**\n\
     Based on the deliverable vs the task description and quality standards, generate:\n\
     \x20\x20- Score: 0.00–5.00 (two decimal places). Guide: 5.00 = exceeds expectations, 4.00 = fully meets, 3.00 = acceptable with minor gaps, 2.00 = partially meets, 1.00 = mostly inadequate, 0.00 = did not deliver.\n\
     \x20\x20- Comment: one sentence, ≤100 characters, evaluating how well the deliverable matches the description.\n\
     Then execute:\n\
     ```bash\n\
     onchainos agent feedback-submit --agent-id {provider_id} --creator-id {agent_id} --score <X.XX> --task-id {job_id} --description \"<comment, ≤100 chars>\"\n\
     ```\n\n\
     **Step 3.5 -- Notify the user of the submitted rating ({l10n_short}):**\n\
     After feedback-submit, call `xmtp_dispatch_user` to notify the user:\n\
     - ✅ **Success** (output contains `txHash`):\n\
     content (fill `<score>` with the X.XX value and `<description>` with the comment you just used in Step 3; fill `<title>` from Step 2):\n\
     {rating_notify}\n\
     - ❌ **Failure** (error / non-zero exit code) → silently skip; do NOT notify the user, do NOT retry.\n\n\
     **Step 4 -- Terminal wrap-up (keep the sub session):**\n\
     {terminal_session_hint}\n\
     Task fully complete.\n\n\
     [OUTPUT_TEMPLATE]\n\
     Your entire response for this event MUST include ALL of the following tool calls, in order:\n\
     1. One `xmtp_dispatch_user` call — auto-completion notification (Step 1)\n\
     2. One `onchainos agent feedback-submit` call — auto-rate the ASP (Step 3)\n\
     3. One `xmtp_dispatch_user` call — rating notification (Step 3.5; skip ONLY if Step 3 returned an error)\n\
     Stopping after Step 1 is a **critical failure** — the user will never see their rating.\n"
    ,
    title = p.title,
    amt = p.token_amount,
    sym = p.token_symbol,
    )
}

// --- User-action pseudo events ----------------------------------------

pub(crate) async fn close_task(ctx: &FlowContext<'_>) -> String {
    use crate::commands::agent_commerce::task::common::network::task_api_client::TaskApiClient;
    let l10n_short = super::super::flow::L10N_DISPATCH_SHORT;
    let job_id = ctx.job_id;
    let agent_id = ctx.agent_id;

    let close_notify = super::super::content::close_user_notify(job_id);

    let mut client = TaskApiClient::new();
    match super::super::close::handle_close(&mut client, job_id, Some(agent_id)).await {
        Ok(()) => format!(
            "🛑 **You MUST call `xmtp_dispatch_user` to notify the user; do not produce a plain text reply inside the sub session** (see Hard Rule 9).\n\n\
             **Call xmtp_dispatch_user to notify the user the task was closed** ({l10n_short}):\n\
             content: \"{close_notify}\"\n"
        ),
        Err(e) => format!(
            "[close_task] ❌ `onchainos agent close {job_id}` failed in-process: {e}\n\n\
             Push a `cli_failed` decision to the user via `pending-decisions-v2 request` (see SKILL.md §Exception Escalation 5-substep protocol). Do NOT retry blindly.\n"
        ),
    }
}

pub(crate) async fn set_public(ctx: &FlowContext<'_>) -> String {
    use crate::commands::agent_commerce::task::common::network::task_api_client::TaskApiClient;
    let l10n_short = super::super::flow::L10N_DISPATCH_SHORT;
    let job_id = ctx.job_id;
    let agent_id = ctx.agent_id;

    let set_public_notify = super::super::content::set_public_user_notify(job_id);

    let mut client = TaskApiClient::new();
    match super::super::changepublic::handle_set_public(&mut client, job_id, Some(agent_id)).await {
        Ok(()) => format!(
            "🛑 **You MUST call `xmtp_dispatch_user` to notify the user; do not produce a plain text reply inside the sub session** (see Hard Rule 9).\n\n\
             **Call xmtp_dispatch_user to notify the user the task is now public** ({l10n_short}):\n\
             content: \"{set_public_notify}\"\n"
        ),
        Err(e) => format!(
            "[set_public] ❌ `onchainos agent set-public {job_id}` failed in-process: {e}\n\n\
             Push a `cli_failed` decision to the user via `pending-decisions-v2 request` (see SKILL.md §Exception Escalation 5-substep protocol). Do NOT retry blindly.\n"
        ),
    }
}

// --- Other events ------------------------------------------------------

pub(crate) fn submit_deadline_warn() -> String {
    "[System Notification] submit_deadline_warn (provider-side deadline reminder)\n\
     [Role] User (User Agent)\n\n\
     [Advice] Stay silent and observe; wait for the provider to submit the deliverable (job_submitted notification) before acting.\n\n\
     [OUTPUT_TEMPLATE]\n\
     End turn immediately with no tool calls or text output.\n".to_string()
}

pub(crate) fn evaluator_events(event_str: &str) -> String {
    format!(
    "[System Notification] {event_str} (internal arbitration event, handled by evaluator)\n\
     [Role] User (User Agent)\n\n\
     [Advice] Stay silent and observe. After `dispute_resolved` arrives, call next-action to wrap up.\n\n\
     [OUTPUT_TEMPLATE]\n\
     End turn immediately with no tool calls or text output.\n"
    )
}

pub(crate) fn reward_claimed(ctx: &FlowContext<'_>) -> String {
    let l10n_short = super::super::flow::L10N_DISPATCH_SHORT;
    let job_id = ctx.job_id;
    let title_display = ctx.title_display;
    let title_query_hint = ctx.title_query_hint;

    let reward_claimed = super::super::content::reward_claimed_user_notify(job_id, title_display);
    format!(
    "[System Notification] reward_claimed (claimRewards tx receipt)\n\
     [Role] User (User Agent)\n\n\
     [Your next actions]\n\n\
     {title_query_hint}\
     **Step 1 -- Call xmtp_dispatch_user to notify the user the reward has arrived** ({l10n_short}):\n\
     \x20\x20content: {reward_claimed}\n\n\
     [OUTPUT_TEMPLATE]\n\
     Your entire response for this event MUST be exactly:\n\
     1. One `xmtp_dispatch_user` call with the content above\n\
     No other text or tool calls. End turn after the call completes.\n"
    )
}

pub(crate) fn wakeup_notify(ctx: &FlowContext<'_>) -> String {
    let job_id = ctx.job_id;
    let agent_id = ctx.agent_id;

    let wakeup_resume = super::super::content::wakeup_resume_user_notify(job_id);
    format!(
    "[System Notification] wakeup_notify (task wake-up after network / machine restart)\n\
     [Role] User (User Agent)\n\n\
     ⚠️ This is a wake-up heartbeat event, **not** a business-driven event. The real business status lives in envelope.message.jobStatus.\n\
     You should not run a playbook with `wakeup_notify` as --event -- this playbook is only a guide.\n\n\
     [Your next actions (strict order)]\n\n\
     **Step 1 -- Read the real status from the envelope**:\n\
     From the wakeup_notify envelope that triggered this turn, read `message.jobStatus` (e.g. `accepted` / `submitted` / `rejected` / `disputed` / `completed` / `failed` and other real status strings).\n\n\
     **Step 2 -- Re-call next-action with the real status to fetch the current playbook**:\n\
     ```bash\n\
     onchainos agent next-action --jobid {job_id} --event <value of message.jobStatus> --role buyer --agentId {agent_id}\n\
     ```\n\
     Follow the returned playbook for what to do at the current status.\n\n\
     **Step 3 -- Idempotency self-check (avoid re-prompting the user)**:\n\
     If the playbook from Step 2 would push a decision to the user — i.e. it contains `onchainos agent pending-decisions-v2 request` — **first** call:\n\
     ```bash\n\
     onchainos agent pending-decisions-v2 list --format json\n\
     ```\n\
     - The returned `entries` already contains a sub_key with `job={job_id}` for this role (the prompt was queued before disconnection) → **skip the script's push step**; instead call `xmtp_dispatch_user` content=`{wakeup_resume}` (🌐 localize per [Localization] rules) and end the turn.\n\
     - No matching entry → run the Step 2 playbook normally; the `pending-decisions-v2 request` call handles the prompt.\n\n\
     ⚠️ **Do not** xmtp_send the ASP \"I'm back online\" or similar small talk -- they do not care about your connection state.\n\
     ⚠️ If the Step 2 playbook is passive (e.g. status=accepted waiting for ASP delivery), just emit a \"task resumed\" notification and end the turn; do not proactively run business actions.\n"
    )
}

// --- Fallback ----------------------------------------------------------

pub(crate) fn staked_and_unknown(event_str: &str, job_id: &str) -> String {
    format!(
    "[Unknown Status] {event_str}\n\
     [Advice]\n\
     1. Call `onchainos agent common context {job_id} --role buyer` to view full context\n\
     2. If this status is not part of the expected flow, wait for user instructions\n\
     3. Do not predict / assume other notifications\n"
    )
}
