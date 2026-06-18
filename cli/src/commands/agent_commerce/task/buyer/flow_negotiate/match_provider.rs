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
     Run `okx-a2a task requests --json`. The returned `payload` array contains per-ASP entries — capture `toAgentId` (the ASP's agent ID) / `groupId` / `name` for each entry. If `name` is absent at the top level, extract it from `messages[0].content` (JSON) → `sender.name`.\n\n\
     If the returned `payload` array is empty -> run `okx-a2a user notify`:\n\
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
     <N>. agentId: <toAgentId> | name: <name or sender.name, omit if absent>\n\
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
        "a2a" => provider_conversation_pick_a2a(job_id, agent_id, short_id, dp_id),
        "x402" => super::designated::branch_x402(job_id, agent_id, short_id, dp_id),
        "error" => super::designated::branch_error(job_id, agent_id, short_id, dp_id),
        _ => format!(
            "[provider_conversation_pick] ERROR: unknown route value '{route}' in designated-route response: {route_json}\n"
        ),
    }
}

/// A2A branch for public-task ASP pick. Creates session + SKILL_PREFETCH,
/// then returns LLM instructions to run asp-match + set-asp so the server
/// knows who the provider is and can send `job_asp_selected`.
fn provider_conversation_pick_a2a(job_id: &str, agent_id: &str, short_id: &str, dp_id: &str) -> String {
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
         ✅ Sub session and SKILL_PREFETCH sent.\n\n\
         **You MUST now set the provider on this task via `set-asp` so the server can notify the ASP.**\n\n\
         **Step 1 — fetch the ASP's service info:**\n\
         ```bash\n\
         onchainos agent asp-match --job-id {job_id} --provider-agent-id {dp_id}\n\
         ```\n\
         From the result, extract the ASP's **top service**: `serviceId`, `serviceName`, `serviceDescription`, \
         `feeAmount` (→ serviceTokenAmount), `feeToken` (→ serviceTokenAddress), `feeTokenSymbol`.\n\
         If `asp-match` returns no services, notify the user (🌐 localized): \
         \"Provider {dp_id} has no registered services.\" and end the turn.\n\n\
         **Step 2 — collect serviceParams if needed:**\n\
         If `serviceDescription` is non-empty, ask the user for serviceParams — enqueue:\n\
         ```bash\n\
         onchainos agent pending-decisions-v2 request --job-id {job_id} --role buyer --agent-id {agent_id} \
         --source-event set_asp_params \
         --user-content \"<compose from template below>\" \
         --list-label \"[SetASP {short_id}] provide service params\"\n\
         ```\n\
         `--user-content` template (canonical English; 🌐 localize per user's language):\n\
         You selected Agent {dp_id} — <serviceName>.\n\
         Service: <serviceDescription>\n\
         Fee: <feeAmount> <feeTokenSymbol>\n\n\
         Please describe the input for this service (serviceParams):\n\
         [SERVICE_CONTEXT providerAgentId={dp_id} serviceId=<sid> serviceTokenAddress=<feeToken> serviceTokenAmount=<feeAmount>]\n\
         **`--list-label` must be localized to the user's language.**\n\
         Then **end this turn** and wait for the user's reply.\n\n\
         If `serviceDescription` is empty, skip the decision and go to Step 3 directly (serviceParams = `''`).\n\n\
         **Step 3 — call `set-asp`:**\n\
         ```bash\n\
         onchainos agent set-asp {job_id} --provider-agent-id {dp_id} --service-id <sid> --service-params '<params or empty>' \
         --service-token-address <feeToken> --service-token-amount <feeAmount>\n\
         ```\n\
         On success → notify user (🌐 localized): \"Provider set to Agent {dp_id}. Waiting for provider to accept.\"\n\n\
         🛑 **End this turn after `set-asp` succeeds.** Wait for the `provider_applied` system event.\n\
         ❌ Do NOT call `confirm-accept` / `set-payment-mode` — the ASP has not applied yet.\n\n\
         [What happens next]\n\
         `set-asp` → server sends `job_asp_selected` to ASP → ASP on-chain apply → system fires `provider_applied` event.\n"
    )
}

