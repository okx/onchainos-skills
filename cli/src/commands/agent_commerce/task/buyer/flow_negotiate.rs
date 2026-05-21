//! Prompt-generation functions for the negotiation / matching phase
//!
//! Negotiation-related events split out from `flow.rs`:
//! - `job_created` (job on-chain -> recommend / designated-provider routing)
//! - `switch_provider` (kick off a new flow immediately after the user swaps provider)
//! - `provider_conversation` (a public-job ASP reaches out)
//! - `job_visibility_changed` (visibility toggle)
//! - `job_payment_mode_changed` (payment-mode switch on-chain)
//! - `negotiate_reply` / `negotiate_ack` / `negotiate_counter` (negotiation relays)

use super::flow::FlowContext;

/// Designated-provider D-Step routing (service-list query -> x402 or A2A branch entry)
pub(super) fn designated_provider_d_steps(job_id: &str, agent_id: &str, short_id: &str, dp_id: &str) -> String {
    let provider_offline = super::content::provider_offline_user_prompt(job_id, short_id, dp_id);
    format!("\
             🎯 **Designated ASP**: {dp_id}\n\
             ⚠️ The persisted designated-provider file has already been removed by the CLI when this prompt was generated (consume-on-read); no manual cleanup needed.\n\n\
             **D-Step 1 - query the ASP's service-list:**\n\
             ```bash\n\
             onchainos agent service-list --agent-id {dp_id}\n\
             ```\n\
             ⚠️ `--agent-id` is a **required named flag** — do NOT pass the agent ID as a positional argument (e.g. `service-list {dp_id}` will error). Always use `--agent-id {dp_id}`.\n\
             If the command returns an error (e.g. \"unexpected argument\", \"unrecognized\"), **retry once** using the exact command above with `--agent-id`. Do NOT skip D-Steps on error — the routing decision depends on this result.\n\
             Check whether the response contains services (non-empty `services` array) and inspect the `endpoint`, `feeAmount`, `feeTokenSymbol` fields on each service.\n\n\
             **D-Step 1.5 - online-status check (only effective on the escrow path):**\n\
             Query the ASP's profile to get its online status:\n\
             ```bash\n\
             onchainos agent profile {dp_id}\n\
             ```\n\
             Read `onlineStatus` from the response (1=online / 2=offline). If the field is missing, null, or empty, treat the ASP as **online** (the backend may not yet return this field).\n\
             - `onlineStatus == 1` **or field missing/null/empty** (online / unknown) -> continue to D-Step 2.\n\
             - `onlineStatus == 2` AND **no endpoint** (so you are about to enter the escrow negotiation path) -> the ASP is offline and cannot negotiate.\n\
             \x20\x20Call xmtp_prompt_user to notify the user:\n\
             \x20\x20llmContent: [USER_DECISION_REQUEST][sub_key: <full sessionKey from session_status>][job: {job_id}][role: buyer] If the user's intent is \"option A / specify ASP\" and they provide an agentId -> call xmtp_dispatch_session(sessionKey=\"<full sessionKey from session_status>\", content=\"[USER_DECISION_RELAY][intent:PICK_PROVIDER agentId=<agentId provided by user>] User reply verbatim: <user reply>\") to relay back to the sub session; \"option B / public\" -> call xmtp_dispatch_session(sessionKey=\"<same sessionKey>\", content=\"[USER_DECISION_RELAY][intent:SET_PUBLIC] User reply verbatim: <user reply>\"); \"option C / close\" -> call xmtp_dispatch_session(sessionKey=\"<same sessionKey>\", content=\"[USER_DECISION_RELAY][intent:CLOSE_TASK] User reply verbatim: <user reply>\"). ⚠️ Routing-tag protocol: intent names must be inserted verbatim in ALL-CAPS ASCII; never translate or rewrite. ⚠️ The relay must use xmtp_dispatch_session. The user session agent must NOT execute task CLI itself. {CONSTRAINT}\n\
             \x20\x20userContent: {provider_offline}\n\
             \x20\x20-> **end this turn** and wait for the user's reply.\n\
             - `onlineStatus == 2` but **has an endpoint** (x402 path) -> x402 is automated payment and does not depend on the ASP being online in real time, so continue to D-Step 2.\n\n\
             **D-Step 2 - route by service-list result:**\n\
             - **Has services and contains an endpoint (x402-capable)** -> extract `feeAmount`, `feeTokenSymbol`, `endpoint` from `services[0]`.\n\
             \x20\x20⚠️ **`feeAmount` is the value the ASP manually entered at registration time, and is not necessarily equal to the on-chain price**; it must be verified by DX-Step 1 `x402-check`. When showing it to the user, label it \"registered fee\".\n\
             \x20\x20Execute the designated-provider x402 flow below (do NOT jump to A-Step 1):\n\n\
             \x20\x20**DX-Step 1 - validate the endpoint:**\n\
             \x20\x20```bash\n\
             \x20\x20onchainos agent x402-check --endpoint <endpoint> --agent-id {agent_id}\n\
             \x20\x20```\n\
             \x20\x20- `valid=false` -> call xmtp_prompt_user to tell the user the endpoint is invalid and guide them to pick a next step (user decision required):\n\
             \x20\x20\x20\x20llmContent: [USER_DECISION_REQUEST][sub_key: <full sessionKey from session_status>][job: {job_id}][role: buyer] If the user's intent is \"option A / specify ASP\" and they provide an agentId -> call xmtp_dispatch_session(sessionKey=\"<full sessionKey from session_status>\", content=\"[USER_DECISION_RELAY][intent:PICK_PROVIDER agentId=<agentId provided by user>] User reply verbatim: <user reply>\"); \"option B / public\" -> call xmtp_dispatch_session(sessionKey=\"<same sessionKey>\", content=\"[USER_DECISION_RELAY][intent:SET_PUBLIC] User reply verbatim: <user reply>\"); \"option C / close\" -> call xmtp_dispatch_session(sessionKey=\"<same sessionKey>\", content=\"[USER_DECISION_RELAY][intent:CLOSE_TASK] User reply verbatim: <user reply>\"). ⚠️ Routing-tag protocol: intent names verbatim in ALL-CAPS ASCII; do not translate or rewrite. ⚠️ Relay must use xmtp_dispatch_session. The user session agent must NOT execute task CLI itself. {CONSTRAINT}\n\
             \x20\x20\x20\x20userContent: [Job {short_id} - you are the User Agent] The x402 endpoint of the designated ASP (agentId={dp_id}) is invalid and cannot be used. Choose next step:\n\
             \x20\x20\x20\x20A. Specify another ASP - provide the ASP's agentId\n\
             \x20\x20\x20\x20B. Make the job public - let more ASPs discover it\n\
             \x20\x20\x20\x20C. Close the job\n\
             \x20\x20\x20\x20-> **end this turn** and wait for the user's reply.\n\n\
             \x20\x20**DX-Step 2 - amount sanity check:**\n\
             \x20\x20Compare `amountHuman` from x402-check with `feeAmount` from `services[0]`:\n\
             \x20\x20- Mismatch (delta > 1%) -> call xmtp_prompt_user to ask the user whether to accept the actual price:\n\
             \x20\x20\x20\x20llmContent: [USER_DECISION_REQUEST][sub_key: <full sessionKey from session_status>][job: {job_id}][role: buyer] If the user's intent is \"affirmative / accept / accept / OK / agree, etc.\" -> call xmtp_dispatch_session(sessionKey=\"<full sessionKey from session_status>\", content=\"[USER_DECISION_RELAY][intent:ACCEPT_X402_PRICE] User reply verbatim (do not interpret or translate): <user reply>\") to relay back to the sub session and continue to DX-Step 3; \"negative / reject / decline / no, etc.\" -> call xmtp_dispatch_session(sessionKey=\"<same sessionKey>\", content=\"[USER_DECISION_RELAY][intent:REJECT_X402_PRICE] User reply verbatim (do not interpret or translate): <user reply>\") to relay and switch ASP. ⚠️ **Routing-tag protocol**: `[intent:ACCEPT_X402_PRICE]` / `[intent:REJECT_X402_PRICE]` MUST be inserted verbatim in **ALL-CAPS ASCII**; never translate or rewrite - the sub branches on the intent tag, it does NOT pattern-match on the user's words. ⚠️ The relay must use xmtp_dispatch_session (do NOT use sessions_send). The user session agent must NOT execute task CLI itself. {CONSTRAINT}\n\
             \x20\x20\x20\x20userContent: Job `{job_id}` - the specified ASP (agentId={dp_id}) actually charges <amountHuman> <tokenSymbol>, which differs from the registered fee <feeAmount> <feeTokenSymbol>. Accept this price?\n\
             \x20\x20- Match -> continue to DX-Step 3.\n\n\
             \x20\x20**DX-Step 3 - budget check:**\n\
             \x20\x20First call `onchainos agent common context {job_id} --role buyer --agent-id {agent_id}` and extract `paymentMostTokenAmount` (max budget) and the task's `tokenSymbol`.\n\
             \x20\x20⚠️ **Currency check**: compare `tokenSymbol` from x402-check with the task's `tokenSymbol` -\n\
             \x20\x20- Mismatch (e.g. job in USDG, x402 charges USDT) -> since USDT and USDG are both USD stablecoins (~1:1), still compare numerically against the budget.\n\
             \x20\x20\x20\x20`set-payment-mode` will switch the on-chain payment token to **the x402 endpoint's token** (no longer the token used at job creation).\n\
             \x20\x20- Match -> compare directly.\n\
             \x20\x20Compare `amountHuman` with `paymentMostTokenAmount` (**NOT `tokenAmount`; `tokenAmount` is the base budget**):\n\
             \x20\x20- Over -> call xmtp_prompt_user to tell the user the fee exceeds their max budget and guide them to pick a next step (user decision required):\n\
             \x20\x20\x20\x20llmContent: [USER_DECISION_REQUEST][sub_key: <full sessionKey from session_status>][job: {job_id}][role: buyer] If the user's intent is \"option A / specify ASP\" and they provide an agentId -> call xmtp_dispatch_session(sessionKey=\"<full sessionKey from session_status>\", content=\"[USER_DECISION_RELAY][intent:PICK_PROVIDER agentId=<agentId provided by user>] User reply verbatim: <user reply>\"); \"option B / public\" -> call xmtp_dispatch_session(sessionKey=\"<same sessionKey>\", content=\"[USER_DECISION_RELAY][intent:SET_PUBLIC] User reply verbatim: <user reply>\"); \"option C / close\" -> call xmtp_dispatch_session(sessionKey=\"<same sessionKey>\", content=\"[USER_DECISION_RELAY][intent:CLOSE_TASK] User reply verbatim: <user reply>\"). ⚠️ Routing-tag protocol: intent names verbatim in ALL-CAPS ASCII; never translate or rewrite. ⚠️ Relay must use xmtp_dispatch_session. The user session agent must NOT execute task CLI itself. {CONSTRAINT}\n\
             \x20\x20\x20\x20userContent: [Job {short_id} - you are the User Agent] The actual x402 fee from the designated ASP (agentId={dp_id}) is <amountHuman> <tokenSymbol>, which exceeds your max budget and cannot be used. Choose next step:\n\
             \x20\x20\x20\x20A. Specify another ASP - provide the ASP's agentId\n\
             \x20\x20\x20\x20B. Make the job public - let more ASPs discover it\n\
             \x20\x20\x20\x20C. Close the job\n\
             \x20\x20\x20\x20-> **end this turn** and wait for the user's reply.\n\
             \x20\x20- Within budget -> execute **A-Step 3** below.\n\n\
             \x20\x20**A-Step 3 - set-payment-mode (push x402 on-chain):**\n\
             \x20\x20```bash\n\
             \x20\x20onchainos agent set-payment-mode {job_id} --payment-mode x402 --token-symbol <tokenSymbol returned by x402-check> --token-amount <amountHuman returned by x402-check> --endpoint <endpoint>\n\
             \x20\x20```\n\
             \x20\x20⚠️ Use the **actual values returned by x402-check** for `tokenSymbol` and `tokenAmount` (NOT the original budget used at job creation).\n\n\
             \x20\x20**A-Step 3 result branch (🛑 MANDATORY - getting this wrong = the flow stalls):**\n\
             \x20\x20Inspect the CLI output (JSON) of set-payment-mode:\n\
             \x20\x20- Output contains `\"alreadySet\": true` (paymentMode is already on-chain so the on-chain call was skipped) -> **do NOT wait for `job_payment_mode_changed`**;\n\
             \x20\x20\x20\x20no event will fire on-chain. **Within this same turn, immediately execute the x402 flow for job_payment_mode_changed**:\n\
             \x20\x20\x20\x20call `onchainos agent next-action --jobid {job_id} --jobStatus job_payment_mode_changed --role buyer --agentId {agent_id}` and follow the returned script (task-402-pay).\n\
             \x20\x20- Output contains `\"confirming\": true` (normal on-chain submission in flight) -> **end this turn** and wait for the `job_payment_mode_changed` system notification.\n\n\
             - **No service or no endpoint (no x402 support)** -> enter **B-Step 1** to create a chat and negotiate.",
             CONSTRAINT = super::flow::PROMPT_USER_SESSION_CONSTRAINT)
}

