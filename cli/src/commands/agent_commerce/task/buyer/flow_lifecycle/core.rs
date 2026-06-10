//! Core happy-path lifecycle prompt generators.

use super::super::flow::FlowContext;

// --- Execution stage ----------------------------------------------------

pub(crate) fn provider_applied(ctx: &FlowContext<'_>) -> String {
    let job_id = ctx.job_id;
    let agent_id = ctx.agent_id;

    let step1 = if ctx.prefetched.is_some() {
        format!("\
         **Step 1 -- Use pre-fetched task context above:**\n\
         Read providerAgentId, paymentMode, tokenSymbol, tokenAmount from the `[Pre-fetched task context]` block.\n\
         ⚠️ If any field is missing, fall back to:\n\
         ```bash\n\
         onchainos agent common context {job_id} --role buyer --agent-id {agent_id}\n\
         ```\n\
         ⚠️ paymentMode should be escrow (1) at this point.\n\n")
    } else {
        format!("\
         **Step 1 -- Fetch task info:**\n\
         ```bash\n\
         onchainos agent common context {job_id} --role buyer --agent-id {agent_id}\n\
         ```\n\
         Extract: providerAgentId, paymentMode, tokenSymbol, tokenAmount.\n\
         ⚠️ paymentMode should be escrow (1) at this point.\n\n")
    };

    format!(
    "[Current Status] provider_applied (ASP has submitted an on-chain apply)\n\
     [Role] User (User Agent)\n\n\
     [Your next actions (strict order)]\n\n\
     {step1}\
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

    let pm = ctx.payment_mode;
    let pm_extract = if pm.is_some() { "" } else { "paymentMode (int: 1=escrow, 3=x402), " };
    let branch_header = if pm.is_none() { "**Step 2 -- Branch by payment mode:**\n\n" } else { "" };

    let escrow_section = if pm == Some(3) { String::new() } else { format!("\
     --------- Branch A: escrow ---------\n\n\
     Call xmtp_dispatch_user to notify the user that accept succeeded:\n\
     {l10n_dispatch}\n\
     \x20\x20content:\n\
     {accepted_escrow_notify}\n\n\
     [Follow-up events]\n\
     - job_submitted → review the deliverable\n\n") };

    let x402_section = if pm == Some(1) { String::new() } else { format!("\
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
     🛑 **broadcast ≠ on-chain confirmed**: `complete` CLI success = transaction broadcast submitted, not on-chain confirmed.\n\
     ❌ Do NOT call `xmtp_dispatch_user` here — the final completion summary is owned by the `job_completed` event (fired after on-chain confirmation).\n\
     ❌ Do NOT say \"task complete\" / \"funds settled\" / \"任务完成\" — factually wrong at this point.\n\n\
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
     onchainos agent next-action --jobid {job_id} --event job_completed --role buyer --agentId {agent_id}\n\
     ```\n\
     Follow the returned playbook (it will guide you to notify the user that the job is complete).\n\
     ❌ **NEVER** ignore the `job_completed` event -- ignoring it = user never learns the job is done.\n\
     ❌ **NEVER** skip `next-action` and compose the completion notice yourself -- the playbook contains the full summary.\n") };

    let step1 = if ctx.prefetched.is_some() {
        format!("\
     **Step 1 -- Use pre-fetched task context above:**\n\
     Read {title_in_extract}description, providerAgentId, {pm_extract}tokenAmount, tokenSymbol from the `[Pre-fetched task context]` block.\n\
     ⚠️ If any field is missing, fall back to `onchainos agent common context {job_id} --role buyer --agent-id {agent_id}`.\n\
     [Failure fallback] If all sources fail, degrade to `[Job Accepted] Job `{job_id}` has been accepted; execution begins.` — the user MUST still receive a notification.\n\n")
    } else {
        format!("\
     **Step 1 -- Fetch full task info:**\n\
     ```bash\n\
     onchainos agent common context {job_id} --role buyer --agent-id {agent_id}\n\
     ```\n\
     Extract: {title_in_extract}description, providerAgentId, {pm_extract}tokenAmount, tokenSymbol.\n\
     [common context failure fallback] If the command fails or fields are missing, drop dynamic fields and degrade to `[Job Accepted] Job `{job_id}` has been accepted; execution begins.` — the user MUST still receive a notification.\n\n")
    };

    format!(
    "[Current Status] job_accepted (user has confirmed accept; task enters execution stage)\n\
     [Role] User (User Agent)\n\n\
     🛑 **You MUST call `xmtp_dispatch_user` to notify the user; do not produce a plain text reply inside the sub session** (see Hard Rule 9).\n\n\
     [Your next actions (strict order)]\n\n\
     {step1}\
     {branch_header}\
     {escrow_section}\
     {x402_section}"
    )
}

pub(crate) fn deliverable_received(ctx: &FlowContext<'_>) -> String {
    let l10n_dispatch = super::super::flow::L10N_DISPATCH_SHORT;
    let job_id = ctx.job_id;
    let agent_id = ctx.agent_id;
    let short_id = ctx.short_id;

    format!(
    "[Current action] deliverable_received — download, persist, and notify\n\
     [Role] User (User Agent)\n\n\
     🛑 This playbook fires when the ASP's a2a-agent-chat message contains `[intent:deliver]`.\n\
     Its sole purpose is: **download → save → brief notification**. The full review card is owned by `job_submitted`.\n\n\
     [Your next actions]\n\n\
     **Step 0 — Task context** (pre-fetched; no CLI call needed):\n\
     Read from the `[Pre-fetched task context]` block above:\n\
     \x20\x20- `title`, `providerAgentId`, `providerName` (best-effort), `tokenSymbol`, `tokenAmount`\n\
     If any field is missing, use best-effort values from session context; a missing field does not block the save.\n\n\
     **Step 1 — Download/extract + save + notify** (complete all sub-steps before ending the turn):\n\n\
     --- Case A: deliverableType=file (message contains fileKey / digest / salt / nonce / secret) ---\n\n\
     1a. Call xmtp_file_download:\n\
     \x20\x20- fileKey, digest, salt, nonce, secret: from the ASP's message\n\
     \x20\x20- agentId: {agent_id}\n\
     \x20\x20- filename: (optional) save filename\n\
     ⚠️ Before calling, print: `[buyer-xmtp] xmtp_file_download: fileKey=<fileKey>, agentId={agent_id}`\n\
     ⚠️ After calling, print: `[buyer-xmtp] xmtp_file_download result: localPath=<returned local path>`\n\
     On success, record localPath (full absolute path). If download fails → note it; `job_submitted` will re-attempt.\n\n\
     1b. Persist the deliverable:\n\
     ```bash\n\
     onchainos agent task-deliverable-save --job-id {job_id} --role buyer \\\n\
       --file \"<localPath>\" --deliverable-type file --title \"<title>\" \\\n\
       --short-id {short_id} --file-key \"<fileKey>\" \\\n\
       --counterparty-agent-id \"<providerAgentId>\" --counterparty-name \"<providerName>\" \\\n\
       --token-symbol \"<tokenSymbol>\" --token-amount \"<tokenAmount>\"\n\
     ```\n\
     Record the saved path from the command output. If save fails, log the error but continue.\n\n\
     --- Case B: deliverableType=text (body content between `- - -` separators) ---\n\n\
     1a. Extract the text between `- - -` separators; **keep the original wording in full**. Write to a temp .txt file.\n\n\
     1b. Persist:\n\
     ```bash\n\
     onchainos agent task-deliverable-save --job-id {job_id} --role buyer \\\n\
       --file \"<temp .txt path>\" --deliverable-type text \\\n\
       --title \"<title>\" --short-id {short_id} \\\n\
       --counterparty-agent-id \"<providerAgentId>\" --counterparty-name \"<providerName>\" \\\n\
       --token-symbol \"<tokenSymbol>\" --token-amount \"<tokenAmount>\"\n\
     ```\n\
     Record the saved path from the command output. If save fails, log the error but continue.\n\n\
     --- After save returns (both cases) — 🛑 SAME TURN, do NOT end the turn yet ---\n\n\
     1c. Call `xmtp_dispatch_user` with a preview card:\n\
     {l10n_dispatch}\n\
     \x20\x20content template (fill from Step 0 + 1a/1b results; translate to user's language):\n\
     \x20\x20```\n\
     \x20\x20[Deliverable Received] <title> (`{short_id}`)\n\
     \x20\x20Provider: <providerName> (<providerAgentId>)\n\
     \x20\x20Type: <file|text>\n\
     \x20\x20Saved at: <savedPath from 1b output>\n\
     \x20\x20\n\
     \x20\x20▸ deliverableType=file: no inline preview; the user can open the file at the path above.\n\
     \x20\x20▸ deliverableType=text: show the first 200 characters of deliverableText below; if total length ≤ 200 show full text.\n\
     \x20\x20---Preview---\n\
     \x20\x20<first 200 characters; if truncated append: (… full content saved at the path above)>\n\
     \x20\x20---End of preview---\n\
     \x20\x20\n\
     \x20\x20Awaiting on-chain submission confirmation; acceptance review will follow.\n\
     \x20\x20```\n\
     ⚠️ This is a preview card, NOT the formal review card. Do NOT include A/B acceptance choices.\n\n\
     **Step 2 — End this turn**. Wait for the `job_submitted` system event.\n\
     When `job_submitted` arrives, call `onchainos agent next-action --jobid {job_id} --event job_submitted --role buyer --agentId {agent_id}`.\n\
     The `job_submitted` playbook will check for already-saved deliverables and skip re-download if found.\n"
    )
}

pub(crate) fn job_submitted(ctx: &FlowContext<'_>) -> String {
    let l10n_prompt_bold = super::super::flow::L10N_PROMPT_BOLD;
    let l10n_dispatch = super::super::flow::L10N_DISPATCH_SHORT;
    let job_id = ctx.job_id;
    let agent_id = ctx.agent_id;
    let short_id = ctx.short_id;
    let title_display = ctx.title_display;
    let terminal_session_hint = &ctx.terminal_session_hint;
    let rating_notify = super::super::content::rating_submitted_user_notify(job_id);
    // Branch A (escrow) push protocol — user_content is composed at runtime from the
    // deliverable variables extracted in Step 2 (file vs text); pass the placeholder
    // and the templates below the helper output guide the LLM through the composition.
    let request_block = crate::commands::agent_commerce::task::common::pending_v2::request_command_block(
        job_id,
        "buyer",
        agent_id,
        "<composed in Step 3a from the deliverableType template above — paste the localized result here verbatim, including the A. and B. option lines>",
        &format!("[Decision {short_id}] {title_display} acceptance decision"),
        "job_submitted",
    );

    let pm = ctx.payment_mode;
    let pm_extract = if pm.is_some() { "" } else { "Extract `paymentMode` (int: 1=escrow, 3=x402). " };
    let step2b_branch_header = if pm.is_none() { "**Branch by paymentMode** (from Step 1):\n\n" } else { "" };
    let step3_branch_header = if pm.is_none() { "**Step 3 — Branch by payment mode:**\n\n" } else { "" };

    let step1 = if ctx.prefetched.is_some() {
        "**Step 1 — Task context (pre-fetched; no CLI call needed):**\n\
         All task fields (paymentMode, tokenSymbol, providerAgentId, etc.) are in the `[Pre-fetched task context]` block above.\n\
         qualityStandards: extract from the description field above (task creation time value is authoritative).\n\n"
            .to_string()
    } else {
        format!("\
         **Step 1 — Query task details; extract deliverable and payment mode:**\n\
         ```bash\n\
         onchainos agent status {job_id}\n\
         ```\n\
         {pm_extract}The status endpoint does not return deliverableUrl; extract that from the chat history in Step 2. Get qualityStandards from the `[Pre-fetched task context]` description above (the value at task creation time is authoritative).\n\n")
    };

    let step2a = if let Some(d) = ctx.prefetched.and_then(|p| p.deliverable.as_ref()) {
        format!("\
     **Step 2a — Deliverable already saved** (detected by CLI pre-fetch; no need to call `task-deliverable-list`):\n\
     \x20\x20- localPath: {path}\n\
     \x20\x20- deliverableType: {dtype}\n\
     \x20\x20- For text deliverables, read the file content at localPath to get `deliverableText`\n\
     \x20\x20- Call `session_status` to get the current sub session's sessionKey (reused in Step 3)\n\
     \x20\x20- Extract `qualityStandards` from the `[Pre-fetched task context]` description above; if empty, skip that line\n\
     \x20\x20- **Skip Step 2b entirely**\n\
     \x20\x20- Go to Step 3\n\n",
            path = d.path, dtype = d.deliverable_type,
        )
    } else {
        format!("\
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
     \x20\x20- Extract `qualityStandards` from the `[Pre-fetched task context]` description above; if empty, skip that line\n\
     \x20\x20- Go to Step 3\n\n\
     If `deliverables` array is empty → the `deliverable_received` playbook did not fire or failed; fall through to Step 2b.\n\n")
    };

    let has_saved_deliverable = ctx.prefetched.and_then(|p| p.deliverable.as_ref()).is_some();

    let step2b_x402 = if pm == Some(1) || has_saved_deliverable { String::new() } else { "\
     ━━━ paymentMode=x402 (3) ━━━\n\
     In x402, the deliverable was the `replayBody` returned by `task-402-pay` in the earlier `job_payment_mode_changed` turn.\n\
     ✅ The CLI auto-saved the deliverable to disk during `task-402-pay` (no manual `task-deliverable-save` needed).\n\
     Look for the `replayBodyDisplay` value in this sub session's context (it was printed when the CLI output was processed).\n\
     Set deliverable display variables: deliverableType=text, deliverableText=<replayBodyDisplay content>, localPath=<path from Step 2a task-deliverable-list if available>.\n\
     Go to Step 3.\n\n".to_string() };

    let step2b_escrow = if pm == Some(3) || has_saved_deliverable { String::new() } else { format!("\
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
     --- Case B: deliverableType=text (body content between `- - -` separators) ---\n\n\
     Extract the text between `- - -` separators in the `[intent:deliver]` message; **keep the original wording in full**, do not truncate or summarize.\n\n\
     🛑 **IMMEDIATELY after extraction**, persist the text deliverable (REQUIRED — do NOT skip):\n\
     Write deliverableText to a temp .txt file, then:\n\
     ```bash\n\
     onchainos agent task-deliverable-save --job-id {job_id} --role buyer \\\n\
       --file \"<temp .txt path>\" --deliverable-type text \\\n\
       --title \"<task title>\" --short-id {short_id} \\\n\
       --counterparty-agent-id \"<providerAgentId>\" --counterparty-name \"<providerName>\" \\\n\
       --token-symbol \"<tokenSymbol>\" --token-amount \"<tokenAmount>\"\n\
     ```\n\
     If save fails, log the error but do NOT block the review flow.\n\
     After save, record the path printed by the save command as localPath.\n\n\
     Deliverable display variables: deliverableType=text, deliverableText=<full original text sent by the ASP>, localPath=<path from save command output>\n\n") };

    let step3_escrow = if pm == Some(3) { String::new() } else { format!("\
     --------- Branch A: escrow — push the review decision to the user ---------\n\n\
     **Step 3a — Compose `--user-content` from the Step 2 deliverable variables using the template that matches `deliverableType`** (English source — fill `<placeholder>` from runtime values, THEN localize per [Localization] rules):\n\n\
     {l10n_prompt_bold}\n\n\
     ▸ deliverableType=file:\n\
     ```\n\
     [Job {short_id} — you are the User Agent] The ASP has submitted the deliverable (file); downloaded locally.\n\
     Deliverable file path: <localPath> (full absolute path, e.g. /Users/xxx/Downloads/task.png)\n\
     <if deliverableText is non-empty, append: ASP note: <deliverableText>>\n\
     Deliverable URL: <deliverableUrl>\n\
     Payment: escrow\n\
     \n\
     Choose:\n\
     A. Approve the deliverable → reply 'A'\n\
     B. Reject the deliverable (please state your reason; if the ASP files a dispute, your rejection reason will be automatically submitted as evidence to the arbitrator) → reply 'B reason: …'\n\
     ```\n\n\
     ▸ deliverableType=text — branch by localPath availability:\n\n\
     \x20\x20✅ localPath is available (save succeeded):\n\
     \x20\x20```\n\
     \x20\x20[Job {short_id} — you are the User Agent] The ASP has submitted the deliverable (text).\n\
     \x20\x20Deliverable saved at: <localPath> (full absolute path)\n\
     \x20\x20Deliverable URL: <deliverableUrl>\n\
     \x20\x20Payment: escrow\n\
     \x20\x20\n\
     \x20\x20Choose:\n\
     \x20\x20A. Approve the deliverable → reply 'A'\n\
     \x20\x20B. Reject the deliverable (please state your reason; if the ASP files a dispute, your rejection reason will be automatically submitted as evidence to the arbitrator) → reply 'B reason: …'\n\
     \x20\x20```\n\n\
     \x20\x20⚠️ localPath is unavailable (save failed — fallback to inline full text):\n\
     \x20\x20```\n\
     \x20\x20[Job {short_id} — you are the User Agent] The ASP has submitted the deliverable (text).\n\
     \x20\x20---Deliverable---\n\
     \x20\x20<deliverableText — full content, no truncation, no summarization>\n\
     \x20\x20---End of deliverable---\n\
     \x20\x20Deliverable URL: <deliverableUrl>\n\
     \x20\x20Payment: escrow\n\
     \x20\x20\n\
     \x20\x20Choose:\n\
     \x20\x20A. Approve the deliverable → reply 'A'\n\
     \x20\x20B. Reject the deliverable (please state your reason; if the ASP files a dispute, your rejection reason will be automatically submitted as evidence to the arbitrator) → reply 'B reason: …'\n\
     \x20\x20```\n\n\
     **Step 3b — Push to user via the 5-substep protocol** (use the localized `--user-content` from Step 3a; read ALL 5 sub-steps before running any command):\n\n\
     {request_block}\n") };

    let step3_x402 = if pm == Some(1) { String::new() } else { format!("\
     --------- Branch B: x402 — notify the user (no rejection allowed) ---------\n\n\
     ⚠️ In x402 funds are already paid at job_accepted; the user **cannot reject the deliverable**, just notify.\n\n\
     **B-Step 1 — Call xmtp_dispatch_user to notify the user** — split by deliverableType:\n\
     {l10n_dispatch}\n\n\
     \x20\x20▸ deliverableType=file:\n\
     \x20\x20content:\n\
     \x20\x20[Deliverable Received] Job `{job_id}` — the ASP has submitted the deliverable (x402 mode; payment already settled).\n\
     \x20\x20Deliverable file path: <localPath> (full absolute path, e.g. /Users/xxx/Downloads/task.png)\n\
     \x20\x20<if deliverableText is non-empty, append: ASP note: <deliverableText>>\n\
     \x20\x20Deliverable URL: <deliverableUrl>\n\n\
     \x20\x20▸ deliverableType=text — branch by localPath availability:\n\n\
     \x20\x20\x20\x20✅ localPath is available (save succeeded):\n\
     \x20\x20\x20\x20content:\n\
     \x20\x20\x20\x20[Deliverable Received] Job `{job_id}` — the ASP has submitted the deliverable (x402 mode; payment already settled).\n\
     \x20\x20\x20\x20Deliverable saved at: <localPath> (full absolute path)\n\
     \x20\x20\x20\x20Deliverable URL: <deliverableUrl>\n\
     \x20\x20\x20\x20---Deliverable (preview)---\n\
     \x20\x20\x20\x20<first 200 characters of deliverableText; if total length ≤ 200, show full text and use ---Deliverable--- / ---End of deliverable--- headers instead>\n\
     \x20\x20\x20\x20---End of preview---\n\
     \x20\x20\x20\x20<if deliverableText was truncated, append: (… truncated; full content saved locally)>\n\n\
     \x20\x20\x20\x20⚠️ localPath is unavailable (save failed — fallback to inline full text):\n\
     \x20\x20\x20\x20content:\n\
     \x20\x20\x20\x20[Deliverable Received] Job `{job_id}` — the ASP has submitted the deliverable (x402 mode; payment already settled).\n\
     \x20\x20\x20\x20---Deliverable---\n\
     \x20\x20\x20\x20<deliverableText — full content, no truncation, no summarization>\n\
     \x20\x20\x20\x20---End of deliverable---\n\
     \x20\x20\x20\x20Deliverable URL: <deliverableUrl>\n\n\
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
     After feedback-submit, call `xmtp_dispatch_user` to notify the user:\n\
     - ✅ **Success** (output contains `txHash`):\n\
     content (fill `<score>` with the X.XX value and `<description>` with the comment you just used in B-Step 2; fill `<title>` from task context):\n\
     {rating_notify}\n\
     - ❌ **Failure** (error / non-zero exit code) → silently skip; do NOT notify the user, do NOT retry.\n\n\
     **B-Step 3 — Terminal wrap-up (keep the sub session):**\n\
     {terminal_session_hint}\n\
     Task fully complete.\n\n\
     [Follow-up events]\n\
     - escrow: job_completed → task complete / job_rejected → wait for ASP to choose dispute or refund\n\
     - x402: flow ends here\n") };

    let step2b_section = if has_saved_deliverable {
        String::new()
    } else {
        format!("\
     **Step 2b — Fallback: fetch from chat history and save** (only if Step 2a found no saved deliverable):\n\
     First call `session_status` to get the current sub session's sessionKey (reused later; do not call it again this turn).\n\
     Extract `qualityStandards` from the `[Pre-fetched task context]` description above; if empty, skip that line.\n\n\
     {step2b_branch_header}\
     {step2b_x402}\
     {step2b_escrow}")
    };

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
     {step1}\
     **Step 2 — Obtain the deliverable content (check saved first, then fallback to chat history):**\n\n\
     ⚠️ The deliverable content MUST appear in Step 3's userContent — the user has not seen the body yet. **Do not omit, summarize, or just write \"already sent to you\".**\n\n\
     {step2a}\
     {step2b_section}\
     {step3_branch_header}\
     {step3_escrow}\
     {step3_x402}"
    )
}

pub(crate) fn approve_review(ctx: &FlowContext<'_>) -> String {
    let job_id = ctx.job_id;

    format!(
    "[Current Action] Approve review — broadcast the complete transaction\n\
     [Role] User (User Agent)\n\n\
     🛑🛑🛑 You are the **sub session** (executor). Your job is to run the on-chain `complete` command below — NOT to relay, forward, or dispatch the decision.\n\
     ❌ Do NOT call `xmtp_dispatch_session` — that is the user-session agent's tool, not yours.\n\
     ❌ Do NOT skip Step 1 (`onchainos agent complete`) — skipping it = funds stay locked forever.\n\n\
     Routed in via the buyer-side keyword router (the user approved the deliverable in their reply). The pending-decisions-v2 entry was already cleared by `resolve` in the user-session; no manual remove needed here.\n\n\
     **Step 1 -- Broadcast the dual-signature approval:**\n\
     ```bash\n\
     onchainos agent complete {job_id}\n\
     ```\n\
     If this command fails → push a `cli_failed` decision to the user (see Rule 2), end turn.\n\n\
     🛑🛑🛑 **CRITICAL — broadcast ≠ on-chain confirmed:**\n\
     `complete` CLI success = transaction **broadcast** submitted to the network.\n\
     It does NOT mean the transaction is confirmed on-chain.\n\
     ❌ Do NOT call `xmtp_dispatch_user` / `xmtp_prompt_user` here — the user has NOT received funds confirmation yet.\n\
     ❌ Do NOT say \"task complete\" / \"funds released\" / \"任务完成\" in any output — that is factually wrong at this point.\n\
     ❌ Do NOT auto-rate the ASP here — rating happens after on-chain confirmation.\n\n\
     After Step 1 succeeds → **end this turn immediately**.\n\n\
     ⚠️⚠️⚠️ **WHAT HAPPENS NEXT (READ CAREFULLY):**\n\
     After on-chain confirmation, a `job_completed` system event (`source:\"system\"`) will be fired.\n\
     That event is the **on-chain confirmation** — it is the ONLY moment when \"funds released\" becomes true.\n\
     When `job_completed` arrives, you **MUST** run `onchainos agent next-action` and follow its playbook to notify the user.\n\
     🔴 **If you do not handle `job_completed`, the user will never know funds have been released. This is a critical failure.**\n"
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

/// Primary `job_completed` playbook — on-chain confirmation notification.
///
/// This event fires when the blockchain confirms the `complete` transaction.
/// It is the ONLY place where "funds released" is factually true.
/// `approve_review` only broadcasts; this event confirms.
pub(crate) fn job_completed(ctx: &FlowContext<'_>) -> String {
    let l10n_dispatch = super::super::flow::L10N_DISPATCH_SHORT;
    let job_id = ctx.job_id;
    let agent_id = ctx.agent_id;
    let title_display = ctx.title_display;
    let title_in_extract = ctx.title_in_extract;
    let terminal_session_hint = &ctx.terminal_session_hint;

    let completed_escrow_notify = super::super::content::job_completed_escrow_user_notify(job_id, title_display);
    let completed_x402_notify = super::super::content::job_completed_x402_user_notify(job_id, title_display);
    let rating_notify = super::super::content::rating_submitted_user_notify(job_id);

    let pm = ctx.payment_mode;
    let pm_extract = if pm.is_some() { "" } else { ", paymentMode (int: 1=escrow, 3=x402)" };
    let branch_header = if pm.is_none() { "**Step 2 -- Branch by payment mode:**\n\n" } else { "" };

    let escrow_section = if pm == Some(3) { String::new() } else { format!("\
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
     After feedback-submit, call `xmtp_dispatch_user` to notify the user:\n\
     - ✅ **Success** (output contains `txHash`):\n\
     content (fill `<score>` with the X.XX value and `<description>` with the comment you just used in A-Step 2; fill `<title>` from task context):\n\
     {rating_notify}\n\
     - ❌ **Failure** (error / non-zero exit code) → silently skip; do NOT notify the user, do NOT retry.\n\n\
     **A-Step 3 -- Terminal wrap-up (keep the sub session):**\n\
     {terminal_session_hint}\n\
     Task fully complete.\n\n") };

    let x402_section = if pm == Some(1) { String::new() } else { format!("\
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
     🛑 Do NOT end this turn — B-Step 2 (auto-rate) and B-Step 2.5 (notify rating) below are MANDATORY.\n\n\
     **B-Step 2 -- 🛑 Auto-rate the ASP (MANDATORY):**\n\
     Based on the deliverable (the `replayBody` from task-402-pay in this sub session context) vs the task description and quality standards, generate:\n\
     \x20\x20- Score: 0.00–5.00 (two decimal places). Guide: 5.00 = exceeds expectations, 4.00 = fully meets, 3.00 = acceptable with minor gaps, 2.00 = partially meets, 1.00 = mostly inadequate, 0.00 = did not deliver.\n\
     \x20\x20- Comment: one sentence, ≤100 characters, evaluating how well the deliverable matches the description.\n\
     Then execute:\n\
     ```bash\n\
     onchainos agent feedback-submit --agent-id <providerAgentId> --creator-id {agent_id} --score <X.XX> --task-id {job_id} --description \"<comment, ≤100 chars>\"\n\
     ```\n\
     ⚠️ `--agent-id` is the ASP being rated (providerAgentId from Step 1 context); `--creator-id` is the buyer's own agent id ({agent_id}).\n\n\
     **B-Step 2.5 -- Notify the user of the submitted rating:**\n\
     {l10n_dispatch}\n\
     After feedback-submit, call `xmtp_dispatch_user` to notify the user:\n\
     - ✅ **Success** (output contains `txHash`):\n\
     content (fill `<score>` with the X.XX value and `<description>` with the comment you just used in B-Step 2; fill `<title>` from task context):\n\
     {rating_notify}\n\
     - ❌ **Failure** (error / non-zero exit code) → silently skip; do NOT notify the user, do NOT retry.\n\n\
     **B-Step 3 -- Terminal wrap-up (keep the sub session):**\n\
     {terminal_session_hint}\n\
     Task fully complete.\n\n") };

    format!(
    "🚨🚨🚨 **NEW SYSTEM EVENT — ON-CHAIN CONFIRMATION** 🚨🚨🚨\n\
     This is `job_completed` — the blockchain has **confirmed** the complete transaction.\n\
     ⚠️ You may have called `onchainos agent complete` earlier — that was only the **broadcast**.\n\
     This event is NOT a duplicate or confirmation of your previous action — it is a **new mandatory event** carrying the on-chain result.\n\
     🔴 **The user has NOT received the completion summary yet.** If you skip this playbook, the user will never know the task is done.\n\n\
     [Current Status] job_completed (on-chain confirmed — task settled)\n\
     [Role] User (User Agent)\n\n\
     🛑 You are inside a sub/backup session. Execute the steps below verbatim, in order. \
     Do NOT add steps, do NOT skip. Do NOT treat this as redundant.\n\n\
     **Step 1 -- Fetch task context (if needed):**\n\
     Extract {title_in_extract}providerAgentId, tokenAmount, tokenSymbol{pm_extract} from this sub session's context.\n\
     If not available, run:\n\
     ```bash\n\
     onchainos agent common context {job_id} --role buyer --agent-id {agent_id}\n\
     ```\n\
     [common context failure fallback] If the command fails or fields are missing, drop dynamic fields and degrade to `[Job Completed] Job `{job_id}` — completed; funds settled.` — the user MUST still receive a notification.\n\n\
     {branch_header}\
     {escrow_section}\
     {x402_section}\
     🛑 Forbidden: `xmtp_dispatch_session`, `sessions_spawn`, `sessions_yield`, `xmtp_send` to provider, plain text replies.\n"
    )
}
