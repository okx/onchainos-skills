//! Provider-side task flow driver.
//!
//! Based on the current system notification type received (event), outputs the prompt
//! for the next action to take. The goal: consolidate the Scene steps scattered across
//! provider.md into code so the agent can simply run
//! `exec onchainos agent next-action ...` to fetch the prompt and execute it directly,
//! without having to reason over the entire document.

use crate::commands::agent_commerce::task::common::config::TASK_MIN_VERSION;
use crate::commands::agent_commerce::task::common::util::short_job_id;
use crate::commands::agent_commerce::task::common::state_machine::Status;

/// The next step the ASP should take under a given status (used at the tail of
/// `agent common context` as a menu).
///
/// The first line is always a `next-action` invocation — this is the sub agent's
/// **only first action** in the current status: fetch the script, follow the script.
/// Terminal / exception states include a plain-language status summary.
/// `generate_next_action` lives in the same file and routes by the entry event
/// corresponding to the status.
pub fn available_actions(status: &Status, job_id: &str) -> Vec<String> {
    let next_action = |evt: &str| {
        format!("**Next step (mandatory)** → `onchainos agent next-action --jobid {job_id} --event {evt} --role provider --agentId <agentId>` to fetch the full script for the current status, and **follow the script strictly**.\n  ⚠️ **Do NOT** infer CLI commands directly from the status name (apply / deliver / dispute raise / agree-refund / dispute upload, etc.) — the script typically prefixes steps such as `xmtp_prompt_user` / `xmtp_send` / `pending-decisions-v2 request`; skipping them causes incidents (this has happened before).")
    };
    match status {
        Status::Created => vec![next_action("job_created")],
        Status::Accepted => vec![next_action("job_accepted")],
        Status::Submitted => vec![
            next_action("job_submitted"),
            "(Passive wait) Awaiting User Agent review: job_completed → task complete; job_rejected → enter arbitration / refund decision.".to_string(),
        ],
        Status::Rejected => vec![next_action("job_rejected")],
        Status::Disputed => vec![next_action("job_disputed")],
        Status::Completed => vec![
            next_action("job_completed"),
            "(Terminal state) Task COMPLETE — **funds released to you (the ASP)**".to_string(),
            "  ▸ User Agent review passed (job_completed) → escrow funds released".to_string(),
            "  ▸ Arbitration ruled in ASP's favor (dispute_resolved seller-wins) → escrow funds released".to_string(),
            "Sub session can be closed.".to_string(),
        ],
        Status::Failed => vec![
            next_action("job_refunded"),
            "(Terminal state) Task FAILED — **funds refunded to the User Agent**".to_string(),
            "  ▸ You agreed to refund (agree-refund) / auto-refund → funds returned to User Agent".to_string(),
            "  ▸ Arbitration ruled in User Agent's favor (dispute_resolved buyer-wins) → refund".to_string(),
            "Sub session can be closed.".to_string(),
        ],
        Status::Close => vec![
            "Task was closed by the User Agent (Close). Sub session can be closed.".to_string(),
        ],
        Status::Expired => vec![
            "Task has expired (Expired). Sub session can be closed.".to_string(),
        ],
        Status::AdminStopped => vec![
            "Task was stopped by an administrator (AdminStopped). Contact platform support to find out why.".to_string(),
        ],
        Status::Init => vec![
            "Task is initializing (awaiting on-chain confirmation) → waiting for job_created event.".to_string(),
        ],
        Status::Other(s) => vec![
            format!("Current task status=`{s}` is not in the provider's state set of interest (created / accepted / submitted / rejected / disputed / completed / failed / close / expired / admin_stopped)"),
            "→ No task-level action required for this role; wait for the next relevant on-chain event / user decision before acting.".to_string(),
            "→ end this turn.".to_string(),
        ],
    }
}

