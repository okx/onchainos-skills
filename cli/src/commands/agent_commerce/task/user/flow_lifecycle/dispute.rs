//! Rejection / arbitration prompt generators.

use super::super::flow::{FlowContext, notify_and_end};
use crate::commands::agent_commerce::task::common::okx_a2a;

pub(crate) fn job_rejected(ctx: &FlowContext<'_>) -> String {
    let content = super::super::content::job_rejected_user_notify(ctx.job_id, ctx.title_display);
    notify_and_end(&content)
}

pub(crate) fn job_disputed(ctx: &FlowContext<'_>) -> String {
    let job_id = ctx.job_id;
    let agent_id = ctx.agent_id;
    let title_display = ctx.title_display;
    let title_query_hint = ctx.title_query_hint;

    let provider_id = match ctx.prefetched
        .and_then(|p| p.provider_agent_id.as_deref())
        .filter(|s| !s.is_empty())
    {
        Some(s) => s,
        None => return format!(
            "[job_disputed] prefetched.provider_agent_id missing for job {job_id}; cannot fetch chat history for dispute evidence.\n\n\
             See _shared/exception-escalation.md §2 — push `cli_failed` decision.\n"
        ),
    };
    let chat_block = match okx_a2a::session_history(job_id, provider_id) {
        Ok(raw) => {
            let trimmed = raw.trim();
            if trimmed.is_empty() || trimmed == "[]" {
                "(no chat history available)".to_string()
            } else {
                trimmed.to_string()
            }
        }
        Err(e) => return format!(
            "[job_disputed] `okx-a2a session history` failed: {e}\n\n\
             See _shared/exception-escalation.md §2 — push `cli_failed` decision.\n"
        ),
    };

    format!(
    "[Current Status] job_disputed (arbitration opened; CLI auto-submits evidence on this event)\n\
     [Role] User Agent\n\n\
     **This event triggers an AUTOMATIC evidence upload — no user interaction**.\n\
     The agent does NOT ask the user for evidence; it formats the chat history, calls `dispute upload`\n\
     (which also auto-attaches every saved deliverable from `~/.onchainos/deliverables/user/{job_id}/`),\n\
     and then notifies the user via `onchainos agent user-notify`. **Do NOT** use `pending-decisions-v2 request`\n\
     for this event. **Do NOT** send any message to the ASP — both sides see the arbitration via on-chain events.\n\n\
     [Your next actions (strict order)]\n\n\
     {title_query_hint}\
     **Step 1 — Chat history (pre-fetched and inlined below; do NOT call `okx-a2a session history` again):**\n\n\
     ```\n\
     ==== Negotiation / delivery chat history ====\n\
     {chat_block}\n\
     ```\n\n\
     **Step 2 — Extract a `--text` body from the chat history above** (≤16 KB):\n\
     Keep ONLY the key checkpoints — task-detail discussion / deliverable messages + both sides' key dispute points. Prepend `(key checkpoints extracted)` so the arbiter knows it was trimmed. If history is genuinely empty, pass a minimal placeholder like `(no chat history available)`.\n\n\
     **Step 3 — Upload (off-chain multipart):**\n\
     ```bash\n\
     onchainos agent dispute upload {job_id} --role user --agent-id {agent_id} --text \"<chat history block from Step 2>\"\n\
     ```\n\
     The CLI auto-attaches every entry under `~/.onchainos/deliverables/user/{job_id}/manifest.json` as multipart `files[]` parts — **do NOT pass `--file`**; the manifest covers all locally-saved deliverables / attachments. If the upload fails, retry up to 3 times; if it keeps failing, still proceed to Step 4 — the on-chain dispute will continue without off-chain evidence and the arbiter rules on what is available.\n\n\
     **Step 4 — Notify the user via `onchainos agent user-notify` (after upload returns):**\n\
     **Localize first** — translate the content below into the user's language before sending.\n\
     ```bash\n\
     onchainos agent user-notify --content '<localized content>'\n\
     ```\n\
     Content:\n\
     \x20\x20\x20\x20[Dispute opened] Arbitration for **{title_display}** (`{job_id}`) is on-chain. The system has automatically submitted your evidence (chat history + locally-saved deliverables). Awaiting the arbiter's verdict.\n\n\
     **Step 5 — End this turn.** Do NOT send any message to the ASP.\n\n\
"
    )
}

