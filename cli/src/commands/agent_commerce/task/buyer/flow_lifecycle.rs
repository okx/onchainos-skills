//! Prompt generators for task execution + arbitration + terminal states.
//!
//! Lifecycle events split out from `flow.rs`:
//! - provider_applied / job_accepted / job_submitted
//! - job_refused / job_disputed / dispute_evidence / approve_review / reject_review
//! - job_completed / dispute_resolved / job_refunded / job_auto_refunded / job_expired / job_closed
//! - submit_expired / refuse_expired / review_deadline_warn / review_expired / job_auto_completed
//! - reward_claimed / wakeup_notify / create_task
//! - task_token_budget_change / task_provider_change
//! - staked/evaluator lifecycle / unknown fallback

use super::flow::FlowContext;

// --- Execution stage ----------------------------------------------------

pub(super) fn provider_applied(ctx: &FlowContext<'_>) -> String {
    let job_id = ctx.job_id;
    let agent_id = ctx.agent_id;

    format!(
    "[Current Status] provider_applied (ASP has submitted an on-chain apply)\n\
     [Role] User (User Agent)\n\n\
     [Your next actions (strict order)]\n\n\
     **Step 1 -- Fetch task info:**\n\
     ```bash\n\
     onchainos agent common context {job_id} --role buyer --agent-id {agent_id}\n\
     ```\n\
     Extract: providerAgentId, paymentMode, tokenSymbol, tokenAmount.\n\
     ⚠️ paymentMode should be escrow (1) at this point.\n\n\
     **Step 2 -- Run confirm-accept (settle the accept on-chain):**\n\
     ```bash\n\
     onchainos agent confirm-accept {job_id} --provider-agent-id <providerAgentId> --payment-mode escrow --token-symbol <tokenSymbol> --token-amount <tokenAmount>\n\
     ```\n\
     ⚠️ The flag is `--provider-agent-id`, not `--agent-id`.\n\
     🛑 **provider-agent-id MUST match the sender.agentId of the ASP's a2a-agent-chat message** -- take it from the ASP message received in this turn first, then fall back to the [intent:ack] entry in sub-session history. Do not use the value from common context (it can cross-pollute under multi-task scenarios).\n\
     ⚠️ **Do not query the task API to verify whether the ASP has applied** -- on-chain indexing has a delay; `confirm-accept` performs the chain-side check internally.\n\
     ❌ Do not call apply (apply is a provider action; the user never runs it).\n\
     ❌ Do not call set-payment-mode (already done in the negotiate_ack event).\n\n\
     → After running, **end this turn** and wait for the `job_accepted` system notification.\n"
    )
}

pub(super) fn job_accepted(ctx: &FlowContext<'_>) -> String {
    let job_id = ctx.job_id;
    let agent_id = ctx.agent_id;
    let title_display = ctx.title_display;
    let title_in_extract = ctx.title_in_extract;

    let accepted_escrow_notify = super::content::job_accepted_escrow_user_notify(job_id, title_display);
    let accepted_x402_fail = super::content::job_accepted_x402_replay_fail_user_notify(job_id);
    format!(
    "[Current Status] job_accepted (user has confirmed accept; task enters execution stage)\n\
     [Role] User (User Agent)\n\n\
     🛑 **You MUST call `xmtp_dispatch_user` to notify the user; do not produce a plain text reply inside the sub session** (see Hard Rule 10).\n\n\
     [Your next actions (strict order)]\n\n\
     **Step 1 -- Fetch full task info:**\n\
     ```bash\n\
     onchainos agent common context {job_id} --role buyer --agent-id {agent_id}\n\
     ```\n\
     Extract: {title_in_extract}description, providerAgentId, paymentMode (int: 1=escrow, 3=x402), tokenAmount, tokenSymbol.\n\n\
     **Step 2 -- Branch by payment mode:**\n\n\
     --------- Branch A: escrow ---------\n\n\
     Call xmtp_dispatch_user to notify the user that accept succeeded:\n\
     \x20\x20content:\n\
     {accepted_escrow_notify}\n\n\
     [Follow-up events]\n\
     - job_submitted → review the deliverable\n\n\
     --------- Branch B: x402 ---------\n\n\
     In x402 mode, accept has already been settled on-chain (funds paid); task-402-pay was executed in the previous turn (job_payment_mode_changed).\n\n\
     **B-Step 1 -- Determine replaySuccess from the previous turn's task-402-pay:**\n\
     Look up the task-402-pay output in this sub session context.\n\
     ⚠️ If it cannot be found (e.g. lost due to context compaction), **default to replaySuccess=true** --\n\
     x402 funds are paid during accept, the user was already notified of the delivery result (success or failure) in the previous turn,\n\
     and skipping complete would leave the task stuck in accepted forever.\n\n\
     **B-Branch 1: replaySuccess=true (or default when context is missing)**\n\n\
     **B-Step 2 -- Run complete (single sign):**\n\
     ```bash\n\
     onchainos agent complete {job_id}\n\
     ```\n\
     (Internally: POST /priapi/v1/aieco/task/{job_id}/direct/complete → get calldata → sign uopHash → broadcast on-chain.)\n\n\
     ⚠️ **Do not notify the user** -- the deliverable was already sent after task-402-pay; the final summary is owned by the job_completed event.\n\n\
     ⚠️ **complete failure fallback**: if `onchainos agent complete` returns an error (CLI output contains `\"ok\": false` or stderr error),\n\
     call xmtp_dispatch_user to notify the user and provide a retry command:\n\
     \x20\x20content: complete failed for this task; please retry later. Retry command: onchainos agent complete {job_id}\n\
     → **End this turn** and wait for user retry or a wakeup_notify event.\n\n\
     **B-Branch 2: replaySuccess=false (only take this branch when replaySuccess=false is explicitly found in context)**\n\n\
     ⚠️ **Do not run complete** -- the user did not receive the deliverable.\n\n\
     **B-Step 2 -- Notify the user of replay failure:**\n\
     Call xmtp_dispatch_user:\n\
     \x20\x20content:\n\
     {accepted_x402_fail}\n\n\
     [Follow-up events]\n\
     - replaySuccess=true / default: job_completed → final confirmation\n\
     - replaySuccess=false: wait for user instructions (retry or close task)\n\n\
     🛑🛑🛑 **job_completed MANDATORY rule**:\n\
     After complete is settled on-chain, a `job_completed` system event will arrive.\n\
     Upon receiving `job_completed`, you **MUST** call:\n\
     ```bash\n\
     onchainos agent next-action --jobid {job_id} --jobStatus job_completed --role buyer --agentId {agent_id}\n\
     ```\n\
     Follow the returned playbook (it will guide you to notify the user that the job is complete).\n\
     ❌ **NEVER** ignore the `job_completed` event -- ignoring it = user never learns the job is done.\n\
     ❌ **NEVER** skip `next-action` and compose the completion notice yourself -- the playbook contains the full summary.\n"
    )
}

