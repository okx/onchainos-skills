//! Terminal states, timeouts, auto-completion, and fallback prompt generators.

use super::super::flow::{FlowContext, notify_and_end, notify_and_end_terminal};

pub(crate) fn job_refunded(ctx: &FlowContext<'_>) -> String {
    let content = super::super::content::job_refunded_user_notify(ctx.job_id);
    notify_and_end_terminal(&content, &ctx.terminal_session_hint)
}

pub(crate) fn job_auto_refunded(ctx: &FlowContext<'_>) -> String {
    let content = super::super::content::job_auto_refunded_user_notify(ctx.job_id, ctx.title_display);
    notify_and_end_terminal(&content, &ctx.terminal_session_hint)
}

pub(crate) fn job_expired(ctx: &FlowContext<'_>) -> String {
    let content = super::super::content::job_expired_user_notify(ctx.job_id);
    notify_and_end(&content)
}

pub(crate) fn job_closed(ctx: &FlowContext<'_>) -> String {
    let content = super::super::content::job_closed_user_notify(ctx.job_id, ctx.title_display);
    notify_and_end_terminal(&content, &ctx.terminal_session_hint)
}

// --- Timeouts / auto-completion ---------------------------------------

pub(crate) async fn submit_expired(ctx: &FlowContext<'_>) -> String {
    use crate::commands::agent_commerce::task::common::network::task_api_client::TaskApiClient;
    let job_id = ctx.job_id;

    let mut client = TaskApiClient::new();
    match super::super::claim_auto_refund::handle_claim_auto_refund(&mut client, job_id).await {
        Ok(()) => {
            let content = super::super::content::submit_expired_user_notify(job_id);
            notify_and_end(&content)
        }
        Err(e) => format!(
            "[submit_expired] `onchainos agent claim-auto-refund {job_id}` failed in-process: {e}\n\n\
             Push a `cli_failed` decision to the user via `pending-decisions-v2 request` (see _shared/exception-escalation.md §2). Do NOT retry blindly.\n"
        ),
    }
}

