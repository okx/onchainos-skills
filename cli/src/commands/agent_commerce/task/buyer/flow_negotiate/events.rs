//! Event handlers for visibility changes, payment mode changes, and negotiation relays.

use super::super::flow::FlowContext;

pub(crate) fn job_visibility_changed(ctx: &FlowContext<'_>) -> String {
    let l10n_dispatch = super::super::flow::L10N_DISPATCH;
    let job_id = ctx.job_id;
    let title_display = ctx.title_display;
    let title_query_hint = ctx.title_query_hint;

    let visibility_public = super::super::content::visibility_public_user_notify(job_id, title_display);
    let visibility_private = super::super::content::visibility_private_user_notify(job_id, title_display);
    format!(
    "[Current state] job_visibility_changed (public/private toggle is on-chain)\n\
     [Role] User (User Agent)\n\n\
     🛑 **This is not an auxiliary event; you MUST notify the user.**\n\n\
     [Your next actions (strict order)]\n\n\
     {title_query_hint}\
     **Step 1 - read the `visibility` field from the system notification envelope:**\n\
     - `visibility=0` -> public\n\
     - `visibility=1` -> private\n\n\
     **Step 2 - call xmtp_dispatch_user to notify the user that visibility has changed:**\n\
     content:\n\
     \x20\x20- visibility=0 -> {visibility_public}\n\
     \x20\x20- visibility=1 -> {visibility_private}\n\
     {l10n_dispatch}\n\n\
     ⚠️ After switching to public, do **NOT** request the recommended ASP list (recommend); the user just waits for ASPs to reach out.\n\
     -> **end this turn**.\n"
    )
}

