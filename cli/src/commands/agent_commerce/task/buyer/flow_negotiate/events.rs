//! Event handlers for visibility changes, payment mode changes, and negotiation relays.

use super::super::flow::FlowContext;

pub(crate) fn job_visibility_changed(ctx: &FlowContext<'_>) -> String {
    let l10n_dispatch = super::super::flow::L10N_DISPATCH_SHORT;
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
     **Step 2 - call xmtp_dispatch_user to notify the user that visibility has changed** ({l10n_dispatch}):\n\
     content:\n\
     \x20\x20- visibility=0 -> {visibility_public}\n\
     \x20\x20- visibility=1 -> {visibility_private}\n\n\
     ⚠️ After switching to public, do **NOT** request the recommended ASP list (recommend); the user just waits for ASPs to reach out.\n\
     -> **end this turn**.\n"
    )
}

pub(crate) fn job_payment_mode_changed(ctx: &FlowContext<'_>) -> String {
    let l10n_dispatch = super::super::flow::L10N_DISPATCH_SHORT;
    let job_id = ctx.job_id;
    let agent_id = ctx.agent_id;
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
     **Step 2 - send [intent:confirm] (the ONLY legitimate trigger for ASP apply)**:\n\
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
     **Step 3 - notify the user via xmtp_dispatch_user** ({l10n_dispatch}):\n\
     \x20\x20content: {payment_escrow_notify}\n\n\
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
     **Step 2 — 🌐 notify the user that payment is in progress via xmtp_dispatch_user:**\n\
     {l10n_dispatch}\n\
     \x20\x20content: {x402_paying}\n\n\
     **Step 3 — sign + direct/accept + endpoint replay (atomic command):**\n\
     ```bash\n\
     onchainos agent task-402-pay {job_id} --provider-agent-id <providerAgentId> --accepts '<acceptsJson>' --endpoint <endpoint URL> --token-symbol <feeTokenSymbol> --token-amount <feeAmount>\n\
     ```\n\
     Internally executes: x402_pay signing -> direct/accept on-chain -> assemble payment header -> replay endpoint.\n\
     Output: {{ replaySuccess, replayStatus, replayBody, replayBodyDisplay, deliverableSavedPath, signature, authorization, sessionCert, txHash }}\n\
     ✅ The CLI **auto-saves** the deliverable to disk when replaySuccess=true (`deliverableSavedPath` in output). No manual `task-deliverable-save` call needed.\n\n\
     🔴🔴🔴 **CRITICAL — Step 4: notify the user with the FULL deliverable content via xmtp_dispatch_user**\n\
     {l10n_dispatch}\n\
     The `replayBodyDisplay` field in the CLI output IS the deliverable the user paid for. You **MUST** copy it verbatim into the notification template below.\n\
     ❌ Do NOT summarize, truncate, or omit `replayBodyDisplay` — doing so = the user paid but never received the deliverable.\n\
     ❌ Do NOT compose your own \"payment succeeded\" message — use the template below which includes the deliverable content.\n\
     🔴 Real incident: a model composed \"x402 payment succeeded, awaiting confirmation\" and dropped the replayBody deliverable content; the user never saw the data they paid for.\n\n\
     Branch by `replaySuccess`:\n\n\
     ▸ replaySuccess=true:\n\
     {x402_replay_ok}\n\n\
     ▸ replaySuccess=false:\n\
     {x402_replay_fail}\n\n\
     -> **end this turn** and wait for the `job_accepted` system notification.\n\n\
     🛑🛑🛑 **Iron rule (MANDATORY) after receiving `job_accepted`**:\n\
     After the `job_accepted` system event arrives, you **must** call:\n\
     ```bash\n\
     onchainos agent next-action --jobid {job_id} --event job_accepted --role buyer --agentId {agent_id}\n\
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
    let title = ctx.title_display;
    let cmd_over_budget = super::super::flow::pending_cmd(job_id, agent_id, &format!("[Over budget {short_id}] {title} budget decision"), "negotiate_over_budget");
    let title_query_hint = ctx.title_query_hint;

    let over_budget = super::super::content::over_budget_user_prompt(short_id);
    format!(
    "[Negotiation relay] negotiate_reply (ASP natural-language reply, no structured marker)\n\
     [Role] User (User Agent)\n\n\
     During negotiation the ASP sent a natural-language message (could be a quote, detail discussion, a question, etc.). You must **evaluate and respond on your own**.\n\n\
     🛑 **Mandatory pre-evaluation**: Step 1 and Step 2 are mandatory - they must complete before you may send any xmtp_send (including a reject). Do NOT skip evaluation and reply or reject directly.\n\n\
     {title_query_hint}\
     [Your next actions (strict order)]\n\n\
     **Step 1 - load task context (run once per turn if not already done):**\n\
     Read budget (tokenAmount), paymentMostTokenAmount (max_budget), tokenSymbol, description from the `[Pre-fetched task context]` block above if available.\n\
     If any field is missing or the block is absent, fall back to:\n\
     ```bash\n\
     onchainos agent common context {job_id} --role buyer --agent-id {agent_id}\n\
     ```\n\n\
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
     \x20\x20\x20\x20{l10n_prompt}\n\
     \x20\x20\x20\x20`--user-content` template (canonical English):\n\
     {over_budget}\n\
     \x20\x20\x20\x20{follow_playbook}\n\
     \x20\x20\x20\x20-> **end this turn** and wait for the user's reply.\n\
     \x20\x20\x20\x20After the user-session relays the reply as a system envelope (`event:\"user_decision_negotiate_over_budget\"`, `message.data:<verbatim>`), call `next-action --event user_decision_negotiate_over_budget --data \"<message.data>\"` — CLI returns a routing playbook (A=view recommendations / B=specify ASP / C=close); follow it verbatim. Do NOT keyword-match yourself.\n\n\
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
     ⏱ **5-minute timeout**: if the ASP does not reply within 5 minutes, xmtp_send `[intent:reject]` (reason: negotiation timeout), then `onchainos agent mark-failed {job_id} --provider <ASP agentId>` + `onchainos agent recommend {job_id} --agent-id {agent_id}` to switch. Do NOT call `xmtp_delete_conversation` when switching — just ignore further messages from that ASP.\n\
     🚫 🛑 **CRITICAL - this event absolutely forbids save-agreed / set-payment-mode / confirm-accept** - those only run in the later negotiate_ack event. ASP natural-language phrases like \"I accept\", \"agree\", \"OK\", \"no problem\" are **NOT** `[intent:ack]` - only content that starts with the literal `[intent:ack]` square brackets counts. Before the user sends [intent:propose], the ASP cannot reply with [intent:ack]. Violating this = skipping the three-step handshake = the job is permanently stuck.\n\
     -> **end this turn** and wait for the ASP's reply.\n")
}