/// CLI-mode handler for `provider_conversation`. Fetches ASP list in-process,
/// takes the first ASP, and pushes an accept/reject decision card to the user.
/// Reject triggers `provider_conversation_reject` which auto-advances to the
/// next ASP or pushes close options if none remain.
pub(crate) fn provider_conversation_cli(ctx: &FlowContext<'_>) -> String {
    provider_conversation_cli_inner(ctx, None)
}

/// Shared implementation for both initial `provider_conversation` and
/// `provider_conversation_reject` (which passes pre-fetched items to skip
/// the duplicate reject + re-fetch).
pub(crate) fn provider_conversation_cli_inner(
    ctx: &FlowContext<'_>,
    prefetched_items: Option<Vec<serde_json::Value>>,
) -> String {
    use crate::commands::agent_commerce::task::common::{okx_a2a, pending_v2};

    let job_id = ctx.job_id;
    let agent_id = ctx.agent_id;
    let short_id = ctx.short_id;
    let title = ctx.title_display;
    let l10n_dispatch = super::super::flow::L10N_DISPATCH_SHORT;

    let is_after_reject = prefetched_items.is_some();

    if !is_after_reject {
        if pending_v2::has_pending_for_job(job_id, "buyer") {
            return format!(
                "[provider_conversation] Duplicate event — pending decision already exists for job {short_id}. End turn.\n"
            );
        }
    }

    let items: Vec<serde_json::Value> = match prefetched_items {
        Some(v) => v,
        None => match okx_a2a::task_requests() {
            Ok(v) => v.into_iter()
                .filter(|item| {
                    item.get("jobId").and_then(|v| v.as_str()) == Some(job_id)
                        || !item.get("jobId").map_or(false, |v| v.is_string())
                })
                .collect(),
            Err(e) => return format!("[provider_conversation] ERROR: task requests failed: {e}\n"),
        },
    };

    if items.is_empty() {
        if is_after_reject {
            let no_sellers = super::super::content::no_more_sellers_user_notify(job_id);
            let cmd_no_asp = super::super::flow::pending_cmd(
                job_id, agent_id, None,
                &format!("[No ASP {short_id}] {title} next-step decision"),
                "no_asp_found",
            );
            let l10n_prompt = super::super::flow::L10N_PROMPT;
            let follow_playbook = super::super::flow::FOLLOW_PLAYBOOK;
            return format!(
                "[provider_conversation_reject] All pending ASPs rejected; none remaining.\n\n\
                 🛑 Push the next-step decision card via `pending-decisions-v2 request`, then end turn.\n\n\
                 ```bash\n\
                 {cmd_no_asp}\n\
                 ```\n\
                 {l10n_prompt}\n\
                 `--user-content` template (canonical English — translate to user's language):\n\
                 {no_sellers}\n\
                 A. Specify an ASP — provide the ASP's agentId\n\
                 B. Make the job public — let more ASPs discover it\n\
                 C. Close the job — cancel and refund\n\n\
                 {follow_playbook}\n"
            );
        }
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

    let first = &items[0];
    let asp_agent_id = first.get("toAgentId").and_then(|v| v.as_str())
        .or_else(|| first.get("agentId").and_then(|v| v.as_str()))
        .unwrap_or("?");
    let group_id = first.get("groupId").and_then(|v| v.as_str()).unwrap_or("?");
    let sender_name: String = first.get("name").and_then(|v| v.as_str()).map(String::from)
        .or_else(|| first.get("serviceName").and_then(|v| v.as_str()).map(String::from))
        .or_else(|| {
            first.get("messages")?.as_array()?.first()?
                .get("content")?.as_str()
                .and_then(|raw| serde_json::from_str::<serde_json::Value>(raw).ok())
                .and_then(|parsed| parsed.get("sender")?.get("name")?.as_str().map(String::from))
        })
        .unwrap_or_default();
    let name = sender_name.as_str();
    let remaining = items.len() - 1;

    let card_content = super::super::content::provider_pending_single_user_card(
        short_id, title, asp_agent_id, name,
    );

    let cmd = super::super::flow::pending_cmd(
        job_id, agent_id, None,
        &format!("[ASP {short_id}] Accept provider?"),
        "provider_pending",
    );
    let l10n_prompt = super::super::flow::L10N_PROMPT;
    let follow_playbook = super::super::flow::FOLLOW_PLAYBOOK;

    format!(
        "[Trigger] ASP pending contact — showing first of {} ASP(s)\n\
         [Role] User (Buyer)\n\n\
         🛑 Push the accept/reject decision card via `pending-decisions-v2 request`, then end turn.\n\n\
         ASP context (LLM-only; do NOT expose groupId to user):\n\
         \x20\x20agentId: {asp_agent_id} | groupId: {group_id} | name: {name} | remaining after this: {remaining}\n\n\
         ```bash\n\
         {cmd}\n\
         ```\n\
         {l10n_prompt}\n\
         `--user-content` template (canonical English — translate to user's language):\n\
         {card_content}\n\n\
         `--llm-content` block (keep English verbatim — consumed by user-session agent for routing):\n\
         ```\n\
         [USER_DECISION_REQUEST][source: provider_pending][job: {job_id}][role: buyer][agentId: {agent_id}]\n\
         [asp: {asp_agent_id}][groupId: {group_id}][remaining: {remaining}]\n\n\
         Step 1 — Card delivered. **END THE TURN NOW.**\n\
         Step 2 — When the user replies, route by choice:\n\
         \x20\x20• 1 / \"accept\" / \"接受\" / \"yes\" / \"好\"  → run:\n\
         \x20\x20\x20\x20```bash\n\
         \x20\x20\x20\x20onchainos agent next-action --role buyer --agentId {agent_id} --message '{{\"event\":\"provider_conversation_pick\",\"jobId\":\"{job_id}\",\"provider\":\"{asp_agent_id}\"}}'\n\
         \x20\x20\x20\x20```\n\
         \x20\x20\x20\x20Follow the returned playbook verbatim.\n\
         \x20\x20• 2 / \"reject\" / \"拒绝\" / \"no\" / \"不\" / \"换一个\" / \"next\"  → run:\n\
         \x20\x20\x20\x20```bash\n\
         \x20\x20\x20\x20onchainos agent next-action --role buyer --agentId {agent_id} --message '{{\"event\":\"provider_conversation_reject\",\"jobId\":\"{job_id}\",\"groupId\":\"{group_id}\"}}'\n\
         \x20\x20\x20\x20```\n\
         \x20\x20\x20\x20Follow the returned playbook (shows next ASP or close options).\n\
         ```\n\n\
         {follow_playbook}\n",
        items.len(),
    )
}

/// CLI-mode handler for `provider_conversation_reject`. Rejects the current
/// ASP (by groupId), re-fetches the list, and either shows the next ASP's
/// accept/reject card or pushes close options if none remain.
pub(crate) fn provider_conversation_reject_cli(ctx: &FlowContext<'_>, group_id: &str) -> String {
    use crate::commands::agent_commerce::task::common::okx_a2a;

    let job_id = ctx.job_id;

    if let Err(e) = okx_a2a::task_reject(group_id) {
        return format!("[provider_conversation_reject] ERROR: task reject failed: {e}\n");
    }

    let items: Vec<serde_json::Value> = match okx_a2a::task_requests() {
        Ok(v) => v.into_iter()
            .filter(|item| {
                item.get("jobId").and_then(|v| v.as_str()) == Some(job_id)
                    || !item.get("jobId").map_or(false, |v| v.is_string())
            })
            .collect(),
        Err(e) => return format!("[provider_conversation_reject] ERROR: task requests failed: {e}\n"),
    };

    provider_conversation_cli_inner(ctx, Some(items))
}
