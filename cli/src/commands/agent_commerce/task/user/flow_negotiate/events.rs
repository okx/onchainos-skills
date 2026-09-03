//! Event handlers for payment mode changes and negotiation relays.

use super::super::flow::FlowContext;

pub(crate) fn job_payment_mode_changed(ctx: &FlowContext<'_>) -> String {
    let job_id = ctx.job_id;
    let title_display = ctx.title_display;

    if ctx.payment_mode == Some(3) {
        return "[Legacy A2MCP Task payment] This path was removed. Do not sign, replay, or continue this Task flow; restart from an upstream invoke_a2mcp event.\n"
            .to_string();
    }

    let payment_escrow_notify =
        super::super::content::payment_mode_escrow_user_notify(job_id, title_display);
    format!(
        "[Current state] job_payment_mode_changed (A2A escrow is on-chain)\n\
         [Role] User Agent\n\n\
         Notify the user via `onchainos agent user-notify`, using this localized template:\n\
         {payment_escrow_notify}\n\n\
         End this turn and wait for provider_applied.\n"
    )
}
/// Negotiation reply handler — natural-language exchange, max 2 rounds.
///
/// Round counting: the LLM checks how many user replies have already been
/// sent in this sub session. If this would be the 3rd reply, the negotiation
/// has exceeded the 2-round limit → mark-failed + push decision card to user.
pub(crate) async fn negotiate_reply(ctx: &FlowContext<'_>) -> String {
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

    // Price is always locked to the service listing (public tasks / price
    // negotiation were removed). The agent must not discuss price with the ASP.
    let price_rule = "🛑 **Price is locked**: do NOT discuss tokenAmount / tokenSymbol / \
         paymentMode / budget with the ASP. Price was determined by the service listing at \
         creation time and is locked at accept.\n\n";
    let reply_hint = "task details only — no price talk";

    let task_block = format!(
        "**Task fields (already fetched — do NOT call `common context`):**\n\
         \x20\x20• Title: {title}\n\
         \x20\x20• Description: {desc}\n\
         {price_rule}",
        title = p.title,
    );

    let cmd_no_asp = format!("onchainos agent pending-decisions-v2 request --job-id {job_id} --role user --agent-id {agent_id} --user-content \"<compose from template below>\" --list-label \"[No ASP] negotiate timeout — next-step decision\" --source-event no_asp_found");

    let over_limit_section = format!(
            "━━━━━━━━━ [Over-limit] 2-round limit exceeded or timeout ━━━━━━━━━\n\n\
             **Step 1** — mark this ASP as failed:\n\
             ```bash\n\
             onchainos agent mark-failed {job_id} --provider {provider_agent_id}\n\
             ```\n\n\
             **Step 2** — push a decision card to the user:\n\
             **Localize first** — translate the `--user-content` and `--list-label` values below into the user's language before running.\n\
             ```bash\n\
             {cmd_no_asp}\n\
             ```\n\
             `--user-content` template:\n\
             Negotiation with ASP {provider_agent_id} did not reach agreement within 2 rounds.\n\n\
             What would you like to do next?\n\
             A. Browse the ASP list\n\
             B. Designate a specific ASP by agentId\n\
             C. Close the task\n\n\
             → **End this turn.**\n"
    );

    format!(
        "{task_block}\
         [Negotiation] negotiate_reply (ASP sent a natural-language message)\n\
         [Role] User (User)\n\n\
         **2-round limit**: count how many user replies (your `okx-a2a session send` calls) have already been sent in this sub session's conversation history.\n\
         - Rounds sent < 2 → reply normally (see below).\n\
         - Rounds sent ≥ 2 → negotiation exceeded the 2-round limit. **Do NOT reply.** Jump to **[Over-limit]** below.\n\n\
         **Reply about**: scope, requirements, deliverable format, timeline, clarifying questions.\n\n\
         🚫 **Forbidden in this event:**\n\
         \x20\x20❌ `onchainos agent user-notify` / `pending-decisions-v2 request` to ask the user about the ASP's message — negotiation is autonomous in this sub session.\n\
         \x20\x20❌ `set-payment-mode` / `confirm-accept` / `reject-apply` / `apply` — no on-chain action belongs in this event.\n\n\
         [Normal reply — single CLI call, then end the turn]\n\n\
         ```bash\n\
         okx-a2a session send \\\n\
         \x20\x20--job-id {job_id} \\\n\
         \x20\x20--to-agent-id {provider_agent_id} \\\n\
         \x20\x20--content '<natural-language reply, {reply_hint}>' \\\n\
         \x20\x20--json\n\
         ```\n\n\
         ⏱ 5-minute timeout: if the ASP does not reply within 5 minutes, treat as over-limit (see below).\n\n\
         {over_limit_section}",
    )
}

/// `Event::JobProviderReject` — ASP declined via `asp/reject` API (status remains `created`).
/// User-side reaction:
///   Step 0 (in-process): POST `/priapi/v1/aieco/task/{jobId}/reset/asp` to clear the rejected
///                        ASP binding on the task record (no request body).
///   Step 1 (LLM playbook): the agent must localize the `--user-content` payload into the
///                          user's language, then run `pending-decisions-v2 request` to
///                          deliver the A/B/C card. The `--llm-content` routing block
///                          stays English (consumed only by the user-session agent).
pub(crate) async fn provider_reject(ctx: &FlowContext<'_>) -> String {
    use crate::commands::agent_commerce::task::common::network::task_api_client::TaskApiClient;
    let job_id = ctx.job_id;
    let agent_id = ctx.agent_id;
    let short_id = ctx.short_id;

    // Step 0 — reset the rejected ASP binding on the task record (empty body).
    let mut client = TaskApiClient::new();
    let reset_result = client
        .post_with_identity(
            &client.endpoint(job_id, "reset/asp"),
            &serde_json::json!({}),
            agent_id,
        )
        .await;

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
         C. Close the task"
    );
    let request_block =
        crate::commands::agent_commerce::task::common::pending_v2::request_command_block(
            job_id,
            "user",
            agent_id,
            None,
            &user_content,
            &format!("[Reject {short_id}] next-step decision"),
            "job_provider_reject",
        );

    format!(
    "[job_provider_reject] ✅ ASP binding reset (reset/asp) completed in-process.\n\n\
     **Localize first** — translate the `--user-content` value below into the user's language before executing. \
     Keep `[Job {short_id}]` prefix and `A.` / `B.` / `C.` option letters unchanged.\n\n\
     🛑 Push the next-step decision card via `pending-decisions-v2 request`, then end turn.\n\n\
     {request_block}\n"
    )
}
