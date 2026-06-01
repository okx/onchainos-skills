//! Event handlers for job_created, switch_provider, and provider_conversation.

use super::super::flow::FlowContext;

// --- Event handler functions ------------------------------------------------

pub(crate) fn job_created(ctx: &FlowContext<'_>) -> String {
    let l10n_prompt = super::super::flow::L10N_PROMPT;
    let follow_playbook = super::super::flow::FOLLOW_PLAYBOOK;
    let job_id = ctx.job_id;
    let agent_id = ctx.agent_id;
    let short_id = ctx.short_id;
    let title = ctx.title_display;
    let cmd_recommend = super::super::flow::pending_cmd(job_id, agent_id, &format!("[Recommend {short_id}] {title} ASP-pick decision"), "recommend_pick");
    // Note: the "next-page returns empty -> push no_asp_found A/B/C" sub-branch
    // is delegated to the `user_decision_recommend_pick` handler in flow.rs,
    // which embeds the no_asp_found enqueue command + user-content template.

    let designated_provider = super::super::negotiate::get_designated_provider(job_id).ok().flatten();

    let notify_text = match &designated_provider {
        Some(dp_id) => format!("Connecting to the designated ASP {dp_id}..."),
        None => "Auto-querying recommended ASPs...".to_string(),
    };

    let created_notify = super::super::content::job_created_user_notify(job_id, ctx.title_display, &notify_text);

    let attachment_paths = super::super::attachments::list_attachment_paths(job_id);
    let attachment_section_created = if attachment_paths.is_empty() {
        String::new()
    } else {
        let paths_list = attachment_paths.iter()
            .map(|p| format!("  - `{p}`"))
            .collect::<Vec<_>>()
            .join("\n");
        format!(
            "**Step 0.5 — 🛑 Pending local attachments (auto-detected, MUST upload after first xmtp_send):**\n\
             The following files are saved locally and MUST be uploaded to the provider **immediately after the first `xmtp_send`** in B-Step 2 step 1.5:\n\
             {paths_list}\n\
             ⚠️ Do NOT call `list-attachments` again — the paths above are authoritative.\n\
             ⚠️ For each file: `xmtp_file_upload` → `xmtp_send [intent:attachment]` (see step 1.5 template).\n\n"
        )
    };

    let routing_section = if let Some(dp_id) = &designated_provider {
        super::designated::designated_provider_d_steps(job_id, agent_id, short_id, dp_id, ctx.title_display)
    } else {
        format!("\
             **Step 0 - idempotency check: query whether a pending decision already exists for this job:**\n\
             ```bash\n\
             onchainos agent pending-decisions-v2 list --format json\n\
             ```\n\
             If the returned `entries` array already contains an entry with job_id={job_id} and role=buyer -> **the user has already been notified; this is a duplicate event - end the turn without notifying again.**\n\
             If not present -> continue to Step 1.\n\n\
             🛑 **Do NOT ask the user whether to fetch the recommendation list** -- proceed to Step 1 directly and automatically. The recommend query is mandatory, not optional.\n\n\
             **Step 1 - query the recommended ASP list:**\n\
             ```bash\n\
             onchainos agent recommend {job_id} --agent-id {agent_id}\n\
             ```\n\
             Outputs the ASP list (Agent Name / service description / credit / payment modes); ASPs that previously failed negotiation are auto-filtered.\n\n\
             🛑🛑🛑 **ABSOLUTE PROHIBITION - iron rule: in the current session (sub/backup) you must NOT directly print the recommendation list or any text reply.**\n\
             You are inside a sub session or backup session - **the user cannot see any output here**.\n\
             You must push the list to the user session via `pending-decisions-v2 request`; that is the **only** way the user sees the list.\n\
             🔴 Real incident: the Minimax model, after getting recommend results in a backup session, just printed the list as text; the user never saw it and the task stalled.\n\
             ❌ Absolutely forbidden: replacing the `pending-decisions-v2 request` call with a text reply - text reply = invisible to user = task stalls.\n\
             ❌ Absolutely forbidden: using xmtp_dispatch_user instead of `pending-decisions-v2 request` - dispatch_user is pure notification, the user's choice cannot be relayed back.\n\
             ❌ Absolutely forbidden: printing text \"for the user to see\" first and then calling the tool - text output in a sub session never reaches the user.\n\n\
             **Step 2 - show the list to the user and let them choose:**\n\
             🛑🛑🛑 **DO NOT call `xmtp_start_conversation` in this step** — there is no peer agent to talk to yet (the user hasn't picked an ASP). `xmtp_start_conversation` only happens AFTER the user picks (handled by the `next-action --provider <X>` playbook in a later turn). 🔴 Real incident: a model in backup, instead of calling `session_status` to fetch its own backup-key, called `xmtp_start_conversation` to create a brand-new (peer-less) conversation, which produced an unusable sessionKey and broke the relay chain.\n\
             **Action**: call `session_status` (NOT `xmtp_start_conversation`) to get the **current sub/backup session's** sessionKey (call once per turn, reuse the result). The returned string is what you must pass verbatim to `--sub-key` below. For backup-session callers, the key looks like `agent:main:okx-a2a:group:okx-xmtp:backup:<jobId>`; for task-sub callers, it contains `&job=<jobId>&gid=<...>`.\n\
             Then run:\n\
             ```bash\n\
             {cmd_recommend}\n\
             ```\n\
             `--user-content` template (canonical English; 🌐 localize per [Localization] rules):\n\
             [Job {short_id} — you are the User Agent] Recommended ASPs:\n\
             <For each ASP from the recommend output, render one card block using the format below. Preserve ALL fields — do NOT omit any.>\n\n\
             Card format per ASP (repeat for each — field mapping: AgentID=`providerAgentId`, serviceName/serviceDescription/feeAmount/feeTokenSymbol/serviceType from `services[0]`):\n\
             ━━━ <index>. #<providerAgentId> | <serviceName> ━━━\n\
             Description: <serviceDescription, no truncation>\n\
             Fee: <feeAmount> <feeTokenSymbol>\n\
             Payment: <map serviceType: A2A→Escrow, A2MCP→x402>\n\
             <If the ASP has multiple services, append each additional service as a sub-block:>\n\
             \x20\x20┊ <service name> — <description>\n\
             \x20\x20┊ Fee: <fee> | Payment: <map serviceType: A2A→Escrow, A2MCP→x402>\n\
             <blank line between cards>\n\n\
             After the last card:\n\
             ---\n\
             Please choose: reply with an index (e.g. 1, 2, 3) or an AgentID (e.g. 864) to pick an ASP; or reply \"next\" (下一页) to see more / \"public\" (公开) / \"close\" (关闭).\n\n\
             {l10n_prompt}\n\
             {follow_playbook}\n\n\
             -> **end this turn** and wait for the user's reply to be relayed back.\n\n\
             **Step 3 — End this turn. The user-session will relay the user's reply as a system envelope.**\n\n\
             When the system envelope arrives (`event:\"user_decision_recommend_pick\"`, `message.data:<user verbatim>`, e.g. `1` / `864` / `next page` / `公开` / `关闭`), call:\n\
             ```bash\n\
             onchainos agent next-action --jobid {job_id} --event user_decision_recommend_pick --role buyer --agentId {agent_id} --data \"<message.data>\"\n\
             ```\n\
             CLI's routing playbook does the LLM semantic mapping (pick ASP → re-enter via `next-action --provider X` / next page → `recommend --next-page` (auto re-push if results / fall back to `--source-event no_asp_found` if empty) / public → `set-public` / close → `close`). Follow it verbatim.\n\n\
             ===============================================================\n\
             🔴🔴🔴 ABSOLUTE PROHIBITION — before the `user_decision_recommend_pick` next-action returns, you are FORBIDDEN from:\n\
             ❌ Creating groups or conversations\n\
             ❌ Sending ANY message to ANY agent\n\
             ❌ Calling ANY onchainos CLI command other than the next-action above\n\
             ❌ Deciding routing (x402 / A2A / escrow) yourself\n\
             ❌ Composing negotiation content of any kind\n\
             ❌ Keyword-matching the verbatim yourself (CLI's user_decision_recommend_pick handler does the semantic mapping; your job is only to pass `--data \"<message.data>\"` through)\n\
             🔴 Real incident: a model skipped next-action and sent [intent:propose] directly — this broke routing, skipped service-list check, and sent an invalid first message. The ONLY correct path is next-action first.\n\
             ===============================================================\n")
    };

    let mut output = format!(
        "🛑🛑🛑 **IDENTITY CHECK - you are the executor; delegation is forbidden**\n\
         You are inside a sub session or backup session. **You yourself** are the agent responsible for executing this script.\n\
         ❌ **Absolutely forbidden**: `sessions_spawn` - do NOT spawn a child agent to \"help you\" handle this event.\n\
         ❌ **Absolutely forbidden**: `sessions_yield` - do NOT hand off control.\n\
         🔴 Real incident: after receiving job_created, a backup called sessions_spawn to delegate to a child agent, which broke the designated-provider consume-context invariant and made negotiation uncontrollable.\n\
         **Correct behavior**: you yourself execute the CLI commands and xmtp tool calls step by step as below.\n\n\
         [Current state] job_created (job is on-chain, status: pending acceptance)\n\
         [Role] User (User Agent)\n\n\
         ⚠️ **Open != public**: Open is a job lifecycle state (pending acceptance), not a visibility (public/private). Job visibility is governed by the `visibility` field (0=public, 1=private), unrelated to the Open state. Do NOT translate Open as \"public\" in notifications.\n\n\
         🛑 **CLIs forbidden in this event**: save-agreed / set-payment-mode / confirm-accept / apply / complete / reject - no ASP has been picked yet, negotiation has not started, all of these are illegal here.\n\n\
         🛑🛑🛑 You MUST execute ALL steps below immediately in this turn. Do NOT end the turn before completing Step 0 (notify user) and Step 1 (recommend query).\n\
         Ending the turn without executing = user never gets notified = task stalls permanently.\n\
         🔴 Real incident: a model called next-action, received this playbook, then said \"end turn, wait for User Agent\" without executing any step — the user was never notified and the task was permanently stuck.\n\n\
         [Your next actions (strict order)]\n\n\
         **Step 0 - notify the user session + continue execution in the current sub/backup session:**\n\
         Call xmtp_dispatch_user to tell the user the job is on-chain:\n\
         \x20\x20content: {created_notify}\n\
         🌐 **Canonical template — use verbatim after filling `<...>` placeholders; do NOT add extra information (price, budget, ASP capabilities, etc.) not present in the template. Localize per [Localization] rules before sending (rule 4: English → verbatim; rule 5: non-English → faithful translation).**\n\n\
         ⚠️ Subsequent routing -> negotiation / acceptance all run in the **current session**; do NOT switch to the user session, do NOT sessions_spawn.\n\n\
         {attachment_section_created}\
         {routing_section}\n\n"
    );

    if let Some(ref dp_id) = designated_provider {
        output.push_str("\n━━━━━━━━━ The B-Steps below run ONLY when D-Step concludes \"no service or no endpoint\" ━━━━━━━━━\n\
                         🛑 If D-Step already routed to x402 (service-list has an endpoint), then the B-Steps below are **entirely skipped, absolutely forbidden to execute**.\n\
                         Full x402 path: DX-Step 1->2->3 -> A-Step 3 (set-payment-mode) -> wait for job_payment_mode_changed -> task-402-pay.\n\
                         The x402 path **never involves** xmtp_start_conversation / group creation / three-step handshake / xmtp_send negotiation messages.\n\n");
        output.push_str(&super::designated::designated_provider_negotiate(job_id, agent_id, short_id, dp_id, ctx.title_display));
    }

    output
}