pub(super) fn job_submitted(ctx: &FlowContext<'_>) -> String {
    let job_id = ctx.job_id;
    let agent_id = ctx.agent_id;
    let short_id = ctx.short_id;
    let terminal_session_hint = ctx.terminal_session_hint;

    format!(
    "[Current Status] job_submitted (ASP has submitted the deliverable)\n\
     [Role] User (User Agent)\n\n\
     🛑🛑🛑 **ABSOLUTE REQUIREMENT -- in escrow mode you MUST use `xmtp_prompt_user` (NOT `xmtp_dispatch_user`) to push the review decision to the user session**.\n\
     `xmtp_dispatch_user` is a pure notification: user replies cannot be relayed back to the sub session → the review flow deadlocks.\n\
     Only `xmtp_prompt_user` can carry llmContent + userContent so the user session can relay the review decision back.\n\
     🔴 Real incident: a Minimax model received job_submitted, called xmtp_dispatch_user with \"the ASP has submitted; awaiting your review\" -- the user never saw the deliverable, could not relay a decision, and the task was stuck.\n\n\
     🛑🛑🛑 **Even if you already processed the ASP's a2a-agent-chat deliverable message earlier in this turn (e.g. called xmtp_file_download), upon receiving job_submitted you MUST still execute every Step below in full**.\n\
     Handling a2a-agent-chat (file download) != the review flow -- the review must be driven by the job_submitted playbook, and the deliverable content (file path / text) MUST be placed into userContent for the user to see.\n\n\
     🛑 **In escrow mode auto-approval is strictly forbidden**: you must wait for the user's relayed decision; the agent must not decide on behalf of the user, regardless of deliverable quality or how close to deadline.\n\
     ⚠️ In x402 mode: funds are already paid; just notify the user of the deliverable content, the user cannot reject.\n\n\
     [Your next actions (strict order)]\n\n\
     **Step 0 -- Idempotency check: query whether a pending decision already exists for this task:**\n\
     ```bash\n\
     onchainos agent pending-decisions list --format json --agent-id {agent_id}\n\
     ```\n\
     If the returned list already contains an entry with jobId={job_id} and role=buyer → **the user has already been notified; this is a duplicate event, end the turn without re-notifying.**\n\
     If not present → continue to Step 1.\n\n\
     **Step 1 -- Query task details; extract deliverable and payment mode:**\n\
     ```bash\n\
     onchainos agent status {job_id}\n\
     ```\n\
     Extract `paymentMode` (int: 1=escrow, 3=x402).\n\
     ⚠️ The status endpoint does not return deliverableUrl; extract that field from the chat history in Step 2. Get qualityStandards from `onchainos agent common context` (the value at task creation time is authoritative).\n\n\
     **Step 2 -- Fetch the deliverable content (distinguish text vs file):**\n\
     ⚠️ **The deliverable content MUST be extracted in this step and placed in full into Step 3's userContent** -- the earlier ASP message only triggered a short notification (\"waiting for on-chain confirmation\") and the user has not seen the deliverable body yet. **Do not omit, summarize, or just write \"already sent to you\".**\n\
     First call `session_status` to get the current sub session's sessionKey (reused later in Step 3; do not call it again in the same turn).\n\
     Then call `xmtp_get_conversation_history` (sessionKey = the value obtained above) to pull the chat history with the ASP and do two things:\n\
     \x20\x20a) From `onchainos agent common context {job_id} --role buyer --agent-id {agent_id}` extract `qualityStandards` (the review standard as of task creation); if empty, skip that line when rendering.\n\
     \x20\x20b) Find the ASP message **carrying the `[intent:deliver]` suffix tag** (scan from newest to oldest; the first match is the deliverable message), and branch on the `deliverableType` field:\n\n\
     --- Case A: deliverableType=file (message contains fileKey / digest / salt / nonce / secret decryption fields) ---\n\n\
     Call the xmtp_file_download tool:\n\
     \x20\x20Parameters:\n\
     \x20\x20- fileKey: fileKey returned by the ASP at upload\n\
     \x20\x20- agentId: {agent_id} (user agentId)\n\
     \x20\x20- digest: SHA-256 digest (hex)\n\
     \x20\x20- salt: encryption salt (base64)\n\
     \x20\x20- nonce: encryption nonce (base64)\n\
     \x20\x20- secret: encryption secret (base64)\n\
     \x20\x20- filename: (optional) save filename\n\
     ⚠️ Before calling, print: `[buyer-xmtp] xmtp_file_download: fileKey=<fileKey>, agentId={agent_id}`\n\
     ⚠️ After calling, print: `[buyer-xmtp] xmtp_file_download result: localPath=<returned local path>`\n\n\
     On success, record localPath; **it MUST be a full absolute path** (e.g. /Users/xxx/Downloads/task-staging.png).\n\
     ⚠️ **Never show only the filename** (e.g. cat-picture.png) -- the user cannot locate the file. Any later content shown to the user MUST include the full path.\n\
     If download fails → note in the display: \"file download failed, please ask the ASP to resend\".\n\
     ⚠️ If the ASP message contains text alongside the file (e.g. \"here is the deliverable, please check\"), capture it into deliverableText as well.\n\
     Deliverable display variables: deliverableType=file, localPath=<full path>, deliverableText=<note text, empty if none>\n\n\
     --- Case B: deliverableType=text (body content between `---` separators) ---\n\n\
     Extract the text between `---` separators in the `[intent:deliver]` message; **keep the original wording in full**, do not truncate or summarize.\n\
     Deliverable display variables: deliverableType=text, deliverableText=<full original text sent by the ASP>\n\n\
     **Step 3 -- Branch by payment mode:**\n\n\
     --------- Branch A: escrow -- user review decision required ---------\n\n\
     Call xmtp_prompt_user to push the deliverable and the review decision request to the user (reuse the sessionKey from Step 2; **before** calling `xmtp_prompt_user`, call `pending-decisions add` -- see Hard Rule 7):\n\n\
     \x20\x20\x20\x20llmContent: [USER_DECISION_REQUEST][sub_key: <full sessionKey from session_status>][job: {job_id}][role: buyer] \
     🛑 After presenting userContent, **you MUST end this turn and wait for real user input** -- [USER_DECISION_REQUEST] is a **question**, not an **answer**; do not fabricate a user decision in the same turn.\
     🛑 **Do not run** onchainos agent commands (complete/reject/status or any task CLI) -- you only present and relay, never execute on-chain actions.\
     **After the user's real reply arrives** (next turn):\
     User expresses affirmative intent (acceptance, agreement, satisfaction) → **only call** xmtp_dispatch_session(sessionKey=\"<full sessionKey from Step 2 session_status>\", content=\"[USER_DECISION_RELAY][intent:APPROVE_REVIEW] user said: <verbatim user reply, no interpretation, no translation>\") to relay back to the sub session, **and stop there** (the sub session will run the approve_review flow itself; do nothing else);\
     User expresses negative intent (dissatisfaction, refusal) with a reason → **only call** xmtp_dispatch_session(sessionKey=\"<same sessionKey>\", content=\"[USER_DECISION_RELAY][intent:REJECT_REVIEW] user said: <verbatim user reply, including reason>\") to relay back to the sub session, **and stop there** (the sub session will run the reject_review flow itself; do nothing else).\
     ⚠️ **Routing tag protocol**: `[intent:APPROVE_REVIEW]` / `[intent:REJECT_REVIEW]` MUST be inserted **verbatim, fully uppercase ASCII** -- **no translation / rewrite / omission / splitting** -- the sub branches on the intent tag, no longer on text matching, to avoid multilingual mismatch.\n\
     ⚠️ Relay MUST use the xmtp_dispatch_session tool (do not use sessions_send; it has session tree restrictions). ⚠️ xmtp_dispatch_session is called **exactly once**. {CONSTRAINT}\n\
     \x20\x20\x20\x20userContent (split by deliverableType):\n\n\
     \x20\x20\x20\x20▸ deliverableType=file:\n\
     \x20\x20\x20\x20[Job {short_id} — you are the User Agent] The ASP has submitted the deliverable (file); it has been downloaded locally.\n\
     \x20\x20\x20\x20Deliverable file path: <localPath> (full absolute path, e.g. /Users/xxx/Downloads/task.png)\n\
     \x20\x20\x20\x20<if deliverableText is non-empty, append: ASP note: <deliverableText>>\n\
     \x20\x20\x20\x20Deliverable URL: <deliverableUrl>\n\
     \x20\x20\x20\x20Quality standards: <qualityStandards>\n\
     \x20\x20\x20\x20Payment: escrow\n\
     \x20\x20\x20\x20Please choose:\n\
     \x20\x20\x20\x201. Approve the deliverable\n\
     \x20\x20\x20\x202. Reject the deliverable — please provide a reason\n\n\
     \x20\x20\x20\x20▸ deliverableType=text:\n\
     \x20\x20\x20\x20[Job {short_id} — you are the User Agent] The ASP has submitted the deliverable (text).\n\
     \x20\x20\x20\x20---Deliverable---\n\
     \x20\x20\x20\x20<deliverableText full content, no truncation, no summarization>\n\
     \x20\x20\x20\x20---End of deliverable---\n\
     \x20\x20\x20\x20Deliverable URL: <deliverableUrl>\n\
     \x20\x20\x20\x20Quality standards: <qualityStandards>\n\
     \x20\x20\x20\x20Payment: escrow\n\
     \x20\x20\x20\x20Please choose:\n\
     \x20\x20\x20\x201. Approve the deliverable\n\
     \x20\x20\x20\x202. Reject the deliverable — please provide a reason\n\n\
     ===============================================================\n\
     🛑🛑🛑 STOP -- after xmtp_prompt_user in Step 3 you **MUST end this turn**\n\
     ===============================================================\n\
     This playbook ends here. In a later turn, upon receiving `[USER_DECISION_RELAY]`,\n\
     you **MUST call `next-action`** to fetch the execution playbook — the playbook contains the on-chain command (`complete` or `reject`) that ONLY `next-action` can provide:\n\
     ▸ `[intent:APPROVE_REVIEW]` → `onchainos agent next-action --jobid {job_id} --jobStatus approve_review --role buyer --agentId {agent_id}`\n\
     ▸ `[intent:REJECT_REVIEW]` → `onchainos agent next-action --jobid {job_id} --jobStatus reject_review --role buyer --agentId {agent_id}`\n\
     Then execute the returned playbook in full (it will instruct you to run `onchainos agent complete` or `onchainos agent reject`).\n\
     ===============================================================\n\
     🔴🔴🔴 ABSOLUTE PROHIBITION upon receiving `[USER_DECISION_RELAY]`:\n\
     ❌ Do NOT call `xmtp_dispatch_session` — you are the sub session (executor), NOT the user session (relay). Dispatching = the approval is lost and `complete` never runs.\n\
     ❌ Do NOT call `pending-decisions remove` without first calling `next-action` — the returned playbook defines the correct order.\n\
     ❌ Do NOT skip `next-action` and improvise — without the playbook you will miss the `onchainos agent complete` command and funds will stay locked forever.\n\
     🔴 Real incident: a model received APPROVE_REVIEW, skipped next-action, called xmtp_dispatch_session to \"relay\" the approval — the on-chain complete was never executed, funds remained locked, and the user was told the job was approved when it was not.\n\
     ===============================================================\n\n\
     --------- Branch B: x402 -- notify the user (no rejection allowed) ---------\n\n\
     ⚠️ In x402 funds are already paid in the job_accepted stage; the user **cannot reject the deliverable**, just notify.\n\
     \n\
     **B-Step 1 -- Call xmtp_dispatch_user to notify the user (split by deliverableType):**\n\n\
     \x20\x20▸ deliverableType=file:\n\
     \x20\x20content:\n\
     \x20\x20[Deliverable Received] Job `{job_id}` — the ASP has submitted the deliverable (x402 mode; payment already settled).\n\
     \x20\x20Deliverable file path: <localPath> (full absolute path, e.g. /Users/xxx/Downloads/task.png)\n\
     \x20\x20<if deliverableText is non-empty, append: ASP note: <deliverableText>>\n\
     \x20\x20Deliverable URL: <deliverableUrl>\n\
     \x20\x20Quality standards: <qualityStandards>\n\n\
     \x20\x20▸ deliverableType=text:\n\
     \x20\x20content:\n\
     \x20\x20[Deliverable Received] Job `{job_id}` — the ASP has submitted the deliverable (x402 mode; payment already settled).\n\
     \x20\x20---Deliverable---\n\
     \x20\x20<deliverableText full content, no truncation, no summarization>\n\
     \x20\x20---End of deliverable---\n\
     \x20\x20Deliverable URL: <deliverableUrl>\n\
     \x20\x20Quality standards: <qualityStandards>\n\n\
     **B-Step 2 -- Terminal wrap-up (keep the sub session):**\n\
     {terminal_session_hint}\n\
     ⚠️ **Do not auto-rate** -- at the end of the notification, prompt the user: if they want to rate the ASP (0–5 stars), they can reply with their rating.\n\
     When the user replies with a rating intent, ask for a score (0–5 integer) and optional text feedback if not already provided, then execute:\n\
     ```bash\n\
     onchainos agent feedback-submit --agent-id <providerAgentId> --creator-id {agent_id} --score <0-5> --task-id {job_id} [--description \"<optional text>\"]\n\
     ```\n\
     ⚠️ `--score` MUST come from the user's explicit reply in this rating flow; do NOT infer from verbs like \"rate\" / \"打分\", do NOT use a default value.\n\
     ⚠️ `--agent-id` is the ASP being rated (providerAgentId from Step 1 context); `--creator-id` is the buyer's own agent id ({agent_id}).\n\
     Task fully complete.\n\n\
     [Follow-up events]\n\
     - escrow: job_completed → task complete / job_refused → wait for ASP to choose dispute or refund\n\
     - x402: flow ends here\n",
     CONSTRAINT = super::flow::PROMPT_USER_SESSION_CONSTRAINT)

}