pub(crate) fn job_payment_mode_changed(ctx: &FlowContext<'_>) -> String {
    let l10n_dispatch = super::super::flow::L10N_DISPATCH;
    let job_id = ctx.job_id;
    let agent_id = ctx.agent_id;
    let short_id = ctx.short_id;
    let title_display = ctx.title_display;
    let title_query_hint = ctx.title_query_hint;

    let payment_escrow_notify = super::super::content::payment_mode_escrow_user_notify(job_id, title_display);
    let x402_paying = super::super::content::x402_paying_user_notify(job_id, title_display);
    let x402_replay_ok = super::super::content::x402_replay_success_user_notify(job_id);
    let x402_replay_fail = super::super::content::x402_replay_fail_user_notify(job_id);
    format!(
    "[Current state] job_payment_mode_changed (payment-mode switch is on-chain)\n\
     [Role] User (User Agent)\n\n\
     🛑 **You MUST notify the user of the payment-mode change.**\n\n\
     🛑 **Allowed-action whitelist for this event**: escrow path - only xmtp_send [intent:confirm] + xmtp_dispatch_user notify the user; x402 path - only x402-check + task-402-pay + xmtp_dispatch_user.\n\
     ❌ Do NOT call set-payment-mode again (paymentMode is already on-chain; calling again pollutes state).\n\
     ❌ Do NOT call save-agreed (already done in the negotiate_ack event).\n\
     ❌ Do NOT call apply (apply is an ASP action; the user never executes it).\n\
     ❌ Do NOT call confirm-accept (the ASP has not applied yet; must wait for the ASP to apply after seeing CONFIRM).\n\n\
     [Your next actions]\n\n\
     {title_query_hint}\
     **Step 1 - read the `paymentMode` field from the system notification envelope:**\n\
     paymentMode value mapping: 1=escrow, 3=x402.\n\
     ⚠️ Use the `paymentMode` from the envelope directly; no extra API query needed.\n\n\
     ━━━━━━━━━ escrow (paymentMode=1) - send [intent:confirm] to trigger ASP apply ━━━━━━━━━\n\n\
     **Step 3 - send [intent:confirm] (the ONLY legitimate trigger for ASP apply)**:\n\
     On-chain paymentMode is now in place; it is safe to send [intent:confirm] for the ASP to apply.\n\
     Take **all fields verbatim** (paymentMode / tokenSymbol / tokenAmount) from the [intent:propose] you sent / the [intent:ack] you received - just replay the sub session history and copy:\n\n\
     Call xmtp_send:\n\
     \x20\x20content=\n\
     \x20\x20jobId: {job_id}\n\
     \x20\x20paymentMode: escrow\n\
     \x20\x20tokenSymbol: <identical to [intent:ack]>\n\
     \x20\x20tokenAmount: <identical to [intent:ack]>\n\
     \x20\x20[intent:confirm]\n\n\
     ⚠️ **Do NOT** bypass with natural language like \"please apply / please accept\" - the ASP's flow.rs treats the `[intent:confirm]` literal as the only apply trigger; natural-language instructions **will not be recognized**.\n\
     ⚠️ apply is an ASP action; the user does not execute apply.\n\n\
     **Step 4 - notify the user:**\n\
     Call xmtp_dispatch_user:\n\
     \x20\x20content: {payment_escrow_notify}\n\
     {l10n_dispatch}\n\n\
     -> **end this turn** and wait for the ASP's XMTP message announcing the apply (handled by buyer.md routing priority #2).\n\n\
     ━━━━━━━━━ x402 (paymentMode=3) ━━━━━━━━━\n\n\
     From the previous set-payment-mode / x402-check output, extract endpoint, acceptsJson, feeTokenSymbol, feeAmount, providerAgentId.\n\n\
     ⚠️ **Parameter-loss fallback** (context compaction may drop the previous turn's output):\n\
     If providerAgentId or endpoint is missing in context -> first call:\n\
     ```bash\n\
     onchainos agent common context {job_id} --role buyer --agent-id {agent_id}\n\
     ```\n\
     to extract `providerAgentId`; get `endpoint` from `services[0].endpoint` of `onchainos agent service-list --agent-id <providerAgentId>`.\n\n\
     If acceptsJson / feeTokenSymbol / feeAmount is missing -> re-validate with the endpoint above:\n\
     ```bash\n\
     onchainos agent x402-check --endpoint <endpoint> --agent-id {agent_id}\n\
     ```\n\
     Extract `acceptsJson`, `tokenSymbol` (= feeTokenSymbol), `amountHuman` (= feeAmount).\n\n\
     **x402 stage 1.5 - notify the user that payment is in progress (before task-402-pay):**\n\
     Call xmtp_dispatch_user:\n\
     \x20\x20content: {x402_paying}\n\
     {l10n_dispatch}\n\n\
     **x402 stage 2 - sign + direct/accept + endpoint replay (atomic command):**\n\
     ```bash\n\
     onchainos agent task-402-pay {job_id} --provider-agent-id <providerAgentId> --accepts '<acceptsJson>' --endpoint <endpoint URL> --token-symbol <feeTokenSymbol> --token-amount <feeAmount>\n\
     ```\n\
     Internally executes: x402_pay signing -> direct/accept on-chain -> assemble payment header -> replay endpoint.\n\
     Output: {{ replaySuccess, replayStatus, replayBody, replayBodyDisplay, signature, authorization, sessionCert, txHash }}\n\n\
     **x402 stage 2 Step 2.5 — persist the deliverable when replaySuccess=true** (skip if replaySuccess=false):\n\n\
     The replay result only lives in session context; without saving it is lost after context compaction or session end.\n\
     Write `replayBody` (the raw endpoint response — JSON or plain text; full content, no truncation) to a temp file, then call:\n\
     ```bash\n\
     onchainos agent task-deliverable-save --job-id {job_id} --role buyer \\\n\
       --file \"<temp .txt path>\" --deliverable-type text \\\n\
       --title \"<task title from common context>\" --short-id {short_id} \\\n\
       --counterparty-agent-id \"<providerAgentId>\" --counterparty-name \"<providerName>\" \\\n\
       --token-symbol \"<tokenSymbol>\" --token-amount \"<tokenAmount>\"\n\
     ```\n\
     ⚠️ `--title` and counterparty fields: use values from `onchainos agent common context {job_id} --role buyer --agent-id {agent_id}` (already called in the parameter-loss fallback above, or from session context).\n\
     If save fails, log the error but do NOT block — the user notification in the next step is more important.\n\n\
     **x402 stage 2 Step 3 - check replay result and notify the user:**\n\
     Call xmtp_dispatch_user with the following content template (branch by `replaySuccess`):\n\n\
     ▸ replaySuccess=true:\n\
     {x402_replay_ok}\n\n\
     ▸ replaySuccess=false:\n\
     {x402_replay_fail}\n\
     {l10n_dispatch}\n\n\
     🛑 The `replayBodyDisplay` field contains the deliverable content; when replaySuccess=true it **must** be included in full.\n\
     🔴 Real incident: a model composed \"x402 payment succeeded, awaiting confirmation\" and dropped the replayBody deliverable content; the user never saw the data the ASP returned.\n\n\
     -> **end this turn** and wait for the `job_accepted` system notification.\n\n\
     🛑🛑🛑 **Iron rule (MANDATORY) after receiving `job_accepted`**:\n\
     After the `job_accepted` system event arrives, you **must** call:\n\
     ```bash\n\
     onchainos agent next-action --jobid {job_id} --jobStatus job_accepted --role buyer --agentId {agent_id}\n\
     ```\n\
     Follow the returned script (the script will guide you to run `onchainos agent complete`).\n\
     ❌ **Absolutely forbidden**: re-running this turn's `x402-check` / `task-402-pay` / `xmtp_dispatch_user` - those completed in this turn; re-running causes double payment or duplicate notification.\n\
     ❌ **Absolutely forbidden**: skipping `next-action` and deciding the next step yourself - the `job_accepted` script contains the `complete` step; skipping = the job is permanently stuck in the accepted state.\n"
    )
}

