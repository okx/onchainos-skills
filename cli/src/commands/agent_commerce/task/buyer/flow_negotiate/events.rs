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
     ```bash\n\
     okx-a2a user notify --content '<localized content>'\n\
     ```\n\
     Content:\n\
     \x20\x20{notify_content}\n\n\
     {public_only_warning}-> **end this turn**.\n"
    )
}

pub(crate) fn job_payment_mode_changed(ctx: &FlowContext<'_>) -> String {
    let job_id = ctx.job_id;
    let agent_id = ctx.agent_id;
    let title_display = ctx.title_display;
    let title_query_hint = ctx.title_query_hint;
    let pm = ctx.payment_mode;

    let short_id = if job_id.len() >= 8 { &job_id[..8] } else { job_id };

    let mut out = format!(
    "[Current state] job_payment_mode_changed (payment-mode switch is on-chain)\n\
     [Role] User (User Agent)\n\n\
     🛑 **You MUST notify the user of the payment-mode change.**\n\
     ❌ Do NOT call set-payment-mode again / apply / confirm-accept.\n\n\
     [Your next actions]\n\n\
     {title_query_hint}");

    // ── escrow branch ──
    if pm != Some(3) {
        let payment_escrow_notify = super::super::content::payment_mode_escrow_user_notify(job_id, title_display);
        out.push_str(&format!("\
     ━━━━━━━━━ escrow (paymentMode=1) ━━━━━━━━━\n\n\
     **Step 2 - notify the user via `okx-a2a user notify`**:\n\
     \x20\x20```bash\n\
     \x20\x20okx-a2a user notify --content '<translated content from the template below>'\n\
     \x20\x20```\n\
     \x20\x20content template: {payment_escrow_notify}\n\n\
     -> **end this turn** and wait for provider_applied.\n\n"));
    }

    // ── x402 branch ──
    if pm != Some(1) {
        let x402_paying = super::super::content::x402_paying_user_notify(job_id, title_display);
        let x402_replay_ok = super::super::content::x402_replay_success_user_notify(job_id);
        let cmd_replay_input = format!("onchainos agent pending-decisions-v2 request --job-id {job_id} --role buyer --agent-id {agent_id} --user-content \"<compose from template below>\" --list-label \"[x402 replay input {short_id}] field input\" --source-event x402_replay_input");

        out.push_str(&format!("\
     ━━━━━━━━━ x402 (paymentMode=3) ━━━━━━━━━\n\n\
     Extract endpoint, acceptsJson, feeTokenSymbol, feeAmount, providerAgentId from the previous turn.\n\
     ⚠️ **If any parameter is missing** (context compaction): run `onchainos agent common context {job_id} --role buyer --agent-id {agent_id}` for providerAgentId, then `onchainos agent asp-match --job-id {job_id} --provider-agent-id <providerAgentId> --format json` for endpoint, then `onchainos agent x402-check --endpoint <endpoint> --agent-id {agent_id}` for acceptsJson/feeTokenSymbol/feeAmount.\n\n\
     **Step 2 — notify payment in progress**:\n\
     \x20\x20```bash\n\
     \x20\x20okx-a2a user notify --content '<translated>'\n\
     \x20\x20```\n\
     \x20\x20content template: {x402_paying}\n\n\
     **Step 3 — sign + accept + replay (atomic):**\n\
     ```bash\n\
     onchainos agent task-402-pay {job_id} --provider-agent-id <providerAgentId> --accepts '<acceptsJson>' --endpoint <endpoint> --token-symbol <feeTokenSymbol> --token-amount <feeAmount> [--body '<serviceBody JSON>']\n\
     ```\n\
     `--body`: pass the JSON body from `x402_input_required` confirmation if it happened; omit otherwise.\n\
     Output: {{ replaySuccess, replayStatus, replayBody, replayBodyDisplay, deliverableSavedPath, txHash }}\n\n\
     **Step 4 — notify user with deliverable path**:\n\
     If `deliverableSavedPath` is present → show saved path only (no preview/summary needed; user can read the local file).\n\
     If `deliverableSavedPath` is absent (save failed) → embed full `replayBodyDisplay` so the user can still see what they paid for.\n\n\
     ▸ replaySuccess=true:\n\
     {x402_replay_ok}\n\
     -> **end this turn** and wait for `job_accepted`.\n\
     🛑 When `job_accepted` arrives, call `onchainos agent next-action --role buyer --agentId {agent_id} --message '{{\"event\":\"job_accepted\",\"jobId\":\"{job_id}\"}}'`.\n\
     ❌ Do NOT re-run this turn's commands (double payment) or skip next-action (job stuck).\n\n\
     ▸ replaySuccess=false:\n\
     Check `replayBody` for `requiredArgs` / `fields` / `status: \"input_required\"`.\n\n\
     \x20\x20▸▸ **Endpoint needs business parameters** → push decision card:\n\
     \x20\x20```bash\n\
     \x20\x20{cmd_replay_input}\n\
     \x20\x20```\n\
     \x20\x20`--user-content` template:\n\
     \x20\x20[Job {short_id}] x402 payment succeeded but the endpoint requires parameters to deliver.\n\
     \x20\x20Already paid: <feeAmount> <feeTokenSymbol>\n\
     \x20\x20Required: <list each field from replayBody>\n\
     \x20\x20Please provide the values.\n\
     \x20\x20`--llm-content`: `[REPLAY_CONTEXT] endpoint=<> providerAgentId=<> acceptsJson=<> feeTokenSymbol=<> feeAmount=<> requiredFields: <copy from replayBody>`\n\
     \x20\x20-> **end this turn**. `job_accepted` handler will detect the pending decision and skip its notification.\n\n\
     \x20\x20▸▸ **Otherwise** (generic failure) → do NOT notify. **end this turn** and wait for `job_accepted`.\n"));
    }

    out
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
             Push a `cli_failed` decision to the user via `pending-decisions-v2 request` (see _shared/exception-escalation.md §2). Do NOT retry blindly.\n"
        ),
    };
    let provider_agent_id = match p.provider_agent_id.as_deref().filter(|s| !s.is_empty()) {
        Some(s) => s,
        None => {
            let is_public = p.visibility == Some(0) || p.service_id.is_none();
            if is_public {
                return super::match_provider::provider_conversation_cli(ctx);
            }
            return format!(
                "[negotiate_reply] ❌ prefetched task context has no providerAgentId for job {job_id}; cannot send a reply.\n\n\
                 Push a `cli_failed` decision to the user via `pending-decisions-v2 request` (see _shared/exception-escalation.md §2). Do NOT retry blindly.\n"
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

    let cmd_no_asp = format!("onchainos agent pending-decisions-v2 request --job-id {job_id} --role buyer --agent-id {agent_id} --user-content \"<compose from template below>\" --list-label \"[No ASP] negotiate timeout — next-step decision\" --source-event no_asp_found");

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
         `--user-content` template:\n\
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
    let short_id = ctx.short_id;

    // visibility: 0 = public, 1 = private. The "make public" option only makes sense
    // when the task is currently private; otherwise drop the option and renumber close.
    let is_private = visibility == 1;
    let close_label = if is_private { "D" } else { "C" };
    let option_public_line = if is_private {
        "C. Make the task public so any qualified ASP can apply\n         "
    } else {
        ""
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
            "[job_provider_reject] ❌ POST reset/asp failed: {e}\n\n\
             See _shared/exception-escalation.md §2 — push `cli_failed` decision.\n"
        );
    }

    let user_content = format!(
        "[Job {short_id} — you are the User Agent] ASP declined to take this task. What would you like to do next?\n\n\
         A. Browse the ASP list\n\
         B. Designate a specific ASP by agentId\n\
         {option_public_line}{close_label}. Close the task"
    );
    let request_block = crate::commands::agent_commerce::task::common::pending_v2::request_command_block(
        job_id, "buyer", agent_id, None,
        &user_content,
        &format!("[Reject {short_id}] next-step decision"),
        "job_provider_reject",
    );

    format!(
    "[job_provider_reject] ✅ ASP binding reset (reset/asp) completed in-process.\n\n\
     🛑 Push the next-step decision card via `pending-decisions-v2 request`, then end turn.\n\n\
     {request_block}\n"
    )
}