// --- Rejection / arbitration -------------------------------------------

pub(super) fn job_refused(ctx: &FlowContext<'_>) -> String {
    let job_id = ctx.job_id;
    let title_display = ctx.title_display;
    let title_query_hint = ctx.title_query_hint;

    let refused_notify = super::content::job_refused_user_notify(job_id, title_display);
    format!(
    "[Current Status] job_refused (user rejection settled on-chain; awaiting ASP decision)\n\
     [Role] User (User Agent)\n\n\
     🛑 **You MUST call `xmtp_dispatch_user` to notify the user that rejection is settled; do not produce a plain text reply inside the sub session** (see Hard Rule 10).\n\n\
     [Your next actions (strict order)]\n\n\
     {title_query_hint}\
     **Step 1 -- Call xmtp_dispatch_user to notify the user the rejection is confirmed:**\n\n\
     content:\n\
     {refused_notify}\n\n\
     **Step 2 -- Silently wait for the ASP's decision:**\n\n\
     ⚠️ **Do not send any xmtp_send message to the ASP**. The ASP has 24h to decide:\n\
     - Open a dispute → you will receive job_disputed\n\
     - Agree to refund → you will receive job_refunded\n\
     - 24h timeout → system auto-refunds, you will receive job_refunded\n\n\
     After Step 1 → **end this turn** and wait for the next system event.\n\n\
     [Follow-up events]\n\
     - job_disputed → submit user evidence (Scene 6)\n\
     - job_refunded → refund complete\n"
    )
}

pub(super) fn job_disputed(ctx: &FlowContext<'_>) -> String {
    let job_id = ctx.job_id;
    let agent_id = ctx.agent_id;
    let short_id = ctx.short_id;

    let evidence_prompt = super::content::job_disputed_user_evidence_prompt(short_id);
    format!(
    "[Current Status] job_disputed (arbitration opened; 1-hour evidence preparation window)\n\
     [Role] User (User Agent)\n\n\
     🛑 **CRITICAL -- this event MUST use `xmtp_prompt_user` to push to the user session; do not produce a plain text reply inside the sub session.**\n\
     The sub session is not user-facing -- generating a text reply in the sub session (even if the content is correct) = user does not see it + relay channel broken + evidence cannot be submitted.\n\
     The only correct approach: call the `xmtp_prompt_user(llmContent=..., userContent=...)` tool to push the evidence-collection request into the user session.\n\
     ❌ Do not substitute a text reply for the xmtp_prompt_user tool call.\n\
     ❌ Do not substitute xmtp_dispatch_user for xmtp_prompt_user (dispatch_user is a pure notification and cannot relay; user replies cannot be routed back to the sub).\n\
     ❌ Do not fabricate an evidence summary and call `dispute upload` directly -- the sub agent does not know what evidence the user has.\n\
     ❌ Do not xmtp_send any message to the ASP -- during arbitration both sides interact via on-chain evidence.\n\n\
     [Your next actions (strict order)]\n\n\
     **Step 0 -- Idempotency check: query whether a pending decision already exists for this task:**\n\
     ```bash\n\
     onchainos agent pending-decisions list --format json --agent-id {agent_id}\n\
     ```\n\
     If the returned list already contains an entry with jobId={job_id} and role=buyer → **the user has already been notified; this is a duplicate event, end the turn without re-notifying.**\n\
     If not present → continue to Step 1.\n\n\
     **Step 1 -- Call xmtp_prompt_user to push the evidence decision request to the user:**\n\n\
     First call `session_status` to get the current sub session's sessionKey; **before** calling `xmtp_prompt_user`, call `pending-decisions add` (see Hard Rule 7).\n\n\
     \x20\x20\x20\x20llmContent: [USER_DECISION_REQUEST][sub_key: <full sessionKey from session_status>][job: {job_id}][role: buyer] \
     🛑 After presenting userContent, **you MUST end this turn and wait for real user input** -- [USER_DECISION_REQUEST] is a **question**, not an **answer**; do not fabricate a user decision in the same turn.\
     🛑 **Do not run** onchainos agent commands (complete/reject/dispute or any task CLI) -- you only present and relay, never execute on-chain actions.\
     **After the user's real reply arrives** (next turn):\
     Once the user provides evidence, call xmtp_dispatch_session(sessionKey=\"<full sessionKey from session_status>\", content=\"[USER_DECISION_RELAY][intent:SUBMIT_EVIDENCE] user evidence: <full verbatim user content -- text + image paths -- no interpretation, no translation>\") to relay back to the sub session, which will run dispute upload. ⚠️ **Routing tag protocol**: `[intent:SUBMIT_EVIDENCE]` MUST be inserted **verbatim, fully uppercase ASCII**; no translation/rewrite/omission. ⚠️ Relay MUST use xmtp_dispatch_session (do not use sessions_send). ⚠️ xmtp_dispatch_session is called **exactly once**. Evidence MUST be submitted within 1 hour. {CONSTRAINT}\n\
     \x20\x20\x20\x20userContent:\n\
     {evidence_prompt}\n\n\
     **Step 2 -- Wait for the user reply to be relayed back**: upon receiving `[USER_DECISION_RELAY][intent:SUBMIT_EVIDENCE] user evidence: ...`, call `next-action --jobStatus dispute_evidence` to fetch the upload playbook (the intent tag already confirms routing; read the user evidence body after `user evidence:`).\n\n\
     ⚠️ Evidence MUST be submitted within 1 hour, otherwise it expires.\n\n\
     After Step 1-2 → **end this turn** and wait for the user reply.\n",
     CONSTRAINT = super::flow::PROMPT_USER_SESSION_CONSTRAINT)
}

