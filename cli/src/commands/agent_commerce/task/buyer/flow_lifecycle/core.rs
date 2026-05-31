//! Core happy-path lifecycle prompt generators.

use super::super::flow::FlowContext;

// --- Execution stage ----------------------------------------------------

pub(crate) fn provider_applied(ctx: &FlowContext<'_>) -> String {
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

pub(crate) fn job_accepted(ctx: &FlowContext<'_>) -> String {
    let l10n_dispatch = super::super::flow::L10N_DISPATCH_SHORT;
    let l10n_short = super::super::flow::L10N_DISPATCH_SHORT;
    let job_id = ctx.job_id;
    let agent_id = ctx.agent_id;
    let title_display = ctx.title_display;
    let title_in_extract = ctx.title_in_extract;

    let accepted_escrow_notify = super::super::content::job_accepted_escrow_user_notify(job_id, title_display);
    let accepted_x402_fail = super::super::content::job_accepted_x402_replay_fail_user_notify(job_id);
    let complete_failed = super::super::content::complete_failed_user_notify(job_id);
    format!(
    "[Current Status] job_accepted (user has confirmed accept; task enters execution stage)\n\
     [Role] User (User Agent)\n\n\
     🛑 **You MUST call `xmtp_dispatch_user` to notify the user; do not produce a plain text reply inside the sub session** (see Hard Rule 10).\n\n\
     [Your next actions (strict order)]\n\n\
     **Step 1 -- Fetch full task info:**\n\
     ```bash\n\
     onchainos agent common context {job_id} --role buyer --agent-id {agent_id}\n\
     ```\n\
     Extract: {title_in_extract}description, providerAgentId, paymentMode (int: 1=escrow, 3=x402), tokenAmount, tokenSymbol.\n\
     [common context failure fallback] If the command fails or fields are missing, drop dynamic fields and degrade to `[Job Accepted] Job `{job_id}` has been accepted; execution begins.` — the user MUST still receive a notification.\n\n\
     **Step 2 -- Branch by payment mode:**\n\n\
     --------- Branch A: escrow ---------\n\n\
     Call xmtp_dispatch_user to notify the user that accept succeeded:\n\
     {l10n_dispatch}\n\
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
     \x20\x20content ({l10n_short}): {complete_failed}\n\
     → **End this turn** and wait for user retry or a wakeup_notify event.\n\n\
     **B-Branch 2: replaySuccess=false (only take this branch when replaySuccess=false is explicitly found in context)**\n\n\
     ⚠️ **Do not run complete** -- the user did not receive the deliverable.\n\n\
     **B-Step 2 -- Notify the user of replay failure via xmtp_dispatch_user** ({l10n_short}):\n\
     \x20\x20content:\n\
     {accepted_x402_fail}\n\n\
     [Follow-up events]\n\
     - replaySuccess=true / default: job_completed → final confirmation\n\
     - replaySuccess=false: wait for user instructions (retry or close task)\n\n\
     🛑🛑🛑 **job_completed MANDATORY rule**:\n\
     After complete is settled on-chain, a `job_completed` system event will arrive.\n\
     Upon receiving `job_completed`, you **MUST** call:\n\
     ```bash\n\
     onchainos agent next-action --jobid {job_id} --event job_completed --jobStatus job_completed --role buyer --agentId {agent_id}\n\
     ```\n\
     Follow the returned playbook (it will guide you to notify the user that the job is complete).\n\
     ❌ **NEVER** ignore the `job_completed` event -- ignoring it = user never learns the job is done.\n\
     ❌ **NEVER** skip `next-action` and compose the completion notice yourself -- the playbook contains the full summary.\n"
    )
}

pub(crate) fn deliverable_received(ctx: &FlowContext<'_>) -> String {
    let l10n_dispatch = super::super::flow::L10N_DISPATCH_SHORT;
    let job_id = ctx.job_id;
    let agent_id = ctx.agent_id;
    let short_id = ctx.short_id;

    format!(
    "[Current action] deliverable_received — download and persist the ASP's deliverable\n\
     [Role] User (User Agent)\n\n\
     🛑 This playbook fires when the ASP's a2a-agent-chat message contains `[intent:deliver]`.\n\
     Its sole purpose is: **download → save → brief notification**. The full review card is owned by `job_submitted`.\n\n\
     [Your next actions (strict order)]\n\n\
     **Step 0 — Load task context for save metadata**:\n\
     ```bash\n\
     onchainos agent common context {job_id} --role buyer --agent-id {agent_id}\n\
     ```\n\
     Extract and record for use in the save commands below:\n\
     \x20\x20- `title` (task title)\n\
     \x20\x20- `providerAgentId` (ASP agentId — the counterparty)\n\
     \x20\x20- `providerName` (ASP display name, if available)\n\
     \x20\x20- `tokenSymbol`, `tokenAmount`\n\
     ⚠️ If the command fails (e.g. network error), use best-effort values from session context; a missing title does not block the save.\n\n\
     **Step 1 — Extract deliverable metadata from the inbound `[intent:deliver]` message** and branch by type:\n\n\
     --- Case A: deliverableType=file (message contains fileKey / digest / salt / nonce / secret) ---\n\n\
     Call the xmtp_file_download tool:\n\
     \x20\x20Parameters:\n\
     \x20\x20- fileKey, digest, salt, nonce, secret: from the ASP's message\n\
     \x20\x20- agentId: {agent_id}\n\
     \x20\x20- filename: (optional) save filename\n\
     ⚠️ Before calling, print: `[buyer-xmtp] xmtp_file_download: fileKey=<fileKey>, agentId={agent_id}`\n\
     ⚠️ After calling, print: `[buyer-xmtp] xmtp_file_download result: localPath=<returned local path>`\n\n\
     On success, record localPath (must be a full absolute path).\n\
     If download fails → note it; the `job_submitted` playbook will re-attempt.\n\n\
     🛑 **IMMEDIATELY after download succeeds**, persist the deliverable (use values from Step 0):\n\
     ```bash\n\
     onchainos agent task-deliverable-save --job-id {job_id} --role buyer \\\n\
       --file \"<localPath>\" --deliverable-type file --title \"<title from Step 0>\" \\\n\
       --short-id {short_id} --file-key \"<fileKey>\" \\\n\
       --counterparty-agent-id \"<providerAgentId from Step 0>\" --counterparty-name \"<providerName from Step 0>\" \\\n\
       --token-symbol \"<tokenSymbol from Step 0>\" --token-amount \"<tokenAmount from Step 0>\"\n\
     ```\n\
     If save fails, log the error but do NOT block.\n\n\
     --- Case B: deliverableType=text (body content between `---` separators) ---\n\n\
     Extract the text between `---` separators; **keep the original wording in full**.\n\n\
     🛑 **IMMEDIATELY after extraction**, write to a temp .txt file and persist (use values from Step 0):\n\
     ```bash\n\
     onchainos agent task-deliverable-save --job-id {job_id} --role buyer \\\n\
       --file \"<temp .txt path>\" --deliverable-type text \\\n\
       --title \"<title from Step 0>\" --short-id {short_id} \\\n\
       --counterparty-agent-id \"<providerAgentId from Step 0>\" --counterparty-name \"<providerName from Step 0>\" \\\n\
       --token-symbol \"<tokenSymbol from Step 0>\" --token-amount \"<tokenAmount from Step 0>\"\n\
     ```\n\
     If save fails, log the error but do NOT block.\n\n\
     **Step 2 — Notify the user (brief; NO deliverable content)**:\n\n\
     Call `xmtp_dispatch_user`:\n\
     {l10n_dispatch}\n\
     \x20\x20content: The provider has sent the deliverable; awaiting on-chain submission confirmation before entering acceptance review.\n\
     ❌ Do NOT include the deliverable body / summary / file path in this notification — the full content is shown in the `job_submitted` review card.\n\n\
     **Step 3 — End this turn**. Wait for the `job_submitted` system event.\n\
     When `job_submitted` arrives, call `onchainos agent next-action --jobid {job_id} --event job_submitted --jobStatus job_submitted --role buyer --agentId {agent_id}`.\n\
     The `job_submitted` playbook will check for already-saved deliverables and skip re-download if found.\n"
    )
}

pub(crate) fn job_submitted(ctx: &FlowContext<'_>) -> String {
    let l10n_prompt_bold = super::super::flow::L10N_PROMPT_BOLD;
    let l10n_dispatch = super::super::flow::L10N_DISPATCH_SHORT;
    let job_id = ctx.job_id;
    let agent_id = ctx.agent_id;
    let short_id = ctx.short_id;
    let terminal_session_hint = ctx.terminal_session_hint;
    let follow_end = super::super::flow::FOLLOW_PLAYBOOK_END_TURN;
    let rating_notify = super::super::content::rating_submitted_user_notify(job_id);

    format!(
    "[Current Status] job_submitted (ASP has submitted the deliverable)\n\
     [Role] User (User Agent)\n\n\
     🛑🛑🛑 **ABSOLUTE REQUIREMENT -- in escrow mode you MUST push the review decision to the user via `pending-decisions-v2 request` (NOT a plain text reply, NOT `xmtp_dispatch_user`)**.\n\
     `xmtp_dispatch_user` is a pure notification: user replies cannot be relayed back to the sub session → the review flow deadlocks. The correct flow handles this via `pending-decisions-v2 request` → CLI playbook → `xmtp_prompt_user` (with llmContent + userContent) so the user session can relay the review decision back.\n\
     🔴 Real incident: a Minimax model received job_submitted, called xmtp_dispatch_user with \"the ASP has submitted; awaiting your review\" -- the user never saw the deliverable, could not relay a decision, and the task was stuck.\n\n\
     🛑🛑🛑 **Even if you already processed the ASP's a2a-agent-chat deliverable message earlier in this turn (e.g. called xmtp_file_download), upon receiving job_submitted you MUST still execute every Step below in full**.\n\
     Handling a2a-agent-chat (file download) != the review flow -- the review must be driven by the job_submitted playbook, and the deliverable content (file path / text) MUST be placed into the `--user-content` of `pending-decisions-v2 request` for the user to see.\n\n\
     🛑 **In escrow mode auto-approval is strictly forbidden**: you must wait for the user's relayed decision; the agent must not decide on behalf of the user, regardless of deliverable quality or how close to deadline.\n\
     ⚠️ In x402 mode: funds are already paid; just notify the user of the deliverable content; the user cannot reject.\n\n\
     [Your next actions (strict order)]\n\n\
     **Step 1 — Query task details; extract deliverable and payment mode:**\n\
     ```bash\n\
     onchainos agent status {job_id}\n\
     ```\n\
     Extract `paymentMode` (int: 1=escrow, 3=x402). The status endpoint does not return deliverableUrl; extract that from the chat history in Step 2. Get qualityStandards from `onchainos agent common context` (the value at task creation time is authoritative).\n\n\
     **Step 2 — Obtain the deliverable content (check saved first, then fallback to chat history):**\n\n\
     ⚠️ The deliverable content MUST appear in Step 3's userContent — the user has not seen the body yet. **Do not omit, summarize, or just write \"already sent to you\".**\n\n\
     **Step 2a — Check if deliverable was already saved** (by the earlier `deliverable_received` playbook):\n\
     ```bash\n\
     onchainos agent task-deliverable-list --job-id {job_id} --role buyer\n\
     ```\n\
     If `deliverables` array is non-empty → the deliverable has already been downloaded and saved:\n\
     \x20\x20- Use the `path` from the first entry as `localPath`\n\
     \x20\x20- Use the `deliverableType` from the first entry\n\
     \x20\x20- For text deliverables, read the file content at `path` to get `deliverableText`\n\
     \x20\x20- **Skip Step 2b entirely** (no need to re-download or re-save)\n\
     \x20\x20- Call `session_status` to get the current sub session's sessionKey (reused in Step 3)\n\
     \x20\x20- From `onchainos agent common context {job_id} --role buyer --agent-id {agent_id}` extract `qualityStandards`; if empty, skip that line\n\
     \x20\x20- Go to Step 3\n\n\
     If `deliverables` array is empty → the `deliverable_received` playbook did not fire or failed; fall through to Step 2b.\n\n\
     **Step 2b — Fallback: fetch from chat history and save** (only if Step 2a found no saved deliverable):\n\
     First call `session_status` to get the current sub session's sessionKey (reused later; do not call it again this turn).\n\
     From `onchainos agent common context {job_id} --role buyer --agent-id {agent_id}` extract `qualityStandards` (the review standard as of task creation); if empty, skip that line.\n\n\
     **Branch by paymentMode** (from Step 1):\n\n\
     ━━━ paymentMode=x402 (3) ━━━\n\
     In x402, the deliverable was the `replayBody` returned by `task-402-pay` in the earlier `job_payment_mode_changed` turn.\n\
     ✅ The CLI auto-saved the deliverable to disk during `task-402-pay` (no manual `task-deliverable-save` needed).\n\
     Look for the `replayBodyDisplay` value in this sub session's context (it was printed when the CLI output was processed).\n\
     Set deliverable display variables: deliverableType=text, deliverableText=<replayBodyDisplay content>.\n\
     Go to Step 3.\n\n\
     ━━━ paymentMode=escrow (1) ━━━\n\
     Call `xmtp_get_conversation_history` (sessionKey = the value obtained above) and find the ASP message **carrying the `[intent:deliver]` suffix tag** (scan newest to oldest; first match is the deliverable), and branch on `deliverableType`:\n\n\
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
     ⚠️ If the ASP message contains text alongside the file (e.g. \"here is the deliverable, please check\"), capture it into deliverableText as well.\n\n\
     🛑 **IMMEDIATELY after download succeeds**, persist the deliverable (REQUIRED — do NOT skip; without this the file is lost on session restart):\n\
     ```bash\n\
     onchainos agent task-deliverable-save --job-id {job_id} --role buyer \\\n\
       --file \"<localPath>\" --deliverable-type file --title \"<task title>\" \\\n\
       --short-id {short_id} --file-key \"<fileKey>\" \\\n\
       --counterparty-agent-id \"<providerAgentId>\" --counterparty-name \"<providerName>\" \\\n\
       --token-symbol \"<tokenSymbol>\" --token-amount \"<tokenAmount>\"\n\
     ```\n\
     After save, update localPath to the path printed by the save command (the file has been moved to the deliverables directory).\n\
     If save fails, log the error but do NOT block the review flow.\n\n\
     Deliverable display variables: deliverableType=file, localPath=<full path>, deliverableText=<note text, empty if none>\n\n\
     --- Case B: deliverableType=text (body content between `---` separators) ---\n\n\
     Extract the text between `---` separators in the `[intent:deliver]` message; **keep the original wording in full**, do not truncate or summarize.\n\n\
     🛑 **IMMEDIATELY after extraction**, persist the text deliverable (REQUIRED — do NOT skip):\n\
     Write deliverableText to a temp .txt file, then:\n\
     ```bash\n\
     onchainos agent task-deliverable-save --job-id {job_id} --role buyer \\\n\
       --file \"<temp .txt path>\" --deliverable-type text \\\n\
       --title \"<task title>\" --short-id {short_id} \\\n\
       --counterparty-agent-id \"<providerAgentId>\" --counterparty-name \"<providerName>\" \\\n\
       --token-symbol \"<tokenSymbol>\" --token-amount \"<tokenAmount>\"\n\
     ```\n\
     If save fails, log the error but do NOT block the review flow.\n\n\
     Deliverable display variables: deliverableType=text, deliverableText=<full original text sent by the ASP>\n\n\
     **Step 3 — Branch by payment mode:**\n\n\
     --------- Branch A: escrow — enqueue review decision via `pending-decisions-v2 request` ---------\n\n\
     Build the `--user-content` from the deliverable variables above (split by deliverableType). Then run (substitute `<full sessionKey>` from the session_status call in Step 2):\n\
     ```bash\n\
     onchainos agent pending-decisions-v2 request \\\n\
       --sub-key \"<full sessionKey from session_status>\" \\\n\
       --job-id {job_id} --role buyer --agent-id {agent_id} \\\n\
       --user-content \"<deliverable card + A/B options, see templates below>\" \\\n\
       --list-label \"[Decision {short_id}] Approve / Reject\" \\\n\
       --source-event job_submitted\n\
     ```\n\
     {l10n_prompt_bold}\n\
     `--user-content` template (canonical English; localize before passing) — split by deliverableType:\n\n\
     ▸ deliverableType=file:\n\
     ```\n\
     [Job {short_id} — you are the User Agent] The ASP has submitted the deliverable (file); downloaded locally.\n\
     Deliverable file path: <localPath> (full absolute path, e.g. /Users/xxx/Downloads/task.png)\n\
     <if deliverableText is non-empty, append: ASP note: <deliverableText>>\n\
     Deliverable URL: <deliverableUrl>\n\
     Quality standards: <qualityStandards>\n\
     Payment: escrow\n\
     \n\
     Choose:\n\
     A. Approve the deliverable → reply 'A' or 'approve' / '通过'\n\
     B. Reject the deliverable (please state your reason) → reply 'B reason: <...>' or 'reject reason: <...>' / '拒绝, 理由: <...>'\n\
     ```\n\n\
     ▸ deliverableType=text:\n\
     ```\n\
     [Job {short_id} — you are the User Agent] The ASP has submitted the deliverable (text).\n\
     ---Deliverable---\n\
     <deliverableText full content, no truncation, no summarization>\n\
     ---End of deliverable---\n\
     Deliverable URL: <deliverableUrl>\n\
     Quality standards: <qualityStandards>\n\
     Payment: escrow\n\
     \n\
     Choose:\n\
     A. Approve the deliverable → reply 'A' or 'approve' / '通过'\n\
     B. Reject the deliverable (please state your reason) → reply 'B reason: <...>' or 'reject reason: <...>' / '拒绝, 理由: <...>'\n\
     ```\n\n\
     {follow_end}\n\n\
     ===============================================================\n\
     🛑🛑🛑 STOP — after running `pending-decisions-v2 request` and following its returned playbook (one `xmtp_prompt_user` call) in Step 3, you **MUST end this turn**\n\
     ===============================================================\n\
     This playbook ends here for Step 3. In a later turn, when user-session relays the reply as a system envelope (`event: \"user_decision_job_submitted\"`, `message.data: <verbatim>`), continue with Step 4 below.\n\n\
     **Step 4 — After user-session relays as system envelope** (`event: \"user_decision_job_submitted\"`, `message.data: <user's verbatim reply>`):\n\
     Call `onchainos agent next-action --jobid {job_id} --event user_decision_job_submitted --jobStatus user_decision_job_submitted --role buyer --agentId {agent_id} --data \"<message.data>\"` — CLI returns a routing playbook that maps the user's intent (`A` / `通过` / `approve` / 同意 / 接受 → `approve_review`; `B` / `拒绝` / `reject` → `reject_review`; ambiguous → re-ask via pending-decisions-v2 request). Follow the returned routing.\n\n\
     ===============================================================\n\
     🔴🔴🔴 ABSOLUTE PROHIBITION when routing in Step 4:\n\
     ❌ Do NOT skip `next-action` and call `onchainos agent complete` / `onchainos agent reject` directly — the `job_submitted` playbook deliberately splits approve/reject into independent pseudo-events; without the playbook from next-action you will miss internal pre-complete / pre-reject signature steps and funds will stay locked.\n\
     ❌ Do NOT call `xmtp_dispatch_session` yourself — you are the sub session (executor), NOT the user session (relay). The relay has already arrived; your job is to execute the playbook, not to re-dispatch.\n\
     🔴 Real incident: a model received the user's approval, skipped next-action and called `onchainos agent complete` directly — the on-chain complete was misformed, funds remained locked, and the user was told the job was approved when it was not.\n\
     ===============================================================\n\n\
     --------- Branch B: x402 — notify the user (no rejection allowed) ---------\n\n\
     ⚠️ In x402 funds are already paid at job_accepted; the user **cannot reject the deliverable**, just notify.\n\n\
     **B-Step 1 — Call xmtp_dispatch_user to notify the user** — split by deliverableType:\n\
     {l10n_dispatch}\n\n\
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
     🛑 Do NOT end this turn — B-Step 2 (auto-rate) and B-Step 2.5 (notify rating) below are MANDATORY.\n\n\
     **B-Step 2 — 🛑 Auto-rate the ASP (MANDATORY):**\n\
     Based on the deliverable content vs the task description and quality standards, generate:\n\
     \x20\x20- Score: 0.00–5.00 (two decimal places). Guide: 5.00 = exceeds expectations, 4.00 = fully meets, 3.00 = acceptable with minor gaps, 2.00 = partially meets, 1.00 = mostly inadequate, 0.00 = did not deliver.\n\
     \x20\x20- Comment: one sentence, ≤100 characters, evaluating how well the deliverable matches the description.\n\
     Then execute:\n\
     ```bash\n\
     onchainos agent feedback-submit --agent-id <providerAgentId> --creator-id {agent_id} --score <X.XX> --task-id {job_id} --description \"<comment, ≤100 chars>\"\n\
     ```\n\
     ⚠️ `--agent-id` is the ASP being rated (providerAgentId from Step 1 context); `--creator-id` is the buyer's own agent id ({agent_id}).\n\n\
     **B-Step 2.5 — Notify the user of the submitted rating:**\n\
     {l10n_dispatch}\n\
     After feedback-submit succeeds, call `xmtp_dispatch_user` with the rating result so the user knows what score was given.\n\
     ✅ content (fill `<score>` with the X.XX value and `<description>` with the comment you just used in B-Step 2; fill `<title>` from task context):\n\
     {rating_notify}\n\n\
     **B-Step 3 — Terminal wrap-up (keep the sub session):**\n\
     {terminal_session_hint}\n\
     Task fully complete.\n\n\
     [Follow-up events]\n\
     - escrow: job_completed → task complete / job_rejected → wait for ASP to choose dispute or refund\n\
     - x402: flow ends here\n"
    )
}

pub(crate) fn approve_review(ctx: &FlowContext<'_>) -> String {
    let job_id = ctx.job_id;
    let _agent_id = ctx.agent_id;

    format!(
    "[Current Action] Approve review -- run complete to release funds\n\
     [Role] User (User Agent)\n\n\
     🛑🛑🛑 You are the **sub session** (executor). Your job is to run the on-chain `complete` command below — NOT to relay, forward, or dispatch the decision.\n\
     ❌ Do NOT call `xmtp_dispatch_session` — that is the user-session agent's tool, not yours.\n\
     ❌ Do NOT skip Step 1 (`onchainos agent complete`) — skipping it = funds stay locked forever.\n\n\
     Routed in via the buyer-side keyword router (the user approved the deliverable in their reply). The pending-decisions-v2 entry was already cleared by `resolve` in the user-session; no manual remove needed here.\n\n\
     **Step 1 -- Dual-signature approval, release funds:**\n\
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
     After Step 1 → **end this turn**.\n\
     ⚠️ **Your work is not finished** -- when the `job_completed` system event (`source:\"system\"`) arrives, you MUST handle it per SKILL.md Activation rules,\n\
     otherwise the user will never receive a \"task complete\" notification and will not know funds have been released.\n"
    )
}

pub(crate) fn reject_review(ctx: &FlowContext<'_>) -> String {
    let job_id = ctx.job_id;
    let _agent_id = ctx.agent_id;

    format!(
    "[Current Action] Reject review -- run reject\n\
     [Role] User (User Agent)\n\n\
     Routed in via the buyer-side keyword router (the user rejected the deliverable in their reply). The pending-decisions-v2 entry was already cleared by `resolve` in the user-session; no manual remove needed here.\n\
     Extract the rejection reason from the relayed verbatim (look for `理由` / `reason` / `因为`); if not stated, default to `did not meet acceptance criteria`.\n\n\
     **Step 1 -- Dual-signature rejection:**\n\
     ```bash\n\
     onchainos agent reject {job_id} --reason \"<rejection reason from user's words>\"\n\
     ```\n\
     Internal flow:\n\
     \x20\x201. POST /priapi/v1/aieco/task/{job_id}/pre-reject (EIP-712 standard, not uop) → get digest\n\
     \x20\x202. ED25519 sign digest → signature\n\
     \x20\x203. POST /priapi/v1/aieco/task/{job_id}/reject (body: {{\"signature\": \"<sig>\", \"reason\": \"<reason>\"}}) → get uopData\n\
     \x20\x204. Sign uopHash → broadcast on-chain\n\
     \x20\x20→ Task status becomes Rejected; the ASP can open a dispute or agree to a refund.\n\
     \x20\x20⚠️ **The buyer cannot initiate arbitration** — only the ASP can. If the user asks, explain: after rejection the ASP decides whether to dispute; if the ASP takes no action, the system auto-refunds.\n\n\
     ⚠️ **Do not xmtp_send any message to the ASP** (e.g. \"rejected\"); the ASP learns via on-chain events.\n\n\
     After Step 1 → **end this turn** and wait for the `job_rejected` system notification.\n"
    )
}

// --- Terminal states ---------------------------------------------------

pub(crate) fn job_completed(ctx: &FlowContext<'_>) -> String {
    let l10n_dispatch = super::super::flow::L10N_DISPATCH_SHORT;
    let job_id = ctx.job_id;
    let agent_id = ctx.agent_id;
    let title_display = ctx.title_display;
    let title_in_extract = ctx.title_in_extract;
    let terminal_session_hint = ctx.terminal_session_hint;

    let completed_escrow_notify = super::super::content::job_completed_escrow_user_notify(job_id, title_display);
    let completed_x402_notify = super::super::content::job_completed_x402_user_notify(job_id, title_display);
    let rating_notify = super::super::content::rating_submitted_user_notify(job_id);
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
     Extract: {title_in_extract}tokenAmount, tokenSymbol, paymentMode (int: 1=escrow, 3=x402).\n\
     ⚠️ Timestamp: use the `timestamp` field from the system notification envelope (Unix seconds). Convert to human-readable format (e.g. \"2025-05-29 10:00 UTC\"). If the notification has no `timestamp`, omit the \"Settled at\" line entirely.\n\
     [common context failure fallback] If the command fails or fields are missing, drop dynamic fields and degrade to `[Job Completed] Job `{job_id}` — completed; funds settled. This job is complete.` — the user MUST still receive a notification.\n\n\
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
     {l10n_dispatch}\n\
     \x20\x20content:\n\
     {completed_escrow_notify}\n\n\
     🛑 Do NOT end this turn — A-Step 2 (auto-rate) and A-Step 2.5 (notify rating) below are MANDATORY.\n\n\
     **A-Step 2 -- 🛑 Auto-rate the ASP (MANDATORY):**\n\
     Based on the deliverable that was reviewed vs the task description and quality standards, generate:\n\
     \x20\x20- Score: 0.00–5.00 (two decimal places). Guide: 5.00 = exceeds expectations, 4.00 = fully meets, 3.00 = acceptable with minor gaps, 2.00 = partially meets, 1.00 = mostly inadequate, 0.00 = did not deliver.\n\
     \x20\x20- Comment: one sentence, ≤100 characters, evaluating how well the deliverable matches the description.\n\
     Then execute:\n\
     ```bash\n\
     onchainos agent feedback-submit --agent-id <providerAgentId> --creator-id {agent_id} --score <X.XX> --task-id {job_id} --description \"<comment, ≤100 chars>\"\n\
     ```\n\
     ⚠️ `--agent-id` is the ASP being rated (providerAgentId from Step 1 context); `--creator-id` is the buyer's own agent id ({agent_id}).\n\n\
     **A-Step 2.5 -- Notify the user of the submitted rating:**\n\
     {l10n_dispatch}\n\
     After feedback-submit succeeds, call `xmtp_dispatch_user` with the rating result so the user knows what score was given.\n\
     ✅ content (fill `<score>` with the X.XX value and `<description>` with the comment you just used in A-Step 2; fill `<title>` from task context):\n\
     {rating_notify}\n\n\
     **A-Step 3 -- Terminal wrap-up (keep the sub session):**\n\
     {terminal_session_hint}\n\
     Task fully complete.\n\n\
     --------- Branch B: x402 -- final summary + auto-rate ---------\n\n\
     ⚠️ In x402, job_completed means the payment pipeline (accept + complete) is settled on-chain.\n\
     The deliverable was already sent to the user during task-402-pay; this step emits the final summary and rates the ASP.\n\n\
     **B-Step 1 -- 🛑 MUST call `xmtp_dispatch_user` tool (do NOT produce a plain text reply):**\n\
     🛑🛑🛑 You are in a **sub session (backup)**. Any text you output here is invisible to the user.\n\
     The ONLY way to reach the user is the `xmtp_dispatch_user` tool call.\n\
     ❌ Do NOT output the notification as text — it will be trapped in the backup session and the user will never see it.\n\
     🌐 ✅ Call xmtp_dispatch_user with the following content parameter (replace placeholders with real values from Step 1):\n\
     {l10n_dispatch}\n\
     \x20\x20content:\n\
     {completed_x402_notify}\n\n\
     🛑 Do NOT end this turn — B-Step 1.5 (auto-rate) and B-Step 1.6 (notify rating) below are MANDATORY.\n\n\
     **B-Step 1.5 -- 🛑 Auto-rate the ASP (MANDATORY):**\n\
     Based on the deliverable (the `replayBody` from task-402-pay in this sub session context) vs the task description and quality standards, generate:\n\
     \x20\x20- Score: 0.00–5.00 (two decimal places). Guide: 5.00 = exceeds expectations, 4.00 = fully meets, 3.00 = acceptable with minor gaps, 2.00 = partially meets, 1.00 = mostly inadequate, 0.00 = did not deliver.\n\
     \x20\x20- Comment: one sentence, ≤100 characters, evaluating how well the deliverable matches the description.\n\
     Then execute:\n\
     ```bash\n\
     onchainos agent feedback-submit --agent-id <providerAgentId> --creator-id {agent_id} --score <X.XX> --task-id {job_id} --description \"<comment, ≤100 chars>\"\n\
     ```\n\
     ⚠️ `--agent-id` is the ASP being rated (providerAgentId from Step 1 context); `--creator-id` is the buyer's own agent id ({agent_id}).\n\n\
     **B-Step 1.6 -- Notify the user of the submitted rating:**\n\
     {l10n_dispatch}\n\
     After feedback-submit succeeds, call `xmtp_dispatch_user` with the rating result so the user knows what score was given.\n\
     ✅ content (fill `<score>` with the X.XX value and `<description>` with the comment you just used in B-Step 1.5; fill `<title>` from task context):\n\
     {rating_notify}\n\n\
     **B-Step 2 -- Terminal wrap-up (keep the sub session):**\n\
     {terminal_session_hint}\n\
     Task fully complete.\n\
     🛑 Final check: if you did NOT call `xmtp_dispatch_user` in B-Step 1, go back and call it now. A text reply is NOT a substitute.\n"
    )
}
