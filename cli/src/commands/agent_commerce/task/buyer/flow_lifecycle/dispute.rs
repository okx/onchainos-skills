//! Rejection / arbitration prompt generators.

use super::super::flow::FlowContext;

pub(crate) fn job_refused(ctx: &FlowContext<'_>) -> String {
    let l10n_short = super::super::flow::L10N_DISPATCH_SHORT;
    let job_id = ctx.job_id;
    let title_display = ctx.title_display;
    let title_query_hint = ctx.title_query_hint;

    let refused_notify = super::super::content::job_refused_user_notify(job_id, title_display);
    format!(
    "[Current Status] job_refused (user rejection settled on-chain; awaiting ASP decision)\n\
     [Role] User (User Agent)\n\n\
     🛑 **You MUST call `xmtp_dispatch_user` to notify the user that rejection is settled; do not produce a plain text reply inside the sub session** (see Hard Rule 10).\n\n\
     [Your next actions (strict order)]\n\n\
     {title_query_hint}\
     **Step 1 -- Call xmtp_dispatch_user to notify the user the rejection is confirmed:**\n\n\
     content:\n\
     {refused_notify}\n\
     {l10n_short}\n\n\
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

pub(crate) fn job_disputed(ctx: &FlowContext<'_>) -> String {
    let l10n_prompt_bold = super::super::flow::L10N_PROMPT_BOLD;
    let job_id = ctx.job_id;
    let agent_id = ctx.agent_id;
    let short_id = ctx.short_id;
    let title_display = ctx.title_display;
    let title_query_hint = ctx.title_query_hint;
    let session_hint = super::super::flow::SESSION_STATUS_HINT;
    let follow_end = super::super::flow::FOLLOW_PLAYBOOK_END_TURN;
    let idem_check = super::super::flow::idempotency_check(job_id);

    let evidence_prompt = super::super::content::job_disputed_user_evidence_prompt(short_id, title_display);
    format!(
    "[Current Status] job_disputed (arbitration opened; 1-hour evidence preparation window)\n\
     [Role] User (User Agent)\n\n\
     🛑 **CRITICAL -- this event MUST push the evidence request to the user via `pending-decisions-v2 request` (NOT a plain text reply, NOT `xmtp_dispatch_user`).**\n\
     The sub session is not user-facing -- generating a text reply in the sub session (even if the content is correct) = user does not see it + relay channel broken + evidence cannot be submitted.\n\
     The only correct approach: enqueue via `pending-decisions-v2 request` and follow the playbook the CLI returns (which dispatches `xmtp_prompt_user` to the user session).\n\
     ❌ Do not substitute a plain text reply for the `pending-decisions-v2 request` call.\n\
     ❌ Do not substitute `xmtp_dispatch_user` for the `pending-decisions-v2 request` (dispatch_user is pure notification and cannot relay; user replies cannot be routed back to the sub).\n\
     ❌ Do NOT fabricate an evidence summary and call `dispute upload` directly — the sub agent does not know what evidence the user has.\n\
     ❌ Do NOT xmtp_send any message to the ASP — during arbitration both sides interact via on-chain evidence.\n\n\
     [Your next actions (strict order)]\n\n\
     {title_query_hint}\
     {idem_check}\n\
     **Step 1 — Enqueue the evidence decision via `pending-decisions-v2 request`**:\n\n\
     {session_hint}\n\
     ```bash\n\
     onchainos agent pending-decisions-v2 request \\\n\
       --sub-key \"<full sessionKey from session_status>\" \\\n\
       --job-id {job_id} --role buyer --agent-id {agent_id} \\\n\
       --user-content \"{evidence_prompt_for_shell}\" \\\n\
       --list-label \"[Decision {short_id}] Submit Arbitration Evidence\" \\\n\
       --source-event job_disputed\n\
     ```\n\
     {l10n_prompt_bold}\n\n\
     {follow_end}\n\n\
     **Step 2 — After user-session relays as system envelope** (`event: \"user_decision_job_disputed\"`, `message.data: <user's verbatim evidence text>`):\n\
     Call `onchainos agent next-action --jobid {job_id} --jobStatus user_decision_job_disputed --role buyer --agentId {agent_id} --data \"<message.data>\"` — CLI returns a routing playbook pointing to the `dispute_evidence` upload script. The data field IS the evidence; pass it verbatim through (do NOT second-guess length / similarity / detail).\n\n\
     ⚠️ Evidence MUST be submitted within 1 hour, otherwise it expires.\n",
        evidence_prompt_for_shell = evidence_prompt.replace('"', "\\\""),
    )
}

pub(crate) fn dispute_evidence(ctx: &FlowContext<'_>) -> String {
    let job_id = ctx.job_id;
    let agent_id = ctx.agent_id;

    format!(
    "[Current Action] Upload arbitration evidence\n\
     [Role] User (User Agent)\n\n\
     **Step 1 -- Extract evidence content from the user's relay:**\n\
     Routed in via `[USER_DECISION_RELAY] decision: <user verbatim>`. The verbatim text IS the evidence (the pending-decisions-v2 entry was already cleared by `resolve` in the user-session) — extract:\n\
     - Text summary → the text portion the user wrote\n\
     - Image path (if the user provided a local file path) → `--image` parameter\n\
     **At least one** of text and image is required.\n\n\
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

pub(crate) fn dispute_resolved(ctx: &FlowContext<'_>) -> String {
    let l10n_dispatch = super::super::flow::L10N_DISPATCH;
    let job_id = ctx.job_id;
    let agent_id = ctx.agent_id;
    let title_display = ctx.title_display;
    let title_in_extract = ctx.title_in_extract;
    let terminal_session_hint = ctx.terminal_session_hint;

    let dispute_won = super::super::content::dispute_won_user_notify(job_id, title_display);
    let dispute_lost = super::super::content::dispute_lost_user_notify(job_id, title_display);
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
     Extract {title_in_extract}tokenAmount, tokenSymbol.\n\
     [common context failure fallback] If the command fails or fields are missing, drop dynamic fields and degrade — user wins: `[Dispute Won] Job `{job_id}` — dispute resolved; User Agent wins.` / user loses: `[Dispute Lost] Job `{job_id}` — dispute resolved; ASP wins.` — the user MUST still receive a notification.\n\n\
     **Step 3 -- Call xmtp_dispatch_user to notify the user of the arbitration outcome (branch by winner):**\n\n\
     -------------- User wins (jobStatus=rejected) --------------\n\
     content:\n\
     {dispute_won}\n\n\
     -------------- User loses (jobStatus=complete) --------------\n\
     content:\n\
     {dispute_lost}\n\
     {l10n_dispatch}\n\n\
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
