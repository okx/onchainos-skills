//! Event handlers for job_created and provider_conversation.

use super::super::flow::FlowContext;

// --- Event handler functions ------------------------------------------------

pub(crate) fn job_created(ctx: &FlowContext<'_>) -> String {
    // No designated provider → asp-match flow; designated → route_only flow.
    let has_designated = super::super::negotiate::get_designated_provider(ctx.job_id)
        .ok()
        .flatten()
        .is_some();
    if !has_designated {
        return job_created_non_designated_provider(ctx);
    }
    job_created_with_designated_provider(ctx)
}

pub(crate) async fn job_created_cli(ctx: &FlowContext<'_>) -> String {
    // No designated provider → asp-match flow; designated → route_only flow.
    let has_designated = super::super::negotiate::get_designated_provider(ctx.job_id)
        .ok()
        .flatten()
        .is_some();
    if !has_designated {
        return job_created_non_designated_provider_cli(ctx);
    }
    job_created_with_designated_provider_cli(ctx).await
}

fn job_created_non_designated_provider_cli(ctx: &FlowContext<'_>) -> String {
    let title = ctx.title_display;
    let short_id = ctx.short_id;
    let notify_tpl = super::super::content::job_created_non_designated_user_notify();

    let notify_filled = notify_tpl
        .replace("<title>", title)
        .replace("<short_jobId>", short_id);

    format!(
        "[Trigger] job_created (on-chain, public task — no designated provider)\n\
         [Role] User (Buyer)\n\n\
         🛑 Execute the 1 action below, then end the turn. The task is public; ASPs will discover it and reach out via `provider_conversation`.\n\n\
         **Action 1 — Notify the user that the job is on-chain.** Translate the canonical English notification below to the user's chat language (per [Localization] rules), then dispatch it:\n\
         Canonical content (`<title>` and `<short_jobId>` already filled in):\n\
         \x20\x20{notify_filled}\n\
         ```bash\n\
         okx-a2a user notify --content '<your translated content>'\n\
         ```\n\n\
         🛑 End the turn after notifying. Do NOT call `asp-match` — public tasks wait for ASPs to apply.\n"
    )
}

async fn job_created_with_designated_provider_cli(ctx: &FlowContext<'_>) -> String {
    let job_id = ctx.job_id;
    let agent_id = ctx.agent_id;
    let short_id = ctx.short_id;
    let title = ctx.title_display;

    let dp_id = super::super::negotiate::get_designated_provider(job_id)
        .ok()
        .flatten()
        .expect("job_created_with_designated_provider_cli called only when designated provider exists");

    let notify_tpl = super::super::content::job_created_designated_user_notify();
    let designated_endpoint = super::super::negotiate::get_designated_endpoint(job_id).ok().flatten();

    // Fill the static placeholders in the notify template so the LLM only
    // has to translate (no placeholder bookkeeping). Dispatch itself is
    // LLM-driven so the content is in the user's language.
    let notify_filled = notify_tpl
        .replace("<title>", title)
        .replace("<short_jobId>", short_id)
        .replace("<provider_agentId>", &dp_id);
    let notify_prelude = format!(
        "**Action 0 — Notify the user the job is on-chain.** Translate the canonical English notification below to the user's chat language (per [Localization] rules), then dispatch it:\n\
         Canonical content (placeholders already filled in):\n\
         \x20\x20{notify_filled}\n\
         ```bash\n\
         okx-a2a user notify --content '<your translated content>'\n\
         ```\n\n\
         After Action 0 completes, follow the branch-specific playbook below:\n\n---\n\n"
    );

    // D-Step 1 — designated-route query (in-process).
    let route_result = crate::commands::agent_commerce::task::common::designated_route_inner(
        &dp_id,
        designated_endpoint.as_deref(),
    )
    .await;
    let route_json = match route_result {
        Ok(j) => j,
        Err(e) => return format!("[job_created_cli] ERROR: designated-route failed: {e}\n"),
    };

    // D-Step 2 — dispatch in-process to the matching branch playbook, skipping
    // the "LLM calls `next-action --event designated_*`" round-trip entirely.
    // The a2a branch additionally inlines B-Step 0 / 1 / 1.5 (session
    // duplicate guard + create + SKILL_PREFETCH) via `branch_a2a_cli`.
    let route = route_json.get("route").and_then(|v| v.as_str()).unwrap_or("");
    let branch_playbook = match route {
        "a2a" => super::designated::branch_a2a_cli(job_id, agent_id, short_id, &dp_id, title, ctx.prefetched),
        "x402" => super::designated::branch_x402(job_id, agent_id, short_id, &dp_id),
        "error" => super::designated::branch_error(job_id, agent_id, short_id, &dp_id),
        _ => return format!(
            "[job_created_cli] ERROR: unknown route value '{route}' in designated-route response: {route_json}\n"
        ),
    };
    format!("{notify_prelude}{branch_playbook}")
}