/// Designated-provider B-Step negotiation protocol (three-step handshake + group creation + multi-round negotiation + persistence + fallback)
pub(super) fn designated_provider_negotiate(job_id: &str, agent_id: &str, short_id: &str, dp_id: &str) -> String {
    let fallback_cmd = format!("onchainos agent mark-failed {job_id} --provider {dp_id} && onchainos agent recommend {job_id} --agent-id {agent_id}");
    let fallback_lines = format!("First run `onchainos agent mark-failed {job_id} --provider {dp_id}` to flag the failure, then run `onchainos agent recommend {job_id} --agent-id {agent_id}` to fetch a fresh recommendation list.\n\
             \x20\x20If the list is non-empty -> show it to the user via the xmtp_prompt_user template (same format as the non-designated Step 2: list each ASP's info + pick/next-page/public/close options).\n\
             \x20\x20If the list is empty -> guide the user through A/B/C below");
    format!("\
             🛑 **Hard constraint - the three-step handshake is the ONLY legitimate path to get the ASP to apply**\n\n\
             To get the ASP to enter the apply phase (escrow), you **must** complete the full three-step handshake:\n\
             \x20\x201) `[intent:propose]` (you -> ASP, structured proposal)\n\
             \x20\x202) Wait for the ASP to reply with `[intent:ack]` (all fields equal) or `[intent:counter]` (keep negotiating) or `[intent:reject]` (ASP refuses)\n\
             \x20\x203) You reply with `[intent:confirm]` (echo back the ACK fields verbatim - the ASP only applies once it sees this marker)\n\
             \x20\x20⚡ Either side may send `[intent:reject]` at any time to terminate the negotiation (must include jobId + reason); on receipt do **NOT** reply, immediately switch to the next ASP.\n\n\
             ❌ **Do NOT bypass the handshake with natural language** - do NOT send messages like:\n\
             \x20\x20- \"Terms are locked / terms finalized / no further proposal needed / please apply directly / please accept the job directly\"\n\
             \x20\x20- \"Final confirmation: job/price/payment mode ...\" plain-text summaries without the [intent:propose] / [intent:confirm] markers\n\
             \x20\x20- Any kind of \"alternative handshake\" short-circuit - the ASP flow treats the `[intent:confirm]` literal as the only apply trigger, so a natural-language \"please apply\" will simply not be recognized and the ASP will keep waiting for [intent:propose].\n\n\
             Correct behavior: once negotiation aligns, **strictly use** the `[intent:propose]` template (see B-Step 2 Step 4 below) so the handshake parser succeeds. **Even short negotiations must complete all three steps** - even if it's \"can do, original price OK, escrow OK\" three-liner, turn it into [intent:propose] and send it; never skip.\n\n\
             ━━━━━━━━━ Branch B: supportA2MCP=false -> A2A (negotiation required) ━━━━━━━━━\n\n\
             **B-Step 0 - duplicate guard (🛑 hard gate):**\n\
             Call `session_status` to check whether this job already has a sub session (i.e. group already created).\n\
             If a sub session **already exists** -> the first inquiry has already been sent. **End this turn immediately** - do not create a group, do not send a message, do not send an inquiry, do not run any subsequent B-Step.\n\
             If it does **not** exist -> continue to B-Step 1.\n\n\
             **B-Step 1 - create the group:**\n\
             Call xmtp_start_conversation to create the group + the sub session:\n\
             \x20\x20Args: myAgentId={agent_id}, toAgentId=<{dp_id}>, jobId={job_id}\n\
             \x20\x20On success returns sessionKey + xmtpGroupId.\n\
             \x20\x20⚠️ Before the call, print: `[buyer-xmtp] xmtp_start_conversation: myAgentId={agent_id}, toAgentId=<providerAgentId>, jobId={job_id}`\n\
             \x20\x20⚠️ After the call, print: `[buyer-xmtp] xmtp_start_conversation result: sessionKey=<returned value>, xmtpGroupId=<returned value>`\n\n\
             **B-Step 2 - automated negotiation (User Agent <-> ASP Agent multi-turn interaction in the sub session):**\n\
             🛑 **Within the same turn after creating the group you MUST call `xmtp_send` to send the first inquiry** - creating the group only opens the channel; not sending a message = the ASP receives no signal = the flow stalls.\n\
             ❌ Absolutely forbidden: creating the group and ending the turn without sending a message.\n\
             ❌ Absolutely forbidden: using xmtp_dispatch_user / xmtp_dispatch_session instead of xmtp_send - after the group is created use xmtp_send uniformly.\n\n\
             Negotiation goal: reach agreement on the following structured fields (other fields stick to what the user set when publishing and are not negotiated) -\n\
             \x20\x20- paymentMode: payment mode (**fixed to escrow in an A2A negotiation session** - x402 goes through recommend auto-routing and does not enter negotiation)\n\
             \x20\x20- tokenSymbol: payment token\n\
             \x20\x20- tokenAmount: payment amount\n\n\
             ⏱ Timeout rule: wait at most 5 minutes for each ASP reply. On timeout -> first xmtp_send `[intent:reject]` (reason: negotiation timeout, no reply within 5 minutes) to the ASP, then `{fallback_cmd}` to switch to the next ASP (**do NOT xmtp_delete_conversation**). After a timeout, if any further a2a-agent-chat message arrives from that ASP, **do not reply or process it**; just ignore.\n\n\
             ⚠️ **Negotiation message format iron rule**: every structured negotiation message (PROPOSE / CONFIRM / REJECT) **MUST end with the matching `[intent:*]` suffix marker**;\n\
             the last line of `content` must be `[intent:propose]` / `[intent:confirm]` / `[intent:reject]`, **NEVER replaced by natural language**.\n\
             The ASP Agent parses the suffix mechanically; a missing suffix stalls the negotiation flow.\n\n\
             📌 **You hold full negotiation authority - do NOT mechanically accept any ASP quote**. Look at the [job details] + [ASP profile / service-list / historical securityRate / feedback] in context and judge for yourself:\n\
             \x20\x20- Is the ASP's price reasonable for the workload? Don't force it through if it exceeds your max budget.\n\
             \x20\x20- Compare the ASP's profile / service-list unit price for similar services vs the current quote (the ASP's own listed price is a reference anchor).\n\
             \x20\x20- On the A2A negotiation path, paymentMode is fixed to escrow (funds are escrow-protected).\n\
             \x20\x20- With multiple recommended ASPs, don't force a deal with any single one; if it doesn't fit, just let the 5-minute timeout fire and switch.\n\n\
             🛑🛑🛑 **ABSOLUTE PROHIBITION - iron rule: throughout negotiation, never reveal the max budget (max_budget / paymentMostTokenAmount) to the ASP.**\n\
             No message sent to the ASP (natural language, [intent:propose], [intent:confirm]) may **ever** contain the max_budget value.\n\
             Leaking the max budget = the ASP quotes the cap immediately = the user loses all bargaining power.\n\
             ❌ Absolutely forbidden: mentioning \"max budget\", \"cap\", \"max budget\", \"the most I can pay\" or the corresponding value in xmtp_send\n\
             ❌ Absolutely forbidden: writing the `paymentMostTokenAmount` field value into any message to the ASP\n\n\
             Negotiation steps:\n\
             1. Call xmtp_send to send the first inquiry (natural language - let the ASP quote first, then judge):\n\
             \x20\x20content=<job description + expected deliverable + paymentMode preference + budget (base budget); **do NOT expose max_budget**>\n\
             \x20\x20-> wait for the ASP's reply (5-minute timeout)\n\
             2. (Inside the sub session) the ASP replies with a quote (amount, token, payment-mode preference, estimated delivery time).\n\n\
             🛑 **Mandatory pre-evaluation - after the ASP replies, you MUST complete the steps below before sending any xmtp_send**:\n\
             \x20\x20a) `onchainos agent common context {job_id} --role buyer --agent-id {agent_id}` to get budget / max_budget\n\
             \x20\x20b) Extract quote / capability info from the ASP's reply\n\
             \x20\x20c) Evaluate using the decision matrix in Step 2.5 below\n\
             \x20\x20❌ Do NOT send any xmtp_send (including a reject) before a-c complete - skipping evaluation = decisions with no basis.\n\n\
             🔴 **Step 2.5 - first-quote evaluation (fully automated, never ask the user)**:\n\
             After the ASP replies in natural language with a quote, **immediately** extract the minimum price and compare against the task budget / max_budget.\n\
             Get max_budget from the `paymentMostTokenAmount` field of `onchainos agent common context {job_id} --role buyer --agent-id {agent_id}`.\n\n\
             \x20\x20| ASP quote | Action | Notes |\n\
             \x20\x20|---|---|---|\n\
             \x20\x20| <= budget | -> price acceptable; keep confirming other terms, then proceed to Step 4 when all clear | Price within budget but other terms still need negotiating |\n\
             \x20\x20| budget < quote <= max_budget | -> go to Step 3 and counter in natural language | Bargaining room, negotiate down |\n\
             \x20\x20| > max_budget | -> **auto-REJECT + switch** (see below) | Hard cap exceeded, unacceptable |\n\n\
             \x20\x20**Mandatory action when quote > max_budget (fully automated; do NOT ask the user, do NOT xmtp_dispatch_user)**:\n\
             \x20\x20a) xmtp_send `[intent:reject]`:\n\
             \x20\x20\x20\x20content=\n\
             \x20\x20\x20\x20jobId: {job_id}\n\
             \x20\x20\x20\x20reason: quote exceeds max budget\n\
             \x20\x20\x20\x20[intent:reject]\n\
             \x20\x20b) `{fallback_cmd}` to switch to the next ASP\n\
             \x20\x20c) Return to Step 2 routing decision\n\n\
             3. (Inside the sub session) both sides adjust price/terms in natural language (potentially multi-round; 5-minute timeout per round; ASP COUNTER limit 3 times)\n\
             \x20\x20For each round, call xmtp_send with: sessionKey=<same as above>, content=<negotiation content>\n\
             \x20\x20⚠️ **Do NOT mechanically accept ASP markups**: treat the task's **max_budget (max budget) as the absolute cap** - anything above max_budget is rejected, no matter by how much. In the `budget < ASP price <= max_budget` band you may negotiate, accept at the original price, or keep countering; ASP price <= budget can be accepted directly.\n\
             \x20\x20⚠️ **Token is negotiable**: tokenSymbol may be changed by mutual agreement (e.g. USDT <-> USDG), but only with both sides' explicit consent. The starting token comes from `onchainos agent common context`.\n\n\
             \x20\x20⚠️ If any step has no ASP reply within 5 minutes -> treat as negotiation timeout, first xmtp_send `[intent:reject]` (reason: negotiation timeout) to the ASP, then `{fallback_cmd}` to switch (**do NOT delete the group**). After timeout, ignore any further messages from that ASP.\n\n\
             4. After reaching preliminary agreement, call xmtp_send to send the **[intent:propose]** structured proposal (this exact format is mandatory - the ASP Agent parses it mechanically):\n\
             \n\
             📋 **Mandatory self-check before filling fields (prevents \"memory time-travel\")**:\n\
             \x20\x20Before writing any field of [intent:propose], **go back field-by-field through every xmtp_send in this sub session, from the most recent backwards, and find the last value both sides explicitly agreed on**:\n\
             \x20\x20- tokenAmount: use **the price last agreed in natural language** (NOT the job's original budget, NOT the listed price from the recommend list, NOT any intermediate round's quote)\n\
             \x20\x20- paymentMode: same - use the last consensus value\n\
             \x20\x20- If any field has no explicit consensus in the dialogue -> **do NOT send [intent:propose]**; first xmtp_send a natural-language message and confirm once more.\n\
             \x20\x20⚠️ Do NOT fill from memory - your training data does NOT contain this session, the only reliable source is replaying the message history of this sub session.\n\n\
             \x20\x20content=\n\
             jobId: {job_id}\n\
             paymentMode: escrow\n\
             tokenSymbol: <USDT|USDG>\n\
             tokenAmount: <amount>\n\
             [intent:propose]\n\n\
             5. **Wait for the ASP to reply with [intent:ack] or [intent:counter]** (5-minute timeout):\n\n\
             \x20\x20- Got **[intent:ack]** -> verify field-by-field that the values echoed by the ASP exactly match the PROPOSE you sent:\n\
             \x20\x20\x20\x20- All match -> ✅ **execute Step 6 immediately** (do NOT send any message, just run the bash commands):\n\
             \x20\x20\x20\x20\x20\x20🚫 **xmtp_send is forbidden here** - do NOT send [intent:confirm], natural language, or anything else.\n\
             \x20\x20\x20\x20\x20\x20[intent:confirm] must only be sent after the set-payment-mode in Step 6 confirms on-chain (the `job_payment_mode_changed` event).\n\
             \x20\x20\x20\x20\x20\x20-> Jump **now** to Step 6 below and execute save-agreed + set-payment-mode.\n\
             \x20\x20\x20\x20- Any field mismatch -> treat as tampering; xmtp_send a message telling the ASP the fields don't match and resend [intent:propose].\n\n\
             \x20\x20- Got **[intent:counter]** -> **count first**: replay this sub session's history and count the total `[intent:counter]` messages the ASP has sent (including this one).\n\
             \x20\x20\x20\x20🔢 **COUNTER round limit = 3**: if this is the 3rd (or later) COUNTER, **do NOT process the COUNTER contents**; directly xmtp_send `[intent:reject]` (reason: negotiation round limit reached, 3 COUNTERs already), then `{fallback_cmd}` to switch to the next ASP.\n\
             \x20\x20\x20\x20Under the limit -> continue with the value judgment below:\n\n\
             \x20\x20\x20\x20The ASP proposes a counter-offer; **judge by value, do not mechanically accept**:\n\
             \x20\x20\x20\x20⚠️ **Step 0: replay sub session history first to confirm whether the [intent:propose] you just sent had a typo**:\n\
             \x20\x20\x20\x20\x20\x20- Look at the last amount / paymentMode both sides explicitly agreed in natural-language negotiation.\n\
             \x20\x20\x20\x20\x20\x20- If the COUNTER amount **equals** the number you last agreed in natural language -> **YOUR PROPOSE had a typo, not an ASP markup**: resend a new [intent:propose] with the COUNTER amount; **do NOT haggle again** and do NOT insist \"we previously agreed X\" - just correct it.\n\
             \x20\x20\x20\x20\x20\x20- If the COUNTER amount **is higher than** the number you last agreed in natural language -> this is genuinely a markup; handle via the decision matrix below.\n\n\
             \x20\x20\x20\x20- Check tokenSymbol change: if the ASP suggests a different token, evaluate whether to accept (requires mutual explicit consent).\n\
             \x20\x20\x20\x20- Evaluate tokenAmount (**max_budget wins, NOT a percentage**):\n\
             \x20\x20\x20\x20\x20\x20- COUNTER price <= task budget (original budget) -> acceptable; send a new [intent:propose] with the COUNTER value.\n\
             \x20\x20\x20\x20\x20\x20- budget < COUNTER price <= max_budget (max budget) -> acceptable, or keep negotiating and meet in the middle (send a new [intent:propose] with reasoning).\n\
             \x20\x20\x20\x20\x20\x20- COUNTER price > max_budget -> xmtp_send `[intent:reject]` to end negotiation, then **immediately** `{fallback_cmd}` to switch ASP:\n\
             \x20\x20\x20\x20\x20\x20\x20\x20content=\n\
             \x20\x20\x20\x20\x20\x20\x20\x20jobId: {job_id}\n\
             \x20\x20\x20\x20\x20\x20\x20\x20reason: quote exceeds max budget\n\
             \x20\x20\x20\x20\x20\x20\x20\x20[intent:reject]\n\
             \x20\x20\x20\x20\x20\x20- max_budget unknown -> call `onchainos agent common context {job_id} --role buyer --agent-id {agent_id}` and read the `paymentMostTokenAmount` field.\n\
             \x20\x20\x20\x20- paymentMode is fixed to escrow; do not accept any other payment mode.\n\
             \x20\x20\x20\x20- All acceptable -> send a new [intent:propose] using the values from COUNTER and go back to Step 5 to wait for ACK.\n\n\
             \x20\x20- Got **[intent:reject]** -> the ASP actively rejected. **Do not reply** and immediately run `{fallback_cmd}` to switch to the next ASP.\n\n\
             \x20\x20- The reply does **NOT** contain an [intent:ack] / [intent:counter] / [intent:reject] marker -> treat as natural-language discussion; continue negotiation and return to Step 4.\n\n\
             6. **Got [intent:ack] with all fields equal -> persist + setPaymentMode -> only THEN send [intent:confirm]**:\n\n\
             🛑 **Strict ordering rule ([intent:confirm] is the ONLY apply trigger for the ASP; it must only be sent once paymentMode is on-chain, otherwise the ASP applies against the wrong payment state)**:\n\n\
             **Step 6.1 - save-agreed persistence** (unconditional first step):\n\
             ```bash\n\
             onchainos agent save-agreed {job_id} --provider <providerAgentId of the current negotiation> --token-symbol <agreed token> --token-amount <agreed price> --agent-id {agent_id}\n\
             ```\n\
             Skipping this causes later confirm-accept to use the wrong token/amount.\n\n\
             **Step 6.2 - execute setPaymentMode (unconditional; do NOT inspect current on-chain value)**:\n\
             ⚠️ **Whatever the on-chain paymentType currently is (0 / 1 / 2 / 3), you MUST execute set-payment-mode.** Do NOT call common context to compare - just run:\n\
             ⚠️ **A2A negotiation sessions are fixed to escrow**: regardless of whether the ASP has an endpoint, only escrow is used in the negotiation session. set-payment-mode here will overwrite the on-chain value.\n\n\
             ```bash\n\
             onchainos agent set-payment-mode {job_id} --payment-mode escrow --token-symbol <agreed token> --token-amount <agreed price>\n\
             ```\n\
             **Step 6.2 result branch (🛑 MANDATORY - getting this wrong = the flow stalls):**\n\
             Inspect the CLI output (JSON) of set-payment-mode:\n\
             - Output contains `\"alreadySet\": true` (paymentMode already on-chain so the call was skipped) -> **do NOT wait for `job_payment_mode_changed`**;\n\
             \x20\x20no event will fire on-chain. **Within this same turn, immediately execute the escrow flow for job_payment_mode_changed**:\n\
             \x20\x20call `onchainos agent next-action --jobid {job_id} --jobStatus job_payment_mode_changed --role buyer --agentId {agent_id}` and follow the returned script (xmtp_send [intent:confirm]).\n\
             - Output contains `\"confirming\": true` (normal on-chain submission in flight) -> continue to Step 6.3.\n\
             ⚠️ **NEVER** xmtp_send [intent:confirm] while the on-chain call is still confirming - the ASP would apply on seeing [intent:confirm], but the on-chain paymentMode is still in the mempool / unconfirmed, so apply would fail or behave inconsistently. [intent:confirm] must only be sent after the `job_payment_mode_changed` event confirms paymentMode on-chain.\n\n\
             **Step 6.3 - executed only when `confirming`: end this turn** and wait for the `job_payment_mode_changed` system notification.\n\n\
             (New turn) On receiving `job_payment_mode_changed` -> call next-action --jobStatus job_payment_mode_changed -> per script, xmtp_send [intent:confirm] to the ASP. The ASP sees CONFIRM -> apply (escrow); on-chain paymentMode is already in place.\n\n\
             ━━━━━━━━━ Negotiation failed / switching ASP ━━━━━━━━━\n\n\
             Current ASP timed out (5 min) / COUNTER rounds exceeded (>=3) / received `[intent:reject]` / negotiation failed -> first xmtp_send `[intent:reject]` (reason: timeout / round limit / failure cause) to the ASP, then switch:\n\
             \x20\x20{fallback_lines}\n\
             ⚠️ **When switching you MUST first send [intent:reject] before switching away** (so the ASP has a clear termination signal), but **do NOT xmtp_delete_conversation**. After switching, ignore any further messages from that ASP.\n\
             No ASPs left on the current page and pagination also returns nothing -> first call `session_status` for the sessionKey; **before** calling `xmtp_prompt_user`, call `pending-decisions add` (see hard rule 7); then call xmtp_prompt_user to guide the user:\n\
             \x20\x20llmContent: [USER_DECISION_REQUEST][sub_key: <full sessionKey from session_status>][job: {job_id}][role: buyer] \
             If the user's intent is \"option A / specify ASP\" and they provide an agentId -> call xmtp_dispatch_session(sessionKey=\"<full sessionKey from session_status>\", content=\"[USER_DECISION_RELAY][intent:PICK_PROVIDER agentId=<agentId provided by user>] User reply verbatim (do not interpret or translate): <user reply>\") to relay back to the sub session; the sub agent will query service-list and route (x402 or group-negotiation); \
             \"option B / public / public\" -> call xmtp_dispatch_session(sessionKey=\"<same sessionKey>\", content=\"[USER_DECISION_RELAY][intent:SET_PUBLIC] User reply verbatim (do not interpret or translate): <user reply>\") to relay and run set-public; \
             \"option C / close / close\" -> call xmtp_dispatch_session(sessionKey=\"<same sessionKey>\", content=\"[USER_DECISION_RELAY][intent:CLOSE_TASK] User reply verbatim (do not interpret or translate): <user reply>\") to relay and run close. \
             ⚠️ **Routing-tag protocol**: `[intent:PICK_PROVIDER agentId=<...>]` / `[intent:SET_PUBLIC]` / `[intent:CLOSE_TASK]` MUST be inserted verbatim in **ALL-CAPS ASCII**; never translate or rewrite - the sub branches on the intent tag, it does NOT pattern-match on the user's words. \
             ⚠️ Relay must use xmtp_dispatch_session (do NOT use sessions_send). The user session agent must NOT execute task CLI itself. {CONSTRAINT}\n\
             \x20\x20userContent: [Job {short_id} - you are the User Agent] None of the recommended ASPs are a fit. Choose next step:\n\
             \x20\x20A. Specify an ASP - provide the ASP's agentId\n\
             \x20\x20B. Make the job public - let more ASPs discover it\n\
             \x20\x20C. Close the job - cancel and refund\n\
             \x20\x20-> **end this turn** and resume execution once the user's reply is relayed back.\n\n\
             [Subsequent events]\n\
             - x402 -> set-payment-mode -> job_payment_mode_changed -> task-402-pay (sign + direct/accept + endpoint replay) -> job_accepted -> complete\n\
             - escrow -> set-payment-mode -> job_payment_mode_changed -> notify ASP to apply -> ASP applies on-chain -> ASP xmtp_send notifies user -> user receives a2a-agent-chat -> confirm-accept -> job_accepted\n",
             CONSTRAINT = super::flow::PROMPT_USER_SESSION_CONSTRAINT)
}

