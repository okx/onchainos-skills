//! Event handlers for visibility changes, payment mode changes, and negotiation relays.

use super::super::flow::FlowContext;

pub(crate) fn job_visibility_changed(ctx: &FlowContext<'_>, visibility: i64) -> String {
    let job_id = ctx.job_id;
    let title_display = ctx.title_display;
    let title_query_hint = ctx.title_query_hint;

    // visibility: 0 = public, 1 = private. Resolved in Rust so the playbook
    // only renders the branch that actually applies; the LLM no longer has
    // to read the envelope and branch itself.
    let is_public = visibility == 0;
    let notify_content = if is_public {
        super::super::content::visibility_public_user_notify(job_id, title_display)
    } else {
        super::super::content::visibility_private_user_notify(job_id, title_display)
    };
    let public_only_warning = if is_public {
        "⚠️ After switching to public, do **NOT** request the recommended ASP list (recommend); the user just waits for ASPs to reach out.\n     "
    } else {
        ""
    };
    format!(
    "[Current state] job_visibility_changed (public/private toggle is on-chain)\n\
     [Role] User (User Agent)\n\n\
     🛑 **This is not an auxiliary event; you MUST notify the user.**\n\n\
     [Your next action — call ONE command only, then END TURN]\n\n\
     {title_query_hint}\
     🌐 **Localize first** — translate the canonical English notification below into the user's language (preserve every data value verbatim — jobId hex, AgentID digits, fee amounts, symbols).\n\n\
     ```bash\n\
     okx-a2a user notify --content '<your translated content>'\n\
     ```\n\n\
     Canonical English content to translate:\n\
     \x20\x20{notify_content}\n\n\
     {public_only_warning}-> **end this turn**.\n"
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
     ❌ Do NOT call set-payment-mode again (paymentMode is already on-chain; calling again pollutes state).\n\
     ❌ Do NOT call apply (apply is an ASP action; the user never executes it).\n\
     ❌ Do NOT call confirm-accept (the ASP has not applied yet; must wait for the provider_applied system notification).\n\n\
     [Your next actions]\n\n\
     {title_query_hint}\
     **Step 1 - read the `paymentMode` field from the system notification envelope:**\n\
     paymentMode value mapping: 1=escrow, 3=x402.\n\
     ⚠️ Use the `paymentMode` from the envelope directly; no extra API query needed.\n\n\
     ━━━━━━━━━ escrow (paymentMode=1) ━━━━━━━━━\n\n\
     **Step 2 - notify the user via `okx-a2a user notify`** ({l10n_dispatch}):\n\
     \x20\x20```bash\n\
     \x20\x20okx-a2a user notify --content '<translated content from the template below>'\n\
     \x20\x20```\n\
     \x20\x20content (canonical English template — translate before passing): {payment_escrow_notify}\n\n\
     -> **end this turn** and wait for the ASP to submit their apply on-chain (provider_applied system notification).\n\n\
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
     **Step 2 — 🌐 notify the user that payment is in progress via `okx-a2a user notify`:**\n\
     {l10n_dispatch}\n\
     \x20\x20```bash\n\
     \x20\x20okx-a2a user notify --content '<translated content from the template below>'\n\
     \x20\x20```\n\
     \x20\x20content (canonical English template — translate before passing): {x402_paying}\n\n\
     **Step 3 — sign + direct/accept + endpoint replay (atomic command):**\n\
     ```bash\n\
     onchainos agent task-402-pay {job_id} --provider-agent-id <providerAgentId> --accepts '<acceptsJson>' --endpoint <endpoint URL> --token-symbol <feeTokenSymbol> --token-amount <feeAmount>\n\
     ```\n\
     Internally executes: x402_pay signing -> direct/accept on-chain -> assemble payment header -> replay endpoint.\n\
     Output: {{ replaySuccess, replayStatus, replayBody, replayBodyDisplay, deliverableSavedPath, signature, authorization, sessionCert, txHash }}\n\
     ✅ The CLI **auto-saves** the deliverable to disk when replaySuccess=true (`deliverableSavedPath` in output). No manual `task-deliverable-save` call needed.\n\n\
     🔴🔴🔴 **CRITICAL — Step 4: notify the user with the FULL deliverable content via `okx-a2a user notify`**\n\
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
     onchainos agent next-action --role buyer --agentId {agent_id} --message '{{\"event\":\"job_accepted\",\"jobId\":\"{job_id}\"}}'\n\
     ```\n\
     Follow the returned script (the script will guide you to run `onchainos agent complete`).\n\
     ❌ **Absolutely forbidden**: re-running this turn's `x402-check` / `task-402-pay` / `okx-a2a user notify` - those completed in this turn; re-running causes double payment or duplicate notification.\n\
     ❌ **Absolutely forbidden**: skipping `next-action` and deciding the next step yourself - the `job_accepted` script contains the `complete` step; skipping = the job is permanently stuck in the accepted state.\n"
    )
}

/// Negotiation reply handler — natural-language exchange, max 2 rounds.
///
/// Round counting: the LLM checks how many buyer replies have already been
/// sent in this sub session. If this would be the 3rd reply, the negotiation
/// has exceeded the 2-round limit → mark-failed + push decision card to user.
pub(crate) fn negotiate_reply(ctx: &FlowContext<'_>) -> String {
    let job_id = ctx.job_id;
    let agent_id = ctx.agent_id;

    let p = match ctx.prefetched {
        Some(p) => p,
        None => return format!(
            "[negotiate_reply] ❌ no prefetched task context for job {job_id}; cannot resolve providerAgentId.\n\n\
             Push a `cli_failed` decision to the user via `pending-decisions-v2 request` (see SKILL.md §Exception Escalation 5-substep protocol). Do NOT retry blindly.\n"
        ),
    };
    let provider_agent_id = match p.provider_agent_id.as_deref().filter(|s| !s.is_empty()) {
        Some(s) => s,
        None => {
            let is_public = p.visibility == Some(0) || p.service_id.is_none();
            if is_public {
                return super::match_provider::provider_conversation(ctx);
            }
            return format!(
                "[negotiate_reply] ❌ prefetched task context has no providerAgentId for job {job_id}; cannot send a reply.\n\n\
                 Push a `cli_failed` decision to the user via `pending-decisions-v2 request` (see SKILL.md §Exception Escalation 5-substep protocol). Do NOT retry blindly.\n"
            );
        }
    };

    let desc = if p.description.is_empty() {
        "(missing)".to_string()
    } else {
        p.description.clone()
    };
    let is_public = p.visibility == Some(0) || p.service_id.is_none();

    let max_budget_val = p.max_budget.as_deref().unwrap_or("0");
    let (price_rule, price_fields, reply_hint) = if is_public {
        (
            format!(
                "**Public task — price is negotiable**: you MAY discuss tokenAmount with the ASP. \
                 Internally enforce: proposed price must NOT exceed {max_budget_val} {symbol}. \
                 If the ASP proposes above this cap, say the price is too high and ask them to \
                 lower it — but **NEVER reveal the exact max budget number**.\n\n",
                symbol = p.token_symbol,
            ),
            format!(
                "\x20\x20• Budget: {budget} {symbol}\n\
                 \x20\x20• Currency: {symbol}\n\n\
                 🛑 **max budget is confidential** — NEVER mention the max budget value to the ASP.\n\n",
                budget = p.token_amount,
                symbol = p.token_symbol,
            ),
            "task details + price negotiation (never reveal max budget)",
        )
    } else {
        (
            "🛑 **Private task — price is locked**: do NOT discuss tokenAmount / tokenSymbol / \
             paymentMode / budget with the ASP. Price was determined by the service listing at \
             creation time and is locked at accept.\n\n".to_string(),
            String::new(),
            "task details only — no price talk",
        )
    };

    let task_block = format!(
        "**Task fields (already fetched — do NOT call `common context`):**\n\
         \x20\x20• Title: {title}\n\
         \x20\x20• Description: {desc}\n\
         {price_fields}\n\
         {price_rule}",
        title = p.title,
    );

    let cmd_no_asp = super::super::flow::pending_cmd(job_id, agent_id, None, "[No ASP] negotiate timeout — next-step decision", "no_asp_found");

    format!(
        "{task_block}\
         [Negotiation] negotiate_reply (ASP sent a natural-language message)\n\
         [Role] User (Buyer)\n\n\
         **2-round limit**: count how many buyer replies (your `okx-a2a xmtp-send` calls) have already been sent in this sub session's conversation history.\n\
         - Rounds sent < 2 → reply normally (see below).\n\
         - Rounds sent ≥ 2 → negotiation exceeded the 2-round limit. **Do NOT reply.** Jump to **[Over-limit]** below.\n\n\
         **Reply about**: scope, requirements, deliverable format, timeline, clarifying questions{public_price_note}.\n\n\
         🚫 **Forbidden in this event:**\n\
         \x20\x20❌ `okx-a2a user notify` / `pending-decisions-v2 request` to ask the user about the ASP's message — negotiation is autonomous in this sub session.\n\
         \x20\x20❌ `set-payment-mode` / `confirm-accept` / `reject-apply` / `apply` — no on-chain action belongs in this event.\n\n\
         [Normal reply — single CLI call, then end the turn]\n\n\
         ```bash\n\
         okx-a2a xmtp-send \\\n\
         \x20\x20--job-id {job_id} \\\n\
         \x20\x20--to-agent-id {provider_agent_id} \\\n\
         \x20\x20--message '<natural-language reply, {reply_hint}>' \\\n\
         \x20\x20--no-wait\n\
         ```\n\n\
         ⏱ 5-minute timeout: if the ASP does not reply within 5 minutes, treat as over-limit (see below).\n\n\
         ━━━━━━━━━ [Over-limit] 2-round limit exceeded or timeout ━━━━━━━━━\n\n\
         **Step 1** — mark this ASP as failed:\n\
         ```bash\n\
         onchainos agent mark-failed {job_id} --provider {provider_agent_id}\n\
         ```\n\n\
         **Step 2** — push a decision card to the user:\n\
         ```bash\n\
         {cmd_no_asp}\n\
         ```\n\
         `--user-content` template (translate to user's language):\n\
         Negotiation with ASP {provider_agent_id} did not reach agreement within 2 rounds.\n\n\
         What would you like to do next?\n\
         A. Browse the ASP list\n\
         B. Designate a specific ASP by agentId\n\
         C. Close the task\n\n\
         → **End this turn.**\n",
        public_price_note = if is_public { ", and **price** (within max budget)" } else { "" },
    )
}

/// `Event::JobProviderReject` — ASP declined via `asp/reject` API (status remains `created`).
/// Buyer-side reaction:
///   Step 0 (in-process): POST `/priapi/v1/aieco/task/{jobId}/reset/asp` to clear the rejected
///                        ASP binding on the task record (no request body).
///   Step 1 (LLM playbook): the agent must localize the `--user-content` payload into the
///                          user's language, then run `okx-a2a user decision-request` to
///                          deliver the 4-option card. The `--llm-content` routing block
///                          stays English (consumed only by the user-session agent).
pub(crate) async fn provider_reject(ctx: &FlowContext<'_>, visibility: i64) -> String {
    use crate::commands::agent_commerce::task::common::network::task_api_client::TaskApiClient;
    let job_id = ctx.job_id;
    let agent_id = ctx.agent_id;

    // visibility: 0 = public, 1 = private. The "make public" option only makes sense
    // when the task is currently private; otherwise drop the option and renumber close.
    let is_private = visibility == 1;
    let option_count_word = if is_private { "four" } else { "three" };
    let option_count_num = if is_private { "4" } else { "3" };
    let close_num = if is_private { "4" } else { "3" };
    let option3_user_line = if is_private {
        "3. Make the task public so any qualified ASP can apply\n     "
    } else {
        ""
    };
    let option3_llm_line = if is_private {
        format!("\x20\x20• 3 / \"public\" / \"open\" / \"公开\"                  → run `onchainos agent set-public {job_id} --agent-id {agent_id}` then END TURN.\n     ")
    } else {
        String::new()
    };

    // Step 0 — reset the rejected ASP binding on the task record (empty body).
    let mut client = TaskApiClient::new();
    let reset_result = client.post_with_identity(
        &client.endpoint(job_id, "reset/asp"),
        &serde_json::json!({}),
        agent_id,
    ).await;

    if let Err(e) = reset_result {
        return format!(
            "[job_provider_reject] ❌ POST /priapi/v1/aieco/task/{job_id}/reset/asp failed in-process: {e}\n\n\
             Push a `cli_failed` decision to the user via `pending-decisions-v2 request` (see SKILL.md §Exception Escalation 5-substep protocol). Do NOT retry blindly.\n"
        );
    }

    format!(
    "[Your next action — call ONE command only, then END TURN]\n\n\
     🌐 **Localize first** — rewrite the `--user-content` template below into the user's language (preserve the {option_count_word} numbered choices and their order). The `--llm-content` block stays English verbatim — it is consumed by the user-session agent for routing, not by the human user.\n\n\
     Deliver the {option_count_num}-option card:\n\
     ```bash\n\
     onchainos agent pending-decisions-v2 request \\\n\
     \x20\x20--job-id {job_id} \\\n\
     \x20\x20--role buyer \\\n\
     \x20\x20--agent-id {agent_id} \\\n\
     \x20\x20--list-label '' \\\n\
     \x20\x20--user-content '<LOCALIZED user-facing text — see template below>' \\\n\
     \x20\x20--llm-content '<English routing block — see template below; copy verbatim>'\n\
     ```\n\n\
     **`--user-content` template (translate to the user's language; keep the {option_count_num} numbered options):**\n\
     ```\n\
     ASP declined to take this task (jobId: {job_id}).\n\n\
     What would you like to do next?\n\
     1. Browse the ASP list\n\
     2. Designate a specific ASP by agentId\n\
     {option3_user_line}{close_num}. Close the task\n\
     ```\n\n\
     **`--llm-content` block (keep English; copy verbatim — do NOT translate):**\n\
     ```\n\
     [USER_DECISION_REQUEST][source: job_provider_reject][job: {job_id}][role: buyer][agentId: {agent_id}]\n\n\
     Step 1 — Card was just delivered. **END THE TURN NOW** and wait for the user to reply. Do NOT call any tool. Stale user messages in context are NOT replies to this card.\n\
     Step 2 — When the user actually replies (next turn), route by choice:\n\
     \x20\x20• 1 / \"list\" / \"asp-match\" / \"浏览\" / \"推荐\"   → run `onchainos agent asp-match --job-id {job_id}` to fetch the ASP-match list, then re-prompt the user to pick one.\n\
     \x20\x20• 2 / \"designate\" / \"specify\" / \"指定\"           → run `onchainos agent asp-match --job-id {job_id} --provider-agent-id <agentId>` to get service info, then `onchainos agent set-asp {job_id} --provider-agent-id <agentId> --service-id <sid> --service-type <serviceType> --service-params '<params>' --service-token-address <feeToken> --service-token-amount <feeAmount>`.\n\
     {option3_llm_line}\x20\x20• {close_num} / \"close\" / \"cancel\" / \"关闭\"                  → run `onchainos agent close {job_id} --agent-id {agent_id}` then END TURN.\n\
     ```\n\n\
     → After `decision-request` returns, **END THIS TURN**. Do NOT call any other tool in this turn.\n"
    )
}