/// job_created flow when no provider is designated (public task).
///
/// Notify user → end turn. ASPs discover the public task and reach out
/// via `provider_conversation`.
fn job_created_non_designated_provider(ctx: &FlowContext<'_>) -> String {
    let l10n_short = super::super::flow::L10N_DISPATCH_SHORT;
    let short_id = ctx.short_id;
    let title = ctx.title_display;
    let notify_tpl = super::super::content::job_created_non_designated_user_notify();
    format!(
        "[Trigger] job_created (on-chain, public task — no designated provider)\n\
         [Role] User (Buyer)\n\n\
         🛑 You are inside a sub/backup session. Execute the 2 actions below, then end the turn.\n\
         The task is public; ASPs will discover it and reach out via `provider_conversation`.\n\n\
         **Action 1 — Notify the user the job is on-chain** (translate template body to the user's language before sending):\n\
         ```bash\n\
         okx-a2a user notify --content '<translated content from the template below>'\n\
         ```\n\
         content (canonical English template — translate before passing): {notify_tpl}\n\
         Fill: `<title>` = {title} | `<short_jobId>` = {short_id}\n\
         {l10n_short}\n\n\
         **Action 2 — End the turn.**\n\
         Do NOT call `asp-match` — public tasks wait for ASPs to apply.\n\n\
         🛑 Forbidden: `asp-match`, `okx-a2a session create`, `set-payment-mode`, \
         `confirm-accept`, `apply`, `complete`, `reject`.\n"
    )
}

/// job_created flow when a designated provider exists.
///
/// Failure fallback (x402_invalid / not_provider / offline / negotiation fail)
/// is handled by `user_decision_*` events in flow.rs, not here.
fn job_created_with_designated_provider(ctx: &FlowContext<'_>) -> String {
    let l10n_short = super::super::flow::L10N_DISPATCH_SHORT;
    let job_id = ctx.job_id;
    let agent_id = ctx.agent_id;
    let short_id = ctx.short_id;
    let title = ctx.title_display;

    let dp_id = super::super::negotiate::get_designated_provider(job_id)
        .ok()
        .flatten()
        .expect("job_created_with_designated_provider called only when designated provider exists");

    let notify_tpl = super::super::content::job_created_designated_user_notify();
    let fill = format!("Fill: `<title>` = {title} | `<short_jobId>` = {short_id} | `<provider_agentId>` = {dp_id}");

    let designated_endpoint = super::super::negotiate::get_designated_endpoint(job_id).ok().flatten();
    let routing_section = super::designated::route_only(job_id, agent_id, short_id, &dp_id, designated_endpoint.as_deref());

    format!(
        "🛑🛑🛑 **IDENTITY CHECK - you are the executor; delegation is forbidden**\n\
         You are inside a sub session or backup session. **You yourself** are the agent responsible for executing this script.\n\
         ❌ **Absolutely forbidden**: `sessions_spawn` - do NOT spawn a child agent to \"help you\" handle this event.\n\
         ❌ **Absolutely forbidden**: `sessions_yield` - do NOT hand off control.\n\
         🔴 Real incident: after receiving job_created, a backup called sessions_spawn to delegate to a child agent, which broke the designated-provider consume-context invariant and made negotiation uncontrollable.\n\
         **Correct behavior**: you yourself execute the CLI commands step by step as below.\n\n\
         [Current state] job_created (job is on-chain, status: pending acceptance)\n\
         [Role] User (User Agent)\n\n\
         ⚠️ **Open != public**: Open is a job lifecycle state (pending acceptance), not a visibility (public/private). Job visibility is governed by the `visibility` field (0=public, 1=private), unrelated to the Open state. Do NOT translate Open as \"public\" in notifications.\n\n\
         🛑 **CLIs forbidden in this event**: set-payment-mode / confirm-accept / apply / complete / reject - no ASP has been picked yet, negotiation has not started, all of these are illegal here.\n\n\
         🛑🛑🛑 You MUST execute ALL steps below immediately in this turn. Do NOT end the turn before completing Step 0 (notify user) and D-Step 1 (designated-route query).\n\
         Ending the turn without executing = user never gets notified = task stalls permanently.\n\
         🔴 Real incident: a model called next-action, received this playbook, then said \"end turn, wait for User Agent\" without executing any step — the user was never notified and the task was permanently stuck.\n\n\
         [Your next actions (strict order)]\n\n\
         **Step 0 - notify the user session + continue execution in the current sub/backup session:**\n\
         Run `okx-a2a user notify` to tell the user the job is on-chain:\n\
         \x20\x20```bash\n\
         \x20\x20okx-a2a user notify --content '<translated content from the template below>'\n\
         \x20\x20```\n\
         \x20\x20content (canonical English template — translate before passing): {notify_tpl}\n\
         {fill}\n\
         {l10n_short}\n\n\
         ⚠️ Subsequent routing -> negotiation / acceptance all run in the **current session**; do NOT switch to the user session, do NOT sessions_spawn.\n\n\
         {routing_section}\n\n"
    )
}

