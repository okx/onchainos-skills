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
         okx-a2a user notify --content '<your translated content>' --json\n\
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
         okx-a2a user notify --content '<your translated content>' --json\n\
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
         tool: `xmtp_dispatch_user`\n\
         content (canonical English template — translate before passing): {notify_tpl}\n\
         Fill: `<title>` = {title} | `<short_jobId>` = {short_id}\n\
         {l10n_short}\n\n\
         **Action 2 — End the turn.**\n\
         Do NOT call `asp-match` — public tasks wait for ASPs to apply.\n\n\
         🛑 Forbidden: `asp-match`, `xmtp_start_conversation`, `set-payment-mode`, \
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
         **Correct behavior**: you yourself execute the CLI commands and xmtp tool calls step by step as below.\n\n\
         [Current state] job_created (job is on-chain, status: pending acceptance)\n\
         [Role] User (User Agent)\n\n\
         ⚠️ **Open != public**: Open is a job lifecycle state (pending acceptance), not a visibility (public/private). Job visibility is governed by the `visibility` field (0=public, 1=private), unrelated to the Open state. Do NOT translate Open as \"public\" in notifications.\n\n\
         🛑 **CLIs forbidden in this event**: set-payment-mode / confirm-accept / apply / complete / reject - no ASP has been picked yet, negotiation has not started, all of these are illegal here.\n\n\
         🛑🛑🛑 You MUST execute ALL steps below immediately in this turn. Do NOT end the turn before completing Step 0 (notify user) and D-Step 1 (designated-route query).\n\
         Ending the turn without executing = user never gets notified = task stalls permanently.\n\
         🔴 Real incident: a model called next-action, received this playbook, then said \"end turn, wait for User Agent\" without executing any step — the user was never notified and the task was permanently stuck.\n\n\
         [Your next actions (strict order)]\n\n\
         **Step 0 - notify the user session + continue execution in the current sub/backup session:**\n\
         Call xmtp_dispatch_user to tell the user the job is on-chain:\n\
         \x20\x20content: {notify_tpl}\n\
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
    let follow_playbook_short = super::super::flow::FOLLOW_PLAYBOOK_SHORT;
    let route_hint = super::super::flow::ROUTE_VIA_ENVELOPE;
    let job_id = ctx.job_id;
    let agent_id = ctx.agent_id;
    let short_id = ctx.short_id;
    let title = ctx.title_display;
    let cmd_pending_asp = super::super::flow::pending_cmd(job_id, agent_id, None, &format!("[Pending ASP {short_id}] {title} ASP-contact decision"), "provider_pending");
    let cmd_no_asp = super::super::flow::pending_cmd(job_id, agent_id, None, &format!("[No ASP {short_id}] {title} next-step decision"), "no_asp_found");

    let no_sellers = super::super::content::no_more_sellers_user_notify(job_id);
    let pending_empty = super::super::content::pending_list_empty_user_notify();
    let skip_all = super::super::content::skip_all_pending_user_notify(job_id);
    format!(
    "[Trigger] Received an \"ASP pending contact\" style message (user session side)\n\
     [Role] User (User Agent)\n\n\
     🛑 **Do NOT auto-create groups**: after receiving the pending_list notification, you must NOT call xmtp_start_conversation on your own.\n\
     You must first show the list and let the user pick an ASP; only after an explicit user choice may you create the group.\n\n\
     🛑 **CRITICAL - this event MUST push the ASP list to the user session via `pending-decisions-v2 request`; printing text reply in the sub session is forbidden.**\n\
     ❌ Do NOT replace the `pending-decisions-v2 request` call with a text reply (sub-session output is invisible to the user).\n\
     ❌ Do NOT use xmtp_dispatch_user instead of `pending-decisions-v2 request` (the user needs to make an ASP-choice decision; dispatch_user is pure notification and cannot relay).\n\n\
     [Your next actions (strict order)]\n\n\
     **Step 0 - idempotency check: query whether a pending decision already exists for this job:**\n\
     ```bash\n\
     onchainos agent pending-decisions-v2 list --format json\n\
     ```\n\
     If the returned `entries` array already contains an entry with job_id={job_id} and role=buyer -> **the user has already been notified; this is a duplicate event - end the turn without notifying again.**\n\
     If not present -> continue to Step 1.\n\n\
     **Step 1 - fetch the pending-contact ASP list:**\n\
     Run `okx-a2a task requests --json`. The returned `items` array contains per-ASP entries — capture `groupId` / `agentId` / `name` / `serviceName` / `creditScore` / `completedTaskCount` for each entry (`groupId` is required later for Branch C reject; the others are for rendering).\n\n\
     If the returned `items` array is empty -> call xmtp_dispatch_user:\n\
     \x20\x20content: {pending_empty}\n\
     {l10n_short}\n\
     Then finish.\n\n\
     **Step 2 - enqueue the user decision via `pending-decisions-v2 request`:**\n\
     🛑 **You MUST wait for the user's choice**; you may not decide for them.\n\
     Call `session_status` first to get this sub session's sessionKey (only once per turn). Then run:\n\
     ```bash\n\
     {cmd_pending_asp}\n\
     ```\n\
     {l10n_prompt}\n\
     `--user-content` template (canonical English):\n\
     [Job {short_id} — you are the User Agent] The following ASPs have reached out. Pick one to start negotiating:\n\
     \n\
     [iterate pending list; format per ASP (use fields from the `okx-a2a task requests` response):]\n\
     <N>. agentId: <agentId> | name: <name or serviceName, omit if absent> | credit: <creditScore> | completed jobs: <completedTaskCount>\n\
     \n\
     Reply with the ASP's number to start, or reply 「skip all」.\n\n\
     {follow_playbook}\n\n\
     **Step 3 - End this turn. When the user-session relays the reply as a system envelope (`event:\"user_decision_provider_pending\"`, `message.data:<user verbatim>`), branch by intent below.** (You may also follow the routing playbook returned by `next-action` with `event=user_decision_provider_pending` and `data=<message.data>` in --message — both paths point to the same Branch A/B/C below.)\n\n\
     ━━━━━━━━━ Branch A: verbatim is a number (index) or a 3-digit AgentID → map index to AgentID from the pending list above; establish session, then negotiate ━━━━━━━━━\n\n\
     A-Step 1: map the user's reply to agentId (index → AgentID via the pending list, or use a 3-digit AgentID directly); call xmtp_start_conversation to create the group + the sub session:\n\
     \x20\x20Args: myAgentId={agent_id}, toAgentId=<agentId from the pending list above>, jobId={job_id}\n\
     \x20\x20⚠️ Before the call, print: `[buyer-xmtp] xmtp_start_conversation: myAgentId={agent_id}, toAgentId=<agentId>, jobId={job_id}`\n\
     \x20\x20⚠️ After the call, print: `[buyer-xmtp] xmtp_start_conversation result: sessionKey=<returned value>, xmtpGroupId=<returned value>`\n\n\
     🛑 **A-Step 1.5 - SKILL_PREFETCH (mandatory for new sub sessions):**\n\
     Immediately after xmtp_start_conversation returns, call `xmtp_dispatch_session` to pre-load the skill into the newly created sub session:\n\
     \x20\x20sessionKey = <the sessionKey just returned by xmtp_start_conversation>\n\
     \x20\x20content = `[SKILL_PREFETCH] Read okx-agent-task/SKILL.md. No action needed for this message — but process all subsequent messages normally. Do NOT carry over \"no action\" to business messages.`\n\
     ❌ Do NOT skip this step — the sub session has no context yet; without SKILL_PREFETCH, the first inbound message will be processed without the buyer playbook loaded.\n\
     ⚠️ Use `xmtp_dispatch_session` (internal), NOT `xmtp_send` (which the ASP would see).\n\n\
     🛑 **Within the same turn after creating the group you MUST call `xmtp_send` to send the first message** - creating the group only opens the channel; not sending a message = the ASP receives no signal = the flow stalls.\n\
     ❌ Absolutely forbidden: creating the group and ending the turn without sending a message.\n\n\
     A-Step 2: once the group is created you are inside the sub session; call xmtp_send to start negotiating with the ASP (refer to buyer-sub-playbook.md §Peer Message Routing):\n\
     \x20\x20⚠️ **Do NOT** use xmtp_dispatch_user / xmtp_dispatch_session; after the group is created use xmtp_send uniformly.\n\
     \x20\x20content: Hi, I have a job (jobId: {job_id}) - are you interested in taking it on?\n\n\
     A-Step 3: negotiation success -> ASP applies on-chain -> wait for the ASP's XMTP message announcing the apply (buyer-sub-playbook.md routing #1 triggers confirm-accept).\n\n\
     A-Step 4: negotiation failure (ASP rejects / timeout / terms mismatch) -> jump to Branch C.\n\n\
     ━━━━━━━━━ Branch B: verbatim contains `skip all` / `跳过` / `不选` → skip all pending ASPs ━━━━━━━━━\n\n\
     End the flow — call xmtp_dispatch_user:\n\
     \x20\x20content: {skip_all}\n\
     {l10n_short}\n\n\
     ━━━━━━━━━ Branch C: user rejects current ASP / negotiation failed -> reject and return to the list ━━━━━━━━━\n\n\
     C-Step 1: reject this ASP via:\n\
     \x20\x20`okx-a2a task reject --group-id <groupId from Step 1> --agent-id <rejected ASP's agentId> --json`\n\
     \x20\x20⚠️ Keyed by **groupId** (XMTP group), NOT jobId — use the `groupId` field captured from Step 1's `items` response.\n\n\
     C-Step 2: run `okx-a2a task requests --json` again to refresh the pending list.\n\n\
     C-Step 3: if the list is non-empty -> go back to Step 2 and show the remaining ASPs to the user.\n\n\
     C-Step 4: if the list is empty -> enqueue the user decision via `pending-decisions-v2 request`:\n\
     \x20\x20```bash\n\
     \x20\x20{cmd_no_asp}\n\
     \x20\x20```\n\
     \x20\x20{l10n_prompt}\n\
     \x20\x20`--user-content` template (canonical English):\n\
     \x20\x20{no_sellers}\n\
     \x20\x20A. Specify an ASP — provide the ASP's agentId\n\
     \x20\x20B. Make the job public — let more ASPs discover it\n\
     \x20\x20C. Close the job — cancel and refund\n\
     \x20\x20{follow_playbook_short}\n\
     \x20\x20{route_hint}\n\n\
     [Loop termination conditions] `okx-a2a task requests` returns an empty `items` array, OR negotiation succeeds and enters Scene 6.\n")

}
