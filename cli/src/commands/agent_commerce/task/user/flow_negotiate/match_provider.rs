//! Event handlers for job_created.

use super::super::flow::FlowContext;

// --- Event handler functions ------------------------------------------------

pub(crate) async fn job_created(ctx: &FlowContext<'_>) -> String {
    // Every task now carries a designated provider (provider is mandatory at
    // create/publish). If the local designated-provider record is missing
    // (legacy task / lost state), notify the user the job is on-chain and stop —
    // there is no public discovery path to fall back to.
    let has_designated = super::super::negotiate::get_designated_provider(ctx.job_id)
        .ok()
        .flatten()
        .is_some();
    if !has_designated {
        return job_created_no_designated_provider(ctx);
    }
    job_created_with_designated_provider(ctx).await
}

fn job_created_no_designated_provider(ctx: &FlowContext<'_>) -> String {
    let title = ctx.title_display;
    let short_id = ctx.short_id;
    format!(
        "[Trigger] job_created (on-chain, no designated provider recorded locally)\n\
         [Role] User (User)\n\n\
         🛑 Notify the user the job 「{title}」 ({short_id}) is confirmed on-chain, then end the turn. \
         Designate a provider with `onchainos agent set-asp` if one is not already attached.\n\n\
         **Action — Notify the user.** **Localize first** — rewrite the content below in the user's language before sending.\n\
         ```bash\n\
         onchainos agent user-notify --content '<localized content>'\n\
         ```\n\
         Content: [Job Created]「{title}」({short_id}) confirmed on-chain.\n\n\
         🛑 End the turn after notifying.\n"
    )
}

async fn job_created_with_designated_provider(ctx: &FlowContext<'_>) -> String {
    let job_id = ctx.job_id;
    let agent_id = ctx.agent_id;
    let short_id = ctx.short_id;
    let title = ctx.title_display;

    let dp_id = super::super::negotiate::get_designated_provider(job_id)
        .ok()
        .flatten()
        .expect("job_created_with_designated_provider_cli called only when designated provider exists");

    let notify_tpl = super::super::content::job_created_designated_user_notify();
    let designated_endpoint = super::super::negotiate::get_designated_endpoint(job_id).ok().flatten();

    // Fill the static placeholders in the notify template so the LLM only
    // has to translate (no placeholder bookkeeping). Dispatch itself is
    // LLM-driven so the content is in the user's language.
    let notify_filled = notify_tpl
        .replace("<title>", title)
        .replace("<short_jobId>", short_id)
        .replace("<provider_agentId>", &dp_id);
    let notify_body = format!(
        "**Localize first** — rewrite the content below in the user's language before sending. Do NOT pass the English template verbatim to a non-English user.\n\
         Content:\n\
         \x20\x20{notify_filled}\n\
         ```bash\n\
         onchainos agent user-notify --content '<localized content shown below>'\n\
         ```\n\n"
    );

    // D-Step 1 — designated-route query (in-process).
    let route_result = crate::commands::agent_commerce::task::common::designated_route_inner(
        &dp_id,
        designated_endpoint.as_deref(),
    )
    .await;
    let route_json = match route_result {
        Ok(j) => j,
        Err(e) => return format!("[job_created_cli] ERROR: designated-route failed: {e}\n"),
    };

    // D-Step 2 — dispatch in-process to the matching branch playbook, skipping
    // the "LLM calls `next-action --event designated_*`" round-trip entirely.
    // The a2a branch additionally inlines B-Step 0 / 1 / 1.5 (session
    // duplicate guard + create + SKILL_PREFETCH) via `branch_a2a_cli`.
    let route = route_json.get("route").and_then(|v| v.as_str()).unwrap_or("");
    let branch_playbook = match route {
        "a2a" => super::designated::branch_a2a_cli(job_id, agent_id, &dp_id),
        "x402" => Some(super::designated::branch_x402(job_id, agent_id, short_id, &dp_id, Some(&route_json))),
        "error" => Some(super::designated::branch_error(job_id, agent_id, short_id, &dp_id)),
        _ => return format!(
            "[job_created_cli] ERROR: unknown route value '{route}' in designated-route response: {route_json}\n"
        ),
    };
    match branch_playbook {
        Some(p) => format!(
            "**Action 0 — Notify the user the job is on-chain.** {notify_body}\
             After Action 0 completes, follow the branch-specific playbook below:\n\n---\n\n{p}"
        ),
        None => notify_body,
    }
}