pub(crate) fn provider_conversation(ctx: &FlowContext<'_>) -> String {
    let l10n_short = super::super::flow::L10N_DISPATCH_SHORT;
    let l10n_prompt = super::super::flow::L10N_PROMPT;
    let follow_playbook = super::super::flow::FOLLOW_PLAYBOOK;
    let job_id = ctx.job_id;
    let agent_id = ctx.agent_id;
    let short_id = ctx.short_id;
    let title = ctx.title_display;
    let cmd_pending_asp = super::super::flow::pending_cmd(job_id, agent_id, None, &format!("[Pending ASP {short_id}] {title} ASP-contact decision"), "provider_pending");

    let pending_empty = super::super::content::pending_list_empty_user_notify();
    format!(
    "[Trigger] Received an \"ASP pending contact\" style message\n\
     [Role] User (User Agent)\n\n\
     🛑 **Do NOT auto-create groups or auto-negotiate**: you must NOT call `okx-a2a session create`, `okx-a2a xmtp-send`, or send any message to the ASP on your own.\n\
     You must fetch the ASP list and let the user pick; the picked ASP will be routed through the designated-provider flow.\n\n\
     🛑 **CRITICAL - this event MUST push the ASP list to the user session via `pending-decisions-v2 request`; printing text reply in the sub session is forbidden.**\n\
     ❌ Do NOT replace the `pending-decisions-v2 request` call with a text reply (sub-session output is invisible to the user).\n\
     ❌ Do NOT use `okx-a2a user notify` instead of `pending-decisions-v2 request` (the user needs to make an ASP-choice decision; notify is pure notification and cannot relay).\n\n\
     [Your next actions (strict order)]\n\n\
     **Step 0 - idempotency check: query whether a pending decision already exists for this job:**\n\
     ```bash\n\
     onchainos agent pending-decisions-v2 list --format json\n\
     ```\n\
     If the returned `entries` array already contains an entry with job_id={job_id} and role=buyer -> **the user has already been notified; this is a duplicate event - end the turn without notifying again.**\n\
     If not present -> continue to Step 1.\n\n\
     **Step 1 - fetch the pending-contact ASP list:**\n\
     Run `okx-a2a task requests --json`. The returned `items` array contains per-ASP entries — capture `agentId` / `name` / `serviceName` / `creditScore` / `completedTaskCount` for each entry.\n\n\
     If the returned `items` array is empty -> run `okx-a2a user notify`:\n\
     \x20\x20```bash\n\
     \x20\x20okx-a2a user notify --content '<translated content from the template below>'\n\
     \x20\x20```\n\
     \x20\x20content (canonical English template — translate before passing): {pending_empty}\n\
     {l10n_short}\n\
     Then finish.\n\n\
     **Step 2 - enqueue the user decision via `pending-decisions-v2 request`:**\n\
     🛑 **You MUST wait for the user's choice**; you may not decide for them.\n\
     ```bash\n\
     {cmd_pending_asp}\n\
     ```\n\
     {l10n_prompt}\n\
     `--user-content` template (canonical English):\n\
     [Job {short_id} — you are the User Agent] The following ASPs have reached out. Pick one to designate as the provider:\n\
     \n\
     [iterate pending list; format per ASP (use fields from the `okx-a2a task requests` response):]\n\
     <N>. agentId: <agentId> | name: <name or serviceName, omit if absent> | credit: <creditScore> | completed jobs: <completedTaskCount>\n\
     \n\
     Reply with the ASP's number to designate, or reply 「skip all」.\n\n\
     {follow_playbook}\n\n\
     **Step 3 - End this turn.** The user's reply will be relayed as `user_decision_provider_pending`; the `provider_pending` handler routes the picked ASP through the designated-provider flow (designated-route → branch_a2a / branch_x402 / branch_error).\n")

}