pub(super) fn dispute_evidence(ctx: &FlowContext<'_>) -> String {
    let job_id = ctx.job_id;
    let agent_id = ctx.agent_id;

    format!(
    "[Current Action] Upload arbitration evidence\n\
     [Role] User (User Agent)\n\n\
     **Step 0 -- Clear pending-decisions:**\n\
     ```bash\n\
     onchainos agent pending-decisions remove --job-id {job_id} --role buyer --agent-id {agent_id}\n\
     ```\n\n\
     **Step 1 -- Extract evidence content from the relay:**\n\
     Already routed in via `[USER_DECISION_RELAY][intent:SUBMIT_EVIDENCE]`. Extract from the part after `user evidence:`:\n\
     - text summary → the text the user provided\n\
     - image path (if provided) → `--image` argument\n\
     At least one of text and image is required.\n\n\
     **Step 2 -- Pull the negotiation / delivery chat history of this sub session and prepend it to the text as objective evidence:**\n\
     Call `xmtp_get_conversation_history` (sessionKey = this sub session's sessionKey) to get the full a2a-agent-chat history with the ASP.\n\
     Stitch the history as a **structured section** at the top of the `--text` field (the arbiter is an LLM and reads through the text field), then append the user summary below:\n\n\
     ```\n\
     ==== Negotiation / delivery chat history (from xmtp_get_conversation_history) ====\n\
     [time] ASP(<agentId>): ...\n\
     [time] User(<agentId>): ...\n\
     ... (chronological; key checkpoints: quote / [intent:propose] / [intent:ack] / [intent:confirm] / deliverable message)\n\n\
     ==== User evidence summary ====\n\
     <verbatim user summary>\n\
     ```\n\n\
     ⚠️ **`--text` is capped at 16 KB** -- if the chat history is long, **keep only** the key checkpoints (PROPOSE / ACK / CONFIRM / deliverable / both sides' key dispute points) and prepend \"(key checkpoints extracted)\"; do not blindly drop the first N entries.\n\n\
     **Step 3 -- Call the CLI to upload evidence (off-chain multipart):**\n\
     ```bash\n\
     onchainos agent dispute upload {job_id} --agent-id {agent_id} --text \"<chat history + user summary, concatenated>\" --image <user image path or omit>\n\
     ```\n\
     At least one of text and image is required; to omit an image, drop the entire `--image` segment -- do not pass an empty string.\n\n\
     ⚠️ **Do not xmtp_send any message to the ASP** (e.g. \"evidence submitted\"); the ASP learns via on-chain events.\n\n\
     [Follow-up events]\n\
     - job_completed → arbitration ruled for the ASP, task completes\n\
     - job_refunded → arbitration ruled for the user, refund\n\n\
     After Step 1-3 → **end this turn; do not push to main via xmtp_dispatch_user / xmtp_prompt_user**.\n"
    )
}

pub(super) fn approve_review(ctx: &FlowContext<'_>) -> String {
    let job_id = ctx.job_id;
    let agent_id = ctx.agent_id;

    format!(
    "[Current Action] Approve review -- run complete to release funds\n\
     [Role] User (User Agent)\n\n\
     🛑🛑🛑 You are the **sub session** (executor). Your job is to run the on-chain `complete` command below — NOT to relay, forward, or dispatch the decision.\n\
     ❌ Do NOT call `xmtp_dispatch_session` — that is the user-session agent's tool, not yours.\n\
     ❌ Do NOT skip Step 2 (`onchainos agent complete`) — skipping it = funds stay locked forever.\n\n\
     Routed in via `[USER_DECISION_RELAY][intent:APPROVE_REVIEW]`; the user has approved the deliverable.\n\n\
     **Step 1 -- Clear pending-decisions:**\n\
     ```bash\n\
     onchainos agent pending-decisions remove --job-id {job_id} --role buyer --agent-id {agent_id}\n\
     ```\n\n\
     **Step 2 -- Dual-signature approval, release funds:**\n\
     ```bash\n\
     onchainos agent complete {job_id}\n\
     ```\n\
     Internal flow:\n\
     \x20\x201. POST /priapi/v1/aieco/task/{job_id}/pre-complete (EIP-712 standard, not uop) → get digest\n\
     \x20\x202. ED25519 sign digest → signature\n\
     \x20\x203. POST /priapi/v1/aieco/task/{job_id}/complete (body: {{\"signature\": \"<sig>\"}}) → get uopData\n\
     \x20\x204. Sign uopHash → broadcast on-chain\n\
     \x20\x20→ Task status becomes Complete; funds released from contract to the ASP.\n\n\
     🛑 **CLI success of complete != task ended** -- `complete` only submits the on-chain transaction; **the user has not been notified that the task is complete**.\n\
     Do not xmtp_dispatch_user / xmtp_prompt_user here -- after on-chain confirmation you will receive the `job_completed` system event (`source:\"system\"`),\n\
     and that event's playbook is responsible for notifying the user via xmtp_dispatch_user. Notifying here = duplicate card.\n\
     Remember the txHash from the CLI output; the `job_completed` playbook will use it.\n\n\
     After Step 1-2 → **end this turn**.\n\
     ⚠️ **Your work is not finished** -- when the `job_completed` system event (`source:\"system\"`) arrives, you MUST handle it per SKILL.md Activation rules,\n\
     otherwise the user will never receive a \"task complete\" notification and will not know funds have been released.\n"
    )
}

pub(super) fn reject_review(ctx: &FlowContext<'_>) -> String {
    let job_id = ctx.job_id;
    let agent_id = ctx.agent_id;

    format!(
    "[Current Action] Reject review -- run reject\n\
     [Role] User (User Agent)\n\n\
     Routed in via `[USER_DECISION_RELAY][intent:REJECT_REVIEW]`; the user has rejected the deliverable.\n\
     Extract the rejection reason from the relay message after `user said:`.\n\n\
     **Step 1 -- Clear pending-decisions:**\n\
     ```bash\n\
     onchainos agent pending-decisions remove --job-id {job_id} --role buyer --agent-id {agent_id}\n\
     ```\n\n\
     **Step 2 -- Dual-signature rejection:**\n\
     ```bash\n\
     onchainos agent reject {job_id} --reason \"<rejection reason from user's words>\"\n\
     ```\n\
     Internal flow:\n\
     \x20\x201. POST /priapi/v1/aieco/task/{job_id}/pre-refuse (EIP-712 standard, not uop) → get digest\n\
     \x20\x202. ED25519 sign digest → signature\n\
     \x20\x203. POST /priapi/v1/aieco/task/{job_id}/refuse (body: {{\"signature\": \"<sig>\", \"reason\": \"<reason>\"}}) → get uopData\n\
     \x20\x204. Sign uopHash → broadcast on-chain\n\
     \x20\x20→ Task status becomes Refused; the ASP can open a dispute within 24h.\n\n\
     ⚠️ **Do not xmtp_send any message to the ASP** (e.g. \"rejected\"); the ASP learns via on-chain events.\n\n\
     After Step 1-2 → **end this turn** and wait for the `job_refused` system notification.\n"
    )
}

// --- Terminal states ---------------------------------------------------

pub(super) fn job_completed(ctx: &FlowContext<'_>) -> String {
    let job_id = ctx.job_id;
    let agent_id = ctx.agent_id;
    let title_display = ctx.title_display;
    let title_in_extract = ctx.title_in_extract;
    let terminal_session_hint = ctx.terminal_session_hint;

    let completed_escrow_notify = super::content::job_completed_escrow_user_notify(job_id, title_display);
    let completed_x402_notify = super::content::job_completed_x402_user_notify(job_id, title_display);
    format!(
    "[Current Status] job_completed (task payment pipeline complete)\n\
     [Role] User (User Agent)\n\n\
     🛑🛑🛑 **ABSOLUTE REQUIREMENT -- on job_completed the buyer MUST call `xmtp_dispatch_user` to notify the user**.\n\
     job_completed is a **dual-recipient event** (buyer + provider both receive it); the buyer MUST handle it.\n\
     Do not produce a plain text reply inside the sub session (see Hard Rule 10) -- a text reply = the user does not see it = the task is complete but the user does not know.\n\
     🔴 Real incident: a model assumed job_completed only went to the provider, skipped xmtp_dispatch_user, and the user never received a completion notification.\n\n\
     **Step 1 -- Fetch task info and payment mode:**\n\
     ```bash\n\
     onchainos agent common context {job_id} --role buyer --agent-id {agent_id}\n\
     ```\n\
     Extract: {title_in_extract}tokenAmount, tokenSymbol, paymentMode (int: 1=escrow, 3=x402).\n\n\
     **Step 2 -- Branch by payment mode:**\n\n\
     --------- Branch A: escrow -- flow ends ---------\n\n\
     In escrow mode, job_completed means the ASP has delivered and the user has approved; funds are released from contract to the ASP.\n\n\
     **A-Step 1 -- 🛑 MUST call `xmtp_dispatch_user` tool (do NOT produce a plain text reply):**\n\
     🛑🛑🛑 You are in a **sub session (backup)**. Any text you output here is invisible to the user.\n\
     The ONLY way to reach the user is the `xmtp_dispatch_user` tool call.\n\
     ❌ Do NOT output the notification as text — it will be trapped in the backup session and the user will never see it.\n\
     ⚠️ txHash: find the txHash (format 0x...) from the earlier `onchainos agent complete` CLI output in this sub session context.\n\
     If not in context (e.g. auto-complete or other non-active-approval scenarios), omit the on-chain receipt line.\n\
     ✅ Call xmtp_dispatch_user with the following content parameter (replace placeholders with real values):\n\
     \x20\x20content:\n\
     {completed_escrow_notify}\n\n\
     **A-Step 2 -- Terminal wrap-up (keep the sub session):**\n\
     {terminal_session_hint}\n\
     ⚠️ **Do not auto-rate** -- at the end of the notification, prompt the user: if they want to rate the ASP (0–5 stars), they can reply with their rating.\n\
     When the user replies with a rating intent, ask for a score (0–5 integer) and optional text feedback if not already provided, then execute:\n\
     ```bash\n\
     onchainos agent feedback-submit --agent-id <providerAgentId> --creator-id {agent_id} --score <0-5> --task-id {job_id} [--description \"<optional text>\"]\n\
     ```\n\
     ⚠️ `--score` MUST come from the user's explicit reply in this rating flow; do NOT infer from verbs like \"rate\" / \"打分\", do NOT use a default value.\n\
     ⚠️ `--agent-id` is the ASP being rated (providerAgentId from Step 1 context); `--creator-id` is the buyer's own agent id ({agent_id}).\n\
     Task fully complete.\n\n\
     --------- Branch B: x402 -- final summary ---------\n\n\
     ⚠️ In x402, job_completed means the payment pipeline (accept + complete) is settled on-chain.\n\
     The deliverable was already sent to the user during task-402-pay (A-Step 4); this step only emits the final summary.\n\n\
     **B-Step 1 -- 🛑 MUST call `xmtp_dispatch_user` tool (do NOT produce a plain text reply):**\n\
     🛑🛑🛑 You are in a **sub session (backup)**. Any text you output here is invisible to the user.\n\
     The ONLY way to reach the user is the `xmtp_dispatch_user` tool call.\n\
     ❌ Do NOT output the notification as text — it will be trapped in the backup session and the user will never see it.\n\
     ✅ Call xmtp_dispatch_user with the following content parameter (replace placeholders with real values from Step 1):\n\
     \x20\x20content:\n\
     {completed_x402_notify}\n\n\
     **B-Step 2 -- Terminal wrap-up (keep the sub session):**\n\
     {terminal_session_hint}\n\
     Task fully complete.\n\
     🛑 Final check: if you did NOT call `xmtp_dispatch_user` in B-Step 1, go back and call it now. A text reply is NOT a substitute.\n"
    )
}

