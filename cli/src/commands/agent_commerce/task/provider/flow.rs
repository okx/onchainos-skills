//! Provider-side task flow driver.
//!
//! Based on the current system notification type received (event), outputs the prompt
//! for the next action to take. The goal: consolidate the Scene steps scattered across
//! provider.md into code so the agent can simply run
//! `exec onchainos agent next-action ...` to fetch the prompt and execute it directly,
//! without having to reason over the entire document.

use crate::commands::agent_commerce::task::common::util::short_job_id;

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
    message: Option<&serde_json::Value>,
) -> String {
    let _ = message; // currently used only by event handlers that opt in (see JobAspSelected below); silence the unused-arg warning when no scene reads it.
    use crate::commands::agent_commerce::task::common::state_machine::{parse_status_or_event, Event};

    // (Old MCP-era `okx-a2a xmtp-send` `payload` version handshake was removed when the script
    // migrated to `okx-a2a xmtp-send`, which has no equivalent `payload` parameter.
    // Protocol version is now enforced server-side, not via wire-level payload tagging.)

    // Short jobId, used as the `[Job <shortId> — you are the ASP]` prefix on the first
    // line of `okx-a2a user decision-request` userContent (the canonical templates live in content.rs).
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
    // buyerAgentId / description / paymentMode / visibility / providerAgentId / status /
    // serviceId / serviceTokenAddress / serviceTokenAmount / serviceParams.
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
                    "serviceId" => p.service_id.as_deref().filter(|s| !s.is_empty()).map(|v| format!("\x20\x20- serviceId: {v}\n")),
                    "serviceTokenAddress" => p.service_token_address.as_deref().filter(|s| !s.is_empty()).map(|v| format!("\x20\x20- serviceTokenAddress: {v}\n")),
                    "serviceTokenAmount" => p.service_token_amount.as_deref().filter(|s| !s.is_empty()).map(|v| format!("\x20\x20- serviceTokenAmount: {v}\n")),
                    "serviceParams" => p.service_params.as_deref().filter(|s| !s.is_empty()).map(|v| format!("\x20\x20- serviceParams: {v}\n")),
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
    //   - okx-a2a xmtp-send: send to the User Agent (peer sub session); args --job-id + --to-agent-id + --message
    //   - okx-a2a user notify: notify the user (no user decision required); arg --content
    //   - okx-a2a user decision-request: needs user interaction (confirmation / decision);
    //     params: llmContent + userContent
    //     llmContent = instruction injected into the user session LLM (invisible to user;
    //                  contains (jobId, role, agentId, toAgentId?) routing fields so the user agent can relay the decision back to sub)
    //     userContent = the user-visible message to send to the user
    // ──────────────────────────────────────────────────────────────────────
    let send_to_peer = format!(
        "→ Run `okx-a2a xmtp-send` (Current jobId={job_id}, our agentId={agent_id}). Resolve `<buyerAgentId>` from the task fields above (or via `common context`):\n\
         ```bash\n\
         okx-a2a xmtp-send \\\n\
         \x20\x20--job-id {job_id} \\\n\
         \x20\x20--to-agent-id <buyerAgentId> \\\n\
         \x20\x20--message \"<message shown below>\"\n\
         ```\n\
         Message:"
    );

    // Shared "execute task autonomously" guidance for escrow Step 2 — the script does
    // not prescribe how to do it; list a few examples so the agent knows "pick your own
    // tool" is the expected behavior.
    let execute_task = format!(
        "Pick the right tool / capability for the task content to get the work done. For example:\n\
        \x20\x20• `Generate a cat image` → call an image-generation tool, get the local file path\n\
        \x20\x20• `Check the weather` → call wttr.in / a weather API, get a text result\n\
        \x20\x20• `Audit a smart contract` → read the code, produce an audit report\n\
        Tool choice is outside the script's scope; the agent decides autonomously.\n\n\
        ⚠️ If you have questions about task details / acceptance criteria → run `okx-a2a xmtp-send` (resolve `<buyerAgentId>` from the task fields above):\n\
        \x20\x20\x20\x20```bash\n\
        \x20\x20\x20\x20okx-a2a xmtp-send \\\n\
        \x20\x20\x20\x20\x20\x20--job-id {job_id} \\\n\
        \x20\x20\x20\x20\x20\x20--to-agent-id <buyerAgentId> \\\n\
        \x20\x20\x20\x20\x20\x20--message \"<plain natural-language question to the User Agent>\"\n\
        \x20\x20\x20\x20```\n\
        End this turn after sending, wait for the reply; once you have the answer, start the work. Do not guess and produce a deliverable that misses the mark."
    );

    // Terminal-state (completed / refunded / close / dispute_resolved, etc.) session
    // retain-vs-release policy is governed by common::config::KEEP_CONVERSATION_ON_TERMINAL —
    // change the default by modifying that const.
    let terminal_session_hint = format!("\
ℹ️ Task is in terminal state — run the cleanup command (handles pending-decision cancellation automatically):\n\
         ```bash\n\
         onchainos agent session-cleanup --job-id {job_id} --role provider\n\
         ```\n\
         Then follow the command's output to close conversations (if applicable).");

    let event = parse_status_or_event(event_str);
    match event {
        // ─── Scene 3: Apply has been recorded on-chain (escrow path; the User Agent issues the payment) ──
        Event::ProviderApplied => {
            let user_notify = super::content::provider_applied_user_notify(job_id, agent_id);
            format!(
            "[Current state] provider_applied (apply has been recorded on-chain)\n\
             [Role] ASP (Agent Service Provider)\n\n\
             **Step 1 — Use `okx-a2a user notify` to push the apply-submitted notification to the user**:\n\n\
             🌐 **Localize first** — rewrite `content` below in the user's language before sending. Do NOT pass the English template verbatim to a non-English user.\n\
             ```bash\n\
             okx-a2a user notify --content \"<the template below, localized>\"\n\
             ```\n\
             `--content` template (canonical English; do NOT add / drop fields):\n\
             {user_notify}\n\n\
             ❌ Do NOT communicate with the buyer. ❌ Do NOT deliver directly.\n\n\
             [Follow-up events]\n\
             - job_accepted → User Agent has confirm-accepted, escrow funding complete.\n"
            )
        },

        // ─── Scene 4: User Agent has confirmed the apply; execute and deliver ──
        Event::JobAccepted => {
            let user_notify = super::content::job_accepted_user_notify(job_id, agent_id);
            let deliver_text = super::content::deliver_text_to_buyer(job_id);
            let deliver_file = super::content::deliver_file_to_buyer(job_id);
            let task_fields = inline_task_fields(&["title", "description", "tokenAmount", "tokenSymbol", "serviceParams"]);
            format!(
            "[Current state] job_accepted (User Agent has confirmed the apply)\n\
             [Role] ASP (Agent Service Provider)\n\n\
             [Your next action (strict order, do not skip steps)]\n\n\
             {task_fields}\n\
             **Step 1 — Notify the user (apply accepted) via `okx-a2a user notify`**:\n\n\
             🌐 **Localize first** — rewrite the content below in the user's language before sending. Do NOT pass the English template verbatim to a non-English user.\n\
             ```bash\n\
             okx-a2a user notify --content \"<localized content shown below>\"\n\
             ```\n\
             content:\n\
             {user_notify}\n\n\
             Fill the `<title>` / `<description>` / `<amount>` / `<tokenSymbol>` placeholders from the **Task fields** block above.\n\
             ⚠️ Do NOT send `okx-a2a xmtp-send` `received apply confirmation` filler to the User Agent — the User Agent just ran confirm-accept; they already know.\n\n\
             **Step 2 — Autonomously execute the task and prepare the deliverable**:\n\
             {execute_task}\n\n\
             **Step 3 — Deliver** (first `okx-a2a xmtp-send` the deliverable to the User Agent, then deliver on-chain):\n\n\
             ⚠️ **Order**: first `okx-a2a xmtp-send` the deliverable to the User Agent, then deliver on-chain. The on-chain deliver only advances the task state to submitted (giving the User Agent an acceptance entry point); the deliverable itself was already delivered via okx-a2a xmtp-send.\n\n\
             **Step 3a — Prepare the deliverable (branch by type)**:\n\n\
             ▸ **Plain text / URL deliverable**: assemble the text content directly, skip `okx-a2a file upload`, go to Step 3b.\n\n\
             ▸ **File deliverable** (image / PDF / document): run `okx-a2a file upload`:\n\
             \x20\x20```bash\n\
             \x20\x20okx-a2a file upload --file-path \"<absolute local file path>\" --agent-id {agent_id} --job-id {job_id}\n\
             \x20\x20```\n\
             \x20\x20Record all five return fields (`fileKey` / `digest` / `salt` / `nonce` / `secret` — decryption metadata).\n\n\
             **Step 3b — `okx-a2a xmtp-send` the deliverable to the User Agent** (in the same turn, immediately following Step 3a):\n\
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
             **Step 4 — After Step 3c ends this turn immediately** (the deliverable was already delivered to the User Agent in Step 3b; do NOT send any filler `okx-a2a xmtp-send` / `okx-a2a user notify` here).\n\n\
             🛑 **The next system events for this ASP are `job_completed` OR `job_rejected` — both are action-required, NEITHER is observer-only.** Provider does NOT receive a `job_submitted` envelope after deliver. On either event below, you MUST call `next-action` again.\n\n\
             [Follow-up events]\n\
             - `job_completed` (buyer reviewed and accepted) → call `next-action --role provider --agentId {agent_id} --message '{{\"event\":\"job_completed\",\"jobId\":\"{job_id}\"}}'` ← **REQUIRED — auto-rate the buyer + notify the user**\n\
             - `job_rejected`  (buyer rejected the deliverable) → call `next-action --role provider --agentId {agent_id} --message '{{\"event\":\"job_rejected\",\"jobId\":\"{job_id}\"}}'` ← **REQUIRED — push dispute-vs-refund decision to the user**\n"
            )
        }

        // ─── Scene 5: Deliverable confirmed on-chain (observer-only) ──────────────────
        // In the new flow the deliverable was already sent to the User Agent via okx-a2a xmtp-send
        // in Scene 4 A-Step 2; when the job_submitted system event reaches this sub there
        // is no need to okx-a2a xmtp-send again, to avoid the User Agent receiving duplicate messages.
        Event::JobSubmitted => format!(
            "[System notification] job_submitted (deliverable confirmed on-chain; task state is now submitted)\n\
             [Role] ASP (Agent Service Provider)\n\n\
             ⚠️ **observer-only — SCOPE: THIS turn / THIS event only**: the deliverable was already sent to the User Agent in the `job_accepted` script (A-Step 2); this event **must NOT trigger a second okx-a2a xmtp-send** — duplicating would cause the User Agent to receive double messages and trigger a loop.\n\n\
             [Your next action]\n\
             - **Just observe silently**; do NOT call okx-a2a xmtp-send / okx-a2a file upload / okx-a2a user notify / okx-a2a user decision-request\n\
             - **End this turn directly**; wait for the User Agent to complete/reject and trigger the next event\n\n\
             🛑 **DO NOT extend `observe silently` to the next event.** When `job_completed` or `job_rejected` arrives, those are **action-required** events (auto-rate the buyer / push a dispute-vs-refund decision to the user). You MUST call `next-action` again — see [Follow-up events] below. Treating a subsequent `job_completed` envelope as silent = the user never gets the completion notice + the buyer never gets rated.\n\n\
             [Follow-up events]\n\
             - Received `job_completed` (review passed) → `onchainos agent next-action --role provider --agentId {agent_id} --message '{{\"event\":\"job_completed\",\"jobId\":\"{job_id}\"}}'` ← **REQUIRED, not optional**\n\
             - Received `job_rejected`  (User Agent rejected) → `onchainos agent next-action --role provider --agentId {agent_id} --message '{{\"event\":\"job_rejected\",\"jobId\":\"{job_id}\"}}'` ← **REQUIRED, not optional**\n"
        ),

        // ─── Scene 6: User Agent rejected the deliverable ─────────────────────────────────
        Event::JobRejected => {
            let user_prompt = super::content::job_rejected_user_decision_prompt(&short_id);
            let request_block = crate::commands::agent_commerce::task::common::pending_v2::request_command_block(
                job_id,
                "provider",
                agent_id,
                prefetched.and_then(|p| p.buyer_agent_id.as_deref()),
                &user_prompt,
                &format!("[Decision {short_id}] {title_display} dispute decision"),
                "job_rejected",
            );
            format!(
            "[Current state] job_rejected (User Agent rejected the deliverable)\n\
             [Role] ASP (Agent Service Provider)\n\n\
             🛑🛑🛑 **ABSOLUTE REQUIREMENT — you MUST push the decision (dispute vs refund) to the user via `pending-decisions-v2 request` (NOT a plain text reply, NOT just `okx-a2a user notify`)**.\n\
             `okx-a2a user notify` is a pure notification: user replies cannot be relayed back to the sub session → the decision flow deadlocks. The correct flow handles this via `pending-decisions-v2 request` → CLI playbook → `okx-a2a user decision-request` (with llmContent + userContent) so the user session can relay the decision back. Direct text output in this sub session = user doesn't see it + relay channel broken + 24h timeout → auto-refund.\n\
             ❌ Do not substitute a plain text reply for the `pending-decisions-v2 request` call.\n\
             ❌ Do not substitute `okx-a2a user notify` for the `pending-decisions-v2 request`.\n\
             ⚠️ Do NOT send `okx-a2a xmtp-send` `received the rejection` filler to the User Agent — they just rejected; they know. Go straight to the user-decision flow.\n\n\
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
             - Do NOT send any okx-a2a xmtp-send to the User Agent (`dispute raised` is filler; wait until phase 2 completes)\n\
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
             - Do NOT okx-a2a xmtp-send the User Agent (still filler state)\n\
             - Do NOT submit evidence in the same turn (evidence goes through dispute upload; must wait for the `job_disputed` notification + user-provided content)\n\n\
             [Follow-up events]\n\
             - `job_disputed` system notification → enter 1-hour evidence preparation window → next-action will instruct you to `okx-a2a user decision-request` for evidence from the user\n"
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
             ⚠️ Do NOT send `okx-a2a xmtp-send` `agreed to refund` filler to the User Agent — both sides receive the `job_refunded` system event.\n\
             ⚠️ Do NOT push to the user with `okx-a2a user notify`.\n"
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
             ⚠️ Do NOT send `okx-a2a xmtp-send` thanks / `done` filler to the User Agent — they just completed; they know.\n\n\
             {task_fields}\n\
             **Step 2 — Notify the user of task completion via `okx-a2a user notify`**:\n\n\
             🌐 **Localize first** — rewrite the content below in the user's language before sending (mandatory; see the `[Localization]` block at the top of this output). Do NOT pass the English template verbatim to a non-English user.\n\
             ```bash\n\
             okx-a2a user notify --content \"<localized content shown below>\"\n\
             ```\n\
             content:\n\
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
             🌐 **Localize first** — rewrite the content below in the user's language before sending (mandatory; see the `[Localization]` block at the top of this output). Do NOT pass the English template verbatim to a non-English user.\n\
             After feedback-submit, run `okx-a2a user notify` to notify the user:\n\
             - ✅ **Success** (output contains `txHash`):\n\
             ```bash\n\
             okx-a2a user notify --content \"<localized content shown below>\"\n\
             ```\n\
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
             ⚠️ Do NOT send `okx-a2a xmtp-send` `ruling supports party X` filler to the User Agent — both sides receive the `dispute_resolved` system event.\n\n\
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
             **A-Step 3 — Notify the user of the win + claim result via `okx-a2a user notify`**:\n\n\
             Field values for the content template come from the **Task fields** block above.\n\
             ⚠️ content is the **chat the user will see** — plain natural language; **do NOT use** skill names / event names / state names / CLI flags or other technical jargon.\n\
             🌐 **Localize first** — rewrite the content below in the user's language before sending (mandatory; see the `[Localization]` block at the top of this output). Do NOT pass the English template verbatim to a non-English user.\n\
             ```bash\n\
             okx-a2a user notify --content \"<localized content shown below>\"\n\
             ```\n\
             content (choose based on whether A-Step 2 actually claimed):\n\
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
             🌐 **Localize first** — rewrite the content below in the user's language before sending (mandatory; see the `[Localization]` block at the top of this output). Do NOT pass the English template verbatim to a non-English user.\n\
             After feedback-submit, run `okx-a2a user notify` to notify the user:\n\
             - ✅ **Success** (output contains `txHash`):\n\
             ```bash\n\
             okx-a2a user notify --content \"<localized content shown below>\"\n\
             ```\n\
             content (fill `<score>` with the X.XX value and `<description>` with the comment you just used in A-Step 4; fill `<title>` from task context):\n\
             {rating_notify}\n\
             - ❌ **Failure** (error / non-zero exit code) → silently skip; do NOT notify the user, do NOT retry.\n\n\
             ━━━━━━━━━━━━━ Branch B: jobStatus=failed (ASP lost) ━━━━━━━━━━━━━\n\n\
             **B-Step 1 — Notify the user of the loss via `okx-a2a user notify`**:\n\n\
             Field values for the content template come from the **Task fields** block above (same fields as Branch A).\n\
             ⚠️ Same as A-Step 3 — content plain natural language; no technical jargon.\n\
             🌐 **Localize first** — rewrite the content below in the user's language before sending (mandatory; see the `[Localization]` block at the top of this output). Do NOT pass the English template verbatim to a non-English user.\n\
             ```bash\n\
             okx-a2a user notify --content \"<localized content shown below>\"\n\
             ```\n\
             content:\n\
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
             🌐 **Localize first** — rewrite the content below in the user's language before sending (mandatory; see the `[Localization]` block at the top of this output). Do NOT pass the English template verbatim to a non-English user.\n\
             After feedback-submit, run `okx-a2a user notify` to notify the user:\n\
             - ✅ **Success** (output contains `txHash`):\n\
             ```bash\n\
             okx-a2a user notify --content \"<localized content shown below>\"\n\
             ```\n\
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
             ⚠️ Do NOT send `okx-a2a xmtp-send` `refund on-chain` filler to the User Agent — both sides already receive the `job_refunded` system event.\n\
             {terminal_session_hint}\n\n\
             **End this turn directly**; the refund flow is fully complete.\n"
        ),

        // ─── Scene 6.4: Arbitration on-chain; CLI auto-submits evidence ─────────────────────
        Event::JobDisputed => {
            let task_fields = inline_task_fields(&["buyerAgentId"]);
            format!(
            "[Current state] job_disputed (arbitration is on-chain; CLI auto-submits evidence on this event)\n\
             [Role] ASP (Agent Service Provider)\n\n\
             🛑 **This event triggers an AUTOMATIC evidence upload — no user interaction**.\n\
             The agent does NOT ask the user for evidence; it pulls the full chat history from this sub\n\
             session, calls `dispute upload` (which also auto-attaches the deliverable copy saved under\n\
             `~/.onchainos/deliverables/provider/{job_id}/`), and then notifies the user via\n\
             `okx-a2a user notify`. **Do NOT** use `pending-decisions-v2 request` for this event.\n\
             **Do NOT** `okx-a2a xmtp-send` anything to the User Agent — both sides see the arbitration via on-chain events.\n\n\
             {task_fields}\n\
             **Step 1 — Pull this sub session's chat history** (use `buyerAgentId` from the **Task fields** block above):\n\n\
             ```bash\n\
             okx-a2a session history --job-id {job_id} --to-agent-id <buyerAgentId> --json\n\
             ```\n\n\
             **Step 2 — Format the chat history as the `--text` body**:\n\n\
             ```\n\
             ==== Negotiation / delivery chat history (from okx-a2a session history) ====\n\
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
             **Step 5 — End this turn.** Do NOT `okx-a2a xmtp-send` anything to the User Agent.\n\n\
             [Follow-up events]\n\
             - job_completed → won, funds released to the ASP\n\
             - dispute_resolved → lost, funds refunded to the User Agent\n"
            )
        }

        // ─── Scene 1: task is on-chain (job_created) — provider takes no proactive
        // action on this raw event. The active discovery paths are `recommend-task` /
        // `contact-buyer` (user-driven) and `JobAspSelected` (buyer-designated). ────
        Event::JobCreated => "[System notification] job_created (task is on-chain; no provider-side action)\n\
             [Role] ASP (Agent Service Provider)\n\n\
             Silently ignore; end this turn.\n\
             To accept tasks, use `recommend-task` / `contact-buyer`; if the buyer designates this ASP a `job_asp_selected` event will arrive separately.\n".to_string(),

        // ─── Scene 1.5: Buyer designated this ASP for a private task ──────────
        Event::JobAspSelected => {
            // CODE-DRIVEN PATH: fetch service-list, match by serviceId, pre-compute price
            // gate, emit deterministic playbook. LLM only does the semantic capability
            // judgment (does task description fit service description?) and picks ONE
            // of two pre-built actions (apply or okx-a2a xmtp-send-reject). Single turn, no
            // intermediate CLI calls in the LLM context.
            // Field sourcing priority — `--message` envelope wins (it's the inbound
            // system event payload, source-of-truth for this turn). Falls back to
            // `prefetched` (GET /task API response) when the envelope omits a field.
            let p = prefetched;
            let msg_str = |k: &str| -> Option<&str> {
                message.and_then(|m| m.get(k)).and_then(|v| v.as_str()).filter(|s| !s.is_empty())
            };

            let service_id = msg_str("serviceId")
                .or_else(|| p.and_then(|x| x.service_id.as_deref()).filter(|s| !s.is_empty()))
                .unwrap_or("");
            // buyer's offered amount: task-level `tokenAmount`. Envelope wins over prefetched.
            let offer_amount = msg_str("tokenAmount")
                .or_else(|| p.map(|x| x.token_amount.as_str()).filter(|s| !s.is_empty()))
                .unwrap_or("");
            // buyer's token symbol — task-level; envelope wins. Stays as Option so missing
            // tokenSymbol triggers the incomplete-terms guard (do NOT silent-fallback to USDT
            // — applying with the wrong token would lock the wrong escrow currency).
            let buyer_token_symbol_opt = msg_str("tokenSymbol")
                .or_else(|| p.map(|x| x.token_symbol.as_str()).filter(|s| !s.is_empty() && *s != "?"));
            let task_title = msg_str("jobTitle")
                .or_else(|| msg_str("title"))
                .or_else(|| p.map(|x| x.title.as_str()).filter(|s| !s.is_empty()))
                .unwrap_or("");
            let task_desc = msg_str("description")
                .or_else(|| p.map(|x| x.description.as_str()).filter(|s| !s.is_empty()))
                .unwrap_or("");

            // Render-helper for the three early-bailout branches (no service / empty
            // offer / missing token symbol). All share: notify + end turn, no on-chain
            // action, no asp-reject (buyer is in incomplete state and needs to re-route).
            let render_bailout = |header: &str, user_notify: &str| -> String {
                format!(
                    "[Current state] job_asp_selected — {header}. jobId=`{job_id}` agentId={agent_id}\n\n\
                     **Notify the user, then end the turn** (🌐 translate template to user's language first):\n\
                     {user_notify}\n\n\
                     ```bash\n\
                     okx-a2a user notify --content \"<translated text>\"\n\
                     ```\n"
                )
            };

            if service_id.is_empty() {
                let user_notify = super::content::job_asp_selected_no_service_notify(job_id);
                render_bailout("designated by buyer, but no specific `serviceId` was pinned", &user_notify)
            } else if offer_amount.is_empty() {
                let user_notify = super::content::job_asp_selected_missing_terms_notify(job_id, "tokenAmount");
                render_bailout("designation envelope missing `tokenAmount`", &user_notify)
            } else if buyer_token_symbol_opt.is_none() {
                let user_notify = super::content::job_asp_selected_missing_terms_notify(job_id, "tokenSymbol");
                render_bailout("designation envelope missing `tokenSymbol`", &user_notify)
            } else {
                let buyer_token_symbol = buyer_token_symbol_opt.unwrap();
                // CODE: fetch service catalog and find the designated entry.
                let matched = crate::commands::agent_commerce::task::common::find_service(agent_id, service_id).await.ok().flatten();

                // Build a reject template factory — the reason can be either a code-determined
                // fixed string (passed verbatim) or the LLM-fillable `<reason>` placeholder.
                // Backend off-chain endpoint: POST /priapi/v1/aieco/task/{jobId}/asp/reject — no signing required.
                let build_reject_template = |reason_for_cli: &str, reason_for_notify: &str| {
                    let notify_body = super::content::job_asp_selected_rejected_notify(job_id, reason_for_notify);
                    format!(
                        "**REJECT path** — run in order, then end the turn:\n\
                         ```bash\n\
                         onchainos agent asp-reject {job_id} --agent-id {agent_id} --reason \"{reason_for_cli}\"\n\
                         ```\n\
                         Then (🌐 translate template to user's language first):\n\
                         {notify_body}\n\n\
                         ```bash\n\
                         okx-a2a user notify --content \"<translated text>\"\n\
                         ```\n\
                         ❌ Do NOT call `apply`. ❌ Do NOT okx-a2a xmtp-send the buyer.\n"
                    )
                };
                // Generic LLM-fillable reject template (used by capability-mismatch / general fallbacks).
                let reject_template = build_reject_template(
                    "<short reason: capability mismatch / price too low / designated service not registered>",
                    "<reason>",
                );

                match matched {
                    None => {
                        // CODE-decided REJECT: service not in catalog. Reason is fully known.
                        let reject_template_fixed = build_reject_template(
                            "designated service not registered",
                            "designated service not registered",
                        );
                        format!(
                            "[Auto-decision] ❌ REJECT — designated `serviceId={service_id}` is NOT in your registered catalog (service-list returned no match). This is the ONLY action; no LLM judgment needed.\n\n\
                             Task: {task_title}\n\
                             Buyer offer: {offer_amount} {buyer_token_symbol}\n\n\
                             {reject_template_fixed}"
                        )
                    }
                    Some(svc) => {
                        let svc_name = svc.get("serviceName").and_then(|v| v.as_str()).unwrap_or("");
                        let svc_desc = svc.get("serviceDescription").and_then(|v| v.as_str()).unwrap_or("");
                        let svc_fee  = svc.get("fee").and_then(|v| v.as_str()).filter(|s| !s.is_empty()).unwrap_or("");

                        // CODE: numerical price gate.
                        // `fee_num=None` means "service has no registered fee" → LLM estimates by complexity.
                        let offer_num = offer_amount.parse::<f64>().ok();
                        let fee_num = if svc_fee.is_empty() { None } else { svc_fee.parse::<f64>().ok() };
                        let (price_status, price_summary, price_action) = match (offer_num, fee_num) {
                            (Some(o), Some(f)) if o >= f => (
                                "OK",
                                format!("buyer offer {offer_amount} ≥ registered fee {svc_fee} ✅"),
                                "Apply at offer amount."
                            ),
                            (Some(_), Some(_)) => (
                                "TOO_LOW",
                                format!("buyer offer {offer_amount} < registered fee {svc_fee} ❌"),
                                "Reject — price below registered floor."
                            ),
                            (_, None) => (
                                "ESTIMATE",
                                format!("registered fee not set; buyer offer {offer_amount} {buyer_token_symbol} — judge by task complexity"),
                                "If offer is fair for the workload → apply at offer; else reject."
                            ),
                            _ => (
                                "PARSE_FAIL",
                                format!("could not parse offer=`{offer_amount}` fee=`{svc_fee}`"),
                                "Treat as ESTIMATE; LLM judges based on complexity."
                            ),
                        };

                        // Deterministic apply command — uses buyer's token symbol (per spec).
                        // After apply, push a user-facing notification via `okx-a2a user notify`.
                        let apply_user_notify = super::content::job_asp_selected_accepted_notify(job_id);
                        let apply_failed_notify = super::content::job_asp_selected_apply_failed_notify(job_id, "<one-line error from apply's stderr>");
                        let apply_template = format!(
                            "**APPLY path** — run apply, then branch by exit code:\n\
                             ```bash\n\
                             onchainos agent apply {job_id} --agent-id {agent_id} --token-amount {offer_amount} --token-symbol {buyer_token_symbol}\n\
                             ```\n\n\
                             ✅ **On success** (exit code 0 + `txHash` in stdout) — notify the user (🌐 fill `<serviceName>` / `<offerAmount>` / `<tokenSymbol>` from [Auto-decision context] above, then translate):\n\
                             {apply_user_notify}\n\n\
                             ```bash\n\
                             okx-a2a user notify --content \"<filled + translated text>\"\n\
                             ```\n\
                             Then end the turn; wait for the `provider_applied` system event.\n\n\
                             ❌ **On failure** (non-zero exit / stderr / no txHash) — DO NOT proceed to the success notify. Push a failure notification instead (🌐 fill `<one-line error from apply's stderr>`, then translate):\n\
                             {apply_failed_notify}\n\n\
                             ```bash\n\
                             okx-a2a user notify --content \"<filled + translated text>\"\n\
                             ```\n\
                             Then end the turn. Do NOT retry apply automatically — the user will decide manually.\n"
                        );

                        // Decide which branches the LLM can take, based on the code-computed price gate.
                        let llm_decision = match price_status {
                            "OK" => format!(
                                "**LLM judgment** — single question: does the service description capability-match the task description?\n\
                                 \x20\x20• YES → run **APPLY path** below.\n\
                                 \x20\x20• NO  → run **REJECT path** below (reason = capability mismatch).\n\n\
                                 {apply_template}\n\
                                 {reject_template}"
                            ),
                            "TOO_LOW" => {
                                // Price-too-low reason is fully determined in code; no LLM judgment.
                                let too_low_reason = format!(
                                    "price below registered fee: offer {offer_amount} {buyer_token_symbol} < registered fee {svc_fee} {buyer_token_symbol}"
                                );
                                let too_low_template = build_reject_template(&too_low_reason, &too_low_reason);
                                format!(
                                    "**Auto-decision** — price gate already FAILED in code (see Price below). Capability is moot; run **REJECT path** regardless.\n\n\
                                     {too_low_template}"
                                )
                            },
                            "ESTIMATE" | "PARSE_FAIL" => format!(
                                "**LLM judgment** — two questions:\n\
                                 \x20\x20• Capability: does the service description match the task?\n\
                                 \x20\x20• Price: is the buyer's offer fair for this task's workload?\n\
                                 \x20\x20• BOTH yes → run **APPLY path** below.\n\
                                 \x20\x20• Either no → run **REJECT path** below.\n\n\
                                 💰 **Workload tier rubric** (no registered fee on this service — estimate by complexity):\n\
                                 \x20\x20- ✅ Reference comparable tasks / the buyer's offer / task complexity for a reasonable estimate. If the buyer's offer is already at-or-above your workload estimate → ACCEPT; never counter down.\n\
                                 \x20\x20- ❌ Don't blindly throw out something like 100 USDT.\n\
                                 \x20\x20- ❌ Don't self-discount to 0 / free — `price is always asked, never assumed`.\n\
                                 \x20\x20- Simple query tasks (1 API call / 1 datum) typically 0.001–0.05 USDT; complex tasks (multi-step / long text generation / reports) 0.05–1 USDT; deep research > 1 USDT requires solid justification.\n\n\
                                 {apply_template}\n\
                                 {reject_template}"
                            ),
                            _ => unreachable!(),
                        };

                        format!(
                            "[Auto-decision context — pre-computed by CLI]\n\
                             \x20\x20Task title:          {task_title}\n\
                             \x20\x20Task description:    {task_desc}\n\
                             \x20\x20Designated service:  {svc_name} (`{service_id}`)\n\
                             \x20\x20Service description: {svc_desc}\n\
                             \x20\x20Price gate ({price_status}): {price_summary}\n\
                             \x20\x20Recommended action:  {price_action}\n\
                             \x20\x20Apply currency:      {buyer_token_symbol} (buyer's specified token)\n\n\
                             {llm_decision}"
                        )
                    }
                }
            }
        },

        // ─── Buyer-driven tx receipt notifications; no provider action needed ─────
        Event::JobClosed
        | Event::JobVisibilityChanged
        | Event::JobPaymentModeChanged => format!(
            "[System notification] {event} (User Agent-side tx receipt; not the provider's concern)\n\
             [Role] ASP (Agent Service Provider)\n\n\
             Silently ignore; end this turn. \n",
            event = event.as_str()
        ),

        // ─── Buyer-driven timeout events; no provider action needed ─────
        Event::JobExpired
        | Event::SubmitExpired
        | Event::RejectExpired
        | Event::ReviewDeadlineWarn => format!(
            "[System notification] {event} (User Agent-side timeout event; not the provider's concern)\n\
             [Role] ASP (Agent Service Provider)\n\n\
             Silently ignore; end this turn.\n",
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
             - Do NOT send any okx-a2a xmtp-send to the User Agent (filler in between; wait until the job_auto_completed on-chain receipt arrives)\n\
             - Do NOT push to the user with `okx-a2a user notify`\n\n\
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
             - `code` non-zero (failed) → run `okx-a2a user notify`, then end the turn:\n\
             \x20\x20```bash\n\
             \x20\x20okx-a2a user notify --content \"{failed_notify}\"\n\
             \x20\x20```\n\
             - `code` = 0 (success) → continue to Step 2.\n\n\
             **Step 2 — Notify the user of fund arrival via `okx-a2a user notify`**:\n\n\
             Field values for the content template come from the **Task fields** block above.\n\
             🌐 **Localize first** — rewrite the content below in the user's language before sending (mandatory; see the `[Localization]` block at the top of this output). Do NOT pass the English template verbatim to a non-English user.\n\
             ```bash\n\
             okx-a2a user notify --content \"<localized content shown below>\"\n\
             ```\n\
             content:\n\
             {user_notify}\n\n\
             ⚠️ Do NOT send `okx-a2a xmtp-send` filler to the User Agent — both sides receive the `job_auto_completed` system event.\n\n\
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
             🌐 **Localize first** — rewrite the content below in the user's language before sending (mandatory; see the `[Localization]` block at the top of this output). Do NOT pass the English template verbatim to a non-English user.\n\
             After feedback-submit, run `okx-a2a user notify` to notify the user:\n\
             - ✅ **Success** (output contains `txHash`):\n\
             ```bash\n\
             okx-a2a user notify --content \"<localized content shown below>\"\n\
             ```\n\
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
                prefetched.and_then(|p| p.buyer_agent_id.as_deref()),
                &user_prompt,
                &format!("[Decision {short_id}] {title_display} submit decision"),
                "submit_deadline_warn",
            );
            format!(
            "[System notification] submit_deadline_warn (deadline for submitting the deliverable is approaching)\n\
             [Role] ASP (Agent Service Provider)\n\n\
             🛑🛑🛑 **ABSOLUTE REQUIREMENT — you MUST push the deadline decision (submit immediately vs let it time out) to the user via `pending-decisions-v2 request` (NOT a plain text reply, NOT just `okx-a2a user notify`)**.\n\
             `okx-a2a user notify` is a pure notification: user replies cannot be relayed back to the sub session → the user cannot signal `submit now` → the deadline silently expires → auto-refund to the User Agent. The correct flow handles this via `pending-decisions-v2 request` → CLI playbook → `okx-a2a user decision-request` so the user session can relay the decision back.\n\
             ❌ Do not substitute a plain text reply for the `pending-decisions-v2 request` call.\n\
             ❌ Do not substitute `okx-a2a user notify` for the `pending-decisions-v2 request`.\n\
             ❌ Do NOT `okx-a2a xmtp-send` the User Agent — the deadline warning is between the ASP and the user, not the User Agent's business.\n\n\
             **Push the decision to the user (3-substep protocol; read ALL 3 before running any command)**:\n\n\
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

        // ─── Buyer terms-change on-chain receipt — provider does not receive this event; fallback ignore ─────
        Event::TaskTokenBudgetChange => format!(
            "[System notification] {event} (User Agent terms-change receipt; provider does not handle directly)\n\
             [Role] ASP (Agent Service Provider)\n\n\
             Silently ignore; end this turn.\n",
            event = event.as_str()
        ),

        // ─── Buyer attachment received — download + save, no reply ─────
        Event::BuyerAttachmentReceived => {
            let l10n_short = super::content::L10N_DISPATCH_SHORT;
            let att_notify = super::content::buyer_attachment_received_user_notify(job_id);
            format!(
            "[Trigger] buyer_attachment_received (User Agent forwarded an attachment via `[intent:attachment]`)\n\
             [Role] ASP (Agent Service Provider)\n\n\
             🛑 **This is a file-download event, NOT a negotiation message.** Do NOT reply with `[intent:ack]` or any negotiation marker.\n\n\
             [Your next actions (strict order)]\n\n\
             **Step 1 — Parse the attachment fields from the inbound message:**\n\
             Extract all 6 decryption-metadata fields from the `[intent:attachment]` message:\n\
             \x20\x20- `fileKey` (FULL value)\n\
             \x20\x20- `digest` (FULL hex string)\n\
             \x20\x20- `salt` (FULL base64 string)\n\
             \x20\x20- `nonce` (FULL base64 string)\n\
             \x20\x20- `secret` (FULL base64 string, can be 100+ chars)\n\
             \x20\x20- `filename`\n\
             All 6 fields are REQUIRED for decryption. If any field is missing, log the error and end the turn.\n\n\
             **Step 2 — Download the file:**\n\
             Run `okx-a2a file download` (pass every metadata field from the inbound message):\n\
             \x20\x20```bash\n\
             \x20\x20okx-a2a file download --file-key <fileKey> --agent-id {agent_id} --digest <digest> --salt <salt> --nonce <nonce> --secret <secret> --filename <filename> --json\n\
             \x20\x20```\n\
             ⚠️ Before calling, print: `[provider-xmtp] file download: fileKey=<fileKey>, agentId={agent_id}`\n\
             ⚠️ After calling, print: `[provider-xmtp] file download result: localPath=<returned local path>`\n\n\
             On success, record the localPath.\n\
             On failure → log the error; do NOT block negotiation or task execution.\n\n\
             **Step 3 — Notify the user:**\n\
             Run `okx-a2a user notify`:\n\
             \x20\x20```bash\n\
             \x20\x20okx-a2a user notify --content '<translated content from the template below>' --json\n\
             \x20\x20```\n\
             \x20\x20content (canonical English template — translate before passing): {att_notify}\n\
             {l10n_short}\n\n\
             **Step 4 — Silent acknowledgement (do NOT reply to the User Agent):**\n\
             ❌ Do NOT call `okx-a2a xmtp-send` — attachments are supplementary reference materials; no protocol reply is needed.\n\
             ❌ Do NOT treat this as a negotiation turn — do NOT send `[intent:ack]` / `[intent:counter]` / any natural-language reply.\n\n\
             → **End this turn.** Continue negotiation or task execution when the next message arrives.\n")
        }

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
             - `code` non-zero (failed) → run `okx-a2a user notify` to notify the user, then end the turn:\n\
             \x20\x20```bash\n\
             \x20\x20okx-a2a user notify --content \"{failed_notify}\"\n\
             \x20\x20```\n\n\
             - `code` = 0 (success) → continue to Step 2.\n\n\
             **Step 2 — Notify the user that the reward has arrived via `okx-a2a user notify`:**\n\
             \x20\x20```bash\n\
             \x20\x20okx-a2a user notify --content \"{claimed_notify}\"\n\
             \x20\x20```\n"
            )
        }

        // job_auto_refunded — buyer-side tx receipt; not the provider's concern
        Event::JobAutoRefunded => "[System notification] job_auto_refunded (buyer-side claimAutoRefund tx receipt; not the provider's concern)\n\
             [Role] ASP (Agent Service Provider)\n\n\
             Silently ignore; end this turn.\n".to_string(),

        Event::WakeupNotify => {
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
             onchainos agent next-action --role provider --agentId {agent_id} --message '{{\"event\":\"<value of the message.jobStatus field>\",\"jobId\":\"{job_id}\"}}'\n\
             ```\n\
             Follow the returned script for what to do in the current status.\n\n\
             ⚠️ **Do NOT** okx-a2a xmtp-send the User Agent something like `I'm back online` — the peer does not care about your connection status.\n\
             ⚠️ If the Step 2 script is a passive-wait kind (e.g. status=accepted: ASP is working / status=submitted: waiting for User Agent review), only emit a `task resumed` notification and end the turn; do not proactively run business actions.\n"
            )
        }

        // Negotiation relay events are only used by the buyer side; provider ignores
        Event::NegotiateReply
        | Event::NegotiateAck
        | Event::NegotiateCounter => "[System notification] negotiate_* (buyer-side negotiation relay event; not the provider's concern)\n\
             [Recommendation] Ignore; no action needed.\n".to_string(),

        Event::AttachmentAdded | Event::DeliverableReceived => "[System notification] buyer-side event; not the provider's concern.\n\
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
                     onchainos agent next-action --role provider --agentId {agent_id} --message '{{\"event\":\"<dispute_raise|agree_refund>\",\"jobId\":\"{job_id}\"}}'\n\
                     ```\n\
                     If the reply is **truly ambiguous** (e.g. non-committal `OK` / `sure` / `hmm` — could mean either), these are irreversible on-chain actions — **do NOT guess**. Re-ask via `pending-decisions-v2 request` with the same `--to-agent-id` and `--source-event job_rejected`. **`--user-content` must be localized to the user's language**. Reference (English): \"I didn't catch your reply, please clarify: A=file dispute  B=accept refund\".\n"
                ),
                "submit_deadline_warn" => format!(
                    "[User decision relay] source_event=`submit_deadline_warn`, user's verbatim reply: `{reply}`\n\n\
                     **Semantic mapping** — decide which intent the user's reply means:\n\n\
                     \x20\x20• **Submit now** — user wants to deliver immediately (typical intents: 立即提交 / 我提交 / submit now / I'll deliver / ready / 现在交). Route: call `onchainos agent next-action --role provider --agentId {agent_id} --message '{{\"event\":\"job_accepted\",\"jobId\":\"{job_id}\"}}'` and run its Step 2-3 (skip Step 1 apply-accepted notification — user already knows).\n\
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
                     ⚠️ If the reply is truly ambiguous (e.g. unrelated chitchat / a non-committal `hmm` / `got it`), re-ask via `pending-decisions-v2 request` with the same `--to-agent-id` and `--source-event cli_failed`. **`--user-content` must be localized to the user's language** (detect from the user's verbatim reply / prior turn) before sending. Reference (English): \"I didn't catch your reply, please clarify: A=retry  B=stop prompting  C=tell me what to change\".\n"
                ),
                _ => format!(
                    "[User decision relay] source_event=`{source}` (no specific routing rule defined for this scene), user's verbatim reply: `{reply}`\n"
                ),
            }
        }

        // job_provider_reject: off-chain receipt confirming this ASP's own asp-reject;
        // no provider-side action needed (the buyer side handles the re-route).
        Event::JobProviderReject => format!("[System notification] job_provider_reject (your decline was registered; no further action).\n"),
        Event::JobUserReject => {
            let user_notify = super::content::job_user_reject_notify(job_id);
            let l10n = super::content::L10N_DISPATCH_SHORT;
            format!(
                "[Current state] job_user_reject (buyer declined to fund / confirm-accept)\n\
                 [Role] ASP (Agent Service Provider)\n\n\
                 **Notify the user, then end the turn** (🌐 translate template to user's language first):\n\
                 {user_notify}\n\
                 {l10n}\n\n\
                 ```bash\n\
                 okx-a2a user notify --content \"<translated text>\"\n\
                 ```\n\
                 ❌ Do NOT okx-a2a xmtp-send the buyer. ❌ Do NOT retry apply.\n"
            )
        }
        Event::Other(ref other) => format!("[Unknown state] {other}\n"),
    }
}