/// CLI-mode handler for `provider_conversation_pick` — user picked an ASP
/// from the pending list. Runs `designated_route_inner` in-process and
/// dispatches to the matching branch. For the A2A route, handles the
/// public-task case where the daemon already created a session when the
/// ASP sent its conversation request (skips duplicate-guard bail-out).
pub(crate) async fn provider_conversation_pick_cli(
    job_id: &str,
    agent_id: &str,
    short_id: &str,
    dp_id: &str,
    _title_display: &str,
    _prefetched: Option<&crate::commands::agent_commerce::task::common::PreFetchedTaskContext>,
) -> String {
    let route_result = crate::commands::agent_commerce::task::common::designated_route_inner(
        dp_id,
        None,
    )
    .await;
    let route_json = match route_result {
        Ok(j) => j,
        Err(e) => return format!("[provider_conversation_pick] ERROR: designated-route failed: {e}\n"),
    };

    let route = route_json.get("route").and_then(|v| v.as_str()).unwrap_or("");
    match route {
        "a2a" => provider_conversation_pick_a2a(job_id, agent_id, dp_id),
        "x402" => super::designated::branch_x402(job_id, agent_id, short_id, dp_id),
        "error" => super::designated::branch_error(job_id, agent_id, short_id, dp_id),
        _ => format!(
            "[provider_conversation_pick] ERROR: unknown route value '{route}' in designated-route response: {route_json}\n"
        ),
    }
}

/// A2A branch for public-task ASP pick. Unlike `branch_a2a_cli` (which
/// bails on existing sessions), this handles the public-task case where
/// the daemon already created a session when the ASP sent its conversation
/// request: session exists → skip create, ensure SKILL_PREFETCH → wait.
fn provider_conversation_pick_a2a(job_id: &str, agent_id: &str, dp_id: &str) -> String {
    use crate::commands::agent_commerce::task::common::okx_a2a;

    let prefetch = "[SKILL_PREFETCH] Read the okx-agent-task skill. Pre-load buyer role context. \
        This prefetch message itself requires no action — but when the NEXT inbound message arrives \
        (same turn or later turn), you MUST process it normally via buyer-sub-playbook.md \
        §Peer Message Routing (#1–#6). Do NOT carry over \"no action\" to business messages.";

    if !okx_a2a::session_query_exists(job_id, agent_id, dp_id).unwrap_or(false) {
        if let Err(e) = okx_a2a::session_create(job_id, agent_id, dp_id) {
            return format!("[provider_conversation_pick] ERROR: session create failed: {e}\n");
        }
    }
    if let Err(e) = okx_a2a::session_send_by_job(job_id, Some(dp_id), prefetch) {
        return format!("[provider_conversation_pick] ERROR: SKILL_PREFETCH failed: {e}\n");
    }

    format!(
        "[Provider picked: A2A] Provider {dp_id} — session ready.\n\
         [Role] User (Buyer)\n\n\
         ✅ Sub session and SKILL_PREFETCH ready. The ASP will receive `job_asp_selected` from the backend and independently decide to apply on-chain.\n\n\
         🛑 **End this turn immediately.** Your ONLY next action is to wait for the `provider_applied` system event.\n\
         ❌ Do NOT send any message (`xmtp_send`) to the ASP — no negotiation needed; the ASP already expressed interest.\n\
         ❌ Do NOT call `confirm-accept` / `set-payment-mode` — the ASP has not applied yet.\n\n\
         [What happens next]\n\
         The ASP receives `job_asp_selected` → ASP on-chain apply → system fires `provider_applied` event.\n"
    )
}