// --- Event handler functions ------------------------------------------------

pub(super) fn job_created(ctx: &FlowContext<'_>) -> String {
    let job_id = ctx.job_id;
    let agent_id = ctx.agent_id;
    let short_id = ctx.short_id;

    let designated_provider = super::negotiate::take_designated_provider(job_id).ok().flatten();

    let notify_text = match &designated_provider {
        Some(dp_id) => format!("Connecting to the designated ASP {dp_id}..."),
        None => "Auto-querying recommended ASPs...".to_string(),
    };

    let created_notify = super::content::job_created_user_notify(job_id, &notify_text);

    let routing_section = if let Some(dp_id) = &designated_provider {
        designated_provider_d_steps(job_id, agent_id, short_id, dp_id)
    } else {
        format!("\
             **Step 0 - idempotency check: query whether a pending decision already exists for this job:**\n\
             ```bash\n\
             onchainos agent pending-decisions list --format json --agent-id {agent_id}\n\
             ```\n\
             If the returned list already contains an entry with jobId={job_id} and role=buyer -> **the user has already been notified; this is a duplicate event - end the turn without notifying again.**\n\
             If not present -> continue to Step 1.\n\n\
             **Step 1 - query the recommended ASP list:**\n\
             ```bash\n\
             onchainos agent recommend {job_id} --agent-id {agent_id}\n\
             ```\n\
             Outputs the ASP list (Agent Name / service description / credit / payment modes); ASPs that previously failed negotiation are auto-filtered.\n\n\
             🛑🛑🛑 **ABSOLUTE PROHIBITION - iron rule: in the current session (sub/backup) you must NOT directly print the recommendation list or any text reply.**\n\
             You are inside a sub session or backup session - **the user cannot see any output here**.\n\
             You must call the `xmtp_prompt_user` tool to push the list to the user session; that is the **only** way the user sees the list.\n\
             🔴 Real incident: the Minimax model, after getting recommend results in a backup session, just printed the list as text; the user never saw it and the task stalled.\n\
             ❌ Absolutely forbidden: replacing the xmtp_prompt_user tool call with a text reply - text reply = invisible to user = task stalls.\n\
             ❌ Absolutely forbidden: using xmtp_dispatch_user instead of xmtp_prompt_user - dispatch_user cannot relay, the user's choice cannot be routed back to the sub.\n\
             ❌ Absolutely forbidden: printing text \"for the user to see\" first and then calling the tool - text output in a sub session never reaches the user.\n\n\
             **Step 2 - show the list to the user and let them choose:**\n\
             Call `session_status` to get the sessionKey; call `pending-decisions add` (see hard rule 7); then call `xmtp_prompt_user`:\n\n\
             \x20\x20llmContent: [USER_DECISION_REQUEST][sub_key: <full sessionKey from session_status>][job: {job_id}][role: buyer]\n\
             \x20\x20Index -> AgentID mapping: <extract from recommend output, e.g. 1->798, 2->806, 3->866, 4->864, 5->865, 6->916, 7->810>\n\
             \x20\x20If the user replies with a number (e.g. \"2\" / \"4\") -> map to AgentID via the table above; if the user replies with a 3-digit AgentID (e.g. \"864\") -> use directly.\n\
             \x20\x20Routing rules:\n\
             \x20\x20- User picks an ASP (number or AgentID) -> xmtp_dispatch_session(sessionKey=\"<full sessionKey from session_status>\", content=\"[USER_DECISION_RELAY][intent:PICK_PROVIDER agentId=<mapped AgentID>] User reply verbatim: <user reply>\")\n\
             \x20\x20- User says \"next page / more / next\" -> xmtp_dispatch_session(sessionKey=\"<same>\", content=\"[USER_DECISION_RELAY][intent:NEXT_PAGE] User reply verbatim: <user reply>\")\n\
             \x20\x20- User says \"public / public\" -> xmtp_dispatch_session(sessionKey=\"<same>\", content=\"[USER_DECISION_RELAY][intent:SET_PUBLIC] User reply verbatim: <user reply>\")\n\
             \x20\x20- User says \"close / cancel / close\" -> xmtp_dispatch_session(sessionKey=\"<same>\", content=\"[USER_DECISION_RELAY][intent:CLOSE_TASK] User reply verbatim: <user reply>\")\n\
             \x20\x20⚠️ The intent tag must be inserted verbatim in ALL-CAPS ASCII; never translate or rewrite. Relay must use xmtp_dispatch_session. The user session agent must NOT execute task CLI itself. {CONSTRAINT}\n\n\
             \x20\x20⚠️ The \"Index -> AgentID mapping\" inside llmContent **must** be extracted line-by-line from recommend output and inlined - the user session agent cannot see the userContent list, and without the mapping table it cannot turn the user's index into an AgentID, leading to routing failure.\n\n\
             \x20\x20userContent: [Job {short_id} - you are the User Agent] Below is the list of recommended ASPs:\n\
             \x20\x20<paste the recommend output's ASP list in full, one block per ASP: index / Agent Name / AgentID / service name and description / credit / fee / payment modes>\n\
             \x20\x20---\n\
             \x20\x20Please choose: reply with an index (e.g. 1, 2, 3) or an AgentID (e.g. 864) to pick an ASP | reply \"next page\" for more | reply \"public\" to make the job public | reply \"close\" to close the job.\n\n\
             -> **end this turn** and wait for the user's reply to be relayed back.\n\n\
             **Step 3 - handle the relayed user reply:**\n\n\
             - The user picked an ASP (agentId=X) -> call `next-action --provider X` to enter the designated-provider flow:\n\
             ```bash\n\
             onchainos agent next-action --jobid {job_id} --jobStatus job_created --role buyer --agentId {agent_id} --provider <agentId picked by user>\n\
             ```\n\
             Execute the returned script (it internally queries service-list and routes x402 or A2A).\n\n\
             - The user asked for the next page -> run:\n\
             ```bash\n\
             onchainos agent recommend {job_id} --next-page\n\
             ```\n\
             If results -> go back to Step 2 and show the new list to the user.\n\
             If empty -> call xmtp_prompt_user to tell the user there are no more ASPs and guide their choice (user decision required, cannot use xmtp_dispatch_user):\n\
             \x20\x20\x20\x20llmContent: [USER_DECISION_REQUEST][sub_key: <full sessionKey from session_status>][job: {job_id}][role: buyer] If the user's intent is \"option A / specify ASP\" and they provide an agentId -> call xmtp_dispatch_session(sessionKey=\"<full sessionKey from session_status>\", content=\"[USER_DECISION_RELAY][intent:PICK_PROVIDER agentId=<agentId provided by user>] User reply verbatim: <user reply>\"); \"option B / public\" -> call xmtp_dispatch_session(sessionKey=\"<same sessionKey>\", content=\"[USER_DECISION_RELAY][intent:SET_PUBLIC] User reply verbatim: <user reply>\"); \"option C / close\" -> call xmtp_dispatch_session(sessionKey=\"<same sessionKey>\", content=\"[USER_DECISION_RELAY][intent:CLOSE_TASK] User reply verbatim: <user reply>\"). ⚠️ Routing-tag protocol same as above. ⚠️ Relay must use xmtp_dispatch_session. The user session agent must NOT execute task CLI itself. {CONSTRAINT}\n\
             \x20\x20\x20\x20userContent: [Job {short_id} - you are the User Agent] All recommended ASPs have been tried; no match found. Choose next step:\n\
             \x20\x20\x20\x20A. Specify an ASP - provide the ASP's agentId\n\
             \x20\x20\x20\x20B. Make the job public - let more ASPs discover it\n\
             \x20\x20\x20\x20C. Close the job - cancel and refund\n\n\
             - The user chose to make the job public -> `onchainos agent set-public {job_id}`\n\n\
             - The user chose to close the job -> `onchainos agent close {job_id}`",
             CONSTRAINT = super::flow::PROMPT_USER_SESSION_CONSTRAINT)
    };

    let mut output = format!(
        "🛑🛑🛑 **IDENTITY CHECK - you are the executor; delegation is forbidden**\n\
         You are inside a sub session or backup session. **You yourself** are the agent responsible for executing this script.\n\
         ❌ **Absolutely forbidden**: `sessions_spawn` - do NOT spawn a child agent to \"help you\" handle this event.\n\
         ❌ **Absolutely forbidden**: `sessions_yield` - do NOT hand off control.\n\
         🔴 Real incident: after receiving job_created, a backup called sessions_spawn to delegate to a child agent, which broke the designated-provider consume-context invariant and made negotiation uncontrollable.\n\
         **Correct behavior**: you yourself execute the CLI commands and xmtp tool calls step by step as below.\n\n\
         [Current state] job_created (job is on-chain, status: pending acceptance)\n\
         [Role] User (User Agent)\n\n\
         ⚠️ **Open != public**: Open is a job lifecycle state (pending acceptance), not a visibility (public/private). Job visibility is governed by the `visibility` field (0=public, 1=private), unrelated to the Open state. Do NOT translate Open as \"public\" in notifications.\n\n\
         🛑 **CLIs forbidden in this event**: save-agreed / set-payment-mode / confirm-accept / apply / complete / reject - no ASP has been picked yet, negotiation has not started, all of these are illegal here.\n\n\
         [Your next actions (strict order)]\n\n\
         **Step 0 - notify the user session + continue execution in the current sub/backup session:**\n\
         Call xmtp_dispatch_user to tell the user the job is on-chain (pure notification, no LLM thinking):\n\
         \x20\x20content: {created_notify}\n\n\
         ⚠️ Subsequent routing -> negotiation / acceptance all run in the **current session**; do NOT switch to the user session, do NOT sessions_spawn.\n\n\
         **Step 0.5 - check local attachments:**\n\
         ```bash\n\
         onchainos agent list-attachments {job_id}\n\
         ```\n\
         If the output is a non-empty JSON array (files exist), remember these file paths — they must be uploaded and forwarded to the provider via `xmtp_file_upload` + `xmtp_send` with `[intent:attachment]` suffix **after the task is accepted** (status=1). Do NOT upload now (no provider yet).\n\
         If empty (`[]`), skip.\n\n\
         {routing_section}\n\n"
    );

    if let Some(ref dp_id) = designated_provider {
        output.push_str("\n━━━━━━━━━ The B-Steps below run ONLY when D-Step concludes \"no service or no endpoint\" ━━━━━━━━━\n\
                         🛑 If D-Step already routed to x402 (service-list has an endpoint), then the B-Steps below are **entirely skipped, absolutely forbidden to execute**.\n\
                         Full x402 path: DX-Step 1->2->3 -> A-Step 3 (set-payment-mode) -> wait for job_payment_mode_changed -> task-402-pay.\n\
                         The x402 path **never involves** xmtp_start_conversation / group creation / three-step handshake / xmtp_send negotiation messages.\n\n");
        output.push_str(&designated_provider_negotiate(job_id, agent_id, short_id, dp_id));
    }

    output
}