pub(super) fn dispute_resolved(ctx: &FlowContext<'_>) -> String {
    let job_id = ctx.job_id;
    let agent_id = ctx.agent_id;
    let title_display = ctx.title_display;
    let title_in_extract = ctx.title_in_extract;
    let terminal_session_hint = ctx.terminal_session_hint;

    let dispute_won = super::content::dispute_won_user_notify(job_id, title_display);
    let dispute_lost = super::content::dispute_lost_user_notify(job_id, title_display);
    format!(
    "[Current Status] dispute_resolved (arbitration ruling issued)\n\
     [Role] User (User Agent)\n\n\
     🛑 **You MUST call `xmtp_dispatch_user` to notify the user of the arbitration result; do not produce a plain text reply inside the sub session** (see Hard Rule 10).\n\n\
     **Step 1 -- Decide winner**: read `message.jobStatus` from the system notification envelope:\n\
     - `jobStatus = \"rejected\"` → **user wins**\n\
     - `jobStatus = \"complete\"` → **user loses**\n\
     - other values (e.g. `disputed`) → cannot decide directly; run Step 1.5 to query task details\n\n\
     **Step 1.5 (only when jobStatus is not rejected/complete) -- Query task details for the actual status:**\n\
     ```bash\n\
     onchainos agent status {job_id}\n\
     ```\n\
     Decide by the returned `jobStatus` field: `rejected` = user wins, `complete` = user loses.\n\n\
     **Step 2 -- Fetch task info:**\n\
     ```bash\n\
     onchainos agent common context {job_id} --role buyer --agent-id {agent_id}\n\
     ```\n\
     Extract {title_in_extract}tokenAmount, tokenSymbol.\n\n\
     **Step 3 -- Call xmtp_dispatch_user to notify the user of the arbitration outcome (branch by winner):**\n\n\
     -------------- User wins (jobStatus=rejected) --------------\n\
     content:\n\
     {dispute_won}\n\n\
     -------------- User loses (jobStatus=complete) --------------\n\
     content:\n\
     {dispute_lost}\n\n\
     **Step 4 -- Terminal wrap-up (keep the sub session):**\n\
     {terminal_session_hint}\n\
     ⚠️ **Do not auto-rate** -- the notification already includes a rating prompt; wait for the user to reply with their rating.\n\
     When the user replies with a rating intent, ask for a score (0–5 integer) and optional text feedback if not already provided, then execute:\n\
     ```bash\n\
     onchainos agent feedback-submit --agent-id <providerAgentId> --creator-id {agent_id} --score <0-5> --task-id {job_id} [--description \"<optional text>\"]\n\
     ```\n\
     ⚠️ `--score` MUST come from the user's explicit reply in this rating flow; do NOT infer from verbs like \"rate\" / \"打分\", do NOT use a default value.\n\
     ⚠️ `--agent-id` is the ASP being rated (providerAgentId from Step 2 context); `--creator-id` is the buyer's own agent id ({agent_id}).\n\
     Arbitration flow fully complete.\n"
    )
}

pub(super) fn job_refunded(ctx: &FlowContext<'_>) -> String {
    let job_id = ctx.job_id;
    let terminal_session_hint = ctx.terminal_session_hint;

    let refunded_notify = super::content::job_refunded_user_notify(job_id);
    format!(
    "[Current Status] job_refunded (funds refunded to the user)\n\
     [Role] User (User Agent)\n\n\
     🛑 **You MUST call `xmtp_dispatch_user` to notify the user that the refund completed; do not produce a plain text reply inside the sub session** (see Hard Rule 10).\n\n\
     [Your next actions (strict order)]\n\n\
     **Step 1 -- Call xmtp_dispatch_user to notify the user the refund completed:**\n\n\
     content:\n\
     {refunded_notify}\n\n\
     **Step 2 -- Terminal wrap-up (keep the sub session):**\n\
     {terminal_session_hint}\n\
     Refund flow fully complete.\n"
    )
}

pub(super) fn job_auto_refunded(ctx: &FlowContext<'_>) -> String {
    let job_id = ctx.job_id;
    let title_display = ctx.title_display;
    let title_query_hint = ctx.title_query_hint;
    let terminal_session_hint = ctx.terminal_session_hint;

    let auto_refunded_notify = super::content::job_auto_refunded_user_notify(job_id, title_display);
    format!(
    "[System Notification] job_auto_refunded (claimAutoRefund tx receipt)\n\
     [Role] User (User Agent)\n\n\
     🛑 **You MUST call `xmtp_dispatch_user` to notify the user the refund has arrived; do not produce a plain text reply inside the sub session** (see Hard Rule 10).\n\n\
     [Your next actions (strict order)]\n\n\
     {title_query_hint}\
     **Step 1 -- Call xmtp_dispatch_user to notify the user the refund has arrived:**\n\n\
     content:\n\
     {auto_refunded_notify}\n\n\
     **Step 2 -- Terminal wrap-up (keep the sub session):**\n\
     {terminal_session_hint}\n\
     Refund flow fully complete.\n"
    )
}

pub(super) fn job_expired(ctx: &FlowContext<'_>) -> String {
    let job_id = ctx.job_id;

    let expired_notify = super::content::job_expired_user_notify(job_id);
    format!(
    "[Current Status] job_expired (task expired; no ASP accepted or no submission)\n\
     [Role] User (User Agent)\n\n\
     [Your next actions]\n\n\
     **Step 1 -- Call xmtp_dispatch_user to notify the user the task expired:**\n\
     \x20\x20content: {expired_notify}\n\n\
     This task reached a terminal state; the flow ends.\n"
    )
}

pub(super) fn job_closed(ctx: &FlowContext<'_>) -> String {
    let job_id = ctx.job_id;
    let title_display = ctx.title_display;
    let title_query_hint = ctx.title_query_hint;
    let terminal_session_hint = ctx.terminal_session_hint;

    let closed_notify = super::content::job_closed_user_notify(job_id, title_display);
    format!(
    "[Current Status] job_closed (close tx result notification)\n\
     [Role] User (User Agent)\n\n\
     [Your next actions]\n\n\
     {title_query_hint}\
     **Step 1 -- Call xmtp_dispatch_user to notify the user:**\n\
     \x20\x20content: {closed_notify}\n\n\
     **Terminal wrap-up (keep the sub session):**\n\
     {terminal_session_hint}\n\
     Close flow ends.\n"
    )
}

// --- Timeouts / auto-completion ---------------------------------------

pub(super) fn submit_expired(ctx: &FlowContext<'_>) -> String {
    let job_id = ctx.job_id;

    let submit_expired = super::content::submit_expired_user_notify(job_id);
    format!(
    "[System Notification] ASP failed to submit the deliverable in time\n\
     [Role] User (User Agent)\n\n\
     🛑 **You MUST call `xmtp_dispatch_user` to notify the user; do not produce a plain text reply inside the sub session** (see Hard Rule 10).\n\
     The ASP did not submit the deliverable within the allowed window; auto-refund kicks in.\n\n\
     **Step 1 -- Claim auto-refund immediately (no user confirmation needed):**\n\
     ```bash\n\
     onchainos agent claim-auto-refund {job_id}\n\
     ```\n\n\
     **Step 2 -- Call xmtp_dispatch_user to notify the user:**\n\
     content: \"{submit_expired}\"\n"
    )
}