pub(crate) fn switch_provider(ctx: &FlowContext<'_>) -> String {
    let job_id = ctx.job_id;
    let agent_id = ctx.agent_id;
    let short_id = ctx.short_id;

    let designated_provider = super::super::negotiate::get_designated_provider(job_id).ok().flatten();
    let dp_id = match &designated_provider {
        Some(id) => id.clone(),
        None => {
            return format!("[Error] switch_provider is missing the --provider argument.\n\
                 Please call again: onchainos agent next-action --jobid {job_id} --event switch_provider --role buyer --agentId {agent_id} --provider <new ASP agentId>\n");
        }
    };

    let attachment_paths = super::super::attachments::list_attachment_paths(job_id);
    let attachment_section = if attachment_paths.is_empty() {
        String::new()
    } else {
        let paths_list = attachment_paths.iter()
            .map(|p| format!("  - `{p}`"))
            .collect::<Vec<_>>()
            .join("\n");
        format!(
            "**Pre-step — 🛑 Pending local attachments (auto-detected, MUST upload after first xmtp_send):**\n\
             The following files are saved locally and MUST be uploaded to the new provider **immediately after the first `xmtp_send`** in B-Step 2 step 1.5:\n\
             {paths_list}\n\
             ⚠️ Do NOT call `list-attachments` again — the paths above are authoritative.\n\
             ⚠️ For each file: `xmtp_file_upload` → `xmtp_send [intent:attachment]` (see step 1.5 template).\n\n"
        )
    };

    let d_steps = super::designated::designated_provider_d_steps(job_id, agent_id, short_id, &dp_id, ctx.title_display);
    let negotiate = super::designated::designated_provider_negotiate(job_id, agent_id, short_id, &dp_id, ctx.title_display);
    format!("\
         [Provider switch] set-provider has been submitted; start the new ASP flow immediately (do NOT wait for the task_provider_change on-chain confirmation).\n\
         [Role] User (User Agent) | [Execution environment] user session\n\n\
         🛑 **CLIs forbidden in this event**: save-agreed / set-payment-mode / confirm-accept / apply / complete / reject - negotiation with the new ASP has not started, all of these are illegal here.\n\n\
         ⚠️ The old ASP's sub session will automatically send [intent:reject] when it receives the `task_provider_change` on-chain event; no intervention from you required.\n\n\
         {attachment_section}\
         [Your next actions (strict order)]\n\n\
         {d_steps}\n\n\
         ━━━━━━━━━ The B-Steps below run ONLY when D-Step concludes \"no service or no endpoint\" ━━━━━━━━━\n\
         🛑 If D-Step already routed to x402 (service-list has an endpoint), then the B-Steps below are **entirely skipped, absolutely forbidden to execute**.\n\
         Full x402 path: DX-Step 1->2->3 -> A-Step 3 (set-payment-mode) -> wait for job_payment_mode_changed -> task-402-pay.\n\
         The x402 path **never involves** xmtp_start_conversation / group creation / three-step handshake / xmtp_send negotiation messages.\n\n\
         {negotiate}\n")
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
    let cmd_pending_asp = super::super::flow::pending_cmd(job_id, agent_id, &format!("[Pending ASP {short_id}] {title} ASP-contact decision"), "provider_pending");
    let cmd_no_asp = super::super::flow::pending_cmd(job_id, agent_id, &format!("[No ASP {short_id}] {title} next-step decision"), "no_asp_found");

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
     Call the xmtp_get_pending_list tool to fetch the pending-contact ASP list.\n\
     ⚠️ Before the call, print: `[buyer-xmtp] xmtp_get_pending_list`\n\
     ⚠️ After the call, print: `[buyer-xmtp] xmtp_get_pending_list result: <returned value>`\n\n\
     If the result is an empty list -> call xmtp_dispatch_user:\n\
     \x20\x20content: {pending_empty}\n\
     {l10n_short}\n\
     Then finish.\n\n\
     **Step 2 - enqueue the user decision via `pending-decisions-v2 request`:**\n\
     🛑 **You MUST wait for the user's choice**; you may not decide for them.\n\
     Call `session_status` first to get this sub session's sessionKey (only once per turn). Then run:\n\
     ```bash\n\
     {cmd_pending_asp}\n\
     ```\n\
     `--user-content` template (canonical English; 🌐 localize per [Localization] rules):\n\
     [Job {short_id} — you are the User Agent] The following ASPs have reached out. Pick one to start negotiating:\n\
     \n\
     [iterate pending list; format per ASP (use fields from xmtp_get_pending_list response):]\n\
     <N>. agentId: <agentId> | name: <name or serviceName, omit if absent> | credit: <creditScore> | completed jobs: <completedTaskCount>\n\
     \n\
     Reply with the ASP's number to start, or reply \"skip all\".\n\n\
     {l10n_prompt}\n\
     {follow_playbook}\n\n\
     **Step 3 - End this turn. When the user-session relays the reply as a system envelope (`event:\"user_decision_provider_pending\"`, `message.data:<user verbatim>`), branch by intent below.** (You may also follow the routing playbook returned by `next-action --event user_decision_provider_pending --data \"<message.data>\"` — both paths point to the same Branch A/B/C below.)\n\n\
     ━━━━━━━━━ Branch A: verbatim is a number (index) or a 3-digit AgentID → map index to AgentID from the pending list above; establish session, then negotiate ━━━━━━━━━\n\n\
     A-Step 1: map the user's reply to agentId (index → AgentID via the pending list, or use a 3-digit AgentID directly); call xmtp_start_conversation to create the group + the sub session:\n\
     \x20\x20Args: myAgentId={agent_id}, toAgentId=<agentId from the pending list above>, jobId={job_id}\n\
     \x20\x20⚠️ Before the call, print: `[buyer-xmtp] xmtp_start_conversation: myAgentId={agent_id}, toAgentId=<agentId>, jobId={job_id}`\n\
     \x20\x20⚠️ After the call, print: `[buyer-xmtp] xmtp_start_conversation result: sessionKey=<returned value>, xmtpGroupId=<returned value>`\n\n\
     🛑 **A-Step 1.5 - SKILL_PREFETCH (mandatory for new sub sessions):**\n\
     Immediately after xmtp_start_conversation returns, call `xmtp_dispatch_session` to pre-load the skill into the newly created sub session:\n\
     \x20\x20sessionKey = <the sessionKey just returned by xmtp_start_conversation>\n\
     \x20\x20content = `[SKILL_PREFETCH] Read the okx-agent-task skill. Pre-load buyer role context and wait for the next inbound message. Do NOT execute any business action or call any CLI command.`\n\
     ❌ Do NOT skip this step — the sub session has no context yet; without SKILL_PREFETCH, the first inbound message will be processed without the buyer playbook loaded.\n\
     ⚠️ Use `xmtp_dispatch_session` (internal), NOT `xmtp_send` (which the ASP would see).\n\n\
     🛑 **Within the same turn after creating the group you MUST call `xmtp_send` to send the first message** - creating the group only opens the channel; not sending a message = the ASP receives no signal = the flow stalls.\n\
     ❌ Absolutely forbidden: creating the group and ending the turn without sending a message.\n\n\
     A-Step 2: once the group is created you are inside the sub session; call xmtp_send to start negotiating with the ASP (refer to buyer.md 3.2 negotiation three-step handshake):\n\
     \x20\x20⚠️ **Do NOT** use xmtp_dispatch_user / xmtp_dispatch_session; after the group is created use xmtp_send uniformly.\n\
     \x20\x20content: Hi, I have a job (jobId: {job_id}) - are you interested in taking it on?\n\n\
     A-Step 3: negotiation success -> ASP applies on-chain -> wait for the ASP's XMTP message announcing the apply (buyer.md routing #2 triggers confirm-accept).\n\n\
     A-Step 4: negotiation failure (ASP rejects / timeout / terms mismatch) -> jump to Branch C.\n\n\
     ━━━━━━━━━ Branch B: verbatim contains `skip all` / `跳过` / `不选` → skip all pending ASPs ━━━━━━━━━\n\n\
     End the flow — call xmtp_dispatch_user:\n\
     \x20\x20content: {skip_all}\n\
     {l10n_short}\n\n\
     ━━━━━━━━━ Branch C: user rejects current ASP / negotiation failed -> reject and return to the list ━━━━━━━━━\n\n\
     C-Step 1: call xmtp_deny_pending_conversation to reject this ASP:\n\
     \x20\x20Args: agentId=<rejected ASP's agentId>, jobId={job_id}\n\
     \x20\x20⚠️ Before the call, print: `[buyer-xmtp] xmtp_deny_pending_conversation: agentId=<agentId>, jobId={job_id}`\n\n\
     C-Step 2: call xmtp_get_pending_list again to refresh the pending list.\n\n\
     C-Step 3: if the list is non-empty -> go back to Step 2 and show the remaining ASPs to the user.\n\n\
     C-Step 4: if the list is empty -> enqueue the user decision via `pending-decisions-v2 request`:\n\
     \x20\x20```bash\n\
     \x20\x20{cmd_no_asp}\n\
     \x20\x20```\n\
     \x20\x20`--user-content` template (canonical English; 🌐 localize per [Localization] rules):\n\
     \x20\x20{no_sellers}\n\
     \x20\x20A. Specify an ASP — provide the ASP's agentId\n\
     \x20\x20B. Make the job public — let more ASPs discover it\n\
     \x20\x20C. Close the job — cancel and refund\n\
     \x20\x20{l10n_prompt}\n\
     \x20\x20{follow_playbook_short}\n\
     \x20\x20{route_hint}\n\n\
     [Loop termination conditions] xmtp_get_pending_list returns an empty list, OR negotiation succeeds and enters Scene 6.\n")

}
