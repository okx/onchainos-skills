//! Event handlers for job_created and provider_conversation.

use super::super::flow::FlowContext;

// --- Event handler functions ------------------------------------------------

pub(crate) async fn job_created(ctx: &FlowContext<'_>) -> String {
    // No designated provider → asp-match flow; designated → route_only flow.
    let has_designated = super::super::negotiate::get_designated_provider(ctx.job_id)
        .ok()
        .flatten()
        .is_some();
    if !has_designated {
        return job_created_non_designated_provider(ctx);
    }
    job_created_with_designated_provider(ctx).await
}

fn job_created_non_designated_provider(ctx: &FlowContext<'_>) -> String {
    let title = ctx.title_display;
    let short_id = ctx.short_id;
    let notify_tpl = super::super::content::job_created_non_designated_user_notify();

    let notify_filled = notify_tpl
        .replace("<title>", title)
        .replace("<short_jobId>", short_id);

    format!(
        "[Trigger] job_created (on-chain, public task — no designated provider)\n\
         [Role] User (User)\n\n\
         🛑 Execute the 1 action below, then end the turn. The task is public; ASPs will discover it and reach out via `provider_conversation`.\n\n\
         **Action 1 — Notify the user that the job is on-chain.** **Localize first** — rewrite the content below in the user's language before sending. Do NOT pass the English template verbatim to a non-English user.\n\
         Canonical content (`<title>` and `<short_jobId>` already filled in):\n\
         \x20\x20{notify_filled}\n\
         ```bash\n\
         onchainos agent user-notify --content '<localized content shown below>'\n\
         ```\n\n\
         🛑 End the turn after notifying. Do NOT call `asp-match` — public tasks wait for ASPs to apply.\n"
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


/// CLI-mode handler for `provider_conversation_pick` — user picked an ASP
/// from the pending list. Runs `designated_route_inner` in-process and
/// dispatches to the matching branch. For the A2A route, handles the
/// public-task case where the daemon already created a session when the
/// ASP sent its conversation request (skips duplicate-guard bail-out).
pub(crate) async fn provider_conversation_pick_cli(
    job_id: &str,
    agent_id: &str,
    short_id: &str,
    dp_id: &str,
    _title_display: &str,
    _prefetched: Option<&crate::commands::agent_commerce::task::common::PreFetchedTaskContext>,
) -> String {
    let route_result = crate::commands::agent_commerce::task::common::designated_route_inner(
        dp_id,
        None,
    )
    .await;
    let route_json = match route_result {
        Ok(j) => j,
        Err(e) => return format!("[provider_conversation_pick] ERROR: designated-route failed: {e}\n"),
    };

    let route = route_json.get("route").and_then(|v| v.as_str()).unwrap_or("");
    match route {
        "a2a" => provider_conversation_pick_a2a(job_id, agent_id, short_id, dp_id),
        "x402" => super::designated::branch_x402(job_id, agent_id, short_id, dp_id, Some(&route_json)),
        "error" => super::designated::branch_error(job_id, agent_id, short_id, dp_id),
        _ => format!(
            "[provider_conversation_pick] ERROR: unknown route value '{route}' in designated-route response: {route_json}\n"
        ),
    }
}

/// A2A branch for public-task ASP pick. Returns LLM instructions to run
/// asp-match + set-asp, then create sub session + SKILL_PREFETCH only after
/// set-asp succeeds (avoids orphan sessions when asp-match finds no services
/// or set-asp fails).
fn provider_conversation_pick_a2a(job_id: &str, agent_id: &str, short_id: &str, dp_id: &str) -> String {
    let prefetch = "[SKILL_PREFETCH] Read the okx-agent-task skill. Pre-load user role context. \
        This prefetch message itself requires no action — but when the NEXT inbound message arrives \
        (same turn or later turn), you MUST process it normally via user-sub-playbook.md \
        §Peer Message Routing (#1–#6). Do NOT carry over \"no action\" to business messages.";

    format!(
        "[ASP picked: A2A] ASP {dp_id}\n\
         [Role] User (User)\n\n\
         **Step 1 — fetch the ASP's service info:**\n\
         ```bash\n\
         onchainos agent asp-match --job-id {job_id} --provider-agent-id {dp_id} --format json\n\
         ```\n\
         From the result, extract the ASP's **top service**: `serviceId`, `serviceName`, `serviceDescription`, \
         `feeAmount` (→ serviceTokenAmount), `feeToken` (→ serviceTokenAddress), `feeTokenSymbol`.\n\
         If `asp-match` returns no services, notify the user (localized): \
         \"ASP {dp_id} has no registered services.\" and end the turn.\n\n\
         **Step 2 — collect serviceParams if needed:**\n\
         If `serviceDescription` is non-empty, ask the user for serviceParams — enqueue:\n\
         **Localize first** — translate the `--user-content` and `--list-label` values below into the user's language before running.\n\
         ```bash\n\
         onchainos agent pending-decisions-v2 request --job-id {job_id} --role user --agent-id {agent_id} \
         --source-event set_asp_params \
         --user-content \"<compose from template below>\" \
         --list-label \"[SetASP {short_id}] provide service params\"\n\
         ```\n\
         `--user-content` template:\n\
         You selected Agent {dp_id} — <serviceName>.\n\
         Service: <serviceDescription>\n\
         Fee: <feeAmount> <feeTokenSymbol>\n\n\
         Please describe the input for this service (serviceParams):\n\
         [SERVICE_CONTEXT providerAgentId={dp_id} serviceId=<sid> serviceTokenAddress=<feeToken> serviceTokenAmount=<feeAmount>]\n\
         Then **end this turn** and wait for the user's reply.\n\n\
         If `serviceDescription` is empty, skip the decision and go to Step 3 directly (serviceParams = `''`).\n\n\
         **Step 3 — call `set-asp`:**\n\
         ```bash\n\
         onchainos agent set-asp {job_id} --provider-agent-id {dp_id} --service-id <sid> --service-params '<params or empty>' \
         --service-token-address <feeToken> --service-token-amount <feeAmount>\n\
         ```\n\
         On success → notify user (localized): \"ASP set to Agent {dp_id}. Waiting for ASP to accept.\"\n\n\
         **Step 4 — create sub session + SKILL_PREFETCH (only after Step 3 succeeds):**\n\
         ```bash\n\
         okx-a2a session create --job-id {job_id} --my-agent-id {agent_id} --to-agent-id {dp_id} --json\n\
         ```\n\
         Then send SKILL_PREFETCH to the newly created session:\n\
         ```bash\n\
         okx-a2a session send --session-key <sessionKey from above> --content '{prefetch}'\n\
         ```\n\n\
         **Step 5 — upload pending attachments (if any):**\n\
         ```bash\n\
         onchainos agent list-attachments {job_id}\n\
         ```\n\
         If the output is a non-empty JSON array, iterate over each file path:\n\
         a) `okx-a2a file upload --file-path <path> --agent-id {agent_id} --job-id {job_id}` → obtain fileKey + decryption-metadata fields.\n\
         b) `okx-a2a xmtp-send --job-id {job_id} --to-agent-id {dp_id}` with the attachment content (all fields verbatim from the upload output).\n\
         ⚠️ Attachment upload failure MUST NOT block the flow — skip failed files and continue.\n\
         If empty (`[]`), skip this step.\n\n\
         🛑 **End this turn after Step 5.** Wait for the `provider_applied` system event.\n\
         ❌ Do NOT call `confirm-accept` / `set-payment-mode` — the ASP has not applied yet.\n"
    )
}

/// CLI-mode handler for `provider_conversation`. Fetches ASP list in-process,
/// takes the first ASP, and pushes an accept/reject decision card to the user.
/// Reject triggers `provider_conversation_reject` which auto-advances to the
/// next ASP or pushes close options if none remain.
pub(crate) fn provider_conversation(ctx: &FlowContext<'_>) -> String {
    provider_conversation_cli_inner(ctx, None)
}

/// Shared implementation for both initial `provider_conversation` and
/// `provider_conversation_reject` (which passes pre-fetched items to skip
/// the duplicate reject + re-fetch).
pub(crate) fn provider_conversation_cli_inner(
    ctx: &FlowContext<'_>,
    prefetched_items: Option<Vec<serde_json::Value>>,
) -> String {
    use crate::commands::agent_commerce::task::common::{okx_a2a, pending_v2};

    let job_id = ctx.job_id;
    let agent_id = ctx.agent_id;
    let short_id = ctx.short_id;
    let title = ctx.title_display;

    let is_after_reject = prefetched_items.is_some();

    if !is_after_reject {
        if pending_v2::has_pending_for_job(job_id, "user") {
            return format!(
                "[provider_conversation] Duplicate event — pending decision already exists for job {short_id}. End turn.\n"
            );
        }
    }

    let items: Vec<serde_json::Value> = match prefetched_items {
        Some(v) => v,
        None => match okx_a2a::task_requests() {
            Ok(v) => v.into_iter()
                .filter(|item| {
                    item.get("jobId").and_then(|v| v.as_str()) == Some(job_id)
                        || !item.get("jobId").map_or(false, |v| v.is_string())
                })
                .collect(),
            Err(e) => return format!("[provider_conversation] ERROR: task requests failed: {e}\n"),
        },
    };

    if items.is_empty() {
        if is_after_reject {
            let no_sellers = super::super::content::no_more_sellers_user_notify(job_id);
            let user_content = format!(
                "{no_sellers}\n\
                 A. Specify an ASP — provide the ASP's agentId\n\
                 B. Make the job public — let more ASPs discover it\n\
                 C. Close the job — cancel and refund"
            );
            let request_block = crate::commands::agent_commerce::task::common::pending_v2::request_command_block(
                job_id, "user", agent_id, None,
                &user_content,
                &format!("[No ASP {short_id}] {title} next-step decision"),
                "no_asp_found",
            );
            return format!(
                "[provider_conversation_reject] All pending ASPs rejected; none remaining.\n\n\
                 🛑 Push the next-step decision card via `pending-decisions-v2 request`, then end turn.\n\n\
                 {request_block}\n"
            );
        }
        let content = super::super::content::pending_list_empty_user_notify();
        return format!(
            "[provider_conversation] No pending ASPs.\n\n\
             **Action — notify the user:**\n\
             **Localize first** — translate the content below into the user's language before sending.\n\
             Content: {content}\n\
             ```bash\n\
             onchainos agent user-notify --content '<localized content>'\n\
             ```\n\
             🛑 End turn after notifying.\n"
        );
    }

    let first = &items[0];
    let asp_agent_id = first.get("toAgentId").and_then(|v| v.as_str())
        .or_else(|| first.get("agentId").and_then(|v| v.as_str()))
        .unwrap_or("?");
    let group_id = first.get("groupId").and_then(|v| v.as_str()).unwrap_or("?");
    let sender_name: String = first.get("name").and_then(|v| v.as_str()).map(String::from)
        .or_else(|| first.get("serviceName").and_then(|v| v.as_str()).map(String::from))
        .or_else(|| {
            first.get("messages")?.as_array()?.first()?
                .get("content")?.as_str()
                .and_then(|raw| serde_json::from_str::<serde_json::Value>(raw).ok())
                .and_then(|parsed| parsed.get("sender")?.get("name")?.as_str().map(String::from))
        })
        .unwrap_or_default();
    let name = sender_name.as_str();
    let remaining = items.len() - 1;

    let card_content = super::super::content::provider_pending_single_user_card(
        short_id, title, asp_agent_id, name,
    );

    let cmd = format!("onchainos agent pending-decisions-v2 request --job-id {job_id} --role user --agent-id {agent_id} --user-content \"<compose from template below>\" --list-label \"[ASP {short_id}] Accept provider?\" --source-event provider_pending");

    format!(
        "[Trigger] ASP pending contact — showing first of {} ASP(s)\n\
         [Role] User (User)\n\n\
         🛑 Push the accept/reject decision card via `pending-decisions-v2 request`, then end turn.\n\n\
         ASP context (LLM-only; do NOT expose groupId to user):\n\
         \x20\x20agentId: {asp_agent_id} | groupId: {group_id} | name: {name} | remaining after this: {remaining}\n\n\
         **Localize first** — translate the `--user-content` and `--list-label` values below into the user's language before running.\n\
         ```bash\n\
         {cmd}\n\
         ```\n\
         `--user-content` template:\n\
         {card_content}\n\n\
         `--llm-content` block (keep English verbatim — consumed by user-session agent for routing):\n\
         ```\n\
         [USER_DECISION_REQUEST][source: provider_pending][job: {job_id}][role: user][agentId: {agent_id}]\n\
         [asp: {asp_agent_id}][groupId: {group_id}][remaining: {remaining}]\n\n\
         Step 1 — Card delivered. **END THE TURN NOW.**\n\
         Step 2 — When the user replies, route by choice:\n\
         \x20\x20• 1 / \"accept\" / \"接受\" / \"yes\" / \"好\"  → run:\n\
         \x20\x20\x20\x20```bash\n\
         \x20\x20\x20\x20onchainos agent next-action --role user --agentId {agent_id} --message '{{\"event\":\"provider_conversation_pick\",\"jobId\":\"{job_id}\",\"provider\":\"{asp_agent_id}\"}}'\n\
         \x20\x20\x20\x20```\n\
         \x20\x20• 2 / \"reject\" / \"拒绝\" / \"no\" / \"不\" / \"换一个\" / \"next\"  → run:\n\
         \x20\x20\x20\x20```bash\n\
         \x20\x20\x20\x20onchainos agent next-action --role user --agentId {agent_id} --message '{{\"event\":\"provider_conversation_reject\",\"jobId\":\"{job_id}\",\"groupId\":\"{group_id}\"}}'\n\
         \x20\x20\x20\x20```\n\
         ```\n",
        items.len(),
    )
}

/// CLI-mode handler for `provider_conversation_reject`. Rejects the current
/// ASP (by groupId), re-fetches the list, and either shows the next ASP's
/// accept/reject card or pushes close options if none remain.
pub(crate) fn provider_conversation_reject_cli(ctx: &FlowContext<'_>, group_id: &str) -> String {
    use crate::commands::agent_commerce::task::common::okx_a2a;

    let job_id = ctx.job_id;

    if let Err(e) = okx_a2a::task_reject(group_id, None) {
        return format!("[provider_conversation_reject] ERROR: task reject failed: {e}\n");
    }

    let items: Vec<serde_json::Value> = match okx_a2a::task_requests() {
        Ok(v) => v.into_iter()
            .filter(|item| {
                item.get("jobId").and_then(|v| v.as_str()) == Some(job_id)
                    || !item.get("jobId").map_or(false, |v| v.is_string())
            })
            .collect(),
        Err(e) => return format!("[provider_conversation_reject] ERROR: task requests failed: {e}\n"),
    };

    provider_conversation_cli_inner(ctx, Some(items))
}

// --- Auto-consume for public tasks (visibility=0) ---

const MAX_AUTO_CONSUME_ATTEMPTS: usize = 50;

/// Auto-consume the ASP message queue for public tasks (R1–R21).
///
/// Picks ASPs in FIFO order, routes each one via `designated_route_inner`,
/// and either starts negotiation (A2A/x402) or rejects and loops to the next.
/// Returns an LLM prompt for the first viable ASP, or a silent-wait message
/// when the queue is exhausted.
pub(crate) async fn provider_conversation_auto_consume(ctx: &FlowContext<'_>) -> String {
    use crate::commands::agent_commerce::task::common::okx_a2a;
    use std::collections::HashSet;
    use std::time::Duration;

    let job_id = ctx.job_id;

    // R2: strict serial — skip if there's an active session
    if let Ok(Some(_)) = super::super::negotiate::get_designated_provider(job_id) {
        crate::audit::log(
            "cli", "auto_consume/skip_active_session", true, Duration::default(),
            Some(vec![format!("jobId={job_id}")]), None,
        );
        return "[auto_consume] Active session exists; skip (R2). End turn.\n".to_string();
    }

    let failed_list = super::super::negotiate::load_failed(job_id);
    let mut skip_groups: HashSet<String> = HashSet::new();

    for attempt in 0..MAX_AUTO_CONSUME_ATTEMPTS {
        // Fetch FIFO queue, filter by job_id
        let items: Vec<serde_json::Value> = match okx_a2a::task_requests() {
            Ok(v) => v.into_iter()
                .filter(|item| {
                    item.get("jobId").and_then(|v| v.as_str()) == Some(job_id)
                        || !item.get("jobId").map_or(false, |v| v.is_string())
                })
                .collect(),
            Err(e) => {
                // F17: task_requests API failure — break, don't advance
                crate::audit::log(
                    "cli", "auto_consume/task_requests_failed", false, Duration::default(),
                    Some(vec![format!("jobId={job_id}"), format!("error={e}")]), None,
                );
                return format!("[auto_consume] task_requests failed: {e}; end turn.\n");
            }
        };

        if items.is_empty() {
            if attempt > 0 {
                crate::audit::log(
                    "cli", "auto_consume/queue_exhausted", true, Duration::default(),
                    Some(vec![format!("jobId={job_id}"), format!("attempts={attempt}")]), None,
                );
            }
            // R7: silent wait
            return "[auto_consume] Queue empty; silent wait (R7). End turn.\n".to_string();
        }

        let first = &items[0];
        let asp_agent_id = first.get("toAgentId").and_then(|v| v.as_str())
            .or_else(|| first.get("agentId").and_then(|v| v.as_str()))
            .unwrap_or("?")
            .to_string();
        let group_id = first.get("groupId").and_then(|v| v.as_str())
            .unwrap_or("?")
            .to_string();

        // F18: skip messages whose reject previously failed
        if skip_groups.contains(&group_id) {
            return "[auto_consume] Stuck on unrejectable message; end turn.\n".to_string();
        }

        // R15: failed list guard
        if failed_list.contains(&asp_agent_id) {
            crate::audit::log(
                "cli", "auto_consume/skip_failed_asp", true, Duration::default(),
                Some(vec![format!("jobId={job_id}"), format!("asp={asp_agent_id}")]), None,
            );
            if let Err(e) = okx_a2a::task_reject(&group_id, None) {
                skip_groups.insert(group_id);
                crate::audit::log(
                    "cli", "auto_consume/reject_failed", false, Duration::default(),
                    Some(vec![format!("jobId={job_id}"), format!("error={e}")]), None,
                );
            }
            continue;
        }

        // Route check (async)
        let route_json = match crate::commands::agent_commerce::task::common::designated_route_inner(
            &asp_agent_id, None,
        ).await {
            Ok(j) => j,
            Err(e) => {
                crate::audit::log(
                    "cli", "auto_consume/route_check_error", false, Duration::default(),
                    Some(vec![format!("jobId={job_id}"), format!("asp={asp_agent_id}"), format!("error={e}")]), None,
                );
                if okx_a2a::task_reject(&group_id, None).is_err() {
                    skip_groups.insert(group_id);
                }
                continue;
            }
        };

        let route = route_json.get("route").and_then(|v| v.as_str()).unwrap_or("error");
        match route {
            "a2a" => {
                let _ = super::super::negotiate::save_designated_provider(job_id, &asp_agent_id);
                crate::audit::log(
                    "cli", "auto_consume/pick_a2a", true, Duration::default(),
                    Some(vec![
                        format!("jobId={job_id}"), format!("asp={asp_agent_id}"),
                        format!("attempt={attempt}"),
                    ]), None,
                );
                return provider_conversation_pick_a2a_auto(ctx, &asp_agent_id, &group_id);
            }
            "x402" => {
                let ep = route_json["endpoint"].as_str().unwrap_or("");
                let _ = super::super::negotiate::save_designated_provider_with_endpoint(
                    job_id, &asp_agent_id, Some(ep),
                );
                crate::audit::log(
                    "cli", "auto_consume/pick_x402", true, Duration::default(),
                    Some(vec![
                        format!("jobId={job_id}"), format!("asp={asp_agent_id}"),
                        format!("attempt={attempt}"),
                    ]), None,
                );
                return branch_x402_auto_consume(ctx, &asp_agent_id, &group_id, &route_json);
            }
            _ => {
                // route="error" — ASP offline / not provider (R4)
                crate::audit::log(
                    "cli", "auto_consume/route_error", true, Duration::default(),
                    Some(vec![
                        format!("jobId={job_id}"), format!("asp={asp_agent_id}"),
                        format!("route={route}"),
                    ]), None,
                );
                if okx_a2a::task_reject(&group_id, None).is_err() {
                    skip_groups.insert(group_id);
                }
                continue;
            }
        }
    }

    // R19: max attempts reached — silent wait + audit alert
    crate::audit::log(
        "cli", "auto_consume/max_attempts", false, Duration::default(),
        Some(vec![format!("jobId={job_id}"), format!("max={MAX_AUTO_CONSUME_ATTEMPTS}")]), None,
    );
    "[auto_consume] Max attempts reached (R19); silent wait. End turn.\n".to_string()
}

/// A2A auto-consume prompt — like `provider_conversation_pick_a2a()` but with
/// auto-inferred serviceParams (R21) and auto-advance on failure (R20).
fn provider_conversation_pick_a2a_auto(ctx: &FlowContext<'_>, asp_id: &str, group_id: &str) -> String {
    let job_id = ctx.job_id;
    let agent_id = ctx.agent_id;

    let prefetch = "[SKILL_PREFETCH] Read the okx-agent-task skill. Pre-load user role context. \
        This prefetch message itself requires no action — but when the NEXT inbound message arrives \
        (same turn or later turn), you MUST process it normally via user-sub-playbook.md \
        §Peer Message Routing (#1–#6). Do NOT carry over \"no action\" to business messages.";

    let auto_advance = format!(
        "onchainos agent next-action --role user --agentId {agent_id} \
         --message '{{\"event\":\"auto_advance_next\",\"jobId\":\"{job_id}\",\
         \"failedProvider\":\"{asp_id}\",\"reason\":\"<REASON>\"}}'"
    );

    format!(
        "[Auto-Consume: A2A] ASP {asp_id}\n\
         [Role] User — Public Task Auto-Consume\n\n\
         🛑 Execute ALL steps below silently (no user-notify, no decision cards).\n\
         On ANY failure → reject + auto-advance (instructions at each step).\n\n\
         **Step 1 — Fetch ASP service info:**\n\
         ```bash\n\
         onchainos agent asp-match --job-id {job_id} --provider-agent-id {asp_id} --format json\n\
         ```\n\
         Extract: `serviceId`, `serviceName`, `serviceDescription`, \
         `feeAmount` (→ serviceTokenAmount), `feeToken` (→ serviceTokenAddress), `feeTokenSymbol`.\n\n\
         ⚠ If `asp-match` returns **no services** or **API error** → reject + auto-advance:\n\
         ```bash\n\
         okx-a2a task reject --group-id {group_id} --json\n\
         {auto_advance}\n\
         ```\n\
         (Replace `<REASON>` with `asp_match_no_services` or `asp_match_error`)\n\
         🛑 End turn after calling next-action.\n\n\
         **Step 2 — Auto-infer serviceParams (R21 → R11 aligned):**\n\
         - If `serviceDescription` is empty → serviceParams = `''` → go to Step 3.\n\
         - If `serviceDescription` is non-empty → infer serviceParams from the task \
         title/description + serviceDescription context. Use your best judgment.\n\
         \x20\x20- If inference succeeds → use the inferred value → go to Step 3.\n\
         \x20\x20- If you cannot infer → reject + auto-advance (do NOT use empty params):\n\
         \x20\x20\x20\x20```bash\n\
         \x20\x20\x20\x20okx-a2a task reject --group-id {group_id} --content '[user_rejected]:Your request has been declined. The task submitted by the User is missing required input parameters for your service. Please look for another task that provides the necessary inputs.' --json\n\
         \x20\x20\x20\x20{auto_advance}\n\
         \x20\x20\x20\x20```\n\
         \x20\x20\x20\x20(Replace `<REASON>` with `service_params_not_inferred`)\n\
         \x20\x20\x20\x20🛑 End turn after calling next-action.\n\
         - Do NOT ask the user for serviceParams.\n\n\
         **Step 3 — Set ASP:**\n\
         ```bash\n\
         onchainos agent set-asp {job_id} --provider-agent-id {asp_id} \
         --service-id <sid> --service-params '<inferred or empty>' \
         --service-token-address <feeToken> --service-token-amount <feeAmount>\n\
         ```\n\
         ⚠ If `set-asp` fails → reject + auto-advance:\n\
         ```bash\n\
         okx-a2a task reject --group-id {group_id} --json\n\
         {auto_advance}\n\
         ```\n\
         (Replace `<REASON>` with `set_asp_failed`)\n\
         🛑 End turn after calling next-action.\n\n\
         **Step 4 — Create sub session + SKILL_PREFETCH (only after Step 3 succeeds):**\n\
         ```bash\n\
         okx-a2a session create --job-id {job_id} --my-agent-id {agent_id} \
         --to-agent-id {asp_id} --json\n\
         ```\n\
         Then send SKILL_PREFETCH:\n\
         ```bash\n\
         okx-a2a session send --session-key <sessionKey from above> \
         --content '{prefetch}'\n\
         ```\n\n\
         **Step 5 — Upload pending attachments (best-effort):**\n\
         ```bash\n\
         onchainos agent list-attachments {job_id}\n\
         ```\n\
         If non-empty JSON array, iterate each file:\n\
         a) `okx-a2a file upload --file-path <path> --agent-id {agent_id} --job-id {job_id}` → obtain fileKey.\n\
         b) `okx-a2a xmtp-send --job-id {job_id} --to-agent-id {asp_id}` with attachment content.\n\
         ⚠ Attachment failure MUST NOT block the flow — skip failed files.\n\n\
         🛑 **End this turn after Step 5.** Wait for `provider_applied` system event.\n\
         ❌ Do NOT call `confirm-accept` / `set-payment-mode` — the ASP has not applied yet.\n\
         ❌ Do NOT send user-notify or push decision cards.\n"
    )
}

/// x402 auto-consume prompt — like `branch_x402()` but auto-resolves all
/// decision points (price, budget, inputRequired) without user interaction.
fn branch_x402_auto_consume(
    ctx: &FlowContext<'_>,
    asp_id: &str,
    group_id: &str,
    route_json: &serde_json::Value,
) -> String {
    let job_id = ctx.job_id;
    let agent_id = ctx.agent_id;

    let ep = route_json["endpoint"].as_str().unwrap_or("");
    let fa = route_json["feeAmount"].as_str().unwrap_or("");
    let ft = route_json["feeTokenSymbol"].as_str().unwrap_or("");

    let validate_cmd = format!(
        "onchainos agent x402-validate --endpoint {ep} --agent-id {agent_id} \
         --job-id {job_id} --fee-amount {fa} --fee-token {ft}"
    );

    let auto_advance = format!(
        "onchainos agent next-action --role user --agentId {agent_id} \
         --message '{{\"event\":\"auto_advance_next\",\"jobId\":\"{job_id}\",\
         \"failedProvider\":\"{asp_id}\",\"reason\":\"<REASON>\"}}'"
    );

    format!(
        "[Auto-Consume: x402] ASP {asp_id} — x402 endpoint\n\
         [Role] User — Public Task Auto-Consume\n\n\
         🛑 Execute ALL steps below silently (no user-notify, no decision cards).\n\
         On failure → reject + auto-advance (instructions at each step).\n\n\
         **DX-Step 1 — Validate endpoint + price + budget:**\n\
         ```bash\n\
         {validate_cmd}\n\
         ```\n\
         Branch on `result`:\n\n\
         - **`result == \"x402_invalid\"`** or **`result == \"over_budget\"`** → reject + auto-advance:\n\
         \x20\x20```bash\n\
         \x20\x20okx-a2a task reject --group-id {group_id} --json\n\
         \x20\x20{auto_advance}\n\
         \x20\x20```\n\
         \x20\x20(Replace `<REASON>` with `x402_invalid` or `x402_over_budget`)\n\
         \x20\x20🛑 End turn after calling next-action.\n\n\
         - **`result == \"price_mismatch\"`** → auto-accept the actual price (public task). \
         Proceed to A-Step 3 using `amountHuman` and `tokenSymbol` from the response.\n\n\
         - **`result == \"input_required\"`** → auto-infer body parameters:\n\
         \x20\x20Read the `fields`/`requiredAnyOf` list from the response.\n\
         \x20\x20Read `serviceParams` from the `[Pre-fetched task context]` block.\n\
         \x20\x20For each field: try to match from serviceParams or task description.\n\
         \x20\x20If ALL required fields can be inferred → proceed to A-Step 3 with `--body '<inferred JSON>'`.\n\
         \x20\x20If ANY required field cannot be inferred → reject + auto-advance:\n\
         \x20\x20```bash\n\
         \x20\x20okx-a2a task reject --group-id {group_id} --json\n\
         \x20\x20{auto_advance}\n\
         \x20\x20```\n\
         \x20\x20(Replace `<REASON>` with `x402_input_uninferrable`)\n\
         \x20\x20🛑 End turn after calling next-action.\n\n\
         - **`result == \"pass\"`** → all checks passed. Proceed to A-Step 3.\n\n\
         **A-Step 3 — set-payment-mode (if needed):**\n\
         Check `paymentMode` from the `[Pre-fetched task context]` block.\n\n\
         ▸ If paymentMode is already `3` (x402) → skip, call next-action:\n\
         ```bash\n\
         onchainos agent next-action --role user --agentId {agent_id} \
         --message '{{\"event\":\"job_payment_mode_changed\",\"jobId\":\"{job_id}\",\"paymentMode\":3}}'\n\
         ```\n\n\
         ▸ Otherwise → push payment mode on-chain:\n\
         ```bash\n\
         onchainos agent set-payment-mode {job_id} --payment-mode x402 \
         --token-symbol <tokenSymbol from x402-validate> \
         --token-amount <amountHuman from x402-validate> --endpoint {ep}\n\
         ```\n\
         ⚠️ Use the **actual values** from x402-validate.\n\n\
         ⚠ If `set-payment-mode` fails → reject + auto-advance:\n\
         ```bash\n\
         okx-a2a task reject --group-id {group_id} --json\n\
         {auto_advance}\n\
         ```\n\
         (Replace `<REASON>` with `set_payment_mode_failed`)\n\
         🛑 End turn after calling next-action.\n\n\
         **A-Step 3 result branch:**\n\
         - `\"alreadySet\": true` → call next-action immediately (same as paymentMode=3 above).\n\
         - `\"confirming\": true` → **end this turn** and wait for `job_payment_mode_changed`.\n\n\
         ❌ Do NOT send user-notify or push decision cards.\n\
         ❌ Do NOT notify the user of price mismatches — auto-accept within budget.\n"
    )
}