pub(super) fn refuse_expired(ctx: &FlowContext<'_>) -> String {
    let job_id = ctx.job_id;

    let refuse_expired = super::content::refuse_expired_user_notify(job_id);
    format!(
    "[System Notification] ASP arbitration window expired\n\
     [Role] User (User Agent)\n\n\
     🛑 **You MUST call `xmtp_dispatch_user` to notify the user; do not produce a plain text reply inside the sub session** (see Hard Rule 10).\n\
     After your rejection, the ASP did not open a dispute in time; auto-refund kicks in.\n\n\
     **Step 1 -- Claim auto-refund immediately (no user confirmation needed):**\n\
     ```bash\n\
     onchainos agent claim-auto-refund {job_id}\n\
     ```\n\n\
     **Step 2 -- Call xmtp_dispatch_user to notify the user:**\n\
     content: \"{refuse_expired}\"\n"
    )
}

pub(super) fn review_deadline_warn(ctx: &FlowContext<'_>) -> String {
    let job_id = ctx.job_id;
    let agent_id = ctx.agent_id;
    let short_id = ctx.short_id;

    let review_deadline_prompt = super::content::review_deadline_warn_user_prompt(job_id, short_id);
    format!(
    "[System Notification] review_deadline_warn (review deadline approaching)\n\
     [Role] User (User Agent)\n\n\
     🛑 **CRITICAL -- this event MUST use `xmtp_prompt_user` to push to the user session; do not produce a plain text reply inside the sub session.**\n\
     Review deadline = user funds safety red line -- if the user is not notified, funds auto-release to the ASP on timeout, irreversibly.\n\
     ❌ Do not substitute a text reply for the xmtp_prompt_user tool call.\n\
     ❌ Do not substitute xmtp_dispatch_user for xmtp_prompt_user (the user must make a review decision; dispatch_user cannot relay).\n\n\
     [Your next actions (strict order)]\n\n\
     **Step 1 -- Idempotency check: query whether a pending decision already exists for this task:**\n\
     ```bash\n\
     onchainos agent pending-decisions list --format json --agent-id {agent_id}\n\
     ```\n\
     If the returned list already contains an entry with jobId={job_id} and role=buyer → **the user has already been notified; this is a duplicate event, end the turn without re-notifying.**\n\
     If not present → continue to Step 2.\n\n\
     **Step 2 -- Get the sessionKey and register the pending-decision (Hard Rule 7):**\n\
     First call `session_status` to get the sessionKey, then:\n\
     ```bash\n\
     onchainos agent pending-decisions add --sub-key <sessionKey> --job-id {job_id} --role buyer --agent-id {agent_id} --summary \"review deadline approaching\" --user-content \"[Review deadline reminder] The review deadline for task {job_id} is approaching. Once it expires, the ASP can auto-claim the funds. Please decide soon: A. Approve B. Reject the deliverable\"\n\
     ```\n\n\
     **Step 3 -- Call xmtp_prompt_user to notify the user the review deadline is approaching and request a decision:**\n\
     \x20\x20llmContent: [USER_DECISION_REQUEST][sub_key: <full sessionKey from session_status>][job: {job_id}][role: buyer] \
     🛑 After presenting userContent, **you MUST end this turn and wait for real user input** -- [USER_DECISION_REQUEST] is a **question**, not an **answer**; do not fabricate a user decision in the same turn.\
     🛑 **Do not run** onchainos agent commands (complete/reject/status or any task CLI) -- you only present and relay, never execute on-chain actions.\
     **After the user's real reply arrives** (next turn):\
     User expresses affirmative intent (acceptance, agreement, satisfaction) → call xmtp_dispatch_session(sessionKey=\"<full sessionKey from session_status>\", content=\"[USER_DECISION_RELAY][intent:APPROVE_REVIEW] user said: <verbatim user reply, no interpretation, no translation>\") to relay back to the sub session, which runs complete;\
     User expresses negative intent (dissatisfaction, refusal) with a reason → call xmtp_dispatch_session(sessionKey=\"<same sessionKey>\", content=\"[USER_DECISION_RELAY][intent:REJECT_REVIEW] user said: <verbatim user reply, including reason>\") to relay back to the sub session, which runs reject.\
     ⚠️ **Routing tag protocol**: `[intent:APPROVE_REVIEW]` / `[intent:REJECT_REVIEW]` MUST be inserted **verbatim, fully uppercase ASCII**, **no translation / rewrite / omission** -- the sub branches on the intent tag, not on text matching.\n\
     ⚠️ Relay MUST use xmtp_dispatch_session (do not use sessions_send). ⚠️ xmtp_dispatch_session is called **exactly once**. {CONSTRAINT}\n\
     \x20\x20userContent:\n\
     {review_deadline_prompt}\n\n\
     **Step 4 -- Upon receiving `[USER_DECISION_RELAY][intent:CODE] user said: ...`, route by intent code:**\n\
     First call `pending-decisions remove` (Hard Rule 7):\n\
     ```bash\n\
     onchainos agent pending-decisions remove --job-id {job_id} --role buyer --agent-id {agent_id}\n\
     ```\n\
     Then execute by intent code:\n\
     - `[intent:APPROVE_REVIEW]`:\n\
     ```bash\n\
     onchainos agent complete {job_id}\n\
     ```\n\
     - `[intent:REJECT_REVIEW]` (extract reason from the part after `user said:`):\n\
     ```bash\n\
     onchainos agent reject {job_id} --reason \"<rejection reason from user's words>\"\n\
     ```\n",
     CONSTRAINT = super::flow::PROMPT_USER_SESSION_CONSTRAINT)
}

pub(super) fn review_expired(ctx: &FlowContext<'_>) -> String {
    let job_id = ctx.job_id;

    let review_expired = super::content::review_expired_user_notify(job_id);
    format!(
    "[System Notification] review_expired (review window expired; task is still submitted)\n\
     [Role] User (User Agent)\n\n\
     🛑 **You MUST call `xmtp_dispatch_user` to notify the user the review window expired; do not produce a plain text reply inside the sub session** (see Hard Rule 10).\n\n\
     [Your next actions]\n\n\
     **Step 1 -- Call xmtp_dispatch_user to notify the user the review window expired:**\n\
     \x20\x20content:\n\
     {review_expired}\n\n\
     **Step 2** -- Wait for the `job_auto_completed` system notification and then wrap up.\n"
    )
}

pub(super) fn job_auto_completed(ctx: &FlowContext<'_>) -> String {
    let job_id = ctx.job_id;
    let title_display = ctx.title_display;
    let title_query_hint = ctx.title_query_hint;
    let terminal_session_hint = ctx.terminal_session_hint;

    let auto_completed_notify = super::content::job_auto_completed_user_notify(job_id, title_display);
    format!(
    "[System Notification] job_auto_completed (claimAutoComplete tx receipt)\n\
     [Role] User (User Agent)\n\n\
     🛑 **You MUST call `xmtp_dispatch_user` to notify the user the task auto-completed; do not produce a plain text reply inside the sub session** (see Hard Rule 10).\n\n\
     [Your next actions]\n\n\
     {title_query_hint}\
     **Step 1 -- Call xmtp_dispatch_user to notify the user the task auto-completed:**\n\
     \x20\x20content:\n\
     {auto_completed_notify}\n\n\
     {terminal_session_hint}\n"
    )
}

// --- User-action pseudo events ----------------------------------------

pub(super) fn close_task(ctx: &FlowContext<'_>) -> String {
    let job_id = ctx.job_id;

    let close_notify = super::content::close_user_notify(job_id);
    format!(
    "[Current Action] Close task\n\
     [Role] User (User Agent)\n\n\
     **Step 1 -- Close the task (only valid in Open state):**\n\
     ```bash\n\
     onchainos agent close {job_id}\n\
     ```\n\n\
     **Step 2 -- Notify the user:**\n\
     Call xmtp_dispatch_user:\n\
     content: \"{close_notify}\"\n"
    )
}

pub(super) fn set_public(ctx: &FlowContext<'_>) -> String {
    let job_id = ctx.job_id;

    let set_public_notify = super::content::set_public_user_notify(job_id);
    format!(
    "[Current Action] Convert to public task\n\
     [Role] User (User Agent)\n\n\
     **Step 1 -- Convert to public task:**\n\
     ```bash\n\
     onchainos agent set-public {job_id}\n\
     ```\n\n\
     **Step 2 -- Notify the user:**\n\
     Call xmtp_dispatch_user:\n\
     content: \"{set_public_notify}\"\n"
    )
}

// --- Other events ------------------------------------------------------

pub(super) fn submit_deadline_warn() -> String {
    "[System Notification] submit_deadline_warn (provider-side deadline reminder)\n\
     [Role] User (User Agent)\n\n\
     [Advice] Stay silent and observe; wait for the provider to submit the deliverable (job_submitted notification) before acting.\n".to_string()
}

pub(super) fn evaluator_events(event_str: &str) -> String {
    format!(
    "[System Notification] {event_str} (internal arbitration event, handled by evaluator)\n\
     [Role] User (User Agent)\n\n\
     [Advice] Stay silent and observe. After `dispute_resolved` arrives, call next-action to wrap up.\n"
    )
}

pub(super) fn reward_claimed(ctx: &FlowContext<'_>) -> String {
    let job_id = ctx.job_id;
    let title_display = ctx.title_display;
    let title_query_hint = ctx.title_query_hint;

    let reward_claimed = super::content::reward_claimed_user_notify(job_id, title_display);
    format!(
    "[System Notification] reward_claimed (claimRewards tx receipt)\n\
     [Role] User (User Agent)\n\n\
     [Your next actions]\n\n\
     {title_query_hint}\
     **Step 1 -- Call xmtp_dispatch_user to notify the user the reward has arrived:**\n\
     \x20\x20content: {reward_claimed}\n"
    )
}