pub(crate) fn negotiate_reply(ctx: &FlowContext<'_>) -> String {
    let l10n_prompt = super::super::flow::L10N_PROMPT;
    let session_hint = super::super::flow::SESSION_STATUS_HINT;
    let follow_playbook = super::super::flow::FOLLOW_PLAYBOOK;
    let job_id = ctx.job_id;
    let agent_id = ctx.agent_id;
    let short_id = ctx.short_id;
    let cmd_over_budget = super::super::flow::pending_cmd(job_id, agent_id, &format!("[Over budget {short_id}] A/B/C"));
    let title_query_hint = ctx.title_query_hint;

    let over_budget = super::super::content::over_budget_user_prompt(short_id);
    format!(
    "[Negotiation relay] negotiate_reply (ASP natural-language reply, no structured marker)\n\
     [Role] User (User Agent)\n\n\
     During negotiation the ASP sent a natural-language message (could be a quote, detail discussion, a question, etc.). You must **evaluate and respond on your own**.\n\n\
     🛑 **Mandatory pre-evaluation**: Step 1 and Step 2 are mandatory - they must complete before you may send any xmtp_send (including a reject). Do NOT skip evaluation and reply or reject directly.\n\n\
     {title_query_hint}\
     [Your next actions (strict order)]\n\n\
     **Step 1 - fetch task context (run once per turn if not already done):**\n\
     ```bash\n\
     onchainos agent common context {job_id} --role buyer --agent-id {agent_id}\n\
     ```\n\
     Extract the key fields: budget, paymentMostTokenAmount (max_budget), tokenSymbol, description.\n\n\
     **Step 2 - evaluate the ASP's reply:**\n\n\
     🛑 **Iron rule: any message replying to the ASP must NEVER reveal the max_budget value** - leaking = the ASP quotes the cap immediately = the user loses all bargaining power.\n\
     🚫 **Negotiation-autonomy red line**: except for the \"quote > max_budget\" auto-REJECT path below, do NOT call **any** user-facing tool (`xmtp_dispatch_user` / `pending-decisions-v2 request`) to make the user decide on negotiation. Negotiation is autonomous in the sub session - evaluate via the decision matrix and reply directly to the ASP (natural-language discussion / [intent:propose]); do NOT forward the quote to the user asking \"do you accept?\" or \"please confirm\".\n\
     🔴 Real incident: model correctly called next-action but then used `xmtp_dispatch_user` to forward the quote to the user — `xmtp_dispatch_user` is equally forbidden for this purpose.\n\n\
     Extract quote info from the ASP's message if any: amount, token, payment-mode preference, delivery time.\n\n\
     🔴 **Quote evaluation decision matrix** (if the ASP gave an explicit price):\n\
     \x20\x20| ASP quote | Action |\n\
     \x20\x20|---|---|\n\
     \x20\x20| <= budget | Price acceptable; after confirming other terms, send [intent:propose] |\n\
     \x20\x20| budget < quote <= max_budget | Bargaining room, counter on your own |\n\
     \x20\x20| > max_budget | **auto-REJECT + switch** (see below) |\n\n\
     **Mandatory action when quote > max_budget**:\n\
     \x20\x20a) xmtp_send `[intent:reject]`:\n\
     \x20\x20\x20\x20content=\n\
     \x20\x20\x20\x20jobId: {job_id}\n\
     \x20\x20\x20\x20reason: quote exceeds max budget\n\
     \x20\x20\x20\x20[intent:reject]\n\
     \x20\x20b) `onchainos agent mark-failed {job_id} --provider <current ASP agentId>`\n\
     \x20\x20c) Enqueue the user decision via `pending-decisions-v2 request`:\n\
     \x20\x20\x20\x20{session_hint}\n\
     \x20\x20\x20\x20```bash\n\
     \x20\x20\x20\x20{cmd_over_budget}\n\
     \x20\x20\x20\x20```\n\
     \x20\x20\x20\x20`--user-content` template (canonical English; 🌐 localize per [Localization] rules):\n\
     {over_budget}\n\
     \x20\x20\x20\x20{l10n_prompt}\n\
     \x20\x20\x20\x20{follow_playbook}\n\
     \x20\x20\x20\x20-> **end this turn** and wait for the user's reply.\n\
     \x20\x20\x20\x20After receiving `[USER_DECISION_RELAY] decision: <user verbatim>`, keyword-route:\n\
     \x20\x20\x20\x20- Verbatim is `A` / `选A` / contains `推荐` / `recommend` / `列表` / `list` → `onchainos agent recommend {job_id} --agent-id {agent_id}` then show the list via `pending-decisions-v2 request` (same format as Step 2 in job_created)\n\
     \x20\x20\x20\x20- Verbatim is `B` / `选B` / contains `指定` / `specify` or looks like an agentId → `onchainos agent next-action --jobid {job_id} --jobStatus job_created --role buyer --agentId {agent_id} --provider <agentId>`\n\
     \x20\x20\x20\x20- Verbatim is `C` / `选C` / contains `关闭` / `close` / `取消` → `onchainos agent close {job_id}`\n\
     \x20\x20\x20\x20- Otherwise → `pending-decisions-v2 request` again with clarifying userContent to re-ask.\n\n\
     **Step 3 - reply to the ASP (depends on Step 2 evaluation):**\n\n\
     - **ASP is still in discussion (no explicit price yet or asking for details)** -> xmtp_send a natural-language reply to keep discussing.\n\n\
     - **Both sides agree on tokenAmount / tokenSymbol / paymentMode** -> send [intent:propose]:\n\
     \x20\x20📋 **Mandatory pre-fill self-check**: replay sub session history field-by-field and find **the last value both sides explicitly agreed on**.\n\
     \x20\x20content=\n\
     \x20\x20jobId: {job_id}\n\
     \x20\x20paymentMode: escrow\n\
     \x20\x20tokenSymbol: <USDT|USDG>\n\
     \x20\x20tokenAmount: <amount>\n\
     \x20\x20[intent:propose]\n\n\
     ⚠️ **In an A2A negotiation session paymentMode is fixed to escrow.**\n\
     ⚠️ **Do NOT replace [intent:propose] with natural language** - the ASP Agent only recognizes structured markers; \"please apply / terms locked\" in natural language will not be parsed.\n\
     ⚠️ **Only one xmtp_send per turn.**\n\
     🚫 🛑 **CRITICAL - this event absolutely forbids save-agreed / set-payment-mode / confirm-accept** - those only run in the later negotiate_ack event. ASP natural-language phrases like \"I accept\", \"agree\", \"OK\", \"no problem\" are **NOT** `[intent:ack]` - only content that starts with the literal `[intent:ack]` square brackets counts. Before the user sends [intent:propose], the ASP cannot reply with [intent:ack]. Violating this = skipping the three-step handshake = the job is permanently stuck.\n\
     -> **end this turn** and wait for the ASP's reply.\n")
}