pub(crate) fn dispute_resolved(ctx: &FlowContext<'_>) -> String {
    let job_id = ctx.job_id;
    let agent_id = ctx.agent_id;
    let title_display = ctx.title_display;
    let terminal_session_hint = &ctx.terminal_session_hint;

    let dispute_won = super::super::content::dispute_won_user_notify(job_id, title_display);
    let dispute_lost = super::super::content::dispute_lost_user_notify(job_id, title_display);
    let rating_notify = super::super::content::rating_submitted_user_notify(job_id, title_display);

    // dispute_resolved fires when the chain has settled the arbitration —
    // prefetched.status MUST be 6 (Completed, ASP wins) or 9 (Failed, user
    // wins) at this point. Anything else (None / unexpected value / missing
    // provider) is a data anomaly — bail to a cli_failed instruction instead
    // of running a half-blind double-branch playbook.
    let p = match ctx.prefetched {
        Some(p) => p,
        None => return format!(
            "[dispute_resolved] no prefetched task context for job {job_id}; cannot decide winner.\n\n\
             See _shared/exception-escalation.md §2 — push `cli_failed` decision.\n"
        ),
    };
    let user_won = match p.status {
        Some(9) => true,
        Some(6) => false,
        Some(other) => return format!(
            "[dispute_resolved] unexpected prefetched status {other} for job {job_id}; expected 6 (completed/ASP wins) or 9 (failed/user wins).\n\n\
             See _shared/exception-escalation.md §2 — push `cli_failed` decision.\n"
        ),
        None => return format!(
            "[dispute_resolved] prefetched.status missing for job {job_id}; cannot decide winner.\n\n\
             See _shared/exception-escalation.md §2 — push `cli_failed` decision.\n"
        ),
    };
    let provider_id = match p.provider_agent_id.as_deref().filter(|s| !s.is_empty()) {
        Some(s) => s,
        None => return format!(
            "[dispute_resolved] prefetched.provider_agent_id missing for job {job_id}; auto-rate cannot run.\n\n\
             See _shared/exception-escalation.md §2 — push `cli_failed` decision.\n"
        ),
    };

    let winner_line = if user_won {
        "**Arbitration outcome: user WINS** (chain status = 9/failed).\n\n"
    } else {
        "**Arbitration outcome: user LOSES** (chain status = 6/completed; ASP wins).\n\n"
    };
    let dispatch_content = if user_won { &dispute_won } else { &dispute_lost };
    let score_guide = if user_won {
        "provider at fault → 0.00–2.00"
    } else {
        "provider delivered adequately → 3.00–5.00"
    };

    format!(
    "[Current Status] dispute_resolved (arbitration ruling issued)\n\
     [Role] User Agent\n\n\
     **You MUST notify the user of the arbitration result + auto-rating in ONE consolidated message** — auto-rate FIRST, then send a single `onchainos agent user-notify` combining both pieces.\n\n\
     {winner_line}\
     **Step 1 — Task fields (pre-fetched; do NOT call `common context`):**\n\
     \x20\x20- title: {title}\n\
     \x20\x20- tokenAmount: {amt} | tokenSymbol: {sym}\n\
     \x20\x20- providerAgentId: {provider_id}\n\n\
     **Step 2 — Auto-rate the ASP FIRST (MANDATORY; must complete before Step 3):**\n\
     Based on the deliverable vs the task description, quality standards, and the arbitration outcome, generate:\n\
     \x20\x20- Score: 0.00–5.00 (two decimal places). Guide: {score_guide}. Adjust within the range based on specific circumstances.\n\
     \x20\x20- Comment: one sentence, ≤100 characters, evaluating how well the deliverable matches the description.\n\
     Then execute:\n\
     ```bash\n\
     onchainos agent feedback-submit --agent-id {provider_id} --creator-id {agent_id} --score <X.XX> --task-id {job_id} --description \"<comment, ≤100 chars>\"\n\
     ```\n\
     Record whether feedback-submit succeeded (output contains `txHash`) or failed; the result decides whether the rating half is included in Step 3.\n\n\
     **Step 3 — Notify the user with a SINGLE consolidated message:**\n\
     **Localize first** — translate the composed content into the user's language before sending.\n\
     ```bash\n\
     onchainos agent user-notify --content '<localized content>'\n\
     ```\n\
     Compose by merging the two halves below (concatenate with two blank lines between them):\n\n\
     ▸ Arbitration outcome (always included):\n\
     \x20\x20{dispatch_content}\n\n\
     ▸ Rating info (include ONLY if Step 2's feedback-submit succeeded; if it failed, omit this entire half):\n\
     \x20\x20{rating_notify}\n\
     \x20\x20(fill `<score>` with the X.XX value used in Step 2, `<description>` with the comment from Step 2, `<title>` with the task title above)\n\n\
     **Step 4 — Terminal wrap-up (keep the sub session):**\n\
     {terminal_session_hint}\n\
     Arbitration flow fully complete.\n",
        title = p.title,
        amt = p.token_amount,
        sym = p.token_symbol,
    )
}