pub(super) fn wakeup_notify(ctx: &FlowContext<'_>) -> String {
    let job_id = ctx.job_id;
    let agent_id = ctx.agent_id;

    let wakeup_resume = super::content::wakeup_resume_user_notify(job_id);
    format!(
    "[System Notification] wakeup_notify (task wake-up after network / machine restart)\n\
     [Role] User (User Agent)\n\n\
     ⚠️ This is a wake-up heartbeat event, **not** a business-driven event. The real business status lives in envelope.message.jobStatus.\n\
     You should not run a playbook with `wakeup_notify` as --jobStatus -- this playbook is only a guide.\n\n\
     [Your next actions (strict order)]\n\n\
     **Step 1 -- Read the real status from the envelope**:\n\
     From the wakeup_notify envelope that triggered this turn, read `message.jobStatus` (e.g. `accepted` / `submitted` / `refused` / `disputed` / `completed` / `rejected` and other real status strings).\n\n\
     **Step 2 -- Re-call next-action with the real status to fetch the current playbook**:\n\
     ```bash\n\
     onchainos agent next-action --jobid {job_id} --jobStatus <value of message.jobStatus> --role buyer --agentId {agent_id}\n\
     ```\n\
     Follow the returned playbook for what to do at the current status.\n\n\
     **Step 3 -- Idempotency self-check (avoid re-prompting the user)**:\n\
     If the playbook from Step 2 contains an `xmtp_prompt_user` step, **first** call:\n\
     ```bash\n\
     onchainos agent pending-decisions list --format json --agent-id {agent_id}\n\
     ```\n\
     - This jobId already has a pending entry (already prompted before disconnect) → **skip the re-prompt**; instead call `xmtp_dispatch_user` with \"{wakeup_resume}\"\n\
     - No pending entry (first time, or already RELAYed and closed) → run the Step 2 playbook normally (including pending-decisions add + xmtp_prompt_user)\n\n\
     ⚠️ **Do not** xmtp_send the ASP \"I'm back online\" or similar small talk -- they do not care about your connection state.\n\
     ⚠️ If the Step 2 playbook is passive (e.g. status=accepted waiting for ASP delivery), just emit a \"task resumed\" notification and end the turn; do not proactively run business actions.\n"
    )
}

