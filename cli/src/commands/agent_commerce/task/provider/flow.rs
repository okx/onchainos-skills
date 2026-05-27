//! Provider-side task flow driver.
//!
//! Based on the current system notification type received (jobStatus), outputs the prompt
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
        format!("**Next step (mandatory)** → `onchainos agent next-action --jobid {job_id} --jobStatus {evt} --role provider --agentId <agentId>` to fetch the full script for the current status, and **follow the script strictly**.\n  ⚠️ **Do NOT** infer CLI commands directly from the status name (apply / deliver / dispute raise / agree-refund / dispute upload, etc.) — the script typically prefixes steps such as `xmtp_prompt_user` / `xmtp_send` / `pending-decisions-v2 request`; skipping them causes incidents (this has happened before).")
    };
    match status {
        Status::Created => vec![next_action("job_created")],
        Status::Accepted => vec![next_action("job_accepted")],
        Status::Submitted => vec![
            next_action("job_submitted"),
            "(Passive wait) Awaiting User Agent review: job_completed → task complete; job_refused → enter arbitration / refund decision.".to_string(),
        ],
        Status::Refused => vec![next_action("job_refused")],
        Status::Disputed => vec![next_action("job_disputed")],
        Status::Completed => vec![
            next_action("job_completed"),
            "(Terminal state) Task COMPLETE — **funds released to you (the ASP)**".to_string(),
            "  ▸ User Agent review passed (job_completed) → escrow funds released".to_string(),
            "  ▸ Arbitration ruled in ASP's favor (dispute_resolved seller-wins) → escrow funds released".to_string(),
            "Sub session can be closed.".to_string(),
        ],
        Status::Rejected => vec![
            next_action("job_refunded"),
            "(Terminal state) Task REJECTED — **funds refunded to the User Agent**".to_string(),
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
            format!("Current task status=`{s}` is not in the provider's state set of interest (open / accepted / submitted / refused / disputed / completed / rejected / close / expired / admin_stopped)"),
            "→ No task-level action required for this role; wait for the next relevant on-chain event / user decision before acting.".to_string(),
            "→ **Do NOT** rerun `agent status` / `agent common context` (results are the same); end this turn.".to_string(),
        ],
    }
}

