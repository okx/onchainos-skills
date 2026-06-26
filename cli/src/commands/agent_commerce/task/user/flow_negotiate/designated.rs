//! Designated-provider routing and per-branch playbook generation (CLI mode).
//!
//! `branch_a2a_cli` / `branch_x402` / `branch_error` — each branch's playbook,
//! called inline after `designated_route_inner` resolves the route.

// ── Phase-split functions (route_only + per-branch) ─────────────────

/// Phase 1: call `designated-route`, then dispatch to the matching branch pseudo-event.
/// Outputs only the route command + a hard gate — no branch playbooks inlined.
pub(crate) fn route_only(job_id: &str, agent_id: &str, _short_id: &str, dp_id: &str, endpoint: Option<&str>) -> String {
    let endpoint_flag = match endpoint.filter(|s| !s.is_empty()) {
        Some(ep) => format!(" --endpoint {ep}"),
        None => String::new(),
    };
    format!("\
             🎯 **Designated ASP**: {dp_id}\n\
             ⚠️ The persisted designated-provider file has already been removed by the CLI when this prompt was generated (consume-on-read); no manual cleanup needed.\n\n\
             **D-Step 1 — query ASP route:**\n\
             ```bash\n\
             onchainos agent designated-route --provider {dp_id}{endpoint_flag}\n\
             ```\n\
             Response fields: `route` (`x402` | `a2a` | `error`), `errorType` (if error), `providerName`, `onlineStatus`, `endpoint`, `feeAmount`, `feeTokenSymbol` (if x402).\n\n\
             🛑 **Multi-service selection (when `services` array is present):**\n\
             If the response contains a `services` array, this ASP offers **multiple** x402 services.\n\
             The top-level `endpoint`/`feeAmount`/`feeTokenSymbol` default to the FIRST service — this may NOT be the one the user requested.\n\
             You MUST check the task description / user's original request to identify the intended service:\n\
             \x20\x20- Match by `serviceName`, `serviceDescription`, or endpoint path against keywords in the task description.\n\
             \x20\x20- Once matched, use THAT service's `endpoint`, `feeAmount`, `feeTokenSymbol` for ALL subsequent steps (x402-validate, set-payment-mode).\n\
             \x20\x20- If no clear match, present the service list to the user via `pending-decisions-v2 request` and let them pick.\n\n\
             **D-Step 2 — call `next-action` with the matching branch pseudo-event:**\n\n\
             | `route` value | `errorType` | next-action `--event` |\n\
             |---|---|---|\n\
             | `a2a` | — | `designated_a2a` |\n\
             | `x402` | — | `designated_x402` |\n\
             | `error` | `not_provider` | `designated_error` |\n\
             | `error` | `offline` | `designated_error` |\n\
             | `error` | `endpoint_not_found` | `designated_error` |\n\n\
             Execute:\n\
             ```bash\n\
             onchainos agent next-action --role buyer --agentId {agent_id} --message '{{\"event\":\"<from table above>\",\"jobId\":\"{job_id}\",\"provider\":\"{dp_id}\"}}'\n\
             ```\n\
             🛑 **Do NOT execute any D-Step / B-Step / DX-Step in this turn** — the next-action call above returns the matching branch playbook. Follow it verbatim.\n\
             🛑 Do NOT create groups, send messages, or call set-payment-mode before getting the branch playbook.\n\n\
             **End this turn after executing the branch playbook returned by next-action.**\n")
}