/// CLI-mode variant of `negotiate_reply`. Differences vs the MCP-path version:
/// - Task fields (title / description / token_symbol / base budget / max_budget)
///   are inlined verbatim from `ctx.prefetched`, so the LLM doesn't need to
///   run `common context` (Step 1 removed).
/// - The user-facing tool calls switch from MCP `xmtp_send` to bash
///   `okx-a2a xmtp-send`, so we don't depend on an MCP host being present.
/// - Routing through `use_cli_minimal` short-circuits the negotiate preamble
///   and `version_prefix`, leaving only `LOCALIZATION_PREFIX` + this body.
///
/// What stays LLM-driven: extracting the ASP's quote (natural language),
/// applying the decision matrix, and composing the reply body. max_budget is
/// inlined because the LLM MUST know it to detect over-budget — but the
/// "never reveal to ASP" iron rule remains.
pub(crate) fn negotiate_reply_cli(ctx: &FlowContext<'_>) -> String {
    let job_id = ctx.job_id;
    let agent_id = ctx.agent_id;
    let short_id = ctx.short_id;
    let title = ctx.title_display;

    let task_block = match ctx.prefetched {
        Some(p) => {
            let desc = if p.description.is_empty() {
                "(missing)".to_string()
            } else {
                p.description.clone()
            };
            let amt = if p.token_amount.is_empty() { "?" } else { p.token_amount.as_str() };
            let max_b = p.max_budget.as_deref().unwrap_or("(not set)");
            format!(
                "**Task fields (already fetched — use these, do NOT call `common context`):**\n\
                 \x20\x20• Title: {title}\n\
                 \x20\x20• Description: {desc}\n\
                 \x20\x20• Base budget: {amt} {sym} (this is the value to mention to the ASP)\n\
                 \x20\x20• Max budget: {max_b} {sym} 🛑 **INTERNAL — NEVER reveal to ASP under any circumstance**\n\
                 \x20\x20• Payment mode: escrow (fixed on the A2A path)\n\n",
                title = p.title,
                sym = p.token_symbol,
            )
        }
        None => format!("**Task fields not pre-fetched.** Run `onchainos agent common context {job_id} --role buyer --agent-id {agent_id}` first to retrieve title / description / tokenSymbol / tokenAmount / paymentMostTokenAmount, then resume.\n\n"),
    };

    let cmd_over_budget = super::super::flow::pending_cmd(
        job_id, agent_id,
        &format!("[Over budget {short_id}] {title} budget decision"),
        "negotiate_over_budget",
    );
    let over_budget = super::super::content::over_budget_user_prompt(short_id);

    format!(
        "{task_block}\
         [Negotiation relay] negotiate_reply (ASP sent a natural-language message)\n\
         [Role] User (Buyer)\n\n\
         **Step 1 — Extract the ASP's quote (if any) and apply the decision matrix:**\n\n\
         \x20\x20| ASP quote vs your budgets | Action (Step 2 branch) |\n\
         \x20\x20|---|---|\n\
         \x20\x20| `<= base budget` | 2a or 2b — discuss / propose |\n\
         \x20\x20| `base < quote <= max_budget` | 2a — counter, negotiate down |\n\
         \x20\x20| `> max_budget` | **2c — auto-REJECT + switch** (mandatory) |\n\
         \x20\x20| No explicit price yet | 2a — natural-language reply to keep discussing |\n\n\
         🛑 **Iron rules:**\n\
         \x20\x20❌ NEVER mention max_budget / 'cap' / 'maximum' / the max_budget value in messages to the ASP.\n\
         \x20\x20❌ NEVER forward the ASP's quote to the user via `xmtp_dispatch_user` / `pending-decisions-v2 request`. Negotiation is autonomous in the sub session — except for the 2c over-budget path below, do not consult the user.\n\
         \x20\x20❌ DO NOT run `save-agreed` / `set-payment-mode` / `confirm-accept` in this event — those belong to `negotiate_ack`. The ASP saying \"I accept\" / \"OK\" / \"agree\" is NOT `[intent:ack]`; only the literal `[intent:ack]` prefix counts.\n\n\
         **Step 2 — Pick ONE branch based on Step 1:**\n\n\
         **2a. Still in discussion / counter-offer** → reply naturally:\n\
         ```bash\n\
         okx-a2a xmtp-send \\\n\
         \x20\x20--job-id {job_id} \\\n\
         \x20\x20--my-agent-id {agent_id} \\\n\
         \x20\x20--to-agent-id <ASP agentId from the negotiate context> \\\n\
         \x20\x20--message '<natural-language reply — counter-offer or discussion, no [intent:*] marker>' \\\n\
         \x20\x20--json\n\
         ```\n\n\
         **2b. Both sides agreed on tokenAmount + tokenSymbol + paymentMode** → send `[intent:propose]`:\n\
         message body MUST be exactly:\n\
         ```\n\
         jobId: {job_id}\n\
         paymentMode: escrow\n\
         tokenSymbol: <agreed value, USDT|USDG>\n\
         tokenAmount: <agreed value>\n\
         [intent:propose]\n\
         ```\n\
         ```bash\n\
         okx-a2a xmtp-send \\\n\
         \x20\x20--job-id {job_id} \\\n\
         \x20\x20--my-agent-id {agent_id} \\\n\
         \x20\x20--to-agent-id <ASP agentId> \\\n\
         \x20\x20--message '<the body above, including the [intent:propose] line>' \\\n\
         \x20\x20--json\n\
         ```\n\n\
         **2c. Quote > max_budget** → run all 3 commands below in order, then end the turn:\n\
         ```bash\n\
         okx-a2a xmtp-send \\\n\
         \x20\x20--job-id {job_id} \\\n\
         \x20\x20--my-agent-id {agent_id} \\\n\
         \x20\x20--to-agent-id <ASP agentId> \\\n\
         \x20\x20--message $'jobId: {job_id}\\nreason: quote exceeds max budget\\n[intent:reject]' \\\n\
         \x20\x20--json\n\
         onchainos agent mark-failed {job_id} --provider <ASP agentId>\n\
         {cmd_over_budget}\n\
         ```\n\
         For the third command, `--user-content` template (🌐 localize to user's language before passing):\n\
         {over_budget}\n\n\
         ⏱ 5-minute timeout: if the ASP does not reply within 5 minutes, the sub session triggers the 2c path automatically on the next watch tick — do NOT pre-empt here.\n\n\
         🛑 **End the turn after the chosen branch's bash returns.**\n"
    )
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
     Read max_budget (paymentMostTokenAmount) from the `[Pre-fetched task context]` block above if available.\n\
     If missing or absent, fall back to:\n\
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