/// Generate the structured next-action prompt for the ASP based on jobStatus.
///
/// `job_status` accepts either an event name (provider_applied / job_accepted / ...)
/// or a status name (open / accepted / ...) — internally normalized via state_machine
/// into an `Event`; unrecognized strings fall through as `Event::Other(s)`.
pub fn generate_next_action(job_id: &str, job_status: &str, agent_id: &str) -> String {
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

    // Short jobId, used as the `[Task <shortID> you as seller]` prefix on the first
    // line of `xmtp_prompt_user` userContent. When multiple prompts run concurrently
    // it provides the user and the user agent a dual disambiguation anchor. See
    // SKILL.md Session Communication Contract §5.
    let short_id = short_job_id(job_id);

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
    // sub_key it relays back to sub via path 3
    // (`xmtp_dispatch_session(sessionKey=<sub>, [USER_DECISION_RELAY] ...)`).
    // ──────────────────────────────────────────────────────────────────────
    let send_to_peer = format!(
        "→ Call `xmtp_send` to send to the User Agent.\n\
         Params: sessionKey=<current session sessionKey, obtain via session_status (call only once per turn, reuse afterwards)>, content=<plain natural language, no markdown / code blocks>.\n\
         Current jobId={job_id}, our agentId={agent_id}.\n\
         content:"
    );

    // Shared "execute task autonomously" guidance for escrow Step 2 — the script does
    // not prescribe how to do it; list a few examples so the agent knows "pick your own
    // tool" is the expected behavior.
    let execute_task = "Pick the right tool / capability for the task content to get the work done. For example:\n\
        \x20\x20• `Generate a cat image` → call an image-generation tool, get the local file path\n\
        \x20\x20• `Check the weather` → call wttr.in / a weather API, get a text result\n\
        \x20\x20• `Audit a smart contract` → read the code, produce an audit report\n\
        Tool choice is outside the script's scope; the agent decides autonomously.\n\n\
        ⚠️ If you have questions about task details / acceptance criteria → first call `xmtp_send(sessionKey=<current session sessionKey, fetched via session_status>, content=<plain natural language question>)` to ask the User Agent for clarification, end this turn, and wait for the reply; once you have the answer, start the work. Do not guess and produce a deliverable that misses the mark.";

    // Terminal-state (completed / refunded / close / dispute_resolved, etc.) session
    // retain-vs-release policy is governed by common::config::KEEP_CONVERSATION_ON_TERMINAL —
    // change the default by modifying that const.
    let terminal_session_hint = if crate::commands::agent_commerce::task::common::config::KEEP_CONVERSATION_ON_TERMINAL {
        "ℹ️ Task is in terminal state. Clean up the stale pending decision entry but keep the conversation:\n\
         \x20\x201. Call `session_status` to fetch the current sub `sessionKey`.\n\
         \x20\x202. Run `onchainos agent pending-decisions-v2 cancel --sub-key \"<sessionKey from step 1>\"` to remove any leftover pending decision entry (otherwise it waits 7-day TTL and pollutes the queue).\n\
         ⚠️ **Do NOT call `xmtp_delete_conversation`** — keep the conversation history for later reference."
    } else {
        "ℹ️ Task is in terminal state; to fully release session resources (no follow-up events expected):\n\
         \x20\x201. Call `session_status` to fetch the current sub `sessionKey`.\n\
         \x20\x202. Run `onchainos agent pending-decisions-v2 cancel --sub-key \"<sessionKey from step 1>\"` to clean up any pending decision entry for this sub (otherwise it waits 7-day TTL).\n\
         \x20\x203. Call `xmtp_delete_conversation` with `sessionKey=<sessionKey from step 1>` to close the conversation."
    };

    // User-facing content templates for the preamble's exception-escalation hard rules
    // (single source of truth in content.rs).
    let escalation_protocol_misread = super::content::escalation_protocol_misread_notify(job_id);
    let escalation_cli_failed = super::content::escalation_cli_failed_notify(job_id);

    let context_preamble = format!(
        "🔒 If you have not read `skills/okx-agent-task/SKILL.md Session Communication Contract` in this turn → read it first before proceeding (envelope whitelist / xmtp_send two-step / xmtp_dispatch_user · xmtp_prompt_user push-to-user iron rules). The steps below reference its sections (3 / 4 / 5 / 6).\n\n\
         ⚠️ **Exception-escalation hard rules** (apply to every scene; see _shared/exception-escalation.md + provider.md §5):\n\
         \x20\x201) Protocol misread (peer keeps repeating after ≥1 clarification on the same flow) → **stop replying to the peer**, call `xmtp_dispatch_user`, content=`{escalation_protocol_misread}`, end the turn\n\
         \x20\x202) Execution error (`onchainos agent <cmd>` failed) → **do NOT retry**; enqueue an error decision via `pending-decisions-v2 request` (first `session_status` to get sessionKey if not cached this turn; `--user-content` = `{escalation_cli_failed}` localized to the user's language; `--list-label` = `[Error <short jobId>] CLI failed`). Follow the playbook the CLI returns. After receiving `[USER_DECISION_RELAY] decision: <verbatim>`, route: verbatim is `A` / `选A` / `retry` / `重试` / `try again` → re-run the same command once (if it fails again, enqueue another error decision; do NOT loop); verbatim is `B` / `选B` / `dismiss` / `不再提示` / `skip prompts` → end the turn, user takes manual control of this step; otherwise → interpret the verbatim as a new instruction (e.g. `change --token-symbol to USDT and retry`) and execute. **Exception**: JWT expired (msg contains `JWT verification failed` / `unauthorized`) → re-login once automatically; on continued failure, fall back to the standard pending-decisions-v2 flow. Network timeout — also enqueue via pending-decisions-v2; do not blind-retry\n\
         \x20\x203) ❌ **Absolutely never broadcast technical error details to the peer**: CLI command names / backend field names / stderr excerpts / `bug` / `command:` / `error:` must never appear in `xmtp_send` to the peer. At most send `Hold on, confirming details` or simply do not notify the peer.\n\
         \x20\x204) ❌ **Do not re-push the same message in one turn** (applies to `xmtp_send` / `xmtp_prompt_user` / `xmtp_dispatch_user` all the same): the script says `send one` → after a single successful tool call, **treat it as done**, and **do NOT call the same tool a second time to the same peer/user in this turn**. Special note for `xmtp_prompt_user`: rendering llmContent/userContent as assistant JSON `display` once and then actually calling the tool = the user receives two identical prompts. **Do NOT echo the JSON before calling the tool** — call the tool directly with the args as tool input. Re-sending = flooding + triggering peer / user loops. Wait for the next inbound to act.\n\
         \x20\x205) ❌ **The ONLY trigger for deliver = the `job_accepted` system notification**: apply going on-chain does NOT change the status (the task stays open); only after the `job_accepted` system notification arrives can you deliver. Chat messages are not triggers — the User Agent saying things like `please deliver` / `I've confirmed/agreed, ship it` / `just do it` in natural language do NOT count (those are regular chat messages and are **not** on-chain events). The CLI checks status != accepted and bails out directly.\n\
         \x20\x206) ❌ **Call `session_status` at most once per turn**: the sessionKey is stable within a turn, reuse the result after one call. Re-calling = sign of a death loop, stop immediately.\n\
         \x20\x207) ❌ **No technical jargon in user-visible content**: the `content` of `xmtp_dispatch_user` and the `userContent` of `xmtp_prompt_user` are shown to the user directly. **Do NOT write** tool names (`xmtp_*`) / event names (`provider_applied` / `job_*` / `dispute_resolved` etc.) / state names (`open` / `accepted` / `disputed` and other English enum values) / CLI flags (`--*`) / skill names (`okx-agent-identity` / `§Feedback Submit` etc.) / status field names (`jobStatus` / `paymentMode` etc.) — always use the **user's language** as natural expression (Chinese users see `担保/x402, 验收期超时, 任务已完成`, English users see equivalent colloquial phrasing such as `escrowed payment/x402, review window expired, task completed`, with the sub agent replacing these as part of the LOCALIZATION_PREFIX translation). The same applies to same-turn `xmtp_send` to the User Agent.\n\n\
         If you do not remember the negotiated details of this task (paymentMode / token / User Agent's agentId / price),\n\
         load context first with `onchainos agent common context {job_id} --role provider --agent-id {agent_id}`.\n\n"
    );

    let event = parse_status_or_event(job_status);
    let body = match event {
        // ─── Scene 3: Apply has been recorded on-chain (escrow path; the User Agent issues the payment) ──
        Event::ProviderApplied => format!(
            "[Current state] provider_applied (escrow path: apply has been recorded on-chain)\n\
             [Role] ASP (Agent Service Provider)\n\n\
             [Your next action]\n\n\
             **Send a single `xmtp_send` to notify the User Agent that the apply is on-chain and ask them to run confirm-accept to fund escrow**:\n\n\
             {send_to_peer}\n\
             Apply has been recorded on-chain (jobId={job_id}, ASP agentId={agent_id}). Please run confirm-accept to fund escrow.\n\
             [intent:applied]\n\n\
             ⚠️ **Do NOT call `onchainos agent deliver` at this stage**: current status is still open (apply on-chain does not change the status); you can only deliver once the User Agent has confirm-accepted and the `job_accepted` notification has arrived. The CLI has a guard that bails out directly.\n\n\
             After xmtp_send → **end this turn immediately**, wait for the `job_accepted` notification.\n\n\
             [Follow-up events]\n\
             - job_accepted → User Agent has confirm-accepted, escrow funding complete; **only then** can you deliver\n"
        ),

        // ─── Scene 4: User Agent has confirmed the apply; execute and deliver (branch by paymentMode) ──
        Event::JobAccepted => {
            let user_notify = super::content::job_accepted_user_notify(job_id, agent_id);
            let deliver_text = super::content::deliver_text_to_buyer(job_id);
            let deliver_file = super::content::deliver_file_to_buyer(job_id);
            format!(
            "[Current state] job_accepted (User Agent has confirmed the apply; escrow funded)\n\
             [Role] ASP (Agent Service Provider)\n\n\
             [Your next action (strict order, do not skip steps)]\n\n\
             **Step 1 — Use `xmtp_dispatch_user` to push the apply-accepted notification to the user**:\n\n\
             🌐 **Localize first** — rewrite `content` below in the user's language before sending (mandatory; see LOCALIZATION_PREFIX at top of this output). Do NOT pass the English template verbatim to a non-English user.\n\
             tool: xmtp_dispatch_user\n\
             content:\n\
             {user_notify}\n\n\
             Field values are read from the output of `onchainos agent common context {job_id} --role provider --agent-id {agent_id}`.\n\
             ⚠️ Do NOT send `xmtp_send` `received apply confirmation` filler to the User Agent — the User Agent just ran confirm-accept; they already know.\n\n\
             **Step 2 — Autonomously execute the task and prepare the deliverable**:\n\
             {execute_task}\n\n\
             **Step 3 — Branch by paymentMode for delivery** (you MUST first call `onchainos agent common context {job_id} --role provider --agent-id {agent_id}` to confirm paymentMode):\n\n\
             ━━━━━ Branch A: paymentMode=escrow (escrow trade, 1) ━━━━━\n\n\
             ⚠️ **Order**: first `xmtp_send` the deliverable to the User Agent, then deliver on-chain. The on-chain deliver only advances the task state to submitted (giving the User Agent an acceptance entry point); the deliverable itself was already delivered via xmtp_send.\n\n\
             **A-Step 1 — Prepare the deliverable (branch by type)**:\n\n\
             ▸ **Plain text / URL deliverable**: assemble the text content directly, skip xmtp_file_upload, go to A-Step 2.\n\n\
             ▸ **File deliverable** (image / PDF / document): call `xmtp_file_upload` (mechanism: see skills/okx-agent-task/SKILL.md Session Communication Contract §4.8):\n\
             \x20\x20Params `filePath` = absolute local file path, `agentId` = {agent_id}, `jobId` = {job_id}\n\
             \x20\x20Record all five return fields (`fileKey` / `digest` / `salt` / `nonce` / `secret` — decryption metadata)\n\n\
             **A-Step 2 — `xmtp_send` the deliverable to the User Agent** (in the same turn, immediately following A-Step 1):\n\
             ⚠️ content **MUST end with the `[intent:deliver]` line as a trailing suffix** — the User Agent routes by this suffix to recognize the deliverable. Missing suffix = the User Agent cannot recognize it as a deliverable = the flow stalls.\n\n\
             Text-deliverable content:\n\
             {send_to_peer}\n\
             {deliver_text}\n\n\
             File-deliverable content (paste all 5 fields verbatim):\n\
             {send_to_peer}\n\
             {deliver_file}\n\n\
             **A-Step 3 — Run `deliver` CLI to go on-chain** (advances task state to submitted so the User Agent gets the complete entry point):\n\
             ```bash\n\
             onchainos agent deliver {job_id} --file \"\" --message \"Task completed, please review\" --agent-id {agent_id}\n\
             ```\n\
             CLI internals: POST submit API → sign uopHash → broadcast on-chain.\n\n\
             **A-Step 4 — After A-Step 3 ends this turn immediately** (the deliverable was already delivered to the User Agent in A-Step 2; when the subsequent `job_submitted` notification arrives, **observe only** — do not xmtp_send / xmtp_dispatch_user / any filler message).\n\n\
             [Follow-up events]\n\
             - On-chain task state enters submitted (the job_submitted system event may arrive; observe only, do not act) → wait for buyer complete/reject\n"
            )
        }

        // ─── Scene 5: Deliverable confirmed on-chain (observer-only) ──────────────────
        // In the new flow the deliverable was already sent to the User Agent via xmtp_send
        // in Scene 4 A-Step 2; when the job_submitted system event reaches this sub there
        // is no need to xmtp_send again, to avoid the User Agent receiving duplicate messages.
        Event::JobSubmitted => format!(
            "[System notification] job_submitted (deliverable confirmed on-chain; task state is now submitted)\n\
             [Role] ASP (Agent Service Provider)\n\n\
             ⚠️ **observer-only**: the deliverable was already sent to the User Agent in the `job_accepted` script (A-Step 2); this event **must NOT trigger a second xmtp_send** — duplicating would cause the User Agent to receive double messages and trigger a loop.\n\n\
             [Your next action]\n\
             - **Just observe silently**; do NOT call xmtp_send / xmtp_file_upload / xmtp_dispatch_user / xmtp_prompt_user\n\
             - **End this turn directly**; wait for the User Agent to complete/reject and trigger the next event\n\n\
             [Follow-up events]\n\
             - Received `job_completed` (review passed) → `onchainos agent next-action --jobid {job_id} --jobStatus job_completed --role provider --agentId {agent_id}`\n\
             - Received `job_refused`   (User Agent rejected) → `onchainos agent next-action --jobid {job_id} --jobStatus job_refused --role provider --agentId {agent_id}`\n"
        ),

        // ─── Scene 6: User Agent rejected the deliverable ─────────────────────────────────
        Event::JobRefused => {
            let user_prompt = super::content::job_refused_user_decision_prompt(&short_id);
            format!(
            "[Current state] job_refused (User Agent rejected the deliverable)\n\
             [Role] ASP (Agent Service Provider)\n\n\
             🛑🛑🛑 **ABSOLUTE REQUIREMENT — you MUST push the decision (dispute vs refund) to the user via `pending-decisions-v2 request` (NOT a plain text reply, NOT just `xmtp_dispatch_user`)**.\n\
             `xmtp_dispatch_user` is a pure notification: user replies cannot be relayed back to the sub session → the decision flow deadlocks. The correct flow handles this via `pending-decisions-v2 request` → CLI playbook → `xmtp_prompt_user` (with llmContent + userContent) so the user session can relay the decision back. Direct text output in this sub session = user doesn't see it + relay channel broken + 24h timeout → auto-refund.\n\
             ❌ Do not substitute a plain text reply for the `pending-decisions-v2 request` call.\n\
             ❌ Do not substitute `xmtp_dispatch_user` for the `pending-decisions-v2 request`.\n\
             ⚠️ Do NOT send `xmtp_send` `received the rejection` filler to the User Agent — they just rejected; they know. Go straight to the user-decision flow.\n\n\
             **Step 1 — Enqueue the decision via `pending-decisions-v2 request`**:\n\n\
             First call `session_status` to get the current sessionKey (only once per turn). Then run:\n\
             ```bash\n\
             onchainos agent pending-decisions-v2 request \\\n\
               --sub-key \"<full sessionKey from session_status>\" \\\n\
               --job-id {job_id} --role provider --agent-id {agent_id} \\\n\
               --user-content \"{user_prompt_for_shell}\" \\\n\
               --list-label \"[Decision] Dispute / Agree Refund\"\n\
             ```\n\
             🌐 Translate `--user-content` AND `--list-label` to the user's language (signal = user's OWN typed messages this session; default English if unsure). See [Localization] above for token mapping.\n\n\
             Follow the playbook the CLI returns verbatim, then end the turn. Do NOT manually construct `llmContent` / call `xmtp_dispatch_session` yourself — that path is owned by `pending-decisions-v2` now.\n\n\
             **Step 2 — After receiving `[USER_DECISION_RELAY] decision: <user verbatim>` from the user-session**:\n\
             Inspect the verbatim text and route to the next on-chain action (treat the match as case-insensitive; trim surrounding whitespace / punctuation before matching):\n\
             - Verbatim is exactly `A` / `a` / `选A` / `选a` / `我选A` / `Choose A` / `option A`, OR contains `发起仲裁` / `dispute` / `start arbitration` / `不接受` / `我做的没问题` → call `onchainos agent next-action --jobid {job_id} --jobStatus dispute_raise --role provider --agentId {agent_id}` (extract the reason from the verbatim after `理由` / `reason` / `因为`; if not stated — e.g. user only typed `A` — default to \"completed per acceptance criteria\")\n\
             - Verbatim is exactly `B` / `b` / `选B` / `选b` / `我选B` / `Choose B` / `option B`, OR contains `同意退款` / `agree refund` / `refund OK` / `退款` → call `onchainos agent next-action --jobid {job_id} --jobStatus agree_refund --role provider --agentId {agent_id}`\n\
             - Otherwise (user replied something unrelated) → call `pending-decisions-v2 request` again with a clarifying userContent (\"您刚才回复 「<verbatim>」我没理解,请回复 「发起仲裁,理由:<...>」 或 「同意退款」 或 直接回复 A / B\") to re-ask.\n\n\
             ⚠️ Decision must be made within 24h; otherwise funds are auto-refunded to the User Agent.\n",
                user_prompt_for_shell = user_prompt.replace('\'', "'\\''"),
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
             onchainos agent dispute confirm {job_id} --agent-id {agent_id}\n\
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
            format!(
            "[Current state] job_completed (task completed; funds received)\n\
             [Role] ASP (Agent Service Provider)\n\n\
             ⚠️ Differences in fund-arrival paths (for the agent's understanding, no need to spell this out to the user):\n\
             \x20\x20• escrow → escrow contract auto-releases stake to your wallet\n\
             \x20\x20• x402 → the User Agent paid via x402 signature during the accept phase\n\
             Either path means funds have landed; when notifying the user simply say `funds received`.\n\n\
             [Your next action]\n\n\
             ⚠️ Do NOT send `xmtp_send` thanks / `done` filler to the User Agent — they just completed; they know.\n\n\
             **Step 1 — Load task context**:\n\
             ```bash\n\
             onchainos agent common context {job_id} --role provider --agent-id {agent_id}\n\
             ```\n\
             Extract title + tokenAmount + tokenSymbol + buyerAgentId (needed for the next step).\n\n\
             **Step 2 — Use `xmtp_dispatch_user` to notify the user of task completion + a light rating nudge**:\n\n\
             🌐 **Localize first** — rewrite `content` below in the user's language before sending (mandatory; see LOCALIZATION_PREFIX at top of this output). Do NOT pass the English template verbatim to a non-English user.\n\
             tool: xmtp_dispatch_user\n\
             content:\n\
             {user_notify}\n\n\
             **Step 3 — Terminal wrap-up (keep the sub session):**\n\
             {terminal_session_hint}\n\
             ⚠️ **Do not auto-rate** — the notification above already includes a rating nudge; wait for the user to reply with their rating.\n\
             When the user replies with a rating intent, ask for a score (0–5 integer) and optional text feedback if not already provided, then execute:\n\
             ```bash\n\
             onchainos agent feedback-submit --agent-id <buyerAgentId> --creator-id {agent_id} --score <0-5> --task-id {job_id} [--description \"<optional text>\"]\n\
             ```\n\
             ⚠️ `--score` MUST come from the user's explicit reply in this rating flow; do NOT infer from verbs like `rate` / `打分`, do NOT use a default value.\n\
             ⚠️ `--agent-id` is the User Agent being rated (buyerAgentId from Step 1 context); `--creator-id` is the provider's own agent id ({agent_id}).\n\
             Task fully complete.\n"
            )
        }

        // ─── Scene 6.5: Arbitration ruling (won / lost branches distinguished by jobStatus in the inbound envelope) ─
        Event::DisputeResolved => {
            let dispute_won_claim = super::content::dispute_won_with_claim_user_notify(job_id);
            let dispute_won_no_claim = super::content::dispute_won_no_claim_user_notify(job_id);
            let dispute_lost = super::content::dispute_lost_user_notify(job_id);
            format!(
            "[Current state] dispute_resolved (arbitration ruling delivered)\n\
             [Role] ASP (Agent Service Provider)\n\n\
             ⚠️ **Determining win/loss**: read `message.jobStatus` from the system notification envelope you just received:\n\
             - `jobStatus = \"complete\"` → **you (provider) won**; funds released to you\n\
             - `jobStatus = \"rejected\"` → **you (provider) lost**; funds refunded to the User Agent\n\
             [Your next action (branch by win/loss)]\n\n\
             ⚠️ Do NOT send `xmtp_send` `ruling supports party X` filler to the User Agent — both sides receive the `dispute_resolved` system event.\n\n\
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
             From `onchainos agent common context {job_id} --role provider --agent-id {agent_id}` get task title + tokenAmount + tokenSymbol + buyerAgentId.\n\
             ⚠️ content is the **chat the user will see** — plain natural language; **do NOT use** skill names / event names / state names / CLI flags or other technical jargon.\n\
             🌐 **Localize first** — rewrite `content` below in the user's language before sending (mandatory; see LOCALIZATION_PREFIX at top of this output). Do NOT pass the English template verbatim to a non-English user.\n\
             tool: xmtp_dispatch_user\n\
             content (choose based on whether A-Step 2 actually claimed):\n\
             \x20\x20Claimed:\n\
             {dispute_won_claim}\n\
             \x20\x20Nothing to claim:\n\
             {dispute_won_no_claim}\n\n\
             **A-Step 4 — Wait for user rating (do not auto-rate):**\n\
             The notification above includes a rating nudge. When the user replies with a rating intent, ask for a score (0–5 integer) and optional text feedback if not already provided, then execute:\n\
             ```bash\n\
             onchainos agent feedback-submit --agent-id <buyerAgentId> --creator-id {agent_id} --score <0-5> --task-id {job_id} [--description \"<optional text>\"]\n\
             ```\n\
             ⚠️ `--score` MUST come from the user's explicit reply in this rating flow; do NOT infer from verbs like `rate` / `打分`, do NOT use a default value.\n\
             ⚠️ `--agent-id` is the User Agent being rated (buyerAgentId from A-Step 3 context); `--creator-id` is the provider's own agent id ({agent_id}).\n\n\
             ━━━━━━━━━━━━━ Branch B: jobStatus=rejected (ASP lost) ━━━━━━━━━━━━━\n\n\
             **B-Step 1 — Use `xmtp_dispatch_user` to notify the user of the loss**:\n\n\
             From `onchainos agent common context {job_id} --role provider --agent-id {agent_id}` get task title + tokenAmount + tokenSymbol + buyerAgentId.\n\
             ⚠️ Same as A-Step 3 — content plain natural language; no technical jargon.\n\
             🌐 **Localize first** — rewrite `content` below in the user's language before sending (mandatory; see LOCALIZATION_PREFIX at top of this output). Do NOT pass the English template verbatim to a non-English user.\n\
             tool: xmtp_dispatch_user\n\
             content:\n\
             {dispute_lost}\n\n\
             **B-Step 2 — Wait for user rating (do not auto-rate)** — same `feedback-submit` flow as A-Step 4 (`--agent-id <buyerAgentId>`, `--creator-id {agent_id}`); end the turn after the notification.\n\n\
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

        // ─── Scene 6.4: Arbitration on-chain; need user-provided evidence ───────────────────
        Event::JobDisputed => {
            let user_prompt = super::content::job_disputed_user_evidence_prompt(&short_id);
            format!(
            "[Current state] job_disputed (arbitration is on-chain; entering 1-hour evidence preparation window)\n\
             [Role] ASP (Agent Service Provider)\n\n\
             🛑🛑🛑 **ABSOLUTE REQUIREMENT — you MUST push the evidence request to the user via `pending-decisions-v2 request` (NOT a plain text reply, NOT just `xmtp_dispatch_user`)**.\n\
             `xmtp_dispatch_user` is a pure notification: user replies cannot be relayed back to the sub session → evidence cannot be collected → 1-hour window expires → arbiter rules on partial / missing evidence. The correct flow handles this via `pending-decisions-v2 request` → CLI playbook → `xmtp_prompt_user` so the user session can relay the evidence back.\n\
             ❌ Do not substitute a plain text reply for the `pending-decisions-v2 request` call.\n\
             ❌ Do not substitute `xmtp_dispatch_user` for the `pending-decisions-v2 request`.\n\
             ⚠️ **Evidence content MUST come from the user** — this agent doesn't know what evidence the user has (screenshots, chat logs, deliverable links, etc.), do NOT fabricate an evidence summary and call `dispute upload` blindly. **Push the decision request to the user first and let the user decide**.\n\n\
             ⚠️ Do NOT send `xmtp_send` `arbitration on-chain, preparing evidence` filler to the User Agent — both sides already receive the `job_disputed` system event.\n\n\
             **Step 1 — Enqueue the evidence decision via `pending-decisions-v2 request`**:\n\n\
             First call `session_status` to get the current sessionKey (only once per turn). Then run:\n\
             ```bash\n\
             onchainos agent pending-decisions-v2 request \\\n\
               --sub-key \"<full sessionKey from session_status>\" \\\n\
               --job-id {job_id} --role provider --agent-id {agent_id} \\\n\
               --user-content \"{user_prompt_for_shell}\" \\\n\
               --list-label \"[Decision {short_id}] Submit Arbitration Evidence\"\n\
             ```\n\
             🌐 Translate `--user-content` AND `--list-label` to the user's language (signal = user's OWN typed messages this session; default English if unsure). See [Localization] above for token mapping.\n\n\
             Follow the playbook the CLI returns verbatim, then end the turn. Do NOT manually construct `llmContent` / call `xmtp_dispatch_session` yourself — that path is owned by `pending-decisions-v2` now.\n\n\
             **Step 2 — After receiving `[USER_DECISION_RELAY] decision: <user verbatim>` from the user-session**:\n\
             The user's reply IS the evidence — upload it verbatim. Do NOT second-guess whether it's \"too short\" / \"too similar to the dispute reason\" / \"not enough detail\"; if the user wants to add more, they will reply again (each new reply overwrites and re-prompts the same pending entry).\n\
             Call `onchainos agent next-action --jobid {job_id} --jobStatus dispute_evidence --role provider --agentId {agent_id}` for the upload script, and pass the verbatim text + any image paths the user provided through to the upload step.\n\n\
             ⚠️ Must submit evidence within 1 hour; expires after that.\n",
                user_prompt_for_shell = user_prompt.replace('"', "\\\""),
            )
        }

        // ─── Scene 6.4b: User has provided evidence content (user-instruction pseudo-event) ──
        Event::Other(ref s) if s == "dispute_evidence" => format!(
            "[Current action] Upload arbitration evidence\n\
             [Role] ASP (Agent Service Provider)\n\n\
             **Step 1 — Extract evidence content from the user's relay:**\n\
             Routed in via `[USER_DECISION_RELAY] decision: <user verbatim>`. The verbatim text IS the evidence — extract:\n\
             - Text summary → the text portion the user wrote\n\
             - Image path (if the user provided a local file path) → `--image` parameter\n\
             **At least one** of text and image is required.\n\n\
             **Step 2 — Fetch negotiation / delivery chat history to attach as objective evidence at the head of text:**\n\
             Call `xmtp_get_conversation_history` with sessionKey=<current sessionKey, fetched via session_status> to retrieve the full a2a-agent-chat history with the User Agent.\n\
             Splice the history as the following **structured segmented block** at the front of the `--text` field (the arbitrator is an LLM and will read the text field in full); then append the user summary:\n\n\
             ```\n\
             ==== Negotiation / delivery chat history (from xmtp_get_conversation_history) ====\n\
             [time] User Agent(<agentId>): ...\n\
             [time] ASP(<agentId>): ...\n\
             ... (in chronological order; key checkpoints: User Agent inquiry / [intent:propose] / your [intent:ack] / User Agent [intent:confirm] / your deliver message)\n\n\
             ==== User evidence summary ====\n\
             <user's original summary>\n\
             ```\n\n\
             ⚠️ **`--text` limit is 16 KB** — if the chat history is too long **keep only** the key checkpoints (PROPOSE / ACK / CONFIRM / deliverable / each side's key contention points), and prepend `(key checkpoints only)` to mark truncation; do NOT just chop the first N messages.\n\n\
             **Step 3 — Call the CLI to upload evidence (off-chain multipart):**\n\
             ```bash\n\
             onchainos agent dispute upload {job_id} --agent-id {agent_id} --text \"<chat history + user summary spliced into the full text>\" --image <user-provided image path or omit>\n\
             ```\n\
             At least one of text and image; image can be omitted entirely by leaving out `--image`; do not pass an empty string.\n\n\
             [Follow-up events]\n\
             - job_completed → won, funds released to the ASP\n\
             - dispute_resolved → lost, funds refunded to the User Agent\n\n\
             After Steps 1-3 → **end this turn directly**.\n\
             ⚠️ Do NOT send `xmtp_send` `evidence submitted` filler to the User Agent — both sides are uploading evidence in parallel; cross-notifying has no value; the arbitration result is delivered to both sides via the `dispute_resolved` system event.\n\
             ⚠️ Do NOT push to the user with `xmtp_dispatch_user`.\n"
        ),

        // ─── Unknown type fallback ─────────────────────────────────────────────
        Event::JobCreated => format!(
            "[Current state] job_created (task is on-chain)\n\
             [Role] ASP (Agent Service Provider)\n\n\
             ⚠️ **Negotiation phase; do NOT call `onchainos agent apply` directly**: apply is an on-chain action (requires gas, signing, broadcast),\n\
             and a failed negotiation cannot be undone. You MUST complete all three confirmations of negotiation below before apply.\n\n\
             🛑 **Hard constraint — three-step handshake + no `onchainos` CLI in the same turn after an `xmtp_send`**\n\n\
             Negotiation MUST complete the three-step handshake in full (iron rule of the buyer protocol; enforced in the User Agent's code):\n\
             \x20\x201) `[intent:propose]` (buyer → provider)\n\
             \x20\x202) `[intent:ack]` or `[intent:counter]` (provider → buyer) or `[intent:reject]` (either side rejects)\n\
             \x20\x203) `[intent:confirm]` (buyer → provider, echoing all fields verbatim)\n\
             \x20\x20⚡ `[intent:reject]` is **soft-terminal**: sending it ends THIS round (do not auto-reply / do not chase the other side after sending). **But the negotiation thread is NOT permanently closed**:\n\
             \x20\x20\x20\x20- If the OTHER side comes back with a NEW `[intent:propose]` (materially different terms — e.g. higher price after you rejected a low one), treat it as **negotiation reopened**: call `next-action --jobStatus job_created` again, re-evaluate the new fields, and proceed with the normal Propose → Ack → Confirm flow (ACK if acceptable, COUNTER if still off, [intent:reject] again only if still unacceptable).\n\
             \x20\x20\x20\x20- If the other side just sends natural-language follow-up (e.g. \"can you reconsider 0.5 USDT?\") after your reject, you may reply naturally and continue Step 3 first-round negotiation; the prior [intent:reject] does NOT mean ignore them forever.\n\
             \x20\x20\x20\x20- The thread is only **truly dead** when BOTH sides have sent [intent:reject] AND no follow-up arrives, OR when the chain emits `job_closed` / `job_expired`.\n\n\
             apply can only run **after `[intent:confirm]` has been received** (any other inbound does NOT count — including the three questions, the free-form invitation, the User Agent's `agree / accept` natural-language reply, and even the User Agent natural-language `please apply`).\n\n\
             In other words, **the inbound you received in the same turn determines what you can do**:\n\
             \x20\x20• Received a free-form invitation from the User Agent → you can only `xmtp_send` the three questions (Step 3 below); **do NOT apply**\n\
             \x20\x20• Received the User Agent's `[intent:propose]` → you can only `xmtp_send` `[intent:ack]` (Step 3.5 below); **do NOT apply**\n\
             \x20\x20• Received the User Agent's `[intent:confirm]` → verify the fields match, then go to Step 4 to run `apply`\n\
             \x20\x20• You did NOT see the literal `[intent:confirm]` → **never apply**, no matter what the User Agent said in natural language\n\n\
             🛑🛑🛑 **HARDSTOP — what counts as `received [intent:confirm]`**: the ONLY valid evidence is an **actual inbound a2a-agent-chat envelope in this turn's tool_result**, whose `content` field literally contains `[intent:confirm]` AND whose `sender.role == 1`. **Your own thinking / narration / assistant text saying things like `Buyer sent [intent:confirm]` or `received confirm` does NOT count as receiving anything** — that is text you wrote about something you anticipated; it is not an actual inbound. After sending `[intent:ack]`, you MUST end the turn and wait for the NEXT inbound; do NOT predict / pre-declare / pre-narrate that the User Agent's `[intent:confirm]` has arrived, and definitely do NOT call `apply` based on that prediction. If in this turn's tool_result there is no a2a-agent-chat inbound whose content literally contains `[intent:confirm]`, **apply is forbidden** — full stop, no exceptions. Violating this rule = on-chain `apply` based on a hallucinated handshake = polluted state machine + possible escrow loss (this has caused a live incident).\n\n\
             ❌ **Specifically forbidden**: do NOT write self-confirming phrasing such as `I confirm the three items / three items confirmed / I will apply immediately` in the content of `xmtp_send` with the three questions — the three are questions to **ask** the User Agent, not for you to confirm and then immediately apply. Such self-confirmation tricks you into thinking negotiation is done, skipping the [intent:propose]/[intent:ack]/[intent:confirm] handshake and applying illegally (this has caused a live incident).\n\n\
             🛑 **Negotiation-phase iron rule — strictly no producing work content** (between receiving the User Agent's inquiry and receiving [intent:confirm])\n\n\
             ❌ **Do NOT call external tools to produce work content**: during negotiation, do NOT call wttr.in / image generation / any query API / web search / etc. that actually executes the task. Task execution is **ONLY** allowed after the `job_accepted` system notification arrives and you enter Step B of the JobAccepted script.\n\n\
             ❌ **xmtp_send strictly forbids `delivered` phrasing**: in negotiation, `xmtp_send` may only contain one of the three:\n\
             \x20\x20• Natural-language negotiation on the three items (capability / price / paymentMode stance; questions allowed)\n\
             \x20\x20• Literal `[intent:ack]` / `[intent:counter]` / `[intent:reject]` format\n\
             Strictly do NOT write phrases like `Status: ✅ Delivered / data provided / please confirm and pay / here is what you asked for` — these mislead the User Agent into skipping confirm-accept and completing directly.\n\n\
             ❌ **Do NOT be led on by the User Agent's natural language**:\n\
             \x20\x20• User Agent says `escrow / 担保` = **paymentMode on-chain config description** (state-machine semantics), **NOT a command to deliver immediately**\n\
             \x20\x20• User Agent says `please quote / estimated delivery time` = **inquiry**, NOT a request for the final work product\n\
             \x20\x20• User Agent says `I'm in a rush / just do it for me` → still follow the protocol handshake; **do NOT skip negotiation**\n\n\
             📋 **Error-pattern case studies** (all real incidents; do not repeat):\n\n\
             ❌ Case 1: User Agent sends `Check the weather in Changsha; escrow payment`\n\
             \x20\x20Wrong: provider calls wttr.in directly → xmtp_send full weather table + writes `Status: delivered`\n\
             \x20\x20Right: Step 3 natural language: `I can do this task; workload at 0.01 USDG is reasonable; escrow OK. Please send [intent:propose] to lock parameters.`\n\n\
             ❌ Case 2: User Agent sends `I'm in a rush; just do it for me`\n\
             \x20\x20Wrong: agent thinks `the user is urgent` and skips negotiation to do the work\n\
             \x20\x20Right: reply `Understood the urgency, but the contract protocol requires sending [intent:propose] first to lock parameters; takes only 2 minutes`\n\n\
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
             \x20\x20\x20\x20\x20\x20Delivery time ~N minutes. paymentMode preference: escrow (more stable; funds in custody). Please send [intent:propose] to lock parameters.`\n\n\
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
             A-Step 1 (only when YOU initiate): call `xmtp_start_conversation` to create the group and the session:\n\
             \x20\x20Params: myAgentId={agent_id}, toAgentId=<task.buyerAgentId> (from context), jobId={job_id}\n\
             \x20\x20On success returns sessionKey + xmtpGroupId.\n\n\
             A-Step 2: once the group exists (whether YOU created it in A-Step 1 or the User Agent created it earlier), **fall through directly to Step 3 below to run the first negotiation round** (Step 3 ends with the full `xmtp_send` signature + content guidance).\n\n\
             ━━━━━━━━━ Branch B: visibility = Private (visibility=1) — passive wait ━━━━━━━━━\n\n\
             B-Step 1: **Do NOT create the group proactively**. Wait for the User Agent's a2a-agent-chat envelope to arrive first (only the User Agent has permission to designate a provider).\n\
             \x20\x20End this turn; wait for the next inbound to arrive, then run Step 3 (three-item negotiation).\n\
             \x20\x20(If you're already inside an inbound a2a-agent-chat-triggered session, skip B-Step 1 and go straight to Step 3.)\n\n\
             ━━━━━━━━━ Shared: professional matching judgment ━━━━━━━━━\n\n\
             Look at the `Professional matching check` block in context:\n\
             - Domain match → go to Step 3 (private task: wait for User Agent first; public task: A-Step 2 proactive send)\n\
             - Domain mismatch → call `xmtp_send(sessionKey=<current session sessionKey, fetched via session_status>, content=<rejection template provided by the `Professional matching check` block in context, plain natural language>)`, end the turn\n\n\
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
             💰 **Iron rule for pricing — you are the SELLER; your goal is `the higher the price the better`**:\n\n\
             ⚠️ **ASYMMETRIC rule (do NOT apply ±30% symmetrically)** — the seller's reaction depends on which SIDE of registration price the User Agent's offer lands on:\n\n\
             \x20\x20**Registration price NON-ZERO** (e.g. `registration price 1 USDT (anchor for negotiation)`):\n\
             \x20\x20\x20\x20a) User Agent's offer **≥ registration price** → **ACCEPT directly** (or send [intent:ack] when received as [intent:propose]). You are the seller; a higher offer = more profit. **NEVER counter DOWN.** 🔴 Real incident: registration 1 USDT, User Agent offered 2 USDT, provider's agent applied a symmetric `±30%` rule and countered DOWN to 1.3 USDT → wasted negotiation rounds and lost ~0.7 USDT profit. The agent should have ACK'd 2 USDT immediately.\n\
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
             \x20\x20`I can do this; acceptance criteria are fine. 0.1 USDT 比我注册价 1 USDT 低不少;基于工作量我可以做到 0.7 USDT,escrowed 支付适合避免后续争议。如同意请发 [intent:propose] 锁定参数。`\n\n\
             ⚠️ Counter-offer reference: within service-list unit price × (1 ± 30%) usually goes through; absurd quotes (× 5+) get you swapped out by the User Agent directly.\n\n\
             🛑🛑🛑 **Anti-pattern — do NOT abandon negotiation after one low offer**: 🔴 real incident — registered price 1 USDT, User Agent's first offer 0.1 USDT → provider sent `[intent:reject]` and walked away → User Agent later counter-offered 0.5 USDT and then 1 USDT but provider's agent thought \"I already rejected, conversation over\" and stayed silent → task stuck. **Correct behavior**: counter with YOUR floor price in natural language, end the turn, wait for the User Agent's next message. If the User Agent's next message has a new price (whether higher / same / lower) — even after you sent natural-language refusal earlier — you MUST call `next-action --jobStatus job_created` again and re-evaluate. \"I refused in natural language\" or \"my desired price wasn't met yet\" is NOT a reason to ignore the User Agent's follow-up — only literal `[intent:reject]` from EITHER side terminates negotiation.\n\n\
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
             **Decision criteria (asymmetric — you are the seller; higher price = better):**\n\
             \x20\x20• User Agent's tokenAmount **≥ your expectation** (or ≥ registration price) → **ACK directly**; do NOT counter down. Higher offer = more profit; accept and lock the deal.\n\
             \x20\x20• User Agent's tokenAmount is **slightly below your expectation** (within ~10% gap) and paymentMode has no hard conflict → can ACK if acceptable, OR COUNTER UP one more round (your call).\n\
             \x20\x20• User Agent's tokenAmount is **materially below your expectation** (>10% gap; User Agent did not adopt your Step 3 counter / counter margin too small) → COUNTER UP and keep negotiating; do NOT reluctantly ACK and accept a bad deal.\n\
             \x20\x20• paymentMode is opposite to the preference you expressed in Step 3, and amount is non-trivial → COUNTER to change paymentMode.\n\n\
             🛑 **Reminder: NEVER counter DOWN from a high offer**. If the User Agent gives more than you asked for, that is the deal closing — ACK immediately. Countering down here is a bug pattern; one real incident lost ~0.7 USDT this way (see Step 3 iron rule above).\n\n\
             ▸ **Agree to everything** → call xmtp_send to reply with **[intent:ack]** (you MUST use this format strictly, echoing every field verbatim):\n\
             \x20\x20content=\n\
             \x20\x20jobId: <exactly as in PROPOSE>\n\
             \x20\x20paymentMode: <exactly as in PROPOSE>\n\
             \x20\x20tokenSymbol: <exactly as in PROPOSE>\n\
             \x20\x20tokenAmount: <exactly as in PROPOSE>\n\
             \x20\x20[intent:ack]\n\n\
             ▸ **Partial disagreement** (e.g. price too low) → call xmtp_send to reply with **[intent:counter]** (fill in your desired values):\n\
             \x20\x20content=\n\
             \x20\x20jobId: <same as PROPOSE>\n\
             \x20\x20paymentMode: <unchanged if you agree; your version if you disagree>\n\
             \x20\x20tokenSymbol: <unchanged if you agree; your desired symbol if you disagree>\n\
             \x20\x20tokenAmount: <your desired amount>\n\
             \x20\x20reason: <brief explanation of the change>\n\
             \x20\x20[intent:counter]\n\n\
             ▸ **Full rejection** → call xmtp_send to send `[intent:reject]` to end negotiation:\n\
             \x20\x20content=\n\
             \x20\x20jobId: <same as PROPOSE>\n\
             \x20\x20reason: <brief reason for rejection, e.g. `price below cost`, `cannot meet the delivery deadline`>\n\
             \x20\x20[intent:reject]\n\
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
             \x20\x20• Any field differs → treat as tampering; call xmtp_send to reply `[intent:confirm] fields disagree with [intent:ack], rejected` (point out which field is wrong); **do NOT apply**; end\n\n\
             🛑 **After [intent:confirm] fields fully match, only perform the business action in Step 4; strictly do NOT xmtp_send any ACK / thanks / P2P message to the User Agent** —\n\
             \x20\x20• escrow path: run apply CLI → end the turn directly (wait for the provider_applied notification)\n\
             \x20\x20• The User Agent runs confirm-accept immediately after sending [intent:confirm], not waiting for your ACK; your ACK would conversely trigger a User Agent loop + the `no repeated xmtp_send within one turn` iron rule.\n\n\
             ⚠️ Do NOT treat the User Agent's natural-language `agreed / OK / please apply` as [intent:confirm] — only literal messages carrying the `[intent:confirm]` marker count; anything else is treated as incomplete negotiation.\n\n\
             🛑 **Protocol literal whitelist**: `[intent:*]` has exactly 5 legal values — `[intent:propose]` / `[intent:ack]` / `[intent:counter]` / `[intent:confirm]` / `[intent:reject]`. **Strictly do NOT invent**: `[intent:confirm_ack]` / `[intent:confirm_ok]` / `[intent:done]` / `[confirm_ack]` etc. are hallucinations; the User Agent's code does not recognize them, and sending them pollutes the conversation history. `[intent:confirm]` **has no corresponding ACK** (unlike PROPOSE→ACK, which is a symmetric handshake) — on receiving CONFIRM, go straight to Step 4's business action; **do not reply with any P2P message**.\n\n\
             **Step 4 — After receiving [intent:confirm] and verifying consistency, run apply on-chain:**\n\n\
             ```bash\n\
             onchainos agent apply {job_id} --token-amount <negotiated price> --token-symbol <USDT|USDG> --agent-id {agent_id}\n\
             ```\n\
             apply is an on-chain signing action; the CLI internally does unsigned info → sign → broadcast; wait for the on-chain provider_applied notification.\n\n\
             ⚠️ **After apply, end the turn directly**:\n\
             ❌ Do NOT push to the user with `xmtp_dispatch_user` — `apply submitted / txHash / awaiting provider_applied` is filler state\n\
             ❌ Do NOT send any ACK / thanks / `started processing` filler to the User Agent via `xmtp_send` — at this point the User Agent is already running confirm-accept; your ACK is noise and triggers the User Agent's `no repeated xmtp_send within one turn` iron rule (see SKILL.md `🔒 Communication Boundary and Security Gate`)\n\
             ✅ The next step happens only after the on-chain `provider_applied` notification arrives and next-action is called again.\n\n\
             **If any item is not agreed upon** → call `xmtp_send(sessionKey=<current session sessionKey, fetched via session_status>, content=\"Sorry, cannot accept the current terms\")`, end.\n\n\
             [Follow-up events]\n\
             - apply on-chain succeeds → receive `provider_applied` system notification → call next-action again for the script\n"
        ),
        // ─── Buyer-driven tx receipt notifications; no provider action needed ─────
        Event::JobClosed
        | Event::JobVisibilityChanged
        | Event::JobPaymentModeChanged => format!(
            "[System notification] {event} (User Agent-side tx receipt; not the provider's concern)\n\
             [Role] ASP (Agent Service Provider)\n\n\
             Silently ignore; end this turn. For details, call `onchainos agent common context {job_id} --role provider`.\n",
            event = event.as_str()
        ),

        // ─── Buyer-driven timeout events; no provider action needed ─────
        Event::JobExpired
        | Event::SubmitExpired
        | Event::RefuseExpired
        | Event::ReviewDeadlineWarn => format!(
            "[System notification] {event} (User Agent-side timeout event; not the provider's concern)\n\
             [Role] ASP (Agent Service Provider)\n\n\
             Silently ignore; end this turn. For details, call `onchainos agent common context {job_id} --role provider`.\n",
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
            format!(
            "[System notification] job_auto_completed (claimAutoComplete tx receipt)\n\
             [Role] ASP (Agent Service Provider)\n\n\
             **Step 1 — Check the envelope's `message.code` field:**\n\
             - `code` non-zero (failed) → call xmtp_dispatch_user to notify the user:\n\
             \x20\x20content: {failed_notify}\n\
             \x20\x20→ end the turn.\n\n\
             - `code` = 0 (success) → continue to Step 2.\n\n\
             **Step 2 — Use `xmtp_dispatch_user` to notify the user of fund arrival**:\n\n\
             From `onchainos agent common context {job_id} --role provider --agent-id {agent_id}` get task title + tokenAmount + tokenSymbol.\n\
             🌐 **Localize first** — rewrite `content` below in the user's language before sending (mandatory; see LOCALIZATION_PREFIX at top of this output). Do NOT pass the English template verbatim to a non-English user.\n\
             tool: xmtp_dispatch_user\n\
             content:\n\
             {user_notify}\n\n\
             ⚠️ Do NOT send `xmtp_send` filler to the User Agent — both sides receive the `job_auto_completed` system event.\n\
             {terminal_session_hint}\n"
            )
        }

        // ─── Provider's own deadline reminder ─────────────────────────────────────
        Event::SubmitDeadlineWarn => {
            let user_prompt = super::content::submit_deadline_warn_user_prompt(&short_id);
            format!(
            "[System notification] submit_deadline_warn (deadline for submitting the deliverable is approaching)\n\
             [Role] ASP (Agent Service Provider)\n\n\
             🛑🛑🛑 **ABSOLUTE REQUIREMENT — you MUST push the deadline decision (submit immediately vs let it time out) to the user via `pending-decisions-v2 request` (NOT a plain text reply, NOT just `xmtp_dispatch_user`)**.\n\
             `xmtp_dispatch_user` is a pure notification: user replies cannot be relayed back to the sub session → the user cannot signal `submit now` → the deadline silently expires → auto-refund to the User Agent. The correct flow handles this via `pending-decisions-v2 request` → CLI playbook → `xmtp_prompt_user` so the user session can relay the decision back.\n\
             ❌ Do not substitute a plain text reply for the `pending-decisions-v2 request` call.\n\
             ❌ Do not substitute `xmtp_dispatch_user` for the `pending-decisions-v2 request`.\n\n\
             **Step 0 — Idempotency check** (CLI's pending queue is the source of truth):\n\
             ```bash\n\
             onchainos agent pending-decisions-v2 list --format json\n\
             ```\n\
             If the returned `entries` already contains a sub_key with `job={job_id}` for this role → the user has already been notified; this is a duplicate event; **end the turn without re-notifying**. Otherwise → continue to Step 1.\n\n\
             **Step 1 — Enqueue the decision via `pending-decisions-v2 request`**:\n\n\
             First call `session_status` to get the current sessionKey (only once per turn). Then run:\n\
             ```bash\n\
             onchainos agent pending-decisions-v2 request \\\n\
               --sub-key \"<full sessionKey from session_status>\" \\\n\
               --job-id {job_id} --role provider --agent-id {agent_id} \\\n\
               --user-content \"{user_prompt_for_shell}\" \\\n\
               --list-label \"[Decision {short_id}] Submit Now / Let Timeout\"\n\
             ```\n\
             🌐 Translate `--user-content` AND `--list-label` to the user's language (signal = user's OWN typed messages this session; default English if unsure). See [Localization] above for token mapping.\n\n\
             Follow the playbook the CLI returns verbatim, then end the turn. Do NOT manually construct `llmContent` / call `xmtp_dispatch_session` yourself — that path is owned by `pending-decisions-v2` now. Do NOT `xmtp_send` the User Agent — the deadline warning is between the ASP and the user, not the User Agent's business.\n\n\
             **Step 2 — After receiving `[USER_DECISION_RELAY] decision: <user verbatim>` from the user-session**:\n\
             - Contains `立即提交` / `我提交` / `submit now` / `I'll deliver` / `ready` → run the delivery flow (same as JobAccepted Step 2-3): autonomously complete the work → `xmtp_send` the deliverable to the User Agent → run `onchainos agent deliver` on-chain. (Full script: `onchainos agent next-action --jobid {job_id} --jobStatus job_accepted --role provider --agentId {agent_id}` — skip its Step 1 apply-accepted notification; the user already knows the task was accepted.)\n\
             - Otherwise (user replied something unrelated or chose to let it timeout) → end the turn. Let it time out via submit_expired into auto-refund.\n\n\
             ⚠️ **Do NOT auto-run `onchainos agent deliver` in the same turn as the decision relay** — only the user knows whether the deliverable is actually ready; the agent must not decide \"deliverable is ready\" on the user's behalf.\n",
                user_prompt_for_shell = user_prompt.replace('"', "\\\""),
            )
        }

        // ─── Arbitration sub-state-machine events — provider cares about dispute_resolved (already has a dedicated arm); other evaluator-internal events are observed silently ─────
        Event::EvaluatorSelected
        | Event::RevealStarted
        | Event::VoteCommitted
        | Event::VoteRevealed
        | Event::RoundFailed => format!(
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
        | Event::Slashed
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
             You should NOT use `wakeup_notify` as --jobStatus to run the script — this script is just for guidance.\n\n\
             [Your next action (strict order)]\n\n\
             **Step 1 — Read the real status from the envelope**:\n\
             From the wakeup_notify envelope that triggered this turn, read the `message.jobStatus` field (e.g. `accepted` / `submitted` / `refused` / `disputed` / `completed` / `rejected`, etc. — the real status string).\n\n\
             **Step 2 — Use the real status to call next-action and fetch the current script**:\n\
             ```bash\n\
             onchainos agent next-action --jobid {job_id} --jobStatus <value of the message.jobStatus field> --role provider --agentId {agent_id}\n\
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

        Event::SwitchProvider | Event::AttachmentAdded => "[System notification] buyer-side event; not the provider's concern.\n\
             [Recommendation] Ignore; no action needed.\n".to_string(),

        Event::Other(ref other) => format!(
            "[Unknown state] {other}\n\
             [Recommendation]\n\
             1. Call `onchainos agent common context {job_id} --role provider` to view the full context\n\
             2. If this state is not in the expected flow, wait for user instructions\n\
             3. Do NOT predict / assume other notifications\n"
        ),
    };
    format!("{localization_prefix}{version_prefix}{context_preamble}{body}")
}