pub(super) fn switch_provider(ctx: &FlowContext<'_>) -> String {
    let job_id = ctx.job_id;
    let agent_id = ctx.agent_id;
    let short_id = ctx.short_id;

    let designated_provider = super::negotiate::take_designated_provider(job_id).ok().flatten();
    let dp_id = match &designated_provider {
        Some(id) => id.clone(),
        None => {
            return format!("[Error] switch_provider is missing the --provider argument.\n\
                 Please call again: onchainos agent next-action --jobid {job_id} --jobStatus switch_provider --role buyer --agentId {agent_id} --provider <new ASP agentId>\n");
        }
    };

    let d_steps = designated_provider_d_steps(job_id, agent_id, short_id, &dp_id);
    let negotiate = designated_provider_negotiate(job_id, agent_id, short_id, &dp_id);
    format!("\
         [Provider switch] set-provider has been submitted; start the new ASP flow immediately (do NOT wait for the task_provider_change on-chain confirmation).\n\
         [Role] User (User Agent) | [Execution environment] user session\n\n\
         🛑 **CLIs forbidden in this event**: save-agreed / set-payment-mode / confirm-accept / apply / complete / reject - negotiation with the new ASP has not started, all of these are illegal here.\n\n\
         ⚠️ The old ASP's sub session will automatically send [intent:reject] when it receives the `task_provider_change` on-chain event; no intervention from you required.\n\n\
         **Pre-step - check local attachments:**\n\
         ```bash\n\
         onchainos agent list-attachments {job_id}\n\
         ```\n\
         If the output is a non-empty JSON array (files exist), remember these file paths — they must be uploaded and forwarded to the new provider via `xmtp_file_upload` + `xmtp_send` with `[intent:attachment]` suffix **after the task is accepted** (status=1). Do NOT upload now.\n\
         If empty (`[]`), skip.\n\n\
         [Your next actions (strict order)]\n\n\
         {d_steps}\n\n\
         ━━━━━━━━━ The B-Steps below run ONLY when D-Step concludes \"no service or no endpoint\" ━━━━━━━━━\n\
         🛑 If D-Step already routed to x402 (service-list has an endpoint), then the B-Steps below are **entirely skipped, absolutely forbidden to execute**.\n\
         Full x402 path: DX-Step 1->2->3 -> A-Step 3 (set-payment-mode) -> wait for job_payment_mode_changed -> task-402-pay.\n\
         The x402 path **never involves** xmtp_start_conversation / group creation / three-step handshake / xmtp_send negotiation messages.\n\n\
         {negotiate}\n")
}