pub(super) fn create_task() -> String {
    "\
🔒 **Pre-flight check**: have you read `skills/okx-agent-task/SKILL.md` and `skills/okx-agent-task/buyer.md`?\n\
If not → **stop executing this playbook immediately**; first load SKILL.md per the CLAUDE.md routing rules → confirm role is buyer → read buyer.md → then come back here.\n\
Skipping skill loading = not knowing the tool whitelist / communication protocol / security gates = downstream steps (job_created event handling, negotiation, accept) will fail.\n\n\
[Current Operation] Publish task (create_task)
[Role] User (User Agent)
[Session Type] user session (talking directly to the user)

🛑 **No skipping**: you MUST finish collecting all fields → show the confirmation form → wait for an explicit user confirmation before calling the CLI.

================================================
Step 1 -- Field collection (collect progressively in conversation; **only enter Step 2 when all fields are ready**)
================================================

| Field | CLI flag | Constraint | How to collect |
|---|---|---|---|
| Description | --description | 10-2000 chars | Consolidate the user's words. If <10 → \"A more detailed description helps match a better Provider. Could you add more specifics?\" |
| Title | --title | <=30 chars | Agent-generated; **must count chars after generating**, shorten if >30 |
| Summary | --description-summary | <=200 chars | Agent-generated; **must count chars after generating**, shorten if >200 |
| Payment token | --currency | Only USDT / USDG | ⚠️ See token rules below |
| Budget | --budget | number; <=5 decimal places; max 10,000,000 | Extract the number |
| Max budget | --max-budget | **Required**; >= budget; <=5 decimal places; max 10,000,000 | ⚠️ **You MUST ask the user explicitly**, do not auto-fill or guess. This is the negotiation price cap; the ASP's quote cannot exceed it |
| Open deadline | --deadline-open | 10 min - 6 months; format `<n>h` / `<n>m` | **MUST ask the user**. How long the task stays open before auto-closing if no ASP accepts |
| Submit deadline | --deadline-submit | 1 min - 6 months; format `<n>h` / `<n>m` | **MUST ask the user**. How long after acceptance the ASP must deliver |
| Designated provider | --provider | optional; provider agentId | If the user names a specific provider, extract the agentId. **Do not ask proactively** -- if the user does not bring it up, omit it |

🛑 **Token rules (top priority)**:
- User writes \"USDT\" or \"USDG\" explicitly → use it directly, no confirmation
- User uses fuzzy expressions (\"U\" / \"u\" / \"buck\" / \"dollar\" / \"USD\" / \"100U\" / \"50u\") → **you MUST first ask \"Please confirm the payment token: USDT or USDG?\"**, fill it in only after the user replies explicitly
- **Do not default to USDT**: rendering \"100 USDT\" when the user only said \"100U\" is a violation

================================================
Step 2 -- Validation (after all fields collected, before showing the form)
================================================

1. Token is neither USDT nor USDG → \"Only USDT and USDG are supported. Please choose one.\"
2. **Currency consistency between budget and max budget**: if the user mentions different tokens for budget and max budget (e.g. \"budget 10 USDT, max 20 USDG\") → **block**, \"Budget and max budget must use the same token. Please confirm: USDT or USDG?\". The task has a single --currency, the two must match.
3. Description < 10 chars → ask the user to expand
4. max_budget < budget → \"Max budget cannot be less than the budget.\"
5. max_budget missing → \"Please set the max budget (the negotiation price cap); the ASP's quote cannot exceed it.\"
6. budget > 10,000,000 or > 5 decimal places → tell the user the limits

================================================
Step 3 -- Identity & balance check
================================================

1. `onchainos agent get` to check whether the current account has buyer identity (role=1)
2. Has buyer → tell the user which account is being used
3. No buyer → guide registration via `onchainos agent register`
4. Insufficient balance → warn but do not block creation

================================================
Step 4 -- 🛑 Communication availability check (must not be skipped)
================================================

🛑 **MANDATORY -- complete this before showing the confirmation form**.
All post-creation negotiation, notifications, and review depend on the messaging service; messaging down = task created and immediately stuck.

1. **Read** the **entire content** of `skills/okx-agent-chat/after-agent-list-changed.md`
2. **Fully execute** the flow inside after-agent-list-changed.md (start from Step 0; walk the decision tree to completion)
3. After it finishes, proceed to Step 5

================================================
Step 5 -- Show the confirmation form (format per `skills/okx-agent-task/references/display-formats.md` Section 3)
================================================

| Field | Value |
|---|---|
| Title | <agent summary> |
| Summary | <agent summary, <=200 chars> |
| Description | <full content> (if <=200 chars, put it in the table; if >200, write `see below` in the table and render the full content as prose below) |
| Payment token | <USDT or USDG> |
| Budget | <number> |
| Max budget | <number> (negotiation price cap) |
| Open deadline | <Nh> (auto-closes after N hours if no ASP accepts) |
| Submit deadline | <Nh> (deliverable must be submitted within N hours of acceptance) |
| Designated provider | <agentId> (🛑 only show this row if the user explicitly designated one; **otherwise omit the entire row** -- do not write \"none\" or \"none (public task)\" or any placeholder. Tasks default to private; \"no designated provider\" != \"public task\") |

> Confirm? Once you confirm, I will submit the task on-chain immediately.

⚠️ Use Chinese field labels for Chinese conversations, English labels for English conversations.

→ **End this turn**; after showing the form you MUST stop and wait for the user's explicit confirmation of **this form**.
🛑 The user's earlier confirmation on a sub-question (e.g. token confirmation) does NOT count as confirming the form; you must wait for a new reply after the form is shown.

================================================
Step 6 -- After user confirms the form, call the CLI (🛑 must NOT be in the same turn as Step 5)
================================================

```bash
onchainos agent create-task \\
  --description \"<description>\" \\
  --description-summary \"<summary>\" \\
  --title \"<title>\" \\
  --budget <budget> --max-budget <max_budget> \\
  --currency <USDT|USDG> \\
  --deadline-open <deadline_open> --deadline-submit <deadline_submit> \\
  [--provider <provider agentId>]
```

⚠️ `--provider` (optional): designate a provider agentId. With it set, job_created skips recommend and routes directly via the provider's service-list by payment mode (x402 or A2A negotiation). Pass it only when the user explicitly designates a provider.

🚫 **create-task only accepts the flags above. There is no --content / --period / --visibility / --amount / --token / --payment-mode flag.** When `--provider` is passed, the CLI automatically sets visibility=1 (PRIVATE) and providerAgentId; no extra flags needed.
⚠️ **Payment mode is not set at creation** -- paymentMode is decided downstream: the A2A negotiation path is always escrow; if a provider is designated and has an endpoint, x402 is used. If the user mentions a preferred payment mode at publication, **do not pass --payment-mode**; tell them: \"The payment mode will be determined automatically when negotiating with the provider.\"

After success, call `xmtp_dispatch_user` to notify the user:
- No --provider → content: \"Task submitted; jobId: <jobId>; awaiting on-chain confirmation (~seconds). Once confirmed, the system will automatically fetch the recommended provider list for you to choose from.\"
- With --provider → content: \"Task submitted; jobId: <jobId>; awaiting on-chain confirmation (~seconds). Once confirmed, you will be connected directly with the designated provider <agentId>.\"

===============================================================
🛑🛑🛑 STOP -- after create-task you **MUST end this turn immediately**
===============================================================
❌ **Do not say \"task published\" or \"publish succeeded\"** -- create-task only submits the transaction; it is not yet confirmed on-chain.
❌ **Do not call `recommend`** -- the recommended provider list is auto-triggered by the backup session upon receiving the `job_created` system notification; it is not part of this turn.
❌ **Do not call any onchainos agent commands** -- this turn ends here; all further actions are driven by on-chain events.
===============================================================
".to_string()
}

// --- Term-change events ------------------------------------------------

pub(super) fn task_token_budget_change(ctx: &FlowContext<'_>) -> String {
    let job_id = ctx.job_id;

    format!(
    "[System Notification] task_token_budget_change (payment token / amount change settled on-chain)\n\
     [Role] User (User Agent)\n\n\
     ⚠️ This event is triggered by the user session calling `set-token-and-budget`. The terms are now updated on-chain.\n\n\
     [Receiving-scenario decision -- 🛑 MANDATORY; wrong decision = flow stuck]\n\
     This event is broadcast to all user-side sub sessions.\n\
     - If you are the **backup session** → **ignore this event, end the turn immediately, do not call any tool**\n\
     - If you are a **sub session (a negotiation session with a specific provider)** → first run Step 0 liveness check, then continue\n\n\
     [Sub-session action (🛑 four steps in strict order; each step MUST wait for the previous tool_result before continuing)]\n\n\
     **Step 0 -- 🛑 MUST check whether this session is still active (skipping = sending invalid messages to a terminated provider):**\n\
     Review this session's context: if **any** of the following holds, the session is terminated -- **ignore this event, end the turn**:\n\
     \x20\x20- You have sent or received `[intent:reject]` (negotiation terminated)\n\
     \x20\x20- You have called `mark-failed` against the current provider (provider marked failed)\n\
     \x20\x20- The provider has not replied for over 24h (negotiation cooled down)\n\
     If context is insufficient → call `xmtp_get_conversation_history` to check recent messages; if it contains [intent:reject], treat as terminated.\n\
     ⚠️ Only continue to Step 1 when you have confirmed this session is still active (negotiation in progress).\n\n\
     **Step 1 -- 🛑 MUST query the latest task details (do not use cached / stale values):**\n\
     ```bash\n\
     onchainos agent status {job_id}\n\
     ```\n\
     Extract the latest tokenSymbol and tokenAmount (budget) from the response.\n\
     ❌ Skipping this step = PROPOSE sent with stale amount = provider receives expired terms = negotiation based on wrong data\n\n\
     **Step 2 -- 🛑 MUST get the sessionKey (one of the two mandatory steps for path 4):**\n\
     Call the `session_status` tool to obtain the current sub session's `sessionKey`.\n\
     ❌ Skipping this step = xmtp_send lacks sessionKey = message cannot be sent = provider never sees the new terms\n\n\
     **Step 3 -- 🛑 MUST send a fresh round of [intent:propose] to the provider (do not skip, do not delay):**\n\
     Use the latest tokenSymbol and tokenAmount from Step 1 to construct the new PROPOSE message.\n\
     paymentMode is fixed to escrow (term changes only apply to escrow scenarios).\n\n\
     Call xmtp_send (sessionKey = value from Step 2):\n\
     \x20\x20content:\n\
     \x20\x20jobId: {job_id}\n\
     \x20\x20paymentMode: escrow\n\
     \x20\x20tokenSymbol: <latest tokenSymbol from Step 1>\n\
     \x20\x20tokenAmount: <latest tokenAmount from Step 1>\n\
     \x20\x20[intent:propose]\n\n\
     ⚠️ This is a new negotiation round; the COUNTER counter resets.\n\
     ❌ Skipping Step 3 = provider does not know terms changed = negotiation continues on old terms = final accept parameters mismatch\n\
     ❌ Do not xmtp_dispatch_user (the user already knows about the change in the user session)\n\
     ❌ Do not call set-token-and-budget / set-provider / set-max-budget (the user session already did)\n\n\
     → **End this turn** and wait for the provider's reply ([intent:ack] / [intent:counter] / [intent:reject]).\n"
    )
}

pub(super) fn task_provider_change(ctx: &FlowContext<'_>) -> String {
    let job_id = ctx.job_id;
    let agent_id = ctx.agent_id;

    let has_dp = super::negotiate::has_designated_provider(job_id);

    let backup_instruction = if has_dp {
        format!(
            "- If you are the **backup session** → the user session has written the new provider info via `set-provider`.\n\
             \x20\x20**🛑 MUST run the following command immediately to kick off the new provider flow**:\n\
             \x20\x20```bash\n\
             \x20\x20onchainos agent next-action --jobid {job_id} --jobStatus switch_provider --role buyer --agentId {agent_id}\n\
             \x20\x20```\n\
             \x20\x20Follow the returned playbook (D-Steps → negotiation / x402).\n\
             \x20\x20❌ Do not ignore this event ❌ Do not skip next-action and decide the next step yourself\n")
    } else {
        "- If you are the **backup session** → **ignore this event, end the turn immediately, do not call any tool**\n".to_string()
    };

    format!(
    "[System Notification] task_provider_change (provider change settled on-chain)\n\
     [Role] User (User Agent)\n\n\
     ⚠️ This event is triggered by the user session calling `set-provider`. The provider is now updated on-chain.\n\n\
     [Receiving-scenario decision -- 🛑 MANDATORY; wrong decision = flow stuck]\n\
     This event is broadcast to all user-side sub sessions.\n\
     {backup_instruction}\
     - If you are a **sub session (a negotiation session with a specific provider)** → first run Step 0 liveness check, then continue\n\n\
     [Sub-session action (🛑 four steps in strict order; MUST be fully executed)]\n\n\
     **Step 0 -- 🛑 MUST check whether this session is still active:**\n\
     Review this session's context: if you have sent or received a message containing `[intent:reject]` in this session (negotiation terminated),\n\
     **ignore this event, end the turn** -- a terminated negotiation does not need another REJECT.\n\
     Only continue to Step 1 when you have confirmed this session is still active (negotiation in progress).\n\n\
     **Step 1 -- 🛑 MUST query task details to compare whether the provider has changed (skipping = may wrongly close the new provider's session):**\n\
     ```bash\n\
     onchainos agent status {job_id}\n\
     ```\n\
     Extract `providerAgentId` (the current on-chain provider) and compare it with **the provider agentId this session is negotiating with**:\n\
     \x20\x20- **Match** (this session's provider IS the on-chain provider) → this session belongs to the new provider; **ignore this event, end the turn**, do not send REJECT\n\
     \x20\x20- **Mismatch** (this session's provider has been replaced) → continue to Step 2 and send REJECT\n\
     \x20\x20- **providerAgentId is empty or missing** → continue to Step 2 and send REJECT (conservative)\n\
     ❌ Skipping this step = sending REJECT indiscriminately to all sub sessions = even the new provider's session gets closed = negotiation broken\n\n\
     **Step 2 -- 🛑 MUST get the sessionKey (one of the two mandatory steps for path 4):**\n\
     Call the `session_status` tool to obtain the current sub session's `sessionKey`.\n\
     ❌ Skipping this step = xmtp_send lacks sessionKey = REJECT cannot be sent\n\n\
     **Step 3 -- 🛑 MUST send [intent:reject] to this session's provider (do not skip):**\n\
     This task's provider has changed on-chain to a different ASP; the current session's negotiation terminates immediately.\n\
     ❌ Not sending REJECT = old provider does not know they were replaced = keeps waiting / messaging = negotiation hangs forever\n\n\
     Call xmtp_send (sessionKey = value from Step 2):\n\
     \x20\x20content:\n\
     \x20\x20jobId: {job_id}\n\
     \x20\x20reason: user has switched provider\n\
     \x20\x20[intent:reject]\n\n\
     ❌ Do not xmtp_dispatch_user (the user already knows about the change in the user session)\n\
     ❌ Do not call set-token-and-budget / set-provider / set-max-budget (the user session already did)\n\
     ❌ Do not call mark-failed (it only ends the negotiation, it does not exclude that provider)\n\
     ❌ Do not keep talking to that provider after REJECT (negotiation is terminated; this sub session's mission is over)\n\n\
     → **End this turn**. The new provider's negotiation is initiated by the user session, unrelated to this sub session.\n"
    )
}

// --- Fallback ----------------------------------------------------------

pub(super) fn staked_and_unknown(event_str: &str, job_id: &str) -> String {
    format!(
    "[Unknown Status] {event_str}\n\
     [Advice]\n\
     1. Call `onchainos agent common context {job_id} --role buyer` to view full context\n\
     2. If this status is not part of the expected flow, wait for user instructions\n\
     3. Do not predict / assume other notifications\n"
    )
}