pub(crate) fn negotiate_ack(ctx: &FlowContext<'_>) -> String {
    let job_id = ctx.job_id;
    let agent_id = ctx.agent_id;
    let title_query_hint = ctx.title_query_hint;

    format!(
    "[Negotiation relay] negotiate_ack (ASP accepts the PROPOSE and replies [intent:ack])\n\
     [Role] User (User Agent)\n\n\
     The ASP replied [intent:ack] - accepting the terms in your [intent:propose].\n\n\
     {title_query_hint}\
     [Your next actions (strict order)]\n\n\
     **Step 1 - verify field-by-field that the ACK matches your PROPOSE:**\n\
     Replay sub session history and compare the ASP's ACK paymentMode / tokenSymbol / tokenAmount with your most recent PROPOSE.\n\
     - **Any field mismatch** -> treat as tampering; xmtp_send to tell the ASP the fields don't match and resend [intent:propose]; end the turn.\n\
     - **All match** -> continue to Step 2.\n\n\
     🛑 **Allowed-CLI whitelist for this event**: save-agreed -> set-payment-mode; **only these two, in this fixed order**.\n\
     ❌ Do NOT call confirm-accept (the ASP has not applied yet).\n\
     ❌ Do NOT call complete / reject (the job has not entered execution).\n\
     ❌ Do NOT call apply (apply is an ASP action; the user never executes it).\n\n\
     **Step 2 - save-agreed persistence (🛑 do not skip):**\n\
     ```bash\n\
     onchainos agent save-agreed {job_id} --provider <providerAgentId of the current negotiation> --token-symbol <tokenSymbol from ACK> --token-amount <tokenAmount from ACK> --agent-id {agent_id}\n\
     ```\n\
     🛑 save-agreed **must run before set-payment-mode** - it persists the negotiation outcome, and later confirm-accept depends on this data. Skipping save-agreed and going straight to set-payment-mode -> confirm-accept will use wrong parameters.\n\n\
     **Step 3 - set-payment-mode (A2A negotiation is fixed to escrow):**\n\
     ⚠️ **Whatever the on-chain paymentType currently is, you MUST execute this**; do NOT call common context to compare.\n\
     ```bash\n\
     onchainos agent set-payment-mode {job_id} --payment-mode escrow --token-symbol <tokenSymbol from ACK> --token-amount <tokenAmount from ACK>\n\
     ```\n\
     This command returns exit code 2 (confirming).\n\n\
     🛑 **Iron rule: in THIS turn xmtp_send [intent:confirm] is absolutely forbidden** - this is the most common deadlock trigger.\n\
     On-chain paymentMode is still in the mempool; the ASP would apply on seeing CONFIRM, but paymentMode is unconfirmed, so apply would fail.\n\
     [intent:confirm] may **only** be sent after the `job_payment_mode_changed` system event arrives - no exceptions.\n\n\
     -> **end this turn** and wait for the `job_payment_mode_changed` system notification.\n"
    )
}