pub(super) fn provider_conversation(ctx: &FlowContext<'_>) -> String {
    let job_id = ctx.job_id;
    let agent_id = ctx.agent_id;
    let short_id = ctx.short_id;

    let no_sellers = super::content::no_more_sellers_user_notify(job_id);
    format!(
    "[Trigger] Received an \"ASP pending contact\" style message (user session side)\n\
     [Role] User (User Agent)\n\n\
     🛑 **Do NOT auto-create groups**: after receiving the pending_list notification, you must NOT call xmtp_start_conversation on your own.\n\
     You must first show the list and let the user pick an ASP; only after an explicit user choice may you create the group.\n\n\
     🛑 **CRITICAL - this event MUST push the ASP list to the user session via `xmtp_prompt_user`; printing text reply in the sub session is forbidden.**\n\
     ❌ Do NOT replace the xmtp_prompt_user tool call with a text reply (sub-session output is invisible to the user).\n\
     ❌ Do NOT use xmtp_dispatch_user instead of xmtp_prompt_user (the user needs to make an ASP-choice decision; dispatch_user cannot relay).\n\n\
     [Your next actions (strict order)]\n\n\
     **Step 0 - idempotency check: query whether a pending decision already exists for this job:**\n\
     ```bash\n\
     onchainos agent pending-decisions list --format json --agent-id {agent_id}\n\
     ```\n\
     If the returned list already contains an entry with jobId={job_id} and role=buyer -> **the user has already been notified; this is a duplicate event - end the turn without notifying again.**\n\
     If not present -> continue to Step 1.\n\n\
     **Step 1 - fetch the pending-contact ASP list:**\n\
     Call the xmtp_get_pending_list tool to fetch the pending-contact ASP list.\n\
     ⚠️ Before the call, print: `[buyer-xmtp] xmtp_get_pending_list`\n\
     ⚠️ After the call, print: `[buyer-xmtp] xmtp_get_pending_list result: <returned value>`\n\n\
     If the result is an empty list -> call xmtp_dispatch_user:\n\
     \x20\x20content: There are no ASPs to contact right now. You can wait for new ASPs to reach out, or reply \"close\" to close the task.\n\
     Then finish.\n\n\
     **Step 2 - call xmtp_prompt_user to show all pending ASPs and let the user choose:**\n\
     🛑 **You MUST wait for the user's choice**; you may not decide for them.\n\
     Call `session_status` first to get this sub session's sessionKey; **before** calling `xmtp_prompt_user`, call `pending-decisions add` (see hard rule 7).\n\
     List **every** ASP in the pending list one by one for the user to pick from:\n\
     \x20\x20llmContent: [USER_DECISION_REQUEST][sub_key: <full sessionKey from session_status>][job: {job_id}][role: buyer]\n\
     \x20\x20Index -> AgentID mapping: <extract from the pending list, e.g. 1->798, 2->806, 3->866>\n\
     \x20\x20Routing rules:\n\
     \x20\x20- User picks an ASP (number or AgentID) -> map number -> AgentID, then call xmtp_dispatch_session(sessionKey=\"<full sessionKey from session_status>\", content=\"[USER_DECISION_RELAY][intent:PICK_PROVIDER index=<N> agentId=<mapped AgentID>] User reply verbatim: <user reply>\")\n\
     \x20\x20- User says \"skip all / none / skip all / none\" -> xmtp_dispatch_session(sessionKey=\"<same>\", content=\"[USER_DECISION_RELAY][intent:SKIP_ALL_PROVIDERS] User reply verbatim: <user reply>\")\n\
     \x20\x20⚠️ The intent tag must be inserted verbatim in ALL-CAPS ASCII; never translate or rewrite. Relay must use xmtp_dispatch_session. The user session agent must NOT create groups or execute task CLI itself. {CONSTRAINT}\n\
     \x20\x20⚠️ The mapping table must be extracted line-by-line from the pending list and inlined into llmContent - the user session agent cannot see userContent, and without the mapping table it cannot turn an index into an AgentID.\n\
     \x20\x20userContent:\n\
     \x20\x20[Job {short_id} - you are the User Agent] The following ASPs have reached out. Pick one to start negotiating:\n\
     \x20\x20\n\
     \x20\x20[iterate pending list; format per ASP:]\n\
     \x20\x20<N>. agentId: <agentId> | name: <name> | credit: <creditScore> | completed jobs: <completedTaskCount>\n\
     \x20\x20\n\
     \x20\x20Reply with the ASP's number to start, or reply \"skip all\".\n\n\
     **Step 3 - on receiving `[USER_DECISION_RELAY][intent:CODE] User reply verbatim: ...`, route by intent code:**\n\n\
     ━━━━━━━━━ Branch A: `[intent:PICK_PROVIDER index=<N> agentId=<X>]` -> establish session, then negotiate ━━━━━━━━━\n\n\
     A-Step 1: read `agentId=<X>` directly from the intent tag; call xmtp_start_conversation to create the group + the sub session:\n\
     \x20\x20Args: myAgentId={agent_id}, toAgentId=<agentId from the tag>, jobId={job_id}\n\
     \x20\x20⚠️ Before the call, print: `[buyer-xmtp] xmtp_start_conversation: myAgentId={agent_id}, toAgentId=<agentId>, jobId={job_id}`\n\
     \x20\x20⚠️ After the call, print: `[buyer-xmtp] xmtp_start_conversation result: sessionKey=<returned value>, xmtpGroupId=<returned value>`\n\n\
     🛑 **Within the same turn after creating the group you MUST call `xmtp_send` to send the first message** - creating the group only opens the channel; not sending a message = the ASP receives no signal = the flow stalls.\n\
     ❌ Absolutely forbidden: creating the group and ending the turn without sending a message.\n\n\
     A-Step 2: once the group is created you are inside the sub session; call xmtp_send to start negotiating with the ASP (refer to buyer.md 3.2 negotiation three-step handshake):\n\
     \x20\x20⚠️ **Do NOT** use xmtp_dispatch_user / xmtp_dispatch_session; after the group is created use xmtp_send uniformly.\n\
     \x20\x20content: Hi, I have a job (jobId: {job_id}) - are you interested in taking it on?\n\n\
     A-Step 3: negotiation success -> ASP applies on-chain -> wait for the ASP's XMTP message announcing the apply (buyer.md routing #2 triggers confirm-accept).\n\n\
     A-Step 4: negotiation failure (ASP rejects / timeout / terms mismatch) -> jump to Branch B.\n\n\
     ━━━━━━━━━ Branch B: user rejects current ASP / negotiation failed -> reject and return to the list ━━━━━━━━━\n\n\
     B-Step 1: call xmtp_deny_pending_conversation to reject this ASP:\n\
     \x20\x20Args: agentId=<rejected ASP's agentId>, jobId={job_id}\n\
     \x20\x20⚠️ Before the call, print: `[buyer-xmtp] xmtp_deny_pending_conversation: agentId=<agentId>, jobId={job_id}`\n\n\
     B-Step 2: call xmtp_get_pending_list again to refresh the pending list.\n\n\
     B-Step 3: if the list is non-empty -> go back to Step 2 and show the remaining ASPs to the user.\n\n\
     B-Step 4: if the list is empty -> call xmtp_dispatch_user to notify the user:\n\
     \x20\x20content: {no_sellers}\n\n\
     [Loop termination conditions] xmtp_get_pending_list returns an empty list, OR negotiation succeeds and enters Scene 6.\n",
     CONSTRAINT = super::flow::PROMPT_USER_SESSION_CONSTRAINT)

}

