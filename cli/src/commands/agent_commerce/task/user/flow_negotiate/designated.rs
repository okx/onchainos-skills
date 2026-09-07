//! Designated-provider routing and per-branch playbook generation (CLI mode).
//!
//! `branch_a2a_cli` / `branch_error` — each branch's playbook,
//! called inline after `designated_route_inner` resolves the route.

/// Inlines the three calls that begin the A2A
/// negotiation flow:
///   - B-Step 0   (duplicate guard)        → okx_a2a::session_query_exists
///   - B-Step 1   (create sub session)     → okx_a2a::session_create
///   - B-Step 1.5 (SKILL_PREFETCH dispatch) → okx_a2a::session_send
///
/// Everything from B-Step 2 onward (first inquiry, negotiation,
/// timeouts) requires the LLM to author natural-language content and remains
/// in the returned playbook.
pub(crate) fn branch_a2a_cli(job_id: &str, agent_id: &str, dp_id: &str) -> Option<String> {
    use crate::commands::agent_commerce::task::common::okx_a2a;

    // B-Step 0 — duplicate guard: does this job already have a sub session
    // with this provider? If yes, the first inquiry was already sent in a
    // previous turn; bail out so we don't double-send.
    match okx_a2a::session_query_exists(job_id, agent_id, dp_id) {
        Ok(true) => return Some(format!(
            "[Designated ASP route: A2A] ASP {dp_id}\n\n\
             🛑 Sub session already exists for this job; the first inquiry has already been sent in a prior turn. \
             End this turn immediately — do not create a group, do not send any message, do not run `okx-a2a session status` / `okx-a2a session create` / `okx-a2a session send`.\n"
        )),
        Ok(false) => { /* fall through to create */ }
        Err(e) => return Some(format!("[branch_a2a_cli] ERROR: okx-a2a session query failed: {e}\n")),
    }

    // B-Step 1 — create the sub session (group + session record). The CLI
    // helper returns the canonical sessionKey assembled from the three IDs;
    // we use it as <SUB_KEY> in the remaining playbook.
    match okx_a2a::session_create(job_id, agent_id, dp_id) {
        Ok(sk) => sk,
        Err(e) => {
            return Some(format!(
                "[branch_a2a_cli] ERROR: okx-a2a session create failed: {e}\n"
            ))
        }
    };

    // B-Step 1.5 — SKILL_PREFETCH: pre-load the user playbook into the
    // freshly created sub session so its first inbound message has the
    // correct context. The helper performs one bounded JSON-mode send.
    let prefetch = "[SKILL_PREFETCH] Read the okx-ai skill through skills/okx-ai-v2/SKILL.md. Pre-load A2A context. This prefetch message itself requires no action — but when the NEXT inbound message arrives (same turn or later turn), you MUST re-enter through that SKILL.md and process the envelope normally via references/a2a/router.md. Do NOT carry over \"no action\" to business messages.";
    if let Err(e) = okx_a2a::session_send(job_id, Some(dp_id), prefetch) {
        return Some(format!(
            "[branch_a2a_cli] ERROR: okx-a2a session send (SKILL_PREFETCH) failed: {e}\n"
        ));
    }

    // B-Step 1.6 — Upload + forward any pending attachments (best-effort).
    super::super::flow_lifecycle::upload_and_forward_all_attachments(job_id, agent_id, dp_id);

    // Sub session created + SKILL_PREFETCH sent. The ASP receives
    // `job_asp_selected` from the backend and independently decides to
    // apply on-chain. The user does NOTHING until `provider_applied`.
    None
}
/// Phase 2c: error branch — provider or registered-service recovery card.
pub(crate) fn branch_error(job_id: &str, agent_id: &str, short_id: &str, dp_id: &str) -> String {
    let not_provider = super::super::content::not_provider_user_prompt(job_id, short_id, dp_id);
    let provider_offline =
        super::super::content::provider_offline_user_prompt(job_id, short_id, dp_id);

    let service_not_found_content = format!(
        "[Job {short_id} — you are the User Agent] The previously selected registered service of ASP (agentId={dp_id}) is no longer usable. Choose next step:\n\
         A. Specify another ASP — provide the agentId\n\
         B. Make the job public — let more ASPs discover it\n\
         C. Close the job"
    );
    let block_service =
        crate::commands::agent_commerce::task::common::pending_v2::request_command_block(
            job_id,
            "user",
            agent_id,
            Some(dp_id),
            &service_not_found_content,
            &format!("[Service gone {short_id}] next-step decision"),
            "service_not_found",
        );
    let block_not_provider =
        crate::commands::agent_commerce::task::common::pending_v2::request_command_block(
            job_id,
            "user",
            agent_id,
            Some(dp_id),
            &not_provider,
            &format!("[Not ASP {short_id}] next-step decision"),
            "not_provider",
        );
    let block_offline =
        crate::commands::agent_commerce::task::common::pending_v2::request_command_block(
            job_id,
            "user",
            agent_id,
            Some(dp_id),
            &provider_offline,
            &format!("[Offline {short_id}] next-step decision"),
            "provider_offline",
        );

    format!("\
         [Designated ASP route: error] ASP {dp_id} encountered a routing error.\n\
         [Role] User (User)\n\n\
         **Branch by `errorType` from the `designated-route` response above (earlier in this turn):**\n\n\
         - **`errorType == \"service_not_found\"`** -> the selected registered service is no longer available.\n\
         \x20\x20{block_service}\n\
         \x20\x20-> **end this turn** and wait for the user's reply.\n\n\
         - **`errorType == \"not_provider\"`** -> the designated agent does not exist or is not registered as an ASP.\n\
         \x20\x20{block_not_provider}\n\
         \x20\x20-> **end this turn** and wait for the user's reply.\n\n\
         - **`errorType == \"offline\"`** -> the ASP is offline and cannot negotiate.\n\
         \x20\x20{block_offline}\n\
         \x20\x20-> **end this turn** and wait for the user's reply.\n\n")
}
