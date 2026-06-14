//! Rejection / arbitration prompt generators.

use super::super::flow::FlowContext;
use crate::commands::agent_commerce::task::common::okx_a2a;

pub(crate) fn job_rejected(ctx: &FlowContext<'_>) -> String {
    let l10n_short = super::super::flow::L10N_DISPATCH_SHORT;
    let job_id = ctx.job_id;
    let title_display = ctx.title_display;
    let title_query_hint = ctx.title_query_hint;

    let rejected_notify = super::super::content::job_rejected_user_notify(job_id, title_display);
    format!(
    "[Current Status] job_rejected (user rejection settled on-chain; awaiting ASP decision)\n\
     [Role] User (User Agent)\n\n\
     🛑 **You MUST call `xmtp_dispatch_user` to notify the user that rejection is settled; do not produce a plain text reply inside the sub session** (see Hard Rule 9).\n\n\
     [Your next actions (strict order)]\n\n\
     {title_query_hint}\
     **Step 1 -- Call xmtp_dispatch_user to notify the user the rejection is confirmed:**\n\
     {l10n_short}\n\n\
     content:\n\
     {rejected_notify}\n\n\
     **Step 2 -- Silently wait for the ASP's decision:**\n\n\
     ⚠️ **Do not send any xmtp_send message to the ASP**. The ASP will decide:\n\
     - Open a dispute → you will receive job_disputed\n\
     - Agree to refund → you will receive job_refunded\n\
     - Timeout → system auto-refunds, you will receive job_refunded\n\n\
     ⚠️ **The buyer cannot initiate arbitration** — only the ASP can open a dispute. If the user asks \"can I start a dispute?\", reply: the buyer side does not support initiating arbitration; please wait for the ASP's decision.\n\n\
     After Step 1 → **end this turn** and wait for the next system event.\n\n\
     [Follow-up events]\n\
     - job_disputed → submit user evidence\n\
     - job_refunded → refund complete\n"
    )
}

pub(crate) fn job_disputed(ctx: &FlowContext<'_>) -> String {
    let l10n_dispatch = super::super::flow::L10N_DISPATCH_SHORT;
    let job_id = ctx.job_id;
    let agent_id = ctx.agent_id;
    let title_display = ctx.title_display;
    let title_query_hint = ctx.title_query_hint;

    // Fetch sessionKey + chat history in-process and inline the formatted
    // block into the playbook. Errors propagate as an error playbook (LLM
    // pushes a cli_failed decision); no LLM-driven fallback path here.
    let session_key = match okx_a2a::session_status() {
        Ok(Some(sk)) => sk,
        Ok(None) => return format!(
            "[job_disputed] ❌ No active sub session reported by `okx-a2a session status` for job {job_id}; cannot fetch chat history for dispute evidence.\n\n\
             Push a `cli_failed` decision to the user via `pending-decisions-v2 request` so they can decide how to proceed.\n"
        ),
        Err(e) => return format!(
            "[job_disputed] ❌ `okx-a2a session status` failed: {e}\n\n\
             Push a `cli_failed` decision to the user via `pending-decisions-v2 request`.\n"
        ),
    };
    let messages = match okx_a2a::xmtp_get_conversation_history(&session_key) {
        Ok(m) => m,
        Err(e) => return format!(
            "[job_disputed] ❌ `okx-a2a session get` (chat history) failed: {e}\n\n\
             Push a `cli_failed` decision to the user via `pending-decisions-v2 request`.\n"
        ),
    };
    let chat_block = if messages.is_empty() {
        "(no chat history available)".to_string()
    } else {
        messages.into_iter()
            .map(|m| {
                let ts = m.sent_at.map(|v| v.to_string()).unwrap_or_else(|| "?".to_string());
                let status = if m.delivery_status.is_empty() {
                    "?".to_string()
                } else {
                    m.delivery_status
                };
                format!("[{ts}][{status}] sender={}: {}", m.sender_inbox_id, m.content)
            })
            .collect::<Vec<_>>()
            .join("\n")
    };

    format!(
    "[Current Status] job_disputed (arbitration opened; CLI auto-submits evidence on this event)\n\
     [Role] User (User Agent)\n\n\
     🛑 **This event triggers an AUTOMATIC evidence upload — no user interaction**.\n\
     The agent does NOT ask the user for evidence; it formats the chat history, calls `dispute upload`\n\
     (which also auto-attaches every saved deliverable from `~/.onchainos/deliverables/buyer/{job_id}/`),\n\
     and then notifies the user via `xmtp_dispatch_user`. **Do NOT** use `pending-decisions-v2 request`\n\
     for this event. **Do NOT** call `xmtp_send` to the ASP — both sides see the arbitration via on-chain events.\n\n\
     [Your next actions (strict order)]\n\n\
     {title_query_hint}\
     **Step 1 — Chat history (pre-fetched and inlined below; do NOT call `session_status` or `xmtp_get_conversation_history`):**\n\n\
     ```\n\
     ==== Negotiation / delivery chat history ====\n\
     {chat_block}\n\
     ```\n\n\
     **Step 2 — Extract a `--text` body from the chat history above** (≤16 KB):\n\
     Keep ONLY the key checkpoints — PROPOSE / ACK / CONFIRM / deliverable messages + both sides' key dispute points. Prepend `(key checkpoints extracted)` so the arbiter knows it was trimmed. If history is genuinely empty, pass a minimal placeholder like `(no chat history available)`.\n\n\
     **Step 3 — Upload (off-chain multipart):**\n\
     ```bash\n\
     onchainos agent dispute upload {job_id} --role buyer --agent-id {agent_id} --text \"<chat history block from Step 2>\"\n\
     ```\n\
     The CLI auto-attaches every entry under `~/.onchainos/deliverables/buyer/{job_id}/manifest.json` as multipart `files[]` parts — **do NOT pass `--file`**; the manifest covers all locally-saved deliverables / attachments. If the upload fails, retry up to 3 times; if it keeps failing, still proceed to Step 4 — the on-chain dispute will continue without off-chain evidence and the arbiter rules on what is available.\n\n\
     **Step 4 — Notify the user (after upload returns):**\n\n\
     content:\n\
     \x20\x20\x20\x20[Dispute opened] Arbitration for **{title_display}** (`{job_id}`) is on-chain. The system has automatically submitted your evidence (chat history + locally-saved deliverables). Awaiting the arbiter's verdict.\n\
     {l10n_dispatch}\n\n\
     **Step 5 — End this turn.** Do NOT `xmtp_send` anything to the ASP.\n\n\
     [Follow-up events]\n\
     - job_completed → arbitration ruled for the ASP, task completes\n\
     - job_refunded → arbitration ruled for the user, refund\n"
    )
}