/// Inlines the three calls that begin the A2A
/// negotiation flow:
///   - B-Step 0   (duplicate guard)        → okx_a2a::session_query_exists
///   - B-Step 1   (create sub session)     → okx_a2a::session_create
///   - B-Step 1.5 (SKILL_PREFETCH dispatch) → okx_a2a::session_send
/// Everything from B-Step 2 onward (first inquiry, negotiation,
/// timeouts) requires the LLM to author natural-language content and remains
/// in the returned playbook.
pub(crate) fn branch_a2a_cli(
    job_id: &str,
    agent_id: &str,
    dp_id: &str,
) -> Option<String> {
    use crate::commands::agent_commerce::task::common::okx_a2a;

    // B-Step 0 — duplicate guard: does this job already have a sub session
    // with this provider? If yes, the first inquiry was already sent in a
    // previous turn; bail out so we don't double-send.
    match okx_a2a::session_query_exists(job_id, agent_id, dp_id) {
        Ok(true) => return Some(format!(
            "[Designated ASP route: A2A] Provider {dp_id}\n\n\
             🛑 Sub session already exists for this job; the first inquiry has already been sent in a prior turn. \
             End this turn immediately — do not create a group, do not send any message, do not run `okx-a2a session status` / `okx-a2a session create` / `okx-a2a xmtp-send`.\n"
        )),
        Ok(false) => { /* fall through to create */ }
        Err(e) => return Some(format!("[branch_a2a_cli] ERROR: okx-a2a session query failed: {e}\n")),
    }

    // B-Step 1 — create the sub session (group + session record). The CLI
    // helper returns the canonical sessionKey assembled from the three IDs;
    // we use it as <SUB_KEY> in the remaining playbook.
    match okx_a2a::session_create(job_id, agent_id, dp_id) {
        Ok(sk) => sk,
        Err(e) => return Some(format!("[branch_a2a_cli] ERROR: okx-a2a session create failed: {e}\n")),
    };

    // B-Step 1.5 — SKILL_PREFETCH: pre-load the buyer playbook into the
    // freshly created sub session so its first inbound message has the
    // correct context. Fire-and-forget (--no-wait baked into helper).
    let prefetch = "[SKILL_PREFETCH] Read the okx-agent-task skill. Pre-load buyer role context. This prefetch message itself requires no action — but when the NEXT inbound message arrives (same turn or later turn), you MUST process it normally via user-sub-playbook.md §Peer Message Routing (#1–#6). Do NOT carry over \"no action\" to business messages.";
    if let Err(e) = okx_a2a::session_send(job_id, Some(dp_id), prefetch) {
        return Some(format!("[branch_a2a_cli] ERROR: okx-a2a session send (SKILL_PREFETCH) failed: {e}\n"));
    }

    // B-Step 1.6 — Upload + forward any pending attachments (best-effort).
    super::super::flow_lifecycle::upload_and_forward_all_attachments(
        job_id, agent_id, dp_id,
    );

    // Sub session created + SKILL_PREFETCH sent. The ASP receives
    // `job_asp_selected` from the backend and independently decides to
    // apply on-chain. The buyer does NOTHING until `provider_applied`.
    None
}
/// Phase 2b: x402 branch — endpoint validation + set-payment-mode.
///
/// `route_data`: pre-fetched JSON from `designated_route_inner` (when called
/// in-process). Contains `endpoint`, `feeAmount`, `feeTokenSymbol`. When
/// `Some`, values are filled directly into the playbook so the LLM does not
/// need to "recall" them from a prior designated-route response.
pub(crate) fn branch_x402(job_id: &str, agent_id: &str, short_id: &str, dp_id: &str, route_data: Option<&serde_json::Value>) -> String {
    let cmd_x402_invalid = format!("onchainos agent pending-decisions-v2 request --job-id {job_id} --role buyer --agent-id {agent_id} --user-content \"<compose from template below>\" --list-label \"[x402 invalid {short_id}] next-step decision\" --source-event x402_invalid");
    let cmd_input_required = format!("onchainos agent pending-decisions-v2 request --job-id {job_id} --role buyer --agent-id {agent_id} --user-content \"<compose from template below>\" --list-label \"[x402 input {short_id}] field confirmation\" --source-event x402_input_required");
    let cmd_x402_price = format!("onchainos agent pending-decisions-v2 request --job-id {job_id} --role buyer --agent-id {agent_id} --user-content \"<compose from template below>\" --list-label \"[x402 price {short_id}] price decision\" --source-event x402_price_mismatch");
    let cmd_over_budget = format!("onchainos agent pending-decisions-v2 request --job-id {job_id} --role buyer --agent-id {agent_id} --user-content \"<compose from template below>\" --list-label \"[Over budget {short_id}] budget decision\" --source-event over_budget");

    // Extract x402 fields from pre-fetched route data; fall back to placeholders.
    let (ep, fa, ft) = route_data.map(|rd| (
        rd["endpoint"].as_str().unwrap_or(""),
        rd["feeAmount"].as_str().unwrap_or(""),
        rd["feeTokenSymbol"].as_str().unwrap_or(""),
    )).unwrap_or(("", "", ""));
    let has_route = !ep.is_empty() && !fa.is_empty() && !ft.is_empty();

    let validate_cmd = if has_route {
        format!("onchainos agent x402-validate --endpoint {ep} --agent-id {agent_id} --job-id {job_id} --fee-amount {fa} --fee-token {ft}")
    } else {
        format!("onchainos agent x402-validate --endpoint <endpoint from designated-route> --agent-id {agent_id} --job-id {job_id} --fee-amount <feeAmount> --fee-token <feeTokenSymbol>")
    };
    let validate_hint = if has_route { "" } else {
        "⚠️ Use `feeAmount` and `feeTokenSymbol` from the `designated-route` response above (earlier in this turn).\n         "
    };
    let ep_for_spm = if has_route { ep.to_string() } else { "<endpoint>".to_string() };

    format!("\
         [Designated ASP route: x402] Provider {dp_id} has an x402 endpoint.\n\
         [Role] User (Buyer)\n\n\
         🌐 **Localize first** — every `pending-decisions-v2 request` invocation below: translate the `--user-content` body (and the human portion of `--list-label`) to the user's language before running. Keep bash structure, flags, and `--source-event` tokens unchanged.\n\n\
         **DX-Step 1 — validate endpoint + price + budget (single CLI call):**\n\
         ```bash\n\
         {validate_cmd}\n\
         ```\n\
         {validate_hint}Response field `result` determines the branch:\n\n\
         - **`result == \"x402_invalid\"`** -> run:\n\
         \x20\x20```bash\n\
         \x20\x20{cmd_x402_invalid}\n\
         \x20\x20```\n\
         \x20\x20`--user-content` template:\n\
         \x20\x20[Job {short_id} — you are the User Agent] The x402 endpoint of the designated ASP (agentId={dp_id}) is invalid and cannot be used. Choose next step:\n\
         \x20\x20A. Specify another ASP — provide the agentId\n\
         \x20\x20B. Make the job public — let more ASPs discover it\n\
         \x20\x20C. Close the job\n\
         \x20\x20-> **end this turn** and wait for the user's reply.\n\n\
         - **`result == \"input_required\"`** -> the endpoint needs business parameters before payment.\n\
         \x20\x20The response includes `fields` / `requiredAnyOf` describing what the endpoint needs.\n\n\
         \x20\x20**IR-Step 1 — Pre-fill from serviceParams:**\n\
         \x20\x20Read `serviceParams` from the `[Pre-fetched task context]` block above.\n\
         \x20\x20For each field in the `fields`/`requiredAnyOf` list:\n\
         \x20\x20\x20\x20- If `serviceParams` is parseable as JSON, check whether a key matches the field `name` → pre-fill.\n\
         \x20\x20\x20\x20- If `serviceParams` is natural language, try to extract a value that semantically matches the field `description` → pre-fill.\n\
         \x20\x20\x20\x20- Otherwise → mark as \"pending user input\".\n\n\
         \x20\x20**IR-Step 2 — Push confirmation form to the user** (🛑 even if all fields are pre-filled, the user MUST confirm). Run:\n\
         \x20\x20```bash\n\
         \x20\x20{cmd_input_required}\n\
         \x20\x20```\n\
         \x20\x20`--user-content` template (fill `<placeholder>` from runtime values):\n\
         \x20\x20```\n\
         \x20\x20[Job {short_id}] The x402 endpoint requires the following business parameters before payment:\n\n\
         \x20\x20<for each field in the inputRequired list, one line:>\n\
         \x20\x20• <fieldName> (<type>): <description> — [Pre-filled: <value>] or [Please fill in]\n\n\
         \x20\x20<if all fields pre-filled:>\n\
         \x20\x20Please confirm the values above are correct.\n\
         \x20\x20A. Confirm → proceed with payment\n\
         \x20\x20B. Modify → specify which field and new value\n\n\
         \x20\x20<if any field needs user input:>\n\
         \x20\x20Please fill in the blank fields and confirm.\n\
         \x20\x20```\n\
         \x20\x20`--llm-content` block (keep English; replace `<placeholders>` with actual values):\n\
         \x20\x20```\n\
         \x20\x20[IR_CONTEXT] endpoint=<endpoint> feeTokenSymbol=<feeTokenSymbol> feeAmount=<feeAmount>\n\
         \x20\x20inputRequired fields: <copy the fields/requiredAnyOf list from x402-validate output>\n\
         \x20\x20Pre-filled values: <list each pre-filled field=value pair>\n\
         \x20\x20```\n\
         \x20\x20-> **end this turn** and wait for the user's reply.\n\n\
         - **`result == \"price_mismatch\"`** -> run:\n\
         \x20\x20```bash\n\
         \x20\x20{cmd_x402_price}\n\
         \x20\x20```\n\
         \x20\x20`--user-content` template:\n\
         \x20\x20[Job {short_id} — you are the User Agent] The designated ASP (agentId={dp_id}) actually charges <amountHuman> <tokenSymbol>, which differs from the registered fee <feeAmount> <feeTokenSymbol>. Accept this price?\n\
         \x20\x20A. Accept — continue with this price\n\
         \x20\x20B. Reject — switch to another ASP\n\
         \x20\x20`--llm-content` block (keep English):\n\
         \x20\x20```\n\
         \x20\x20[PRICE_CONTEXT] endpoint=<endpoint> amountHuman=<amountHuman> tokenSymbol=<tokenSymbol> acceptsJson=<acceptsJson>\n\
         \x20\x20```\n\
         \x20\x20-> **end this turn** and wait for the user's reply.\n\n\
         - **`result == \"over_budget\"`** -> run:\n\
         \x20\x20```bash\n\
         \x20\x20{cmd_over_budget}\n\
         \x20\x20```\n\
         \x20\x20`--user-content` template:\n\
         \x20\x20[Job {short_id} — you are the User Agent] The x402 fee from the designated ASP (agentId={dp_id}) is <amountHuman> <tokenSymbol>, which exceeds your max budget and cannot be used. Choose next step:\n\
         \x20\x20A. Specify another ASP — provide the ASP's agentId\n\
         \x20\x20B. Make the job public — let more ASPs discover it\n\
         \x20\x20C. Close the job\n\
         \x20\x20-> **end this turn** and wait for the user's reply.\n\n\
         - **`result == \"pass\"`** -> all checks passed. Proceed to **A-Step 3**.\n\n\
         **A-Step 3 — set-payment-mode (if needed):**\n\
         Check `paymentMode` from the `[Pre-fetched task context]` block above.\n\n\
         ▸ **If paymentMode is already `3` (x402)** → skip `set-payment-mode` and call `next-action` immediately:\n\
         ```bash\n\
         onchainos agent next-action --role buyer --agentId {agent_id} --message '{{\"event\":\"job_payment_mode_changed\",\"jobId\":\"{job_id}\",\"paymentMode\":3}}'\n\
         ```\n\n\
         ▸ **Otherwise** → push payment mode on-chain:\n\
         ```bash\n\
         onchainos agent set-payment-mode {job_id} --payment-mode x402 --token-symbol <tokenSymbol from x402-validate> --token-amount <amountHuman from x402-validate> --endpoint {ep_for_spm}\n\
         ```\n\
         ⚠️ Use the **actual values returned by x402-validate** for `tokenSymbol` and `tokenAmount` (NOT the original budget used at job creation).\n\n\
         **A-Step 3 result branch (🛑 MANDATORY — getting this wrong = the flow stalls):**\n\
         - `\"alreadySet\": true` -> call `onchainos agent next-action --role buyer --agentId {agent_id} --message '{{\"event\":\"job_payment_mode_changed\",\"jobId\":\"{job_id}\",\"paymentMode\":3}}'` immediately.\n\
         - `\"confirming\": true` -> **end this turn** and wait for `job_payment_mode_changed`.\n")
}