pub(crate) fn negotiate_counter(ctx: &FlowContext<'_>) -> String {
    let job_id = ctx.job_id;
    let agent_id = ctx.agent_id;
    let title_query_hint = ctx.title_query_hint;

    format!(
    "[Negotiation relay] negotiate_counter (ASP sends a counter-offer [intent:counter])\n\
     [Role] User (User Agent)\n\n\
     The ASP rejected your PROPOSE and sent an [intent:counter] counter-offer.\n\n\
     🛑 **This event forbids save-agreed / set-payment-mode / confirm-accept / apply** - COUNTER means terms are not yet agreed; you may only send a new [intent:propose] or [intent:reject].\n\
     🛑 **Iron rule: any message replying to the ASP must NEVER reveal the max_budget value** - leaking = the ASP quotes the cap immediately = the user loses all bargaining power.\n\n\
     {title_query_hint}\
     [Your next actions (strict order)]\n\n\
     **Step 1 - round counting:**\n\
     Replay sub session history and count the total `[intent:counter]` messages the ASP has sent (including this one).\n\
     🔢 **COUNTER round limit = 3**:\n\
     - This is the 3rd (or later) COUNTER -> **do NOT process the COUNTER content**; directly xmtp_send:\n\
     \x20\x20content=\n\
     \x20\x20jobId: {job_id}\n\
     \x20\x20reason: negotiation round limit reached, 3 COUNTERs already\n\
     \x20\x20[intent:reject]\n\
     \x20\x20then `onchainos agent mark-failed {job_id} --provider <current ASP agentId>`,\n\
     \x20\x20then enqueue the user decision via `pending-decisions-v2 request` (same pattern as negotiate_reply over-budget: A. view recommendations / B. specify ASP / C. close — see that scene for the exact command and keyword routing).\n\
     \x20\x20-> **end this turn** and wait for the user relay.\n\n\
     - Under the limit -> continue to Step 2.\n\n\
     **Step 2 - PROPOSE typo self-check (highest priority):**\n\
     ⚠️ **Replay sub session history first to confirm whether your previous [intent:propose] had a typo**:\n\
     \x20\x20- COUNTER amount **equals** the number you last agreed in natural language -> **your PROPOSE had a typo**: resend [intent:propose] with the COUNTER value; do NOT haggle again.\n\
     \x20\x20- COUNTER amount **is higher than** the number you last agreed in natural language -> this is genuinely an ASP markup; continue to Step 3.\n\n\
     **Step 3 - evaluate the COUNTER terms:**\n\
     Get max_budget:\n\
     ```bash\n\
     onchainos agent common context {job_id} --role buyer --agent-id {agent_id}\n\
     ```\n\
     Extract `paymentMostTokenAmount`.\n\n\
     \x20\x20| COUNTER quote | Action |\n\
     \x20\x20|---|---|\n\
     \x20\x20| <= budget | Acceptable; send a new [intent:propose] with the COUNTER value |\n\
     \x20\x20| budget < quote <= max_budget | Acceptable, or keep negotiating; send a new [intent:propose] |\n\
     \x20\x20| > max_budget | xmtp_send `[intent:reject]`, mark-failed, enqueue user decision via `pending-decisions-v2 request` (same as the over-budget handling in negotiate_reply) |\n\n\
     - Check tokenSymbol change: if the ASP suggests a different token, evaluate whether to accept.\n\
     - paymentMode is fixed to escrow; do not accept any other payment mode.\n\n\
     **Step 4 - send a new [intent:propose] (if you decide to accept or counter):**\n\
     \x20\x20content=\n\
     \x20\x20jobId: {job_id}\n\
     \x20\x20paymentMode: escrow\n\
     \x20\x20tokenSymbol: <USDT|USDG>\n\
     \x20\x20tokenAmount: <amount>\n\
     \x20\x20[intent:propose]\n\n\
     ⚠️ **Do NOT replace [intent:propose] with natural language** - the ASP Agent only recognizes structured markers.\n\
     -> **end this turn** and wait for the ASP's reply with [intent:ack] / [intent:counter] / [intent:reject].\n"
    )
}