pub(crate) fn dispute_resolved(ctx: &FlowContext<'_>) -> String {
    let l10n_dispatch = super::super::flow::L10N_DISPATCH_SHORT;
    let job_id = ctx.job_id;
    let agent_id = ctx.agent_id;
    let title_display = ctx.title_display;
    let title_in_extract = ctx.title_in_extract;
    let terminal_session_hint = &ctx.terminal_session_hint;

    let dispute_won = super::super::content::dispute_won_user_notify(job_id, title_display);
    let dispute_lost = super::super::content::dispute_lost_user_notify(job_id, title_display);
    let rating_notify = super::super::content::rating_submitted_user_notify(job_id);
    format!(
    "[Current Status] dispute_resolved (arbitration ruling issued)\n\
     [Role] User (User Agent)\n\n\
     🛑 **You MUST call `xmtp_dispatch_user` to notify the user of the arbitration result; do not produce a plain text reply inside the sub session** (see Hard Rule 9).\n\n\
     **Step 1 -- Decide winner**: read `message.jobStatus` from the system notification envelope:\n\
     - `jobStatus = \"failed\"` → **user wins**\n\
     - `jobStatus = \"complete\"` → **user loses**\n\
     - other values (e.g. `disputed`) → cannot decide directly; run Step 1.5 to query task details\n\n\
     **Step 1.5 (only when jobStatus is not failed/complete) -- Query task details for the actual status:**\n\
     ```bash\n\
     onchainos agent status {job_id}\n\
     ```\n\
     Decide by the returned `jobStatus` field: `failed` = user wins, `complete` = user loses.\n\n\
     **Step 2 -- Fetch task info:**\n\
     ```bash\n\
     onchainos agent common context {job_id} --role buyer --agent-id {agent_id}\n\
     ```\n\
     Extract {title_in_extract}tokenAmount, tokenSymbol.\n\
     [common context failure fallback] If the command fails or fields are missing, drop dynamic fields and degrade — user wins: `[Dispute Won] Job `{job_id}` — dispute resolved; User Agent wins.` / user loses: `[Dispute Lost] Job `{job_id}` — dispute resolved; ASP wins.` — the user MUST still receive a notification.\n\n\
     **Step 3 -- Call xmtp_dispatch_user to notify the user of the arbitration outcome** — branch by winner:\n\
     {l10n_dispatch}\n\n\
     -------------- User wins (jobStatus=failed) --------------\n\
     content:\n\
     {dispute_won}\n\n\
     -------------- User loses (jobStatus=complete) --------------\n\
     content:\n\
     {dispute_lost}\n\n\
     🛑 Do NOT end this turn — Step 4 (auto-rate) and Step 4.5 (notify rating) below are MANDATORY.\n\n\
     **Step 4 -- 🛑 Auto-rate the ASP (MANDATORY):**\n\
     Based on the deliverable vs the task description, quality standards, and the arbitration outcome (user won / lost), generate:\n\
     \x20\x20- Score: 0.00–5.00 (two decimal places). Guide: user won (provider at fault) → 0.00–2.00; user lost (provider delivered adequately) → 3.00–5.00. Adjust within the range based on specific circumstances.\n\
     \x20\x20- Comment: one sentence, ≤100 characters, evaluating how well the deliverable matches the description.\n\
     Then execute:\n\
     ```bash\n\
     onchainos agent feedback-submit --agent-id <providerAgentId> --creator-id {agent_id} --score <X.XX> --task-id {job_id} --description \"<comment, ≤100 chars>\"\n\
     ```\n\
     ⚠️ `--agent-id` is the ASP being rated (providerAgentId from Step 2 context); `--creator-id` is the buyer's own agent id ({agent_id}).\n\n\
     **Step 4.5 -- Notify the user of the submitted rating:**\n\
     {l10n_dispatch}\n\
     After feedback-submit, call `xmtp_dispatch_user` to notify the user:\n\
     - ✅ **Success** (output contains `txHash`):\n\
     content (fill `<score>` with the X.XX value and `<description>` with the comment you just used in Step 4; fill `<title>` from task context):\n\
     {rating_notify}\n\
     - ❌ **Failure** (error / non-zero exit code) → silently skip; do NOT notify the user, do NOT retry.\n\n\
     **Step 5 -- Terminal wrap-up (keep the sub session):**\n\
     {terminal_session_hint}\n\
     Arbitration flow fully complete.\n"
    )
}
