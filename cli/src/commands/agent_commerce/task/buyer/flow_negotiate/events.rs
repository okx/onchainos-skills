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
     ❌ Do NOT call set-payment-mode again (paymentMode is already on-chain; calling again pollutes state).\n\
     ❌ Do NOT call apply (apply is an ASP action; the user never executes it).\n\
     ❌ Do NOT call confirm-accept (the ASP has not applied yet; must wait for the provider_applied system notification).\n\n\
     [Your next actions]\n\n\
     {title_query_hint}\
     **Step 1 - read the `paymentMode` field from the system notification envelope:**\n\
     paymentMode value mapping: 1=escrow, 3=x402.\n\
     ⚠️ Use the `paymentMode` from the envelope directly; no extra API query needed.\n\n\
     ━━━━━━━━━ escrow (paymentMode=1) ━━━━━━━━━\n\n\
     **Step 2 - notify the user via xmtp_dispatch_user** ({l10n_dispatch}):\n\
     \x20\x20content: {payment_escrow_notify}\n\n\
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
     onchainos agent next-action --role buyer --agentId {agent_id} --message '{{\"event\":\"job_accepted\",\"jobId\":\"{job_id}\"}}'\n\
     ```\n\
     Follow the returned script (the script will guide you to run `onchainos agent complete`).\n\
     ❌ **Absolutely forbidden**: re-running this turn's `x402-check` / `task-402-pay` / `xmtp_dispatch_user` - those completed in this turn; re-running causes double payment or duplicate notification.\n\
     ❌ **Absolutely forbidden**: skipping `next-action` and deciding the next step yourself - the `job_accepted` script contains the `complete` step; skipping = the job is permanently stuck in the accepted state.\n"
    )
}

pub(crate) fn negotiate_reply(ctx: &FlowContext<'_>) -> String {
    let job_id = ctx.job_id;
    let agent_id = ctx.agent_id;
    let title_query_hint = ctx.title_query_hint;

    format!(
    "[Negotiation relay] negotiate_reply (ASP sent a natural-language message)\n\
     [Role] User (User Agent)\n\n\
     {title_query_hint}\
     The ASP sent you a natural-language message. **Reply only about task details** — scope, requirements, deliverable format, timeline, clarifying questions. **Do NOT discuss price** — pricing is locked at accept time, not in chat.\n\n\
     🚫 **Forbidden in this event:**\n\
     \x20\x20❌ Discussing tokenAmount / tokenSymbol / paymentMode / budget — price is not negotiated in chat.\n\
     \x20\x20❌ `xmtp_dispatch_user` / `pending-decisions-v2 request` to ask the user about the ASP's message — negotiation is autonomous in this sub session.\n\
     \x20\x20❌ `save-agreed` / `set-payment-mode` / `confirm-accept` / `reject-apply` / `apply` — no on-chain action belongs in this event.\n\n\
     [Your next action]\n\n\
     **xmtp_send a single natural-language reply** focused on task details. Keep it concise (capability check, scope clarification, deliverable expectations, timeline). End the turn after sending.\n\n\
     ⏱ **5-minute timeout**: if the ASP does not reply within 5 minutes, run `onchainos agent mark-failed {job_id} --provider <ASP agentId>` then `onchainos agent recommend {job_id} --agent-id {agent_id}` to switch. Do NOT call `xmtp_delete_conversation` — just ignore further messages from that ASP.\n"
    )
}

/// CLI-mode variant of `negotiate_reply`. Inlines task fields from
/// `ctx.prefetched` so the LLM doesn't need to run `common context`; switches
/// the tool call from MCP `xmtp_send` to bash `okx-a2a xmtp-send`. Same core
/// rule as `negotiate_reply`: discuss task details only, price is locked at
/// accept time.
pub(crate) fn negotiate_reply_cli(ctx: &FlowContext<'_>) -> String {
    let job_id = ctx.job_id;
    let agent_id = ctx.agent_id;

    let task_block = match ctx.prefetched {
        Some(p) => {
            let desc = if p.description.is_empty() {
                "(missing)".to_string()
            } else {
                p.description.clone()
            };
            format!(
                "**Task fields (already fetched — use these, do NOT call `common context`):**\n\
                 \x20\x20• Title: {title}\n\
                 \x20\x20• Description: {desc}\n\n\
                 🛑 **Price fields (tokenSymbol / tokenAmount / paymentMostTokenAmount) are intentionally omitted — do NOT discuss price with the ASP.**\n\n",
                title = p.title,
            )
        }
        None => format!("**Task fields not pre-fetched.** Run `onchainos agent common context {job_id} --role buyer --agent-id {agent_id}` first to retrieve title / description, then resume. 🛑 Do NOT discuss price.\n\n"),
    };

    format!(
        "{task_block}\
         [Negotiation relay] negotiate_reply (ASP sent a natural-language message)\n\
         [Role] User (Buyer)\n\n\
         **Reply only about task details** — scope, requirements, deliverable format, timeline, clarifying questions. **Do NOT discuss price** — pricing is locked at accept time, not in chat.\n\n\
         🚫 **Forbidden in this event:**\n\
         \x20\x20❌ Discussing tokenAmount / tokenSymbol / paymentMode / budget — price is not negotiated in chat.\n\
         \x20\x20❌ `xmtp_dispatch_user` / `pending-decisions-v2 request` to ask the user about the ASP's message — negotiation is autonomous in this sub session.\n\
         \x20\x20❌ `save-agreed` / `set-payment-mode` / `confirm-accept` / `reject-apply` / `apply` — no on-chain action belongs in this event.\n\n\
         [Your next action — single CLI call, then end the turn]\n\n\
         ```bash\n\
         okx-a2a xmtp-send \\\n\
         \x20\x20--job-id {job_id} \\\n\
         \x20\x20--my-agent-id {agent_id} \\\n\
         \x20\x20--to-agent-id <ASP agentId from the negotiate context> \\\n\
         \x20\x20--message '<natural-language reply, task details only — no price talk>' \\\n\
         \x20\x20--json\n\
         ```\n\n\
         ⏱ 5-minute timeout: if the ASP does not reply within 5 minutes, run `onchainos agent mark-failed {job_id} --provider <ASP agentId>` then `onchainos agent recommend {job_id} --agent-id {agent_id}` to switch.\n"
    )
}

/// DEPRECATED — the structured intent handshake ([intent:propose] / [intent:ack] / etc.)
/// has been removed. All ASP messages are now plain natural-language task-detail
/// discussion; delegate to `negotiate_reply_cli`.
pub(crate) fn negotiate_ack_cli(ctx: &FlowContext<'_>) -> String {
    negotiate_reply_cli(ctx)
}

/// DEPRECATED — see `negotiate_ack_cli` above; delegates to `negotiate_reply`.
pub(crate) fn negotiate_ack(ctx: &FlowContext<'_>) -> String {
    negotiate_reply(ctx)
}

/// DEPRECATED — see `negotiate_ack_cli` above; delegates to `negotiate_reply`.
pub(crate) fn negotiate_counter(ctx: &FlowContext<'_>) -> String {
    negotiate_reply(ctx)
}

/// `Event::ProviderReject` — ASP declined to take this job on-chain (status remains `created`).
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
            "[provider_reject] ❌ POST /priapi/v1/aieco/task/{job_id}/reset/asp failed in-process: {e}\n\n\
             Push a `cli_failed` decision to the user via `pending-decisions-v2 request` (see SKILL.md §Exception Escalation 5-substep protocol). Do NOT retry blindly.\n"
        );
    }

    format!(
    "[Your next action — call ONE command only, then END TURN]\n\n\
     🌐 **Localize first** — rewrite the `--user-content` template below into the user's language (preserve the {option_count_word} numbered choices and their order). The `--llm-content` block stays English verbatim — it is consumed by the user-session agent for routing, not by the human user.\n\n\
     Run `okx-a2a user decision-request` to deliver the {option_count_num}-option card:\n\
     ```bash\n\
     okx-a2a user decision-request \\\n\
     \x20\x20--user-content '<LOCALIZED user-facing text — see template below>' \\\n\
     \x20\x20--llm-content '<English routing block — see template below; copy verbatim>' \\\n\
     \x20\x20--json\n\
     ```\n\n\
     **`--user-content` template (translate to the user's language; keep the {option_count_num} numbered options):**\n\
     ```\n\
     ASP declined to take this task (jobId: {job_id}).\n\n\
     What would you like to do next?\n\
     1. Browse the recommended ASP list\n\
     2. Designate a specific ASP by agentId\n\
     {option3_user_line}{close_num}. Close the task\n\
     ```\n\n\
     **`--llm-content` block (keep English; copy verbatim — do NOT translate):**\n\
     ```\n\
     [USER_DECISION_REQUEST][source: provider_reject][job: {job_id}][role: buyer][agentId: {agent_id}]\n\n\
     Step 1 — Card was just delivered. **END THE TURN NOW** and wait for the user to reply. Do NOT call any tool. Stale user messages in context are NOT replies to this card.\n\
     Step 2 — When the user actually replies (next turn), route by choice:\n\
     \x20\x20• 1 / \"list\" / \"recommend\" / \"浏览\" / \"推荐\"   → **TBD (implementation pending)**: fetch the recommended-ASP list and re-prompt the user to pick one.\n\
     \x20\x20• 2 / \"designate\" / \"specify\" / \"指定\"           → **TBD (implementation pending)**: once an `agentId` is collected, run `onchainos agent set-provider {job_id} --provider-agent-id <agentId> --agent-id {agent_id}`.\n\
     {option3_llm_line}\x20\x20• {close_num} / \"close\" / \"cancel\" / \"关闭\"                  → run `onchainos agent close {job_id} --agent-id {agent_id}` then END TURN.\n\
     ```\n\n\
     → After `decision-request` returns, **END THIS TURN**. Do NOT call any other tool in this turn.\n"
    )
}