pub(super) fn job_visibility_changed(ctx: &FlowContext<'_>) -> String {
    let job_id = ctx.job_id;
    let title_display = ctx.title_display;
    let title_query_hint = ctx.title_query_hint;

    let visibility_public = super::content::visibility_public_user_notify(job_id, title_display);
    let visibility_private = super::content::visibility_private_user_notify(job_id, title_display);
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
     \x20\x20- visibility=1 -> {visibility_private}\n\n\
     ⚠️ After switching to public, do **NOT** request the recommended ASP list (recommend); the user just waits for ASPs to reach out.\n\
     -> **end this turn**.\n"
    )
}

pub(super) fn job_payment_mode_changed(ctx: &FlowContext<'_>) -> String {
    let job_id = ctx.job_id;
    let agent_id = ctx.agent_id;
    let title_display = ctx.title_display;
    let title_query_hint = ctx.title_query_hint;

    let payment_escrow_notify = super::content::payment_mode_escrow_user_notify(job_id, title_display);
    let x402_paying = super::content::x402_paying_user_notify(job_id, title_display);
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
     **x402 stage 1.5 - notify the user that payment is in progress (before task-402-pay):**\n\
     Call xmtp_dispatch_user:\n\
     \x20\x20content: {x402_paying}\n\n\
     **x402 stage 2 - sign + direct/accept + endpoint replay (atomic command):**\n\
     ```bash\n\
     onchainos agent task-402-pay {job_id} --provider-agent-id <providerAgentId> --accepts '<acceptsJson>' --endpoint <endpoint URL> --token-symbol <feeTokenSymbol> --token-amount <feeAmount>\n\
     ```\n\
     Internally executes: x402_pay signing -> direct/accept on-chain -> assemble payment header -> replay endpoint.\n\
     Output: {{ replaySuccess, replayStatus, replayBody, signature, authorization, sessionCert, txHash, userNotify }}\n\n\
     **x402 stage 2 Step 3 - check replay result and notify the user:**\n\
     🛑 **MANDATORY -- use the `userNotify` field from CLI output directly**: task-402-pay has pre-formatted the deliverable/error into the `userNotify` field.\n\
     **Immediately** call xmtp_dispatch_user with content = the `userNotify` field value (pass through as-is; do not rewrite, summarize, or omit deliverable content).\n\
     ❌ Do not compose your own notification instead of `userNotify` -- self-composed notifications lose the deliverable content (replayBody) and the user cannot see the actual data the ASP returned.\n\
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

pub(super) fn negotiate_reply(ctx: &FlowContext<'_>) -> String {
    let job_id = ctx.job_id;
    let agent_id = ctx.agent_id;
    let short_id = ctx.short_id;
    let title_query_hint = ctx.title_query_hint;

    let over_budget = super::content::over_budget_user_prompt(short_id);
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
     🚫 **Negotiation-autonomy red line**: except for the \"quote > max_budget\" auto-REJECT path below, do NOT call **any** user-facing tool (`xmtp_dispatch_user` / `xmtp_prompt_user` / `pending-decisions add`) to make the user decide on negotiation. Negotiation is autonomous in the sub session - evaluate via the decision matrix and reply directly to the ASP (natural-language discussion / [intent:propose]); do NOT forward the quote to the user asking \"do you accept?\" or \"please confirm\".\n\
     🔴 Real incident: model correctly called next-action but then used `xmtp_dispatch_user` (instead of forbidden `xmtp_prompt_user`) to forward the quote to the user — `xmtp_dispatch_user` is equally forbidden for this purpose.\n\n\
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
     \x20\x20c) Call `session_status` for the sessionKey; call `pending-decisions add` (see hard rule 7); call `xmtp_prompt_user` to let the user decide:\n\
     \x20\x20\x20\x20llmContent: [USER_DECISION_REQUEST][sub_key: <sessionKey>][job: {job_id}][role: buyer] If the user's intent is \"option A / view recommendations\" -> call xmtp_dispatch_session(sessionKey=\"<sessionKey>\", content=\"[USER_DECISION_RELAY][intent:VIEW_RECOMMEND] User reply verbatim: <user reply>\"); \"option B / specify ASP\" and provides an agentId -> call xmtp_dispatch_session(sessionKey=\"<sessionKey>\", content=\"[USER_DECISION_RELAY][intent:PICK_PROVIDER agentId=<agentId provided by user>] User reply verbatim: <user reply>\"); \"option C / close\" -> call xmtp_dispatch_session(sessionKey=\"<sessionKey>\", content=\"[USER_DECISION_RELAY][intent:CLOSE_TASK] User reply verbatim: <user reply>\"). ⚠️ Routing-tag protocol: intent names verbatim in ALL-CAPS ASCII; never translate or rewrite. ⚠️ Relay must use xmtp_dispatch_session. The user session agent must NOT execute task CLI itself. {CONSTRAINT}\n\
     \x20\x20\x20\x20userContent:\n\
     {over_budget}\n\
     \x20\x20\x20\x20-> **end this turn** and after the user's reply is relayed back: A -> `recommend`; B -> `next-action --provider <agentId>`; C -> `close`.\n\n\
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
     -> **end this turn** and wait for the ASP's reply.\n",
     CONSTRAINT = super::flow::PROMPT_USER_SESSION_CONSTRAINT)
}

pub(super) fn negotiate_ack(ctx: &FlowContext<'_>) -> String {
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

pub(super) fn negotiate_counter(ctx: &FlowContext<'_>) -> String {
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
     \x20\x20call xmtp_prompt_user to let the user decide (same as negotiate_reply over-budget handling: A. view recommendations / B. specify ASP / C. close).\n\
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
     \x20\x20| > max_budget | xmtp_send `[intent:reject]`, mark-failed, xmtp_prompt_user to let the user decide (same as the over-budget handling in negotiate_reply) |\n\n\
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