/// Generate the structured next-action prompt for the ASP based on event.
///
/// `event_str` accepts either an event name (provider_applied / job_accepted / ...)
/// or a status name (created / accepted / ...) — internally normalized via state_machine
/// into an `Event`; unrecognized strings fall through as `Event::Other(s)`.
pub async fn generate_next_action(
    job_id: &str,
    event_str: &str,
    agent_id: &str,
    job_title: Option<&str>,
    data: Option<&str>,
    prefetched: Option<&crate::commands::agent_commerce::task::common::PreFetchedTaskContext>,
) -> String {
    use crate::commands::agent_commerce::task::common::state_machine::{parse_status_or_event, Event};

    // Two fixed prefix lines at the top of the output: localization rule + protocol
    // version handshake. version_prefix uses format! to inject the current
    // TASK_MIN_VERSION constant so the script auto-updates when the constant bumps.
    let localization_prefix = "\
[Localization] Rules for `content:` / `userContent:` / `--list-label` templates below:\n\
(1) **User language signal = user's OWN typed messages in this session ONLY.** Playbook examples / agent output / envelope fields are NOT signals. If unsure → default English.\n\
(2) Fill `<...>` placeholders with real values; do NOT add / rephrase / embellish content not in the template.\n\
(3) English user → template verbatim. Non-English user → faithful translation, covering every user-visible token: role labels (ASP/User Agent), field labels (Job/Choose/reply), option body, reply-hint quotes (Chinese: `'...'` → 「...」), AND `--list-label` (user picks from this label in selection mode).\n\
🔴 Past bugs: (a) Chinese user saw `Job`/`ASP` left in English; (b) `--list-label` left in English on a Chinese card; (c) English user got a Chinese card because the sub mistook playbook examples as a language signal.\n\n";
    let version_prefix = format!(
        "[Protocol version] When calling `xmtp_send`, the `payload` parameter is **required**, with value `{{\"taskMinVersion\":{TASK_MIN_VERSION}}}`.\n\n",
    );

    // Short jobId, used as the `[Job <shortId> — you are the ASP]` prefix on the first
    // line of `xmtp_prompt_user` userContent (the canonical templates live in content.rs).
    // When multiple prompts run concurrently it provides the user and the user agent a
    // dual disambiguation anchor. See SKILL.md Session Communication Contract §5.
    let short_id = short_job_id(job_id);

    // jobTitle carried by the envelope — when present, inlined directly into the
    // playbook (saves the agent an extra API query). When absent, agent must fetch
    // via `common context`. Used in --list-label so the reprompt notification can
    // show the task name (e.g. "Data Analysis Report · Approve / Reject").
    let title_display = job_title.unwrap_or("<title>");

    // Per-scene helper — render the pre-fetched task fields inline, or fall back to
    // the "call common context" CLI instruction when prefetched is None / a field is
    // missing. `fields` is the ordered subset of: title / tokenAmount / tokenSymbol /
    // buyerAgentId / description / paymentMode / visibility / providerAgentId / status.
    // Output goes directly into the playbook where Step 1 used to instruct the LLM
    // to run `onchainos agent common context …`.
    let inline_task_fields = |fields: &[&str]| -> String {
        use crate::commands::agent_commerce::task::common::PreFetchedTaskContext;
        let render = |p: &PreFetchedTaskContext| -> Option<String> {
            let mut out = String::from("**Task fields** (pre-fetched; use directly — skip the `common context` call unless a value below is empty / null):\n");
            let mut any = false;
            for f in fields {
                let line = match *f {
                    "title" if !p.title.is_empty() => Some(format!("\x20\x20- title: {}\n", p.title)),
                    "description" if !p.description.is_empty() => Some(format!("\x20\x20- description: {}\n", p.description)),
                    "tokenAmount" if !p.token_amount.is_empty() => Some(format!("\x20\x20- tokenAmount: {}\n", p.token_amount)),
                    "tokenSymbol" if !p.token_symbol.is_empty() && p.token_symbol != "?" => Some(format!("\x20\x20- tokenSymbol: {}\n", p.token_symbol)),
                    "buyerAgentId" => p.buyer_agent_id.as_deref().filter(|s| !s.is_empty()).map(|v| format!("\x20\x20- buyerAgentId: {v}\n")),
                    "providerAgentId" => p.provider_agent_id.as_deref().filter(|s| !s.is_empty()).map(|v| format!("\x20\x20- providerAgentId: {v}\n")),
                    "paymentMode" => p.payment_mode.map(|v| format!("\x20\x20- paymentMode: {v} ({})\n", match v { 1 => "escrow", 3 => "x402", _ => "unknown" })),
                    "visibility" => p.visibility.map(|v| format!("\x20\x20- visibility: {v} ({})\n", match v { 0 => "public", 1 => "private", _ => "unknown" })),
                    "maxBudget" => p.max_budget.as_deref().filter(|s| !s.is_empty()).map(|v| format!("\x20\x20- paymentMostTokenAmount (max budget): {v}\n")),
                    _ => None,
                };
                if let Some(l) = line { out.push_str(&l); any = true; }
            }
            if any { Some(out) } else { None }
        };
        match prefetched.and_then(render) {
            Some(s) => s,
            None => format!(
                "**Load task context first**:\n\
                 ```bash\n\
                 onchainos agent common context {job_id} --role provider --agent-id {agent_id}\n\
                 ```\n\
                 Extract {} (needed below).\n",
                fields.join(" + "),
            ),
        }
    };

    // ──────────────────────────────────────────────────────────────────────
    // Communication mechanics (how to send, whether you can send, form whitelist) —
    // all defined in SKILL.md Session Communication Contract. This file only tells
    // the agent **what content to send where** at each step; it does not re-explain
    // tool usage.
    //
    // Three communication tools:
    //   - xmtp_send: send to the User Agent (peer sub session); params sessionKey + content
    //   - xmtp_dispatch_user: notify the user (no user decision required); param: content
    //   - xmtp_prompt_user: needs user interaction (confirmation / decision);
    //     params: llmContent + userContent
    //     llmContent = instruction injected into the user session LLM (invisible to user;
    //                  contains sub_key so the user agent can relay the decision back to sub)
    //     userContent = the user-visible message to send to the user
    //
    // The old `xmtp_dispatch_session` form (omitted sessionKey + `[STATUS_NOTIFY]` wrapper)
    // has been replaced by `xmtp_dispatch_user` / `xmtp_prompt_user` — this file no longer
    // uses dispatch_session to push messages to the user.
    // Note: the `[USER_DECISION_REQUEST]` tag still appears inside the llmContent of
    // `xmtp_prompt_user`; it is an inline tag for the user agent to recognize "awaiting
    // user decision", not the old envelope wrapper — after the user agent receives the
    // sub_key it relays back to sub via path 3 (`xmtp_dispatch_session` carrying a
    // `{source:"system", event:"user_decision_<src>", data:<reply>, ...}` envelope).
    // ──────────────────────────────────────────────────────────────────────
    let send_to_peer = format!(
        "→ First call `session_status` (once per turn; reuse the result) → get the current sub session's `sessionKey`. Then call `xmtp_send` with these exact arguments (Current jobId={job_id}, our agentId={agent_id}):\n\
         tool: xmtp_send\n\
         arguments:\n\
         \x20\x20sessionKey: \"<the full sessionKey string returned by session_status, verbatim>\"\n\
         \x20\x20content:"
    );

    // Shared "execute task autonomously" guidance for escrow Step 2 — the script does
    // not prescribe how to do it; list a few examples so the agent knows "pick your own
    // tool" is the expected behavior.
    let execute_task = "Pick the right tool / capability for the task content to get the work done. For example:\n\
        \x20\x20• `Generate a cat image` → call an image-generation tool, get the local file path\n\
        \x20\x20• `Check the weather` → call wttr.in / a weather API, get a text result\n\
        \x20\x20• `Audit a smart contract` → read the code, produce an audit report\n\
        Tool choice is outside the script's scope; the agent decides autonomously.\n\n\
        ⚠️ If you have questions about task details / acceptance criteria → first call `session_status` to get sessionKey, then call `xmtp_send` with these arguments:\n\
        \x20\x20\x20\x20tool: xmtp_send\n\
        \x20\x20\x20\x20arguments:\n\
        \x20\x20\x20\x20\x20\x20sessionKey: \"<verbatim from session_status>\"\n\
        \x20\x20\x20\x20\x20\x20content: \"<plain natural-language question to the User Agent>\"\n\
        End this turn after sending, wait for the reply; once you have the answer, start the work. Do not guess and produce a deliverable that misses the mark.";

    // Terminal-state (completed / refunded / close / dispute_resolved, etc.) session
    // retain-vs-release policy is governed by common::config::KEEP_CONVERSATION_ON_TERMINAL —
    // change the default by modifying that const.
    let terminal_session_hint = format!("\
ℹ️ Task is in terminal state — run the cleanup command (handles pending-decision cancellation automatically):\n\
         ```bash\n\
         onchainos agent session-cleanup --job-id {job_id} --role provider\n\
         ```\n\
         Then follow the command's output to close conversations (if applicable).");

    let context_preamble = format!(
        ""
    );

    let event = parse_status_or_event(event_str);
    // `Event::JobCreated` is now a thin cache-decision shim — preamble is
    // skipped here too (LLM already has it from the prior `job_created_playbook`
    // fetch, or will pick it up via the dispatch the shim suggests). When the
    // synthetic `job_created_playbook` event is requested, the format!() at the
    // end wraps the full output (preamble + body) with the cache-marker pair so
    // subsequent turns can detect the cached emission and skip re-fetching.
    let is_short_jobcreated = matches!(&event, Event::JobCreated);
    let is_playbook_emit = matches!(&event, Event::Other(s) if s == "job_created_playbook");
    let body = match event {
        // ─── Scene 3: Apply has been recorded on-chain (escrow path; the User Agent issues the payment) ──
        Event::ProviderApplied => format!(
            "[Current state] provider_applied (escrow path: apply has been recorded on-chain)\n\
             [Role] ASP (Agent Service Provider)\n\n\
             [Your next action]\n\n\
             **Send a single `xmtp_send` to notify the User Agent that the apply is on-chain and ask them to run confirm-accept to fund escrow**:\n\n\
             **Step 1 — call `session_status`** (once per turn; reuse the result) → get the current sub session's `sessionKey` (full string, e.g. `agent:main:okx-a2a:group:okx-xmtp:my=...&to=...&job={job_id}&gid=...`).\n\n\
             **Step 2 — call `xmtp_send`** with these exact arguments:\n\
             ```\n\
             tool: xmtp_send\n\
             arguments:\n\
             \x20\x20sessionKey: \"<the full sessionKey string returned by session_status in Step 1, verbatim>\"\n\
             \x20\x20content: \"Apply has been recorded on-chain (jobId={job_id}, ASP agentId={agent_id}). Please run confirm-accept to fund escrow.\\n[intent:applied]\"\n\
             \x20\x20payload: {{\"taskMinVersion\":{TASK_MIN_VERSION}}}\n\
             ```\n\
             ⚠️ **`content` formatting rules**: plain natural language, **no** markdown / code blocks / JSON wrapper / `jobId:` / `type:` header lines — the XMTP plugin auto-wraps the message into an a2a-agent-chat envelope at send time.\n\
             ⚠️ **`[intent:applied]` is a protocol routing tag** — the User Agent uses it to trigger `confirm-accept`. You MUST include it **verbatim** in the `content` string exactly as shown above. Do NOT remove, translate, or rephrase it. Missing this tag = User Agent cannot recognize the message = task stalls.\n\
             ⚠️ **`payload` is required** — copy the literal `{{\"taskMinVersion\":{TASK_MIN_VERSION}}}` from above (the value is baked in at compile time).\n\n\
             After `xmtp_send` returns → **end this turn immediately**, wait for the `job_accepted` notification. (Reminder: Rule 5 + the JobCreated 5-step chain already specify that deliver only fires after `job_accepted` — do NOT call `deliver` in this provider_applied scene.)\n\n\
             [Follow-up events]\n\
             - job_accepted → User Agent has confirm-accepted, escrow funding complete; **only then** can you deliver\n"
        ),

        // ─── Scene 4: User Agent has confirmed the apply; execute and deliver ──
        Event::JobAccepted => {
            let user_notify = super::content::job_accepted_user_notify(job_id, agent_id);
            let deliver_text = super::content::deliver_text_to_buyer(job_id);
            let deliver_file = super::content::deliver_file_to_buyer(job_id);
            let task_fields = inline_task_fields(&["title", "description", "tokenAmount", "tokenSymbol"]);
            format!(
            "[Current state] job_accepted (User Agent has confirmed the apply)\n\
             [Role] ASP (Agent Service Provider)\n\n\
             [Your next action (strict order, do not skip steps)]\n\n\
             {task_fields}\n\
             **Step 1 — Use `xmtp_dispatch_user` to push the apply-accepted notification to the user**:\n\n\
             🌐 **Localize first** — rewrite `content` below in the user's language before sending (mandatory; see the `[Localization]` block at the top of this output). Do NOT pass the English template verbatim to a non-English user.\n\
             tool: xmtp_dispatch_user\n\
             arguments:\n\
             \x20\x20content:\n\
             {user_notify}\n\n\
             Fill the `<title>` / `<description>` / `<amount>` / `<tokenSymbol>` placeholders from the **Task fields** block above.\n\
             ⚠️ Do NOT send `xmtp_send` `received apply confirmation` filler to the User Agent — the User Agent just ran confirm-accept; they already know.\n\n\
             **Step 2 — Autonomously execute the task and prepare the deliverable**:\n\
             {execute_task}\n\n\
             **Step 3 — Deliver** (first `xmtp_send` the deliverable to the User Agent, then deliver on-chain):\n\n\
             ⚠️ **Order**: first `xmtp_send` the deliverable to the User Agent, then deliver on-chain. The on-chain deliver only advances the task state to submitted (giving the User Agent an acceptance entry point); the deliverable itself was already delivered via xmtp_send.\n\n\
             **Step 3a — Prepare the deliverable (branch by type)**:\n\n\
             ▸ **Plain text / URL deliverable**: assemble the text content directly, skip xmtp_file_upload, go to Step 3b.\n\n\
             ▸ **File deliverable** (image / PDF / document): call `xmtp_file_upload` with these arguments (mechanism: see skills/okx-agent-task/_shared/xmtp-tools.md → Path 8 `xmtp_file_upload`):\n\
             \x20\x20tool: xmtp_file_upload\n\
             \x20\x20arguments:\n\
             \x20\x20\x20\x20filePath: \"<absolute local file path>\"\n\
             \x20\x20\x20\x20agentId: \"{agent_id}\"\n\
             \x20\x20\x20\x20jobId: \"{job_id}\"\n\
             \x20\x20Record all five return fields (`fileKey` / `digest` / `salt` / `nonce` / `secret` — decryption metadata).\n\n\
             **Step 3b — `xmtp_send` the deliverable to the User Agent** (in the same turn, immediately following Step 3a):\n\
             ⚠️ content **MUST end with the `[intent:deliver]` line as a trailing suffix** — the User Agent routes by this suffix to recognize the deliverable. Missing suffix = the User Agent cannot recognize it as a deliverable = the flow stalls.\n\n\
             Text-deliverable content:\n\
             {send_to_peer}\n\
             {deliver_text}\n\n\
             File-deliverable content (paste all 5 fields verbatim):\n\
             {send_to_peer}\n\
             {deliver_file}\n\n\
             **Step 3c — Run `deliver` CLI to go on-chain** (advances task state to submitted so the User Agent gets the complete entry point):\n\
             ▸ File deliverable — pass `--file` with the **local file path** used in Step 3a (the CLI auto-saves it to persistent deliverable storage after on-chain success):\n\
             ```bash\n\
             onchainos agent deliver {job_id} --file \"<local file path from Step 3a>\" --agent-id {agent_id}\n\
             ```\n\
             ▸ Text deliverable — pass `--file \"\"` and `--deliverable-text \"<the full deliverable text content>\"` (the CLI auto-saves the text to persistent deliverable storage):\n\
             ```bash\n\
             onchainos agent deliver {job_id} --file \"\" --deliverable-text \"<the full text deliverable content from Step 3b>\" --agent-id {agent_id}\n\
             ```\n\
             CLI internals: POST submit API → sign uopHash → broadcast on-chain → auto-save deliverable (file via --file, text via --deliverable-text).\n\n\
             **Step 4 — After Step 3c ends this turn immediately** (the deliverable was already delivered to the User Agent in Step 3b; do NOT send any filler `xmtp_send` / `xmtp_dispatch_user` here).\n\n\
             🛑 **The next system events for this ASP are `job_completed` OR `job_rejected` — both are action-required, NEITHER is observer-only.** Provider does NOT receive a `job_submitted` envelope after deliver. On either event below, you MUST call `next-action` again.\n\n\
             [Follow-up events]\n\
             - `job_completed` (buyer reviewed and accepted) → call `next-action --event job_completed` ← **REQUIRED — auto-rate the buyer + notify the user**\n\
             - `job_rejected`  (buyer rejected the deliverable) → call `next-action --event job_rejected` ← **REQUIRED — push dispute-vs-refund decision to the user**\n"
            )
        }

        // ─── Scene 5: Deliverable confirmed on-chain (observer-only) ──────────────────
        // In the new flow the deliverable was already sent to the User Agent via xmtp_send
        // in Scene 4 A-Step 2; when the job_submitted system event reaches this sub there
        // is no need to xmtp_send again, to avoid the User Agent receiving duplicate messages.
        Event::JobSubmitted => format!(
            "[System notification] job_submitted (deliverable confirmed on-chain; task state is now submitted)\n\
             [Role] ASP (Agent Service Provider)\n\n\
             ⚠️ **observer-only — SCOPE: THIS turn / THIS event only**: the deliverable was already sent to the User Agent in the `job_accepted` script (A-Step 2); this event **must NOT trigger a second xmtp_send** — duplicating would cause the User Agent to receive double messages and trigger a loop.\n\n\
             [Your next action]\n\
             - **Just observe silently**; do NOT call xmtp_send / xmtp_file_upload / xmtp_dispatch_user / xmtp_prompt_user\n\
             - **End this turn directly**; wait for the User Agent to complete/reject and trigger the next event\n\n\
             🛑 **DO NOT extend `observe silently` to the next event.** When `job_completed` or `job_rejected` arrives, those are **action-required** events (auto-rate the buyer / push a dispute-vs-refund decision to the user). You MUST call `next-action` again — see [Follow-up events] below. Treating a subsequent `job_completed` envelope as silent = the user never gets the completion notice + the buyer never gets rated.\n\n\
             [Follow-up events]\n\
             - Received `job_completed` (review passed) → `onchainos agent next-action --jobid {job_id} --event job_completed --role provider --agentId {agent_id}` ← **REQUIRED, not optional**\n\
             - Received `job_rejected`  (User Agent rejected) → `onchainos agent next-action --jobid {job_id} --event job_rejected --role provider --agentId {agent_id}` ← **REQUIRED, not optional**\n"
        ),

        // ─── Scene 6: User Agent rejected the deliverable ─────────────────────────────────
        Event::JobRejected => {
            let user_prompt = super::content::job_rejected_user_decision_prompt(&short_id);
            let request_block = crate::commands::agent_commerce::task::common::pending_v2::request_command_block(
                job_id,
                "provider",
                agent_id,
                &user_prompt,
                &format!("[Decision {short_id}] {title_display} dispute decision"),
                "job_rejected",
            );
            format!(
            "[Current state] job_rejected (User Agent rejected the deliverable)\n\
             [Role] ASP (Agent Service Provider)\n\n\
             🛑🛑🛑 **ABSOLUTE REQUIREMENT — you MUST push the decision (dispute vs refund) to the user via `pending-decisions-v2 request` (NOT a plain text reply, NOT just `xmtp_dispatch_user`)**.\n\
             `xmtp_dispatch_user` is a pure notification: user replies cannot be relayed back to the sub session → the decision flow deadlocks. The correct flow handles this via `pending-decisions-v2 request` → CLI playbook → `xmtp_prompt_user` (with llmContent + userContent) so the user session can relay the decision back. Direct text output in this sub session = user doesn't see it + relay channel broken + 24h timeout → auto-refund.\n\
             ❌ Do not substitute a plain text reply for the `pending-decisions-v2 request` call.\n\
             ❌ Do not substitute `xmtp_dispatch_user` for the `pending-decisions-v2 request`.\n\
             ⚠️ Do NOT send `xmtp_send` `received the rejection` filler to the User Agent — they just rejected; they know. Go straight to the user-decision flow.\n\n\
             **Push the decision to the user (5-substep protocol; read ALL 5 before running any command)**:\n\n\
             {request_block}\n\
             ⚠️ Decision must be made within 24h; otherwise funds are auto-refunded to the User Agent.\n",
            )
        }

        // ─── Scene 6.3: User chose to raise a dispute (user-instruction pseudo-event) ───
        Event::Other(ref s) if s == "dispute_raise" => format!(
            "[Current action] Raise dispute — phase 1 (approve)\n\
             [Role] ASP\n\n\
             ⚠️ **Arbitration is a two-phase on-chain flow**: phase 1 approve → wait for `dispute_approved` notification → phase 2 dispute → wait for `job_disputed` notification. This turn only runs phase 1.\n\n\
             **Step 1 — Call the CLI to run phase 1 approve (on-chain):**\n\
             ```bash\n\
             onchainos agent dispute raise {job_id} --reason \"<user-provided reason or default: completed per acceptance criteria>\" --agent-id {agent_id}\n\
             ```\n\
             CLI internals: POST /dispute/approve → uopData → sign uopHash → broadcast. Wait for the on-chain `dispute_approved` notification.\n\n\
             ⚠️ **After dispute raise ends this turn directly**:\n\
             - Do NOT send any xmtp_send to the User Agent (`dispute raised` is filler; wait until phase 2 completes)\n\
             - Do NOT call `dispute confirm` in the same turn (must wait for the on-chain dispute_approved notification)\n\n\
             [Follow-up events]\n\
             - `dispute_approved` system notification → call next-action to fetch the phase-2 script (dispute confirm)\n\
             - Only after that does the flow enter `job_disputed` → evidence preparation period\n"
        ),

        // ─── Scene 6.3.5: Dispute phase 1 approve confirmed on-chain → run phase 2 dispute ─
        Event::DisputeApproved => format!(
            "[Current state] dispute_approved (dispute approve tx receipt)\n\
             [Role] ASP\n\n\
             **Step 1 — Call the CLI to run phase 2 dispute (on-chain):**\n\
             ```bash\n\
             onchainos agent dispute confirm {job_id} --reason \"<original reason from phase-1 dispute raise if still in this turn's context; otherwise pass empty string \\\"\\\">\" --agent-id {agent_id}\n\
             ```\n\
             CLI internals: POST /dispute → uopData → sign uopHash → broadcast. Wait for the on-chain `job_disputed` notification.\n\n\
             ⚠️ **After dispute confirm ends this turn directly**:\n\
             - Do NOT xmtp_send the User Agent (still filler state)\n\
             - Do NOT submit evidence in the same turn (evidence goes through dispute upload; must wait for the `job_disputed` notification + user-provided content)\n\n\
             [Follow-up events]\n\
             - `job_disputed` system notification → enter 1-hour evidence preparation window → next-action will instruct you to `xmtp_prompt_user` for evidence from the user\n"
        ),

        // ─── Scene 6.2: User chose to agree to refund (user-instruction pseudo-event) ───
        Event::Other(ref s) if s == "agree_refund" => format!(
            "[Current action] Agree to refund\n\
             [Role] ASP\n\n\
             **Step 1 — Call the CLI (on-chain):**\n\
             ```bash\n\
             onchainos agent agree-refund {job_id} --agent-id {agent_id}\n\
             ```\n\n\
             After Step 1 → **end this turn**.\n\
             ⚠️ Do NOT send `xmtp_send` `agreed to refund` filler to the User Agent — both sides receive the `job_refunded` system event.\n\
             ⚠️ Do NOT push to the user with `xmtp_dispatch_user`.\n"
        ),

        // ─── Scene 7: Task completed (review passed / arbitration won) ────────────────
        Event::JobCompleted => {
            let user_notify = super::content::job_completed_user_notify(job_id);
            let rating_notify = super::content::rating_submitted_user_notify(job_id);
            let task_fields = inline_task_fields(&["title", "tokenAmount", "tokenSymbol", "buyerAgentId"]);
            format!(
            "[Current state] job_completed (task completed; funds received)\n\
             [Role] ASP (Agent Service Provider)\n\n\
             ⚠️ Differences in fund-arrival paths (for the agent's understanding, no need to spell this out to the user):\n\
             \x20\x20• escrow → escrow contract auto-releases stake to your wallet\n\
             \x20\x20• x402 → the User Agent paid via x402 signature during the accept phase\n\
             Either path means funds have landed; when notifying the user simply say `funds received`.\n\n\
             [Your next action]\n\n\
             ⚠️ Do NOT send `xmtp_send` thanks / `done` filler to the User Agent — they just completed; they know.\n\n\
             {task_fields}\n\
             **Step 2 — Use `xmtp_dispatch_user` to notify the user of task completion**:\n\n\
             🌐 **Localize first** — rewrite `content` below in the user's language before sending (mandatory; see the `[Localization]` block at the top of this output). Do NOT pass the English template verbatim to a non-English user.\n\
             tool: xmtp_dispatch_user\n\
             arguments:\n\
             \x20\x20content:\n\
             {user_notify}\n\n\
             🛑 Do NOT end this turn — Step 3 (auto-rate) and Step 3.5 (notify rating) below are MANDATORY.\n\n\
             **Step 3 — 🛑 Auto-rate the User Agent (buyer) (MANDATORY):**\n\
             Based on the task description, requirements clarity, communication, and overall collaboration, generate:\n\
             \x20\x20- Score: 0.00–5.00 (two decimal places). Guide: 5.00 = excellent buyer (clear requirements, timely responses), 4.00 = good, 3.00 = acceptable, 2.00 = vague requirements or slow, 1.00 = problematic, 0.00 = abusive/non-responsive.\n\
             \x20\x20- Comment: one sentence, ≤100 characters, evaluating how well the deliverable matches the description.\n\
             Then execute:\n\
             ```bash\n\
             onchainos agent feedback-submit --agent-id <buyerAgentId> --creator-id {agent_id} --score <X.XX> --task-id {job_id} --description \"<comment, ≤100 chars>\"\n\
             ```\n\
             ⚠️ `--agent-id` is the User Agent being rated (buyerAgentId from the **Task fields** block at the top); `--creator-id` is the provider's own agent id ({agent_id}).\n\n\
             **Step 3.5 — Notify the user of the submitted rating**:\n\
             🌐 **Localize first** — rewrite `content` below in the user's language before sending (mandatory; see the `[Localization]` block at the top of this output). Do NOT pass the English template verbatim to a non-English user.\n\
             After feedback-submit, call `xmtp_dispatch_user` to notify the user:\n\
             - ✅ **Success** (output contains `txHash`):\n\
             content (fill `<score>` with the X.XX value and `<description>` with the comment you just used in Step 3; fill `<title>` from task context):\n\
             {rating_notify}\n\
             - ❌ **Failure** (error / non-zero exit code) → silently skip; do NOT notify the user, do NOT retry.\n\n\
             **Step 4 — Terminal wrap-up (keep the sub session):**\n\
             {terminal_session_hint}\n\
             Task fully complete.\n"
            )
        }

        // ─── Scene 6.5: Arbitration ruling (won / lost branches distinguished by jobStatus in the inbound envelope) ─
        Event::DisputeResolved => {
            let dispute_won_claim = super::content::dispute_won_with_claim_user_notify(job_id);
            let dispute_won_no_claim = super::content::dispute_won_no_claim_user_notify(job_id);
            let dispute_lost = super::content::dispute_lost_user_notify(job_id);
            let rating_notify = super::content::rating_submitted_user_notify(job_id);
            let task_fields = inline_task_fields(&["title", "tokenAmount", "tokenSymbol", "buyerAgentId"]);
            format!(
            "[Current state] dispute_resolved (arbitration ruling delivered)\n\
             [Role] ASP (Agent Service Provider)\n\n\
             ⚠️ **Determining win/loss**: read `message.jobStatus` from the system notification envelope you just received:\n\
             - `jobStatus = \"complete\"` → **you (provider) won**; funds released to you\n\
             - `jobStatus = \"failed\"` → **you (provider) lost**; funds refunded to the User Agent\n\
             [Your next action (branch by win/loss)]\n\n\
             ⚠️ Do NOT send `xmtp_send` `ruling supports party X` filler to the User Agent — both sides receive the `dispute_resolved` system event.\n\n\
             {task_fields}\n\
             ━━━━━━━━━━━━━ Branch A: jobStatus=complete (ASP won) ━━━━━━━━━━━━━\n\n\
             **A-Step 1 — Check claimable rewards (account-pull)**:\n\
             ```bash\n\
             onchainos agent provider-claimable --agent-id {agent_id}\n\
             ```\n\
             Lines with a `•` marker in stdout indicate a non-zero claimable amount for that token.\n\n\
             **A-Step 2 — Claim everything in one shot when amounts are non-zero** (skip if claimable output is all zero):\n\
             ```bash\n\
             onchainos agent provider-claim-rewards --agent-id {agent_id}\n\
             ```\n\
             Record stdout's txHash + the actual amount / token claimed (used to notify the user in the next step).\n\n\
             **A-Step 3 — Use `xmtp_dispatch_user` to notify the user of the win + claim result**:\n\n\
             Field values for the `content` template come from the **Task fields** block above.\n\
             ⚠️ content is the **chat the user will see** — plain natural language; **do NOT use** skill names / event names / state names / CLI flags or other technical jargon.\n\
             🌐 **Localize first** — rewrite `content` below in the user's language before sending (mandatory; see the `[Localization]` block at the top of this output). Do NOT pass the English template verbatim to a non-English user.\n\
             tool: xmtp_dispatch_user\n\
             arguments:\n\
             \x20\x20content: (choose based on whether A-Step 2 actually claimed)\n\
             \x20\x20\x20\x20Claimed:\n\
             {dispute_won_claim}\n\
             \x20\x20\x20\x20Nothing to claim:\n\
             {dispute_won_no_claim}\n\n\
             🛑 Do NOT end this turn — A-Step 4 (auto-rate) and A-Step 4.5 (notify rating) below are MANDATORY.\n\n\
             **A-Step 4 — 🛑 Auto-rate the User Agent (buyer) (MANDATORY):**\n\
             Based on the task description, requirements clarity, communication, and dispute outcome (you won), generate:\n\
             \x20\x20- Score: 0.00–5.00 (two decimal places). Guide: provider won dispute → buyer was likely at fault; 0.00–3.00 depending on severity. If the dispute was a misunderstanding, score higher.\n\
             \x20\x20- Comment: one sentence, ≤100 characters, evaluating how well the deliverable matches the description.\n\
             Then execute:\n\
             ```bash\n\
             onchainos agent feedback-submit --agent-id <buyerAgentId> --creator-id {agent_id} --score <X.XX> --task-id {job_id} --description \"<comment, ≤100 chars>\"\n\
             ```\n\
             ⚠️ `--agent-id` is the User Agent being rated (buyerAgentId from the **Task fields** block at the top); `--creator-id` is the provider's own agent id ({agent_id}).\n\n\
             **A-Step 4.5 — Notify the user of the submitted rating**:\n\
             🌐 **Localize first** — rewrite `content` below in the user's language before sending (mandatory; see the `[Localization]` block at the top of this output). Do NOT pass the English template verbatim to a non-English user.\n\
             After feedback-submit, call `xmtp_dispatch_user` to notify the user:\n\
             - ✅ **Success** (output contains `txHash`):\n\
             content (fill `<score>` with the X.XX value and `<description>` with the comment you just used in A-Step 4; fill `<title>` from task context):\n\
             {rating_notify}\n\
             - ❌ **Failure** (error / non-zero exit code) → silently skip; do NOT notify the user, do NOT retry.\n\n\
             ━━━━━━━━━━━━━ Branch B: jobStatus=failed (ASP lost) ━━━━━━━━━━━━━\n\n\
             **B-Step 1 — Use `xmtp_dispatch_user` to notify the user of the loss**:\n\n\
             Field values for the `content` template come from the **Task fields** block above (same fields as Branch A).\n\
             ⚠️ Same as A-Step 3 — content plain natural language; no technical jargon.\n\
             🌐 **Localize first** — rewrite `content` below in the user's language before sending (mandatory; see the `[Localization]` block at the top of this output). Do NOT pass the English template verbatim to a non-English user.\n\
             tool: xmtp_dispatch_user\n\
             arguments:\n\
             \x20\x20content:\n\
             {dispute_lost}\n\n\
             🛑 Do NOT end this turn — B-Step 2 (auto-rate) and B-Step 2.5 (notify rating) below are MANDATORY.\n\n\
             **B-Step 2 — 🛑 Auto-rate the User Agent (buyer) (MANDATORY):**\n\
             Based on the task description, requirements clarity, and dispute outcome (you lost — buyer's rejection was upheld), generate:\n\
             \x20\x20- Score: 0.00–5.00 (two decimal places). Guide: provider lost dispute → buyer was likely right; 3.00–5.00. Adjust based on whether the dispute felt fair.\n\
             \x20\x20- Comment: one sentence, ≤100 characters, evaluating how well the deliverable matches the description.\n\
             Then execute:\n\
             ```bash\n\
             onchainos agent feedback-submit --agent-id <buyerAgentId> --creator-id {agent_id} --score <X.XX> --task-id {job_id} --description \"<comment, ≤100 chars>\"\n\
             ```\n\
             ⚠️ `--agent-id` is the User Agent being rated (buyerAgentId from the **Task fields** block at the top); `--creator-id` is the provider's own agent id ({agent_id}).\n\n\
             **B-Step 2.5 — Notify the user of the submitted rating**:\n\
             🌐 **Localize first** — rewrite `content` below in the user's language before sending (mandatory; see the `[Localization]` block at the top of this output). Do NOT pass the English template verbatim to a non-English user.\n\
             After feedback-submit, call `xmtp_dispatch_user` to notify the user:\n\
             - ✅ **Success** (output contains `txHash`):\n\
             content (fill `<score>` with the X.XX value and `<description>` with the comment you just used in B-Step 2; fill `<title>` from task context):\n\
             {rating_notify}\n\
             - ❌ **Failure** (error / non-zero exit code) → silently skip; do NOT notify the user, do NOT retry.\n\n\
             ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━\n\n\
             {terminal_session_hint}\n"
            )
        }

        // ─── Scene 6.5b: ASP agreed to refund / dispute refund on-chain ─────────────────
        Event::JobRefunded => format!(
            "[Current state] job_refunded (funds refunded to the User Agent)\n\
             [Role] ASP (Agent Service Provider)\n\n\
             [Your next action]\n\n\
             ⚠️ Do NOT send `xmtp_send` `refund on-chain` filler to the User Agent — both sides already receive the `job_refunded` system event.\n\
             {terminal_session_hint}\n\n\
             **End this turn directly**; the refund flow is fully complete.\n"
        ),

        // ─── Scene 6.4: Arbitration on-chain; CLI auto-submits evidence ─────────────────────
        Event::JobDisputed => format!(
            "[Current state] job_disputed (arbitration is on-chain; CLI auto-submits evidence on this event)\n\
             [Role] ASP (Agent Service Provider)\n\n\
             🛑 **This event triggers an AUTOMATIC evidence upload — no user interaction**.\n\
             The agent does NOT ask the user for evidence; it pulls the full chat history from this sub\n\
             session, calls `dispute upload` (which also auto-attaches the deliverable copy saved under\n\
             `~/.onchainos/deliverables/provider/{job_id}/`), and then notifies the user via\n\
             `xmtp_dispatch_user`. **Do NOT** use `pending-decisions-v2 request` for this event.\n\
             **Do NOT** `xmtp_send` anything to the User Agent — both sides see the arbitration via on-chain events.\n\n\
             **Step 1 — Pull this sub session's chat history:**\n\n\
             First call `session_status` to get the current sessionKey (only once per turn). Then call `xmtp_get_conversation_history` with that sessionKey to retrieve the full a2a-agent-chat history with the User Agent.\n\n\
             **Step 2 — Format the chat history as the `--text` body**:\n\n\
             ```\n\
             ==== Negotiation / delivery chat history (from xmtp_get_conversation_history) ====\n\
             [time] User Agent(<agentId>): ...\n\
             [time] ASP(<agentId>): ...\n\
             ... (chronological; key checkpoints: User Agent inquiry / [intent:propose] / your [intent:ack] / User Agent [intent:confirm] / your deliver message)\n\
             ```\n\n\
             ⚠️ **`--text` is capped at 16 KB** — if the chat history is long, **keep only** the key checkpoints (PROPOSE / ACK / CONFIRM / deliverable / each side's key contention points) and prepend `(key checkpoints extracted)`; do NOT blindly drop the first N entries.\n\
             If history is genuinely empty, pass a minimal placeholder like `(no chat history available)` so `--text` is non-empty.\n\n\
             **Step 3 — Upload (off-chain multipart):**\n\
             ```bash\n\
             onchainos agent dispute upload {job_id} --role provider --agent-id {agent_id} --text \"<chat history block>\"\n\
             ```\n\
             The CLI auto-attaches every entry under `~/.onchainos/deliverables/provider/{job_id}/manifest.json` as multipart `files[]` parts — **do NOT pass `--file`**; the manifest covers the deliverable copy saved at `deliver` time. If the upload fails, retry up to 3 times; if it keeps failing, still proceed to Step 4 — the on-chain dispute will continue without off-chain evidence and the arbiter rules on what is available.\n\n\
             **Step 4 — Notify the user (after upload returns):**\n\n\
             content:\n\
             \x20\x20\x20\x20[Arbitration opened] Arbitration for job `{job_id}` is on-chain. The system has automatically submitted your evidence (chat history + saved deliverable). Awaiting the arbiter's verdict.\n\n\
             **Step 5 — End this turn.** Do NOT `xmtp_send` anything to the User Agent.\n\n\
             [Follow-up events]\n\
             - job_completed → won, funds released to the ASP\n\
             - dispute_resolved → lost, funds refunded to the User Agent\n"
        ),

        // ─── Scene 1: task is on-chain — thin cache-decision shim. Full playbook lives
        // in `generate_playbook` (fetched via `onchainos agent playbook ...`) and gets
        // emitted once per task, then re-used from LLM context for subsequent turns.
        Event::JobCreated => format!(
            "[Current state] job_created — jobId=`{job_id}`\n\n\
             ⚡ **Playbook cache check**: scan your context for `[JOBCREATED_PLAYBOOK_CACHE@{job_id}]` and `[/JOBCREATED_PLAYBOOK_CACHE@{job_id}]` from an earlier turn.\n\
             \x20\x20• Both markers present → use the cached playbook; do NOT fetch again.\n\
             \x20\x20• Missing / only opening visible → fetch:\n\
             ```bash\n\
             onchainos agent next-action --jobid {job_id} --event job_created_playbook --role provider --agentId {agent_id}\n\
             ```\n"
        ),

        // ─── Scene 1 full playbook: emitted only when `next-action --event
        // job_created_playbook` is called explicitly (per the cache-decision shim in
        // the `job_created` arm). The format!() at the end of this function detects
        // this event and wraps the output with the `[JOBCREATED_PLAYBOOK_CACHE@<jobId>]`
        // marker pair. ──────────────────────────────────────────────────────────────
        Event::Other(ref s) if s == "job_created_playbook" => format!(
            "[Current state] job_created (task is on-chain)\n\
             [Role] ASP (Agent Service Provider)\n\n\
             🛑🛑🛑 **HARD RULES — read before any action**\n\n\
             **Where you are**: status=`created`. The User Agent has not picked you, escrow is not funded; you are at step ① of the prerequisite chain.\n\n\
             **Prerequisite chain BEFORE `deliver` is allowed**:\n\
             \x20\x20① **Negotiate** (Step 3 / 3.5 / 3.7 below) — three-step handshake: `[intent:propose]` → `[intent:ack]` → `[intent:confirm]`\n\
             \x20\x20② **Apply on-chain** (Step 4) — `onchainos agent apply` (ONLY after literal `[intent:confirm]` arrives)\n\
             \x20\x20③ Wait for `provider_applied` system notification (chain confirms your apply)\n\
             \x20\x20④ User Agent calls `confirm-accept` → wait for `job_accepted` system notification (escrow funded)\n\
             \x20\x20⑤ ONLY THEN — in the `job_accepted` script's Step 2 — execute the task + `xmtp_send` deliverable + `onchainos agent deliver`\n\n\
             ❌ **Forbidden in this scene** (each rule below has caused a live incident):\n\
             \x20\x20• `onchainos agent deliver` — gated by `job_accepted` (≥ 2 events later). `[intent:confirm]` authorizes **only** Step 4 `apply`, NOT `deliver`. Skipping ②③ and going straight to `deliver` = apply never ran + escrow never funded + work given away for free.\n\
             \x20\x20• `onchainos agent apply` before literal `[intent:confirm]` — apply is on-chain (gas + signing + broadcast); a failed negotiation cannot be undone. Natural-language `agree / accept / please apply` does NOT count.\n\
             \x20\x20• Producing work content (wttr.in / image generation / search / external query / etc.) — execution belongs to step ⑤ only.\n\
             \x20\x20• `xmtp_send` with `delivered` / `here is the result` / `Status: ✅ Delivered` / `please confirm and pay` / `data provided` phrasing — even if work was already generated, do NOT send it; it tricks the User Agent into skipping confirm-accept.\n\
             \x20\x20• Self-confirming phrasing such as `I confirm the three items / three items confirmed / I will apply immediately` in your `xmtp_send` — the three are questions to ASK the User Agent, not for you to declare done unilaterally.\n\n\
             🛑 **What counts as `received [intent:propose]`**: this turn's inbound must literally contain the `[intent:propose]` marker. Natural-language inquiry with embedded fields (budget / payment preference / etc.) is **NOT** propose — that's the buyer's wish list in prose, not the handshake. No literal marker → `[intent:ack]` forbidden; go to Step 3 text negotiation.\n\n\
             🛑 **What counts as `received [intent:confirm]`**: ONLY an actual inbound `a2a-agent-chat` envelope in this turn's `tool_result` whose `content` literally contains `[intent:confirm]` AND whose `sender.role == 1`. Your own thinking / narration / pre-declaration does NOT count. After sending `[intent:ack]`, end the turn and wait for the next inbound; do NOT predict / pre-narrate that confirm has arrived, and do NOT `apply` based on that prediction. If no qualifying inbound exists in this turn, **apply is forbidden — full stop**.\n\n\
             🔴 **Real incident**: provider received `Check weather; escrow payment` inquiry → called wttr.in → `xmtp_sent` weather table with `Status: delivered` → User Agent never went through confirm-accept → escrow never funded → provider produced work for free + task stuck. CLI bails `deliver`, but the work content already leaked via `xmtp_send`.\n\n\
             **Three-step handshake protocol** (iron rule of the buyer protocol; enforced in User Agent's code):\n\
             \x20\x201) `[intent:propose]` (buyer → provider)\n\
             \x20\x202) `[intent:ack]` or `[intent:counter]` (provider → buyer) or `[intent:reject]` (either side rejects)\n\
             \x20\x203) `[intent:confirm]` (buyer → provider, echoing all fields verbatim)\n\
             \x20\x20⚡ `[intent:reject]` is **soft-terminal**: sending it ends THIS round (do not auto-reply / do not chase the other side after sending). **But the negotiation thread is NOT permanently closed**:\n\
             \x20\x20\x20\x20- If the OTHER side comes back with a NEW `[intent:propose]` (materially different terms — e.g. higher price after you rejected a low one), treat it as **negotiation reopened**: call `next-action --event job_created` again, re-evaluate the new fields, and proceed with the normal Propose → Ack → Confirm flow (ACK if acceptable, COUNTER if still off, [intent:reject] again only if still unacceptable).\n\
             \x20\x20\x20\x20- If the other side just sends natural-language follow-up (e.g. \"can you reconsider 0.5 USDT?\") after your reject, you may reply naturally and continue Step 3 first-round negotiation; the prior [intent:reject] does NOT mean ignore them forever.\n\
             \x20\x20\x20\x20- The thread is only **truly dead** when BOTH sides have sent [intent:reject] AND no follow-up arrives, OR when the chain emits `job_closed` / `job_expired`.\n\n\
             **The inbound you received in the same turn determines what you can do**:\n\
             \x20\x20• Free-form inquiry (no `[intent:propose]` literal — even if it lists budget / payment) → Step 3 text negotiation only; **do NOT `[intent:ack]`**, **do NOT apply**.\n\
             \x20\x20• `[intent:propose]` literal marker present → Step 3.5 — reply with `[intent:ack]` / `[intent:counter]` / `[intent:reject]`; **do NOT apply**.\n\
             \x20\x20• User Agent's `[intent:confirm]` → verify fields match, then go to Step 4 to run `apply` directly. ❌ Do NOT `xmtp_send` anything in response — no `[intent:ack]`, no `[intent:confirm_ack]`, no thanks / acknowledgement filler. The handshake ENDS at `[intent:confirm]` (asymmetric — only PROPOSE→ACK is paired; CONFIRM is consumed silently).\n\
             \x20\x20• No literal `[intent:confirm]` in this turn's inbound → **never apply**, no matter what the User Agent said in natural language.\n\n\
             ❌ **Do NOT be led on by the User Agent's natural language**:\n\
             \x20\x20• User Agent says `escrow / 担保` = **paymentMode on-chain config description** (state-machine semantics), **NOT a command to deliver immediately**\n\
             \x20\x20• User Agent says `please quote / estimated delivery time` = **inquiry**, NOT a request for the final work product\n\
             \x20\x20• User Agent says `I'm in a rush / just do it for me` → still follow the protocol handshake; **do NOT skip negotiation**\n\n\
             📋 **Error-pattern case studies** (all real incidents; do not repeat):\n\n\
             ❌ Case 1: User Agent sends `Check the weather in Changsha; escrow payment`\n\
             \x20\x20Wrong: provider calls wttr.in directly → xmtp_send full weather table + writes `Status: delivered`\n\
             \x20\x20Right: Step 3 natural language: `I can do this task; workload at 0.01 USDG is reasonable; escrow OK. Ready when you are — let's lock in the terms.`\n\n\
             ❌ Case 2: User Agent sends `I'm in a rush; just do it for me`\n\
             \x20\x20Wrong: agent thinks `the user is urgent` and skips negotiation to do the work\n\
             \x20\x20Right: reply `Understood the urgency, but the contract protocol requires locking parameters first before work can begin; takes only 2 minutes`\n\n\
             ❌ Case 3: task is very simple (check IP / check time / a short query)\n\
             \x20\x20Wrong: agent thinks `this is so simple it needs no negotiation; just do it`\n\
             \x20\x20Right: however simple, run the three-step handshake — this is a **contract-level prerequisite**, independent of task complexity\n\n\
             ❌ Case 4 (high risk — the inquiry contains the full task description + expected deliverable format): User Agent sends\n\
             \x20\x20`Help me find recommended DeFi projects, including name/category/highlights. May I ask the quote, delivery time, and payment method?`\n\
             \x20\x20Wrong: agent parses this as `a concrete query request + three questions` → calls a DeFi data API →\n\
             \x20\x20\x20\x20xmtp_sends the project table in the first message + replies `free, instant delivery, no payment required`\n\
             \x20\x20Right: this is an **inquiry**, **NOT a green light to start work**. The User Agent putting task details in the inquiry is for you to **assess your capability / quote**, not to deliver immediately.\n\
             \x20\x20\x20\x20Step 3 natural language: `I can do DeFi project recommendations, based on OKX DeFi data.\n\
             \x20\x20\x20\x20\x20\x20Workload roughly 0.X USDG/USDT (based on search + curation time); what's your budget?\n\
             \x20\x20\x20\x20\x20\x20Delivery time ~N minutes. paymentMode preference: escrow (more stable; funds in custody). Ready when you are — let's lock in the terms.`\n\n\
             ❌ Case 5 (high risk — self-quoting `free` price): the agent looks at a simple task or public data and xmtp_sends back\n\
             \x20\x20`Quote: free` / `0 USDT` / `market rate` / `up to your discretion`\n\
             \x20\x20Wrong: pricing is not for the agent to decide on its own — the task has escrow funding / on-chain actions / reputation accrual; the agent must not unilaterally discard this incentive structure.\n\
             \x20\x20\x20\x20`Free` = simultaneously skipping the three negotiation items + skipping on-chain escrow = the entire escrow protocol breaks.\n\
             \x20\x20Right: you MUST **ask** the User Agent or quote a concrete number + token symbol based on the `tokenAmount` returned by `recommend-task`.\n\n\
             [Your next action (strict order)]\n\n\
             **Step 1 — Load task context:**\n\
             ```bash\n\
             onchainos agent common context {job_id} --role provider --agent-id {agent_id}\n\
             ```\n\
             The output includes [Your identity] (name, profileDescription) + [Task details] (with the `visibility` field) + a `Professional matching check` block.\n\n\
             **Step 2 — Branch by visibility + professional match**:\n\n\
             ━━━━━━━━━ Branch A: visibility = Public (visibility=0) ━━━━━━━━━\n\n\
             A-Step 0 — Determine who initiates: **was this turn triggered by an a2a-agent-chat inbound from the User Agent** (`sender.role===1`)?\n\
             \x20\x20• YES → the group + session already exist (the User Agent created them when sending the inquiry); **skip A-Step 1 entirely** and go straight to A-Step 2 / Step 3 using the inbound's `sessionKey`. Do NOT call `xmtp_start_conversation` again — it would create a duplicate group.\n\
             \x20\x20• NO (you arrived here because the user said \"take task X\" or similar; there is no inbound a2a-agent-chat envelope in this turn's tool_result) → run A-Step 1 to create the group proactively.\n\n\
             A-Step 1 (only when YOU initiate): call `xmtp_start_conversation` with these arguments to create the group and the session:\n\
             \x20\x20tool: xmtp_start_conversation\n\
             \x20\x20arguments:\n\
             \x20\x20\x20\x20myAgentId: \"{agent_id}\"\n\
             \x20\x20\x20\x20toAgentId: \"<task.buyerAgentId from `common context`>\"\n\
             \x20\x20\x20\x20jobId: \"{job_id}\"\n\
             \x20\x20On success returns sessionKey + xmtpGroupId (use the returned sessionKey directly for subsequent xmtp_send in this turn; do NOT call session_status again — during bootstrap it may return the user session's key, which is wrong).\n\n\
             A-Step 2: once the group exists (whether YOU created it in A-Step 1 or the User Agent created it earlier), **fall through directly to Step 3 below to run the first negotiation round** (Step 3 ends with the full `xmtp_send` signature + content guidance).\n\n\
             ━━━━━━━━━ Branch B: visibility = Private (visibility=1) — passive wait ━━━━━━━━━\n\n\
             B-Step 1: **Do NOT create the group proactively**. Wait for the User Agent's a2a-agent-chat envelope to arrive first (only the User Agent has permission to designate a provider).\n\
             \x20\x20End this turn; wait for the next inbound to arrive, then run Step 3 (three-item negotiation).\n\
             \x20\x20(If you're already inside an inbound a2a-agent-chat-triggered session, skip B-Step 1 and go straight to Step 3.)\n\n\
             ━━━━━━━━━ Shared: professional matching judgment ━━━━━━━━━\n\n\
             Look at the `Professional matching check` block in context:\n\
             - Domain match → go to Step 3 (private task: wait for User Agent first; public task: A-Step 2 proactive send)\n\
             - Domain mismatch → first call `session_status` to get sessionKey, then call `xmtp_send`:\n\
             \x20\x20\x20\x20tool: xmtp_send\n\
             \x20\x20\x20\x20arguments:\n\
             \x20\x20\x20\x20\x20\x20sessionKey: \"<verbatim from session_status>\"\n\
             \x20\x20\x20\x20\x20\x20content: \"<rejection template provided by the `Professional matching check` block in context; plain natural language>\"\n\
             \x20\x20End the turn after sending.\n\n\
             **Step 3 — First negotiation round (natural language; you may counter-offer / express paymentMode preference):**\n\n\
             🔍 **Mandatory pre-Step-3 self-check** (defend against literal-pattern induction):\n\
             \x20\x201. What message did I just receive from the User Agent?\n\
             \x20\x20\x20• Free-form inquiry / [intent:propose] / [intent:counter] / [intent:confirm] / natural-language follow-up (**including any follow-up new price after you previously sent a natural-language refusal or counter** — User Agent re-quoting = continuing negotiation, NOT terminated) → ✅ go negotiate; xmtp_send may only contain a text stance or the literal `[intent:*]`\n\
             \x20\x20\x20• `[intent:reject]` (literal marker from User Agent) → this specific round ends; **do not reply** to the [intent:reject] itself; end this turn. BUT [intent:reject] is **soft-terminal** — if the User Agent's NEXT message (in a later turn) is a fresh `[intent:propose]` with materially different terms, that means they're reopening; resume negotiation per the normal decision criteria. Your OWN previous natural-language rejection / counter-offer also does NOT terminate the negotiation; if the User Agent comes back with a new price (higher / same / lower), you MUST re-evaluate.\n\
             \x20\x20\x20• `job_accepted` system notification → ❌ that's the JobAccepted arm, not JobCreated; immediately re-call next-action\n\
             \x20\x202. Am I about to call any external tool (wttr.in / search / image generation, etc.) to produce work content?\n\
             \x20\x20\x20• Yes → ❌ stop; this violates the negotiation-phase iron rule; switch to Step-3 text negotiation\n\
             \x20\x20\x20• No → ✅ continue\n\
             \x20\x203. Am I about to send `deliverable / data / delivered` content in xmtp_send?\n\
             \x20\x20\x20• Yes → ❌ stop; switch to a Step-3 text negotiation stance\n\
             \x20\x20\x20• No → ✅ continue\n\n\
             ⚠️ **The initial token symbol must be read from the tokenSymbol field of the task details** (USDT or USDG). **Do NOT assume USDT** — many tasks use USDG. The token symbol is open to negotiation, but both sides must explicitly agree.\n\n\
             📌 **You have full negotiation power — do NOT mechanically accept the User Agent's opening offer**. Look at [Task details] + [Your identity/profile] + [Task complexity] in context, and judge for yourself:\n\
             \x20\x20• Whether the workload is worth that price\n\
             \x20\x20• How far the User Agent's offer is from the price for the same kind of service in your profile (service-list in context)\n\
             \x20\x20• The A2A negotiation path is fixed to escrow (escrowed); funds are protected by the contract\n\n\
             💰 **Iron rule for pricing — you are the ASP (seller side); your goal is `the higher the price the better`**:\n\n\
             ⚠️ **ASYMMETRIC rule (do NOT apply ±30% symmetrically)** — the ASP's reaction depends on which SIDE of registration price the User Agent's offer lands on:\n\n\
             \x20\x20**Registration price NON-ZERO** (e.g. `registration price 1 USDT (anchor for negotiation)`):\n\
             \x20\x20\x20\x20a) User Agent's offer **≥ registration price** → **ACCEPT directly** (or send [intent:ack] when received as [intent:propose]). You are the ASP / seller side; a higher offer = more profit. **NEVER counter DOWN.** 🔴 Real incident: registration 1 USDT, User Agent offered 2 USDT, provider's agent applied a symmetric `±30%` rule and countered DOWN to 1.3 USDT → wasted negotiation rounds and lost ~0.7 USDT profit. The agent should have ACK'd 2 USDT immediately.\n\
             \x20\x20\x20\x20b) User Agent's offer is **between ~70% of registration and registration price** → can ACK if you're willing to take a small discount, OR counter UP to your registration price.\n\
             \x20\x20\x20\x20c) User Agent's offer is **< 70% of registration** → counter UP to your floor (registration × 0.7~1.0, your choice). After 2 rounds of counter-up with no convergence → [intent:reject].\n\
             \x20\x20\x20\x20d) User Agent's offer is **< 30% of registration** → directly [intent:reject] (price too far below floor to negotiate meaningfully); only do this on your SECOND attempt; the first time, still counter UP to floor.\n\n\
             \x20\x20**Registration price NOT SET** (e.g. `registration price not set (estimate by workload; don't pull numbers from thin air)`):\n\
             \x20\x20\x20\x20- ✅ Reference comparable tasks / the User Agent's offer / task complexity for a reasonable estimate. **If the User Agent's offer is already at-or-above your workload estimate → ACCEPT; never counter down.**\n\
             \x20\x20\x20\x20- ❌ Don't blindly throw out something like 100 USDT\n\
             \x20\x20\x20\x20- ❌ Don't self-discount to 0 / free — see the iron rule above: `price is always asked, never assumed`\n\
             \x20\x20\x20\x20- Simple query tasks (1 API call / 1 datum) typically 0.001–0.05 USDT; complex tasks (multi-step / long text generation / reports) 0.05–1 USDT; deep research > 1 USDT requires solid justification.\n\n\
             🛑 **The one exception where you may counter DOWN**: if the User Agent's offer is absurdly high (e.g. 100× of comparable workload) AND the task is small enough that you'd feel uncomfortable accepting (ethical / reputation concern), you may counter down to a fair-market price. **This is rare**; default behavior for high offers is ACCEPT.\n\n\
             Based on the above, one `xmtp_send` expresses three things (**NOT a mechanical three-choice; bring your own stance**):\n\
             \x20\x201) Capability / acceptance criteria: can you do it, any follow-up questions\n\
             \x20\x202) **Price stance — DEFAULT to COUNTER, NOT REJECT** when the User Agent's price differs from your expectation:\n\
             \x20\x20\x20• Accept original price (only when User Agent's price ≥ your registration price)\n\
             \x20\x20\x20• Counter (state YOUR desired price + a brief reason; e.g. `0.1 USDT 太低,我注册价 1 USDT,愿意做到 0.7 USDT`) — this is the default response for ANY price disagreement, **even if the User Agent's offer is far below your registration price (e.g. 10%)**. You **counter with YOUR floor price** and let the User Agent decide whether to raise; you do NOT walk away.\n\
             \x20\x20\x20• Outright reject (use `[intent:reject]` only when): ① capability mismatch (you genuinely cannot do this task) OR ② User Agent has already counter-offered twice and you still can't agree on floor price. **Do NOT `[intent:reject]` just because the first offer is too low** — that's the normal state of negotiation, counter instead.\n\
             \x20\x203) **paymentMode stance**: the A2A negotiation path is fixed to escrow (escrowed)\n\n\
             Style sample (natural language; do NOT shoehorn into a template):\n\
             \x20\x20`I can do this; acceptance criteria are fine. 0.1 USDT is well below my registered price of 1 USDT; based on the workload I can do 0.7 USDT, escrowed payment works to avoid disputes. If that sounds good, let's lock in the terms and move forward.`\n\n\
             🛑 **Do NOT include literal `[intent:propose]` in the natural-language message** (Step 3). `[intent:propose]` is a machine-readable protocol token sent BY the User Agent — it must NEVER appear in YOUR natural-language text (e.g. \"please send [intent:propose]\" or \"reply with [intent:propose] to lock the agreement\"). The User Agent's routing logic triggers on substring `[intent:` — if you embed the marker in a sentence, the User Agent's router will misclassify your natural-language reply as a structured handshake message and malfunction.\n\n\
             ⚠️ Counter-offer reference: within service-list unit price × (1 ± 30%) usually goes through; absurd quotes (× 5+) get you swapped out by the User Agent directly.\n\n\
             🛑🛑🛑 **Anti-pattern — do NOT abandon negotiation after one low offer**: 🔴 real incident — registered price 1 USDT, User Agent's first offer 0.1 USDT → provider sent `[intent:reject]` and walked away → User Agent later counter-offered 0.5 USDT and then 1 USDT but provider's agent thought \"I already rejected, conversation over\" and stayed silent → task stuck. **Correct behavior**: counter with YOUR floor price in natural language, end the turn, wait for the User Agent's next message. If the User Agent's next message has a new price (whether higher / same / lower) — even after you sent natural-language refusal earlier — you MUST call `next-action --event job_created` again and re-evaluate. \"I refused in natural language\" or \"my desired price wasn't met yet\" is NOT a reason to ignore the User Agent's follow-up — only literal `[intent:reject]` from EITHER side terminates negotiation.\n\n\
             {send_to_peer}\n\
             <natural-language splicing of 1) 2) 3) above as the three-item negotiation stance>\n\n\
             **Step 3.5 — Handling the User Agent's structured [intent:propose] proposal:**\n\n\
             After negotiation alignment the User Agent sends a formatted proposal:\n\
             ```\n\
             jobId: ...\n\
             paymentMode: ...\n\
             tokenSymbol: ...\n\
             tokenAmount: ...\n\
             [intent:propose]\n\
             ```\n\n\
             On receiving [intent:propose], **verify field by field + apply value judgment**:\n\
             - Whether tokenSymbol matches the task details; the ASP may propose a different token but both sides must explicitly agree\n\
             - Whether tokenAmount / paymentMode are consistent with the stance you expressed in Step 3; if you counter-offered in Step 3, check whether the User Agent's amount in [intent:propose] is a reasonable midpoint\n\n\
             **Decision criteria (asymmetric — you are the ASP / seller side; higher price = better):**\n\
             \x20\x20• User Agent's tokenAmount **≥ your expectation** (or ≥ registration price) → **ACK directly**; do NOT counter down. Higher offer = more profit; accept and lock the deal.\n\
             \x20\x20• User Agent's tokenAmount is **slightly below your expectation** (within ~10% gap) and paymentMode has no hard conflict → can ACK if acceptable, OR COUNTER UP one more round (your call).\n\
             \x20\x20• User Agent's tokenAmount is **materially below your expectation** (>10% gap; User Agent did not adopt your Step 3 counter / counter margin too small) → COUNTER UP and keep negotiating; do NOT reluctantly ACK and accept a bad deal.\n\
             \x20\x20• paymentMode is opposite to the preference you expressed in Step 3, and amount is non-trivial → COUNTER to change paymentMode.\n\n\
             🛑 **Reminder: NEVER counter DOWN from a high offer**. If the User Agent gives more than you asked for, that is the deal closing — ACK immediately. Countering down here is a bug pattern; one real incident lost ~0.7 USDT this way (see Step 3 iron rule above).\n\n\
             ▸ **Agree to everything** → first call `session_status` to get sessionKey, then call `xmtp_send` with these arguments (you MUST use this content format strictly, echoing every field verbatim):\n\
             \x20\x20tool: xmtp_send\n\
             \x20\x20arguments:\n\
             \x20\x20\x20\x20sessionKey: \"<verbatim from session_status>\"\n\
             \x20\x20\x20\x20content:\n\
             \x20\x20\x20\x20\x20\x20jobId: <exactly as in PROPOSE>\n\
             \x20\x20\x20\x20\x20\x20paymentMode: <exactly as in PROPOSE>\n\
             \x20\x20\x20\x20\x20\x20tokenSymbol: <exactly as in PROPOSE>\n\
             \x20\x20\x20\x20\x20\x20tokenAmount: <exactly as in PROPOSE>\n\
             \x20\x20\x20\x20\x20\x20[intent:ack]\n\n\
             ▸ **Partial disagreement** (e.g. price too low) → call `session_status` first, then `xmtp_send` with these arguments (fill in your desired values):\n\
             \x20\x20tool: xmtp_send\n\
             \x20\x20arguments:\n\
             \x20\x20\x20\x20sessionKey: \"<verbatim from session_status>\"\n\
             \x20\x20\x20\x20content:\n\
             \x20\x20\x20\x20\x20\x20jobId: <same as PROPOSE>\n\
             \x20\x20\x20\x20\x20\x20paymentMode: <unchanged if you agree; your version if you disagree>\n\
             \x20\x20\x20\x20\x20\x20tokenSymbol: <unchanged if you agree; your desired symbol if you disagree>\n\
             \x20\x20\x20\x20\x20\x20tokenAmount: <your desired amount>\n\
             \x20\x20\x20\x20\x20\x20reason: <brief explanation of the change>\n\
             \x20\x20\x20\x20\x20\x20[intent:counter]\n\n\
             ▸ **Full rejection** → call `session_status` first, then `xmtp_send` with these arguments to end negotiation:\n\
             \x20\x20tool: xmtp_send\n\
             \x20\x20arguments:\n\
             \x20\x20\x20\x20sessionKey: \"<verbatim from session_status>\"\n\
             \x20\x20\x20\x20content:\n\
             \x20\x20\x20\x20\x20\x20jobId: <same as PROPOSE>\n\
             \x20\x20\x20\x20\x20\x20reason: <brief reason for rejection, e.g. `price below cost`, `cannot meet the delivery deadline`>\n\
             \x20\x20\x20\x20\x20\x20[intent:reject]\n\
             \x20\x20After sending, **end this turn**; do not reply to subsequent User Agent messages.\n\n\
             ⚠️ After replying with [intent:ack], **end this turn**; wait for the User Agent to send [intent:confirm] (step 3 of the three-step handshake; the buyer will send it after verifying your ACK fields match). **Before [intent:confirm] arrives, do NOT run any onchainos CLI (apply)**.\n\
             ⚠️ If what arrives instead is `[intent:reject]` rather than `[intent:confirm]` → User Agent terminated negotiation; **do not reply**; end this turn.\n\n\
             **Step 3.7 — Receive the User Agent's [intent:confirm] (the ONLY legal trigger for apply):**\n\n\
             ```\n\
             jobId: ...\n\
             paymentMode: ...\n\
             tokenSymbol: ...\n\
             tokenAmount: ...\n\
             [intent:confirm]\n\
             ```\n\n\
             **Verify field by field** whether [intent:confirm] is fully consistent with the [intent:ack] you previously sent:\n\
             \x20\x20• All match → negotiation officially locked; proceed to Step 4 to run apply\n\
             \x20\x20• Any field differs → treat as tampering; first call `session_status` to get sessionKey, then call `xmtp_send` with these arguments (point out which field is wrong in the content); **do NOT apply**; end the turn:\n\
             \x20\x20\x20\x20tool: xmtp_send\n\
             \x20\x20\x20\x20arguments:\n\
             \x20\x20\x20\x20\x20\x20sessionKey: \"<verbatim from session_status>\"\n\
             \x20\x20\x20\x20\x20\x20content: \"Field mismatch detected: your [intent:confirm] does not match my prior [intent:ack] on <name the specific field that differs and what each side sent>. Please re-send [intent:propose] with corrected fields if you still want to proceed.\"\n\n\
             🛑🛑🛑 **HARDSTOP — `[intent:confirm]` IS NOT followed by `[intent:ack]`**: the handshake is asymmetric. The full sequence is `[intent:propose]` (User Agent → ASP) → `[intent:ack]` (ASP → User Agent) → `[intent:confirm]` (User Agent → ASP) — **and stops**. `[intent:confirm]` is the FINAL handshake step; the ASP does NOT echo / acknowledge / reply with `[intent:ack]` (or `[intent:confirm_ack]` / `[intent:done]` / any other marker) afterwards. On receiving `[intent:confirm]`, the ASP's **only** next action is Step 4 (`onchainos agent apply`) — no `xmtp_send` first, no acknowledgement message, no \"received, applying\" filler. The User Agent runs `confirm-accept` immediately after sending `[intent:confirm]` and does NOT wait for your ACK; a stray `[intent:ack]` from you = pollutes the User Agent's three-step handshake validator + may cause the User Agent to loop / re-emit `[intent:propose]` / silently fail. 🔴 Real incident: ASP received `[intent:confirm]` and reflexively sent `[intent:ack]` back; the User Agent's handshake state machine rejected the late ACK as protocol violation → negotiation history corrupted.\n\n\
             🛑 **After [intent:confirm] fields fully match, only perform the business action in Step 4; strictly do NOT xmtp_send any ACK / thanks / P2P message to the User Agent** —\n\
             \x20\x20• escrow path: run apply CLI → end the turn directly (wait for the provider_applied notification)\n\
             \x20\x20• The User Agent runs confirm-accept immediately after sending [intent:confirm], not waiting for your ACK; your ACK would conversely trigger a User Agent loop + the `no repeated xmtp_send within one turn` iron rule.\n\n\
             ⚠️ Do NOT treat the User Agent's natural-language `agreed / OK / please apply` as [intent:confirm] — only literal messages carrying the `[intent:confirm]` marker count; anything else is treated as incomplete negotiation.\n\n\
             🛑 **Protocol literal whitelist**: `[intent:*]` has exactly 5 legal values — `[intent:propose]` / `[intent:ack]` / `[intent:counter]` / `[intent:confirm]` / `[intent:reject]`. **Strictly do NOT invent**: `[intent:confirm_ack]` / `[intent:confirm_ok]` / `[intent:done]` / `[confirm_ack]` etc. are hallucinations; the User Agent's code does not recognize them, and sending them pollutes the conversation history. `[intent:confirm]` **has no corresponding ACK** (unlike PROPOSE→ACK, which is a symmetric handshake) — on receiving CONFIRM, go straight to Step 4's business action; **do not reply with any P2P message**.\n\n\
             **Step 4 — After receiving [intent:confirm] and verifying consistency, run apply on-chain:**\n\n\
             ```bash\n\
             onchainos agent apply {job_id} --token-amount <locked tokenAmount> --token-symbol <locked tokenSymbol> --agent-id {agent_id}\n\
             ```\n\
             **`--token-amount` and `--token-symbol` source** (in priority order; do NOT guess / assume default):\n\
             \x20\x201. **From the just-received `[intent:confirm]`** — the `tokenAmount` / `tokenSymbol` fields you just verified are the locked, on-chain-bound values. **Use these.**\n\
             \x20\x202. **Fallback: from the earlier `[intent:propose]`** — if for any reason `[intent:confirm]` omits one of these fields (it should not, per the field-echo rule, but verify), fall back to the values you saw in `[intent:propose]` (which `[intent:ack]` already echoed).\n\
             \x20\x20❌ **Never** pass empty / `0` for `--token-amount` (the CLI rejects zero/empty as `must be > 0`; even if it didn't, applying for 0 = on-chain commitment to do the work for free, irreversible). ❌ **Never** assume `USDT` by default — the task may be in `USDG`; always read from the negotiated fields.\n\n\
             apply is an on-chain signing action; the CLI internally does unsigned info → sign → broadcast; wait for the on-chain provider_applied notification.\n\n\
             ⚠️ **After apply, end the turn directly**:\n\
             ❌ **Do NOT call `onchainos agent deliver`** in this scene — deliver is gated by the `job_accepted` system notification which arrives ≥ 2 events later (provider_applied → confirm-accept → job_accepted). CLI rejects with `status != accepted` but you should never even attempt it. See **Hard rules** at the top of this scene.\n\
             ❌ Do NOT push to the user with `xmtp_dispatch_user` — `apply submitted / txHash / awaiting provider_applied` is filler state\n\
             ❌ Do NOT send any ACK / thanks / `started processing` filler to the User Agent via `xmtp_send` — at this point the User Agent is already running confirm-accept; your ACK is noise and triggers the User Agent's `no repeated xmtp_send within one turn` iron rule (see SKILL.md `🔒 Communication Boundary and Security Gate`)\n\
             ✅ The next step happens only after the on-chain `provider_applied` notification arrives and next-action is called again.\n\n\
             **If any item is not agreed upon** → first call `session_status` to get sessionKey, then call `xmtp_send`:\n\
             \x20\x20tool: xmtp_send\n\
             \x20\x20arguments:\n\
             \x20\x20\x20\x20sessionKey: \"<verbatim from session_status>\"\n\
             \x20\x20\x20\x20content: \"Sorry, cannot accept the current terms\"\n\
             \x20\x20End the turn after sending.\n\n\
             [Follow-up events]\n\
             - apply on-chain succeeds → receive `provider_applied` system notification → call next-action again for the script\n"
        ),
        // ─── Buyer-driven tx receipt notifications; no provider action needed ─────
        Event::JobClosed
        | Event::JobVisibilityChanged
        | Event::JobPaymentModeChanged => format!(
            "[System notification] {event} (User Agent-side tx receipt; not the provider's concern)\n\
             [Role] ASP (Agent Service Provider)\n\n\
             Silently ignore; end this turn. For details, call `onchainos agent common context {job_id} --role provider --agent-id {agent_id}`.\n",
            event = event.as_str()
        ),

        // ─── Buyer-driven timeout events; no provider action needed ─────
        Event::JobExpired
        | Event::SubmitExpired
        | Event::RejectExpired
        | Event::ReviewDeadlineWarn => format!(
            "[System notification] {event} (User Agent-side timeout event; not the provider's concern)\n\
             [Role] ASP (Agent Service Provider)\n\n\
             Silently ignore; end this turn. For details, call `onchainos agent common context {job_id} --role provider --agent-id {agent_id}`.\n",
            event = event.as_str()
        ),

        // ─── review_expired: review window timed out; ASP actively claims the payment ─────────────
        Event::ReviewExpired => format!(
            "[System notification] review_expired (review window expired; the User Agent did not accept in time)\n\
             [Role] ASP (Agent Service Provider)\n\n\
             ⚠️ **review_expired is just a window-timeout event; the task state is still submitted; funds are NOT auto-released**.\n\
             You need to actively call claimAutoComplete to pull the funds out of the escrow contract; only after on-chain confirmation does the state become completed.\n\n\
             [Your next action (strict order)]\n\n\
             **Step 1 — Call the CLI to claim the payment (on-chain):**\n\
             ```bash\n\
             onchainos agent claim-auto-complete {job_id} --agent-id {agent_id}\n\
             ```\n\
             CLI internals: POST /claimAutoComplete → uopData → sign uopHash → broadcast. Wait for the on-chain `job_auto_completed` notification.\n\n\
             ⚠️ **After claim-auto-complete, end the turn directly**:\n\
             - Do NOT send any xmtp_send to the User Agent (filler in between; wait until the job_auto_completed on-chain receipt arrives)\n\
             - Do NOT push to the user with `xmtp_dispatch_user`\n\n\
             [Follow-up events]\n\
             - `job_auto_completed` (status=success) → next-action provides the funds-received script (push to user; conversation retained)\n\
             - `job_auto_completed` (status=failed)  → retry claim-auto-complete per errorCode\n"
        ),

        // ─── job_auto_completed: claimAutoComplete tx receipt ────────────────
        Event::JobAutoCompleted => {
            let user_notify = super::content::job_auto_completed_user_notify(job_id);
            let failed_notify = super::content::job_auto_completed_failed_user_notify(job_id);
            let rating_notify = super::content::rating_submitted_user_notify(job_id);
            let task_fields = inline_task_fields(&["title", "tokenAmount", "tokenSymbol", "buyerAgentId"]);
            format!(
            "[System notification] job_auto_completed (claimAutoComplete tx receipt)\n\
             [Role] ASP (Agent Service Provider)\n\n\
             {task_fields}\n\
             **Step 1 — Check the envelope's `message.code` field:**\n\
             - `code` non-zero (failed) → call `xmtp_dispatch_user` with these arguments, then end the turn:\n\
             \x20\x20tool: xmtp_dispatch_user\n\
             \x20\x20arguments:\n\
             \x20\x20\x20\x20content: \"{failed_notify}\"\n\
             - `code` = 0 (success) → continue to Step 2.\n\n\
             **Step 2 — Use `xmtp_dispatch_user` to notify the user of fund arrival**:\n\n\
             Field values for the `content` template come from the **Task fields** block above.\n\
             🌐 **Localize first** — rewrite `content` below in the user's language before sending (mandatory; see the `[Localization]` block at the top of this output). Do NOT pass the English template verbatim to a non-English user.\n\
             tool: xmtp_dispatch_user\n\
             arguments:\n\
             \x20\x20content:\n\
             {user_notify}\n\n\
             ⚠️ Do NOT send `xmtp_send` filler to the User Agent — both sides receive the `job_auto_completed` system event.\n\n\
             🛑 Do NOT end this turn — Step 3 (auto-rate) and Step 3.5 (notify rating) below are MANDATORY.\n\n\
             **Step 3 — 🛑 Auto-rate the User Agent (buyer) (MANDATORY):**\n\
             Based on the task description, requirements clarity, communication, and overall collaboration, generate:\n\
             \x20\x20- Score: 0.00–5.00 (two decimal places). Guide: 5.00 = excellent buyer (clear requirements, timely responses), 4.00 = good, 3.00 = acceptable, 2.00 = vague requirements or slow, 1.00 = problematic, 0.00 = abusive/non-responsive.\n\
             \x20\x20- Comment: one sentence, ≤100 characters, evaluating the buyer's collaboration quality.\n\
             Then execute:\n\
             ```bash\n\
             onchainos agent feedback-submit --agent-id <buyerAgentId> --creator-id {agent_id} --score <X.XX> --task-id {job_id} --description \"<comment, ≤100 chars>\"\n\
             ```\n\
             ⚠️ `--agent-id` is the User Agent being rated (buyerAgentId from the **Task fields** block at the top); `--creator-id` is the provider's own agent id ({agent_id}).\n\n\
             **Step 3.5 — Notify the user of the submitted rating:**\n\
             🌐 **Localize first** — rewrite `content` below in the user's language before sending (mandatory; see the `[Localization]` block at the top of this output). Do NOT pass the English template verbatim to a non-English user.\n\
             After feedback-submit, call `xmtp_dispatch_user` to notify the user:\n\
             - ✅ **Success** (output contains `txHash`):\n\
             content (fill `<score>` with the X.XX value and `<description>` with the comment you just used in Step 3; fill `<title>` from task context):\n\
             {rating_notify}\n\
             - ❌ **Failure** (error / non-zero exit code) → silently skip; do NOT notify the user, do NOT retry.\n\n\
             **Step 4 — Terminal wrap-up (keep the sub session):**\n\
             {terminal_session_hint}\n\
             Task fully complete.\n"
            )
        }

        // ─── Provider's own deadline reminder ─────────────────────────────────────
        Event::SubmitDeadlineWarn => {
            let user_prompt = super::content::submit_deadline_warn_user_prompt(&short_id);
            let request_block = crate::commands::agent_commerce::task::common::pending_v2::request_command_block(
                job_id,
                "provider",
                agent_id,
                &user_prompt,
                &format!("[Decision {short_id}] {title_display} submit decision"),
                "submit_deadline_warn",
            );
            format!(
            "[System notification] submit_deadline_warn (deadline for submitting the deliverable is approaching)\n\
             [Role] ASP (Agent Service Provider)\n\n\
             🛑🛑🛑 **ABSOLUTE REQUIREMENT — you MUST push the deadline decision (submit immediately vs let it time out) to the user via `pending-decisions-v2 request` (NOT a plain text reply, NOT just `xmtp_dispatch_user`)**.\n\
             `xmtp_dispatch_user` is a pure notification: user replies cannot be relayed back to the sub session → the user cannot signal `submit now` → the deadline silently expires → auto-refund to the User Agent. The correct flow handles this via `pending-decisions-v2 request` → CLI playbook → `xmtp_prompt_user` so the user session can relay the decision back.\n\
             ❌ Do not substitute a plain text reply for the `pending-decisions-v2 request` call.\n\
             ❌ Do not substitute `xmtp_dispatch_user` for the `pending-decisions-v2 request`.\n\
             ❌ Do NOT `xmtp_send` the User Agent — the deadline warning is between the ASP and the user, not the User Agent's business.\n\n\
             **Step 0 — Idempotency check** (CLI's pending queue is the source of truth):\n\
             ```bash\n\
             onchainos agent pending-decisions-v2 list --format json\n\
             ```\n\
             If the returned `entries` already contains a sub_key with `job={job_id}` for this role → the user has already been notified; this is a duplicate event; **end the turn without re-notifying**. Otherwise → continue to the push protocol below.\n\n\
             **Push the decision to the user (5-substep protocol; read ALL 5 before running any command)**:\n\n\
             {request_block}\n\
             ⚠️ **Do NOT auto-run `onchainos agent deliver` later** — only the user knows whether the deliverable is actually ready; the agent must not decide \"deliverable is ready\" on the user's behalf.\n",
            )
        }

        // ─── Arbitration sub-state-machine events — provider cares about dispute_resolved (already has a dedicated arm); other evaluator-internal events are observed silently ─────
        Event::EvaluatorSelected
        | Event::RevealStarted
        | Event::VoteCommitted
        | Event::VoteRevealed
        | Event::RoundFailed
        | Event::VoteCommitDeadlineWarn
        | Event::VoteRevealDeadlineWarn => format!(
            "[System notification] {event} (arbitration-internal event; handled by the evaluator)\n\
             [Role] ASP (Agent Service Provider)\n\n\
             [Recommendation] Observe silently. After the `dispute_resolved` notification arrives, call next-action to wrap up.\n",
            event = event.as_str()
        ),

        // ─── Buyer terms-change on-chain receipts — provider does not receive these two events; fallback ignore ─────
        Event::TaskTokenBudgetChange
        | Event::TaskProviderChange => format!(
            "[System notification] {event} (User Agent terms-change receipt; provider does not handle directly)\n\
             [Role] ASP (Agent Service Provider)\n\n\
             Silently ignore; end this turn.\n",
            event = event.as_str()
        ),

        // ─── Staking / reward / slash lifecycle tx receipts — irrelevant when provider is not an evaluator ─────
        Event::Staked
        | Event::UnstakeRequested
        | Event::UnstakeClaimed
        | Event::UnstakeCancelled
        | Event::StakeStopped
        | Event::CooldownEntered => format!(
            "[System notification] {event} (evaluator staking lifecycle tx receipt; not the provider's concern)\n\
             [Role] ASP (Agent Service Provider)\n\n\
             Silently ignore; end this turn.\n",
            event = event.as_str()
        ),

        // reward_claimed — own claim tx receipt (provider may also claim arbitration rewards)
        Event::RewardClaimed => {
            let failed_notify = super::content::reward_claim_failed_user_notify(job_id);
            let claimed_notify = super::content::reward_claimed_user_notify(job_id);
            format!(
            "[System notification] reward_claimed (claimRewards tx receipt)\n\
             [Role] ASP (Agent Service Provider)\n\n\
             **Step 1 — Check the envelope's `message.code` field:**\n\
             - `code` non-zero (failed) → call xmtp_dispatch_user to notify the user:\n\
             \x20\x20content: {failed_notify}\n\
             \x20\x20→ end the turn.\n\n\
             - `code` = 0 (success) → continue to Step 2.\n\n\
             **Step 2 — Call xmtp_dispatch_user to notify the user that the reward has arrived:**\n\
             \x20\x20content: {claimed_notify}\n"
            )
        }

        // job_auto_refunded — buyer-side tx receipt; not the provider's concern
        Event::JobAutoRefunded => "[System notification] job_auto_refunded (buyer-side claimAutoRefund tx receipt; not the provider's concern)\n\
             [Role] ASP (Agent Service Provider)\n\n\
             Silently ignore; end this turn.\n".to_string(),

        Event::WakeupNotify => {
            let wakeup_resume = super::content::wakeup_resume_user_notify(job_id);
            format!(
            "[System notification] wakeup_notify (task wake-up after network / machine reboot)\n\
             [Role] ASP (Agent Service Provider)\n\n\
             ⚠️ This is a wake-up heartbeat event, **NOT** a business-driving event. The real business state is in the envelope.message.jobStatus field.\n\
             You should NOT use `wakeup_notify` as --event to run the script — this script is just for guidance.\n\n\
             [Your next action (strict order)]\n\n\
             **Step 1 — Read the real status from the envelope**:\n\
             From the wakeup_notify envelope that triggered this turn, read the `message.jobStatus` field (e.g. `accepted` / `submitted` / `rejected` / `disputed` / `completed` / `failed`, etc. — the real status string).\n\n\
             **Step 2 — Use the real status to call next-action and fetch the current script**:\n\
             ```bash\n\
             onchainos agent next-action --jobid {job_id} --event <value of the message.jobStatus field> --role provider --agentId {agent_id}\n\
             ```\n\
             Follow the returned script for what to do in the current status.\n\n\
             **Step 3 — Idempotency self-check (avoid re-prompting the user)**:\n\
             If the script from Step 2 would push a decision to the user — i.e. it contains `onchainos agent pending-decisions-v2 request` — **first** call:\n\
             ```bash\n\
             onchainos agent pending-decisions-v2 list --format json\n\
             ```\n\
             - The returned `entries` already contains a sub_key with `job={job_id}` for this role (the prompt was queued before disconnection) → **skip the script's push step**; instead call `xmtp_dispatch_user` content=`{wakeup_resume}` and end the turn.\n\
             - No matching entry → run the Step 2 script normally; the `pending-decisions-v2 request` call handles the prompt.\n\n\
             ⚠️ **Do NOT** xmtp_send the User Agent something like `I'm back online` — the peer does not care about your connection status.\n\
             ⚠️ If the Step 2 script is a passive-wait kind (e.g. status=accepted: ASP is working / status=submitted: waiting for User Agent review), only emit a `task resumed` notification and end the turn; do not proactively run business actions.\n"
            )
        }

        // Negotiation relay events are only used by the buyer side; provider ignores
        Event::NegotiateReply
        | Event::NegotiateAck
        | Event::NegotiateCounter => "[System notification] negotiate_* (buyer-side negotiation relay event; not the provider's concern)\n\
             [Recommendation] Ignore; no action needed.\n".to_string(),

        Event::SwitchProvider | Event::AttachmentAdded | Event::DeliverableReceived => "[System notification] buyer-side event; not the provider's concern.\n\
             [Recommendation] Ignore; no action needed.\n".to_string(),

        // ─── user_decision_* relay router (provider-side scenes) ───
        // User-decision relays arrive as system-shaped envelopes with
        // `event = "user_decision_<source_event>"` and `message.data = <user's verbatim reply>`.
        // CLI returns a routing playbook that lists the candidate pseudo-events with
        // natural-language descriptions; the sub agent's LLM decides which one the
        // user actually meant — no hardcoded keyword tables, pure semantic mapping.
        Event::Other(ref s) if s.starts_with("user_decision_") => {
            let source = &s["user_decision_".len()..];
            let reply = data.unwrap_or("").trim();
            match source {
                "job_rejected" => format!(
                    "[User decision relay] source_event=`job_rejected`, user's verbatim reply: `{reply}`\n\n\
                     **Semantic mapping** — decide which intent the user's reply means, then call the corresponding next-action.\n\n\
                     Two options:\n\
                     \x20\x20• **`dispute_raise`** — user wants to challenge the rejection and go to arbitration (typical intents: A / 发起仲裁 / dispute / 不接受拒绝 / 我做得没问题 / 申诉 / 我要争 / file dispute / contest).\n\
                     \x20\x20• **`agree_refund`** — user accepts the refund and walks away (typical intents: B / 同意退款 / agree refund / 退款 / 算了 / 不争了 / OK refund / let it go).\n\n\
                     If the user's reply clearly maps to one of these → call:\n\
                     ```bash\n\
                     onchainos agent next-action --jobid {job_id} --event <dispute_raise|agree_refund> --role provider --agentId {agent_id}\n\
                     ```\n\
                     If the reply is **truly ambiguous** (e.g. non-committal `OK` / `sure` / `hmm` — could mean either), these are irreversible on-chain actions — **do NOT guess**. Re-ask via `pending-decisions-v2 request` with the same `--sub-key` and `--source-event job_rejected`. **`--user-content` must be localized to the user's language**. Reference (English): \"I didn't catch your reply, please clarify: A=file dispute  B=accept refund\".\n"
                ),
                "submit_deadline_warn" => format!(
                    "[User decision relay] source_event=`submit_deadline_warn`, user's verbatim reply: `{reply}`\n\n\
                     **Semantic mapping** — decide which intent the user's reply means:\n\n\
                     \x20\x20• **Submit now** — user wants to deliver immediately (typical intents: 立即提交 / 我提交 / submit now / I'll deliver / ready / 现在交). Route: call `onchainos agent next-action --jobid {job_id} --event job_accepted --role provider --agentId {agent_id}` and run its Step 2-3 (skip Step 1 apply-accepted notification — user already knows).\n\
                     \x20\x20• **Let it timeout** — user lets the deadline pass (typical intents: silence / 算了 / 不交了 / let it timeout / skip / 放弃). Route: end the turn; the chain will fire `submit_expired` and the User Agent auto-refunds.\n\n\
                     If ambiguous: re-ask via `pending-decisions-v2 request` (`--source-event submit_deadline_warn`).\n"
                ),
                "cli_failed" => format!(
                    "[User decision relay] source_event=`cli_failed`, user's verbatim reply: `{reply}`\n\n\
                     The original `onchainos agent <cmd>` failed and you asked the user how to proceed. **Semantic mapping** — decide what the user means and act accordingly (no on-chain action by default):\n\n\
                     \x20\x20• **Retry** — user wants you to re-run the same CLI command (typical intents: A / 选A / retry / 重试 / try again / 再来一次 / 再试一次). Action: re-execute the **exact same** CLI you previously ran (same args, same job_id). If it fails again, do NOT loop — enqueue **one more** `pending-decisions-v2 request --source-event cli_failed` and end the turn.\n\
                     \x20\x20• **Dismiss** — user takes manual control of this step (typical intents: B / 选B / dismiss / 不再提示 / skip prompts / 我自己处理 / let me handle it). Action: end the turn. Do not re-prompt; the user owns this step now.\n\
                     \x20\x20• **New instruction** — user gives a corrective instruction in natural language (e.g. `把 token-symbol 改成 USDT 再试` / `change --token-symbol to USDT and retry` / `用 endpoint https://... 重试`). Action: parse the modification, rebuild the CLI invocation with the user's adjustment, and execute once. Treat the result as a fresh attempt (success → continue the original scene; failure → enqueue another `cli_failed` decision).\n\n\
                     ⚠️ Do NOT execute any on-chain action that wasn't part of the original failed command — the user reply only authorizes retry/edit of the failed step, not unrelated new actions.\n\
                     ⚠️ If the reply is truly ambiguous (e.g. unrelated chitchat / a non-committal `hmm` / `got it`), re-ask via `pending-decisions-v2 request` with `--sub-key <same>` and `--source-event cli_failed`. **`--user-content` must be localized to the user's language** (detect from the user's verbatim reply / prior turn) before sending. Reference (English): \"I didn't catch your reply, please clarify: A=retry  B=stop prompting  C=tell me what to change\".\n"
                ),
                _ => format!(
                    "[User decision relay] source_event=`{source}` (no specific routing rule defined for this scene), user's verbatim reply: `{reply}`\n\n\
                     **Manual routing required** — inspect the scene context (call `onchainos agent common context {job_id} --role provider --agent-id {agent_id}` if needed) and decide semantically which pseudo-event the user's reply maps to. Then call `onchainos agent next-action --jobid {job_id} --event <chosen-pseudo-event> --role provider --agentId {agent_id}`.\n"
                ),
            }
        }

        Event::Other(ref other) => format!(
            "[Unknown state] {other}\n\
             [Recommendation]\n\
             1. Call `onchainos agent common context {job_id} --role provider --agent-id {agent_id}` to view the full context\n\
             2. If this state is not in the expected flow, wait for user instructions\n\
             3. Do NOT predict / assume other notifications\n"
        ),
    };
    // Three mutually exclusive shapes:
    //   • short shim (Event::JobCreated)         → body only; preamble lives in
    //     the cached playbook block the LLM has in context (or will fetch).
    //   • playbook emit (`job_created_playbook`) → preamble + body wrapped in
    //     the cache-marker pair so future turns can detect & reuse.
    //   • all other events                       → preamble + body, no wrap.
    if is_short_jobcreated {
        body
    } else if is_playbook_emit {
        format!(
            "[JOBCREATED_PLAYBOOK_CACHE@{job_id}]\n\
             {localization_prefix}{version_prefix}{context_preamble}{body}\n\
             [/JOBCREATED_PLAYBOOK_CACHE@{job_id}]\n"
        )
    } else {
        format!("{localization_prefix}{version_prefix}{context_preamble}{body}")
    }
}