pub(crate) async fn reject_expired(ctx: &FlowContext<'_>) -> String {
    use crate::commands::agent_commerce::task::common::network::task_api_client::TaskApiClient;
    let job_id = ctx.job_id;

    let mut client = TaskApiClient::new();
    match super::super::claim_auto_refund::handle_claim_auto_refund(&mut client, job_id).await {
        Ok(()) => {
            let content = super::super::content::reject_expired_user_notify(job_id);
            notify_and_end(&content)
        }
        Err(e) => format!(
            "[reject_expired] `onchainos agent claim-auto-refund {job_id}` failed in-process: {e}\n\n\
             Push a `cli_failed` decision to the user via `pending-decisions-v2 request` (see _shared/exception-escalation.md §2). Do NOT retry blindly.\n"
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
        "user",
        agent_id,
        ctx.prefetched.and_then(|p| p.provider_agent_id.as_deref()),
        &review_deadline_prompt,
        &format!("[Decision {short_id}] {title_display} acceptance decision (deadline soon)"),
        "review_deadline_warn",
    );
    format!(
    "[System Notification] review_deadline_warn (review deadline approaching)\n\
     [Role] User Agent\n\n\
     **CRITICAL — this event MUST push the review decision to the user via `pending-decisions-v2 request` (NOT a plain text reply, NOT just `onchainos agent user-notify`).**\n\
     Review deadline = user funds safety red line — if the user is not notified, funds auto-release to the ASP on timeout, irreversibly.\n\
     Do not substitute a plain text reply for the `pending-decisions-v2 request` call.\n\
     Do not substitute `onchainos agent user-notify` for the `pending-decisions-v2 request` (the user must make a review decision; a one-way notify cannot relay).\n\n\
     **Push the review decision to the user (5-substep protocol; read ALL 5 before running any command)**:\n\n\
     {request_block}",
    )
}

// --- User-action pseudo events ----------------------------------------

pub(crate) async fn close_task(ctx: &FlowContext<'_>) -> String {
    use crate::commands::agent_commerce::task::common::network::task_api_client::TaskApiClient;
    let job_id = ctx.job_id;
    let agent_id = ctx.agent_id;

    let close_notify = super::super::content::close_user_notify(job_id);

    let mut client = TaskApiClient::new();
    match super::super::close::handle_close(&mut client, job_id, Some(agent_id)).await {
        Ok(()) => format!(
            "**You MUST notify the user; do not produce a plain text reply inside the sub session** (see Rule 3).\n\n\
             **Notify the user the task was closed via `onchainos agent user-notify`:**\n\
             **Localize first** — translate the canonical English content below.\n\
             ```bash\n\
             onchainos agent user-notify --content '<your translated content>'\n\
             ```\n\
             Canonical English content: \"{close_notify}\"\n"
        ),
        Err(e) => format!(
            "[close_task] `onchainos agent close {job_id}` failed in-process: {e}\n\n\
             Push a `cli_failed` decision to the user via `pending-decisions-v2 request` (see _shared/exception-escalation.md §2). Do NOT retry blindly.\n"
        ),
    }
}

pub(crate) async fn set_public(ctx: &FlowContext<'_>) -> String {
    use crate::commands::agent_commerce::task::common::network::task_api_client::TaskApiClient;
    let job_id = ctx.job_id;
    let agent_id = ctx.agent_id;

    let set_public_notify = super::super::content::set_public_user_notify(job_id);

    let mut client = TaskApiClient::new();
    match super::super::changepublic::handle_set_public(&mut client, job_id, Some(agent_id)).await {
        Ok(()) => format!(
            "**You MUST notify the user; do not produce a plain text reply inside the sub session** (see Rule 3).\n\n\
             **Notify the user the task is now public via `onchainos agent user-notify`:**\n\
             **Localize first** — translate the canonical English content below.\n\
             ```bash\n\
             onchainos agent user-notify --content '<your translated content>'\n\
             ```\n\
             Canonical English content: \"{set_public_notify}\"\n"
        ),
        Err(e) => format!(
            "[set_public] `onchainos agent set-public {job_id}` failed in-process: {e}\n\n\
             Push a `cli_failed` decision to the user via `pending-decisions-v2 request` (see _shared/exception-escalation.md §2). Do NOT retry blindly.\n"
        ),
    }
}

// --- Other events ------------------------------------------------------

pub(crate) fn reward_claimed(ctx: &FlowContext<'_>) -> String {
    let content = super::super::content::reward_claimed_user_notify(ctx.job_id, ctx.title_display);
    notify_and_end(&content)
}

pub(crate) fn wakeup_notify(ctx: &FlowContext<'_>) -> String {
    let job_id = ctx.job_id;
    let agent_id = ctx.agent_id;

    let wakeup_resume = super::super::content::wakeup_resume_user_notify(job_id);
    format!(
    "[System Notification] wakeup_notify (task wake-up after network / machine restart)\n\
     [Role] User Agent\n\n\
     This is a wake-up heartbeat event, **not** a business-driven event. The real business status lives in envelope.message.jobStatus.\n\
     You should not run a playbook with `wakeup_notify` as --event -- this playbook is only a guide.\n\n\
     [Your next actions (strict order)]\n\n\
     **Step 1 — Read the real status from the envelope**:\n\
     From the wakeup_notify envelope that triggered this turn, read `message.jobStatus` (e.g. `accepted` / `submitted` / `rejected` / `disputed` / `completed` / `failed` and other real status strings).\n\n\
     **Step 2 — Re-call next-action with the real status to fetch the current playbook**:\n\
     ```bash\n\
     onchainos agent next-action --role user --agentId {agent_id} --message '{{\"event\":\"<value of message.jobStatus>\",\"jobId\":\"{job_id}\"}}'\n\
     ```\n\
     Follow the returned playbook for what to do at the current status.\n\n\
     **Step 3 — Idempotency self-check (avoid re-prompting the user)**:\n\
     If the playbook from Step 2 would push a decision to the user — i.e. it contains `onchainos agent pending-decisions-v2 request` — **first** call:\n\
     ```bash\n\
     onchainos agent pending-decisions-v2 list --format json\n\
     ```\n\
     - The returned `entries` already contains an entry with `job_id={job_id}` for this role (the prompt was queued before disconnection) → **skip the script's push step**; instead translate the resume notification below into the user's language and send via `onchainos agent user-notify --content '<localized content>'`, then end the turn. Resume notification: {wakeup_resume}\n\
     - No matching entry → run the Step 2 playbook normally; the `pending-decisions-v2 request` call handles the prompt.\n\n\
     **Do not** send the ASP \"I'm back online\" or similar small talk — they do not care about your connection state.\n\
     If the Step 2 playbook is passive (e.g. status=accepted waiting for ASP delivery), just emit a \"task resumed\" notification and end the turn; do not proactively run business actions.\n"
    )
}

// --- Fallback ----------------------------------------------------------

pub(crate) fn staked_and_unknown(event_str: &str, job_id: &str) -> String {
    format!(
    "[Unknown Status] {event_str}\n\
     [Advice]\n\
     1. Call `onchainos agent common context {job_id} --role user` to view full context\n\
     2. If this status is not part of the expected flow, wait for user instructions\n\
     3. Do not predict / assume other notifications\n"
    )
}