/// Phase 2c: error branch — not_provider or offline decision card.
pub(crate) fn branch_error(job_id: &str, agent_id: &str, short_id: &str, dp_id: &str) -> String {
    let not_provider = super::super::content::not_provider_user_prompt(job_id, short_id, dp_id);
    let provider_offline = super::super::content::provider_offline_user_prompt(job_id, short_id, dp_id);

    let endpoint_not_found_content = format!(
        "[Job {short_id} — you are the User Agent] The previously selected service endpoint (`requestedEndpoint` from the response) of ASP (agentId={dp_id}) is no longer available. Choose next step:\n\
         A. Specify another ASP — provide the agentId\n\
         B. Make the job public — let more ASPs discover it\n\
         C. Close the job"
    );
    let block_endpoint = crate::commands::agent_commerce::task::common::pending_v2::request_command_block(
        job_id, "buyer", agent_id, Some(dp_id),
        &endpoint_not_found_content,
        &format!("[Endpoint gone {short_id}] next-step decision"),
        "endpoint_not_found",
    );
    let block_not_provider = crate::commands::agent_commerce::task::common::pending_v2::request_command_block(
        job_id, "buyer", agent_id, Some(dp_id),
        &not_provider,
        &format!("[Not ASP {short_id}] next-step decision"),
        "not_provider",
    );
    let block_offline = crate::commands::agent_commerce::task::common::pending_v2::request_command_block(
        job_id, "buyer", agent_id, Some(dp_id),
        &provider_offline,
        &format!("[Offline {short_id}] next-step decision"),
        "provider_offline",
    );

    format!("\
         [Designated ASP route: error] Provider {dp_id} encountered a routing error.\n\
         [Role] User (Buyer)\n\n\
         **Branch by `errorType` from the `designated-route` response above (earlier in this turn):**\n\n\
         - **`errorType == \"endpoint_not_found\"`** -> the persisted endpoint no longer exists in the ASP's service list.\n\
         \x20\x20{block_endpoint}\n\
         \x20\x20-> **end this turn** and wait for the user's reply.\n\n\
         - **`errorType == \"not_provider\"`** -> the designated agent does not exist or is not registered as an ASP.\n\
         \x20\x20{block_not_provider}\n\
         \x20\x20-> **end this turn** and wait for the user's reply.\n\n\
         - **`errorType == \"offline\"`** -> the ASP is offline and cannot negotiate.\n\
         \x20\x20{block_offline}\n\
         \x20\x20-> **end this turn** and wait for the user's reply.\n\n")
}