/// CLI-mode handler for `provider_conversation`. Sinks Steps 0–1 (idempotency
/// check + ASP list fetch) to in-process Rust, eliminating 2 CLI calls.
/// Fast paths (duplicate event, empty list) skip LLM work entirely.
pub(crate) fn provider_conversation_cli(ctx: &FlowContext<'_>) -> String {
    use crate::commands::agent_commerce::task::common::{okx_a2a, pending_v2};

    let job_id = ctx.job_id;
    let agent_id = ctx.agent_id;
    let short_id = ctx.short_id;
    let title = ctx.title_display;
    let l10n_dispatch = super::super::flow::L10N_DISPATCH_SHORT;

    // Step 0: idempotency — pending decision already queued?
    if pending_v2::has_pending_for_job(job_id, "buyer") {
        return format!(
            "[provider_conversation] Duplicate event — pending decision already exists for job {short_id}. End turn.\n"
        );
    }

    // Step 1: fetch ASP list in-process
    let items = match okx_a2a::task_requests() {
        Ok(v) => v,
        Err(e) => return format!("[provider_conversation] ERROR: task requests failed: {e}\n"),
    };

    if items.is_empty() {
        let content = super::super::content::pending_list_empty_user_notify();
        return format!(
            "[provider_conversation] No pending ASPs.\n\n\
             **Action — notify the user.** Translate the canonical English below to the user's chat language, then dispatch:\n\
             Canonical: {content}\n\
             ```bash\n\
             okx-a2a user notify --content '<your translated content>' --json\n\
             ```\n\
             {l10n_dispatch}\n\
             🛑 End turn after notifying.\n"
        );
    }

    // Pre-format ASP list so the LLM only translates, no JSON iteration
    let mut asp_lines = String::new();
    for (i, item) in items.iter().enumerate() {
        let aid = item.get("agentId").and_then(|v| v.as_str()).unwrap_or("?");
        let name = item.get("name").and_then(|v| v.as_str())
            .or_else(|| item.get("serviceName").and_then(|v| v.as_str()))
            .unwrap_or("");
        let credit = item.get("creditScore").and_then(|v| v.as_u64()).unwrap_or(0);
        let completed = item.get("completedTaskCount").and_then(|v| v.as_u64()).unwrap_or(0);
        let name_part = if name.is_empty() { String::new() } else { format!(" | name: {name}") };
        asp_lines.push_str(&format!(
            "{}. agentId: {aid}{name_part} | credit: {credit} | completed: {completed}\n",
            i + 1
        ));
    }

    let cmd = super::super::flow::pending_cmd(
        job_id, agent_id, None,
        &format!("[Pending ASP {short_id}] {title} ASP-contact decision"),
        "provider_pending",
    );
    let l10n_prompt = super::super::flow::L10N_PROMPT;
    let follow_playbook = super::super::flow::FOLLOW_PLAYBOOK;

    format!(
        "[Trigger] ASP pending contact — {} ASP(s) found (pre-fetched)\n\
         [Role] User (Buyer)\n\n\
         🛑 Push the ASP decision card via `pending-decisions-v2 request`, then end turn.\n\n\
         Pre-fetched ASP list:\n{asp_lines}\n\
         ```bash\n\
         {cmd}\n\
         ```\n\
         {l10n_prompt}\n\
         `--user-content` template (canonical English — ASP details already embedded, just translate):\n\
         [Job {short_id}] The following ASPs have reached out. Pick one to designate as the provider:\n\
         {asp_lines}\
         Reply with the ASP's number to designate, or reply 「skip all」.\n\n\
         {follow_playbook}\n",
        items.len(),
    )
}
