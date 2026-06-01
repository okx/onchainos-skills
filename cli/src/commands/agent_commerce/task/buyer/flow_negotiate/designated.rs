//! Designated-provider D-Step routing and B-Step negotiation protocol.

/// Designated-provider D-Step routing (service-list query -> x402 or A2A branch entry)
pub(crate) fn designated_provider_d_steps(job_id: &str, agent_id: &str, short_id: &str, dp_id: &str, title_display: &str) -> String {
    let l10n_prompt = super::super::flow::L10N_PROMPT;
    let session_hint = super::super::flow::SESSION_STATUS_HINT;
    let follow_playbook = super::super::flow::FOLLOW_PLAYBOOK;
    let follow_playbook_short = super::super::flow::FOLLOW_PLAYBOOK_SHORT;
    let route_hint = super::super::flow::ROUTE_VIA_ENVELOPE;
    let title = title_display;
    let cmd_offline = super::super::flow::pending_cmd(job_id, agent_id, &format!("[Offline {short_id}] {title} next-step decision"), "provider_offline");
    let cmd_x402_invalid = super::super::flow::pending_cmd(job_id, agent_id, &format!("[x402 invalid {short_id}] {title} next-step decision"), "x402_invalid");
    let cmd_x402_price = super::super::flow::pending_cmd(job_id, agent_id, &format!("[x402 price {short_id}] {title} price decision"), "x402_price_mismatch");
    let cmd_over_budget = super::super::flow::pending_cmd(job_id, agent_id, &format!("[Over budget {short_id}] {title} budget decision"), "over_budget");
    let provider_offline = super::super::content::provider_offline_user_prompt(job_id, short_id, dp_id);
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
             ⚠️ This is the **ASP's** profile — the `role` field in the response belongs to the ASP, **NOT to you**. Do NOT use it to determine your own role. You are the **buyer** (`--role buyer`). Only read `onlineStatus` from this response; ignore all other fields.\n\
             Read `onlineStatus` from the response (1=online / 2=offline). If the field is missing, null, or empty, treat the ASP as **online** (the backend may not yet return this field).\n\
             - `onlineStatus == 1` **or field missing/null/empty** (online / unknown) -> continue to D-Step 2.\n\
             - `onlineStatus == 2` AND **no endpoint** (so you are about to enter the escrow negotiation path) -> the ASP is offline and cannot negotiate.\n\
             \x20\x20Enqueue the user decision via `pending-decisions-v2 request`:\n\
             \x20\x20{session_hint}\n\
             \x20\x20```bash\n\
             \x20\x20{cmd_offline}\n\
             \x20\x20```\n\
             \x20\x20`--user-content` template (canonical English; 🌐 localize per [Localization] rules):\n\
             \x20\x20{provider_offline}\n\
             \x20\x20🌐 **Localize `--user-content` AND `--list-label` per [Localization] rules** (rule 4: English → verbatim; rule 5: non-English → faithful translation).\n\
             \x20\x20{follow_playbook}\n\
             \x20\x20-> **end this turn** and wait for the user's reply.\n\
             \x20\x20{route_hint}\n\
             - `onlineStatus == 2` but **has an endpoint** (x402 path) -> x402 is automated payment and does not depend on the ASP being online in real time, so continue to D-Step 2.\n\n\
             **D-Step 2 - route by service-list result:**\n\
             - **Has services and contains an endpoint (x402-capable)** -> extract `feeAmount`, `feeTokenSymbol`, `endpoint` from `services[0]`.\n\
             \x20\x20⚠️ **`feeAmount` is the value the ASP manually entered at registration time, and is not necessarily equal to the on-chain price**; it must be verified by DX-Step 1 `x402-check`. When showing it to the user, label it \"registered fee\".\n\
             \x20\x20Execute the designated-provider x402 flow below (do NOT jump to A-Step 1):\n\n\
             \x20\x20**DX-Step 1 - validate the endpoint:**\n\
             \x20\x20```bash\n\
             \x20\x20onchainos agent x402-check --endpoint <endpoint> --agent-id {agent_id}\n\
             \x20\x20```\n\
             \x20\x20- `valid=false` -> enqueue the user decision via `pending-decisions-v2 request`:\n\
             \x20\x20\x20\x20{session_hint}\n\
             \x20\x20\x20\x20```bash\n\
             \x20\x20\x20\x20{cmd_x402_invalid}\n\
             \x20\x20\x20\x20```\n\
             \x20\x20\x20\x20`--user-content` template (canonical English; 🌐 localize per [Localization] rules):\n\
             \x20\x20\x20\x20[Job {short_id} — you are the User Agent] The x402 endpoint of the designated ASP (agentId={dp_id}) is invalid and cannot be used. Choose next step:\n\
             \x20\x20\x20\x20A. Specify another ASP — provide the agentId\n\
             \x20\x20\x20\x20B. Make the job public — let more ASPs discover it\n\
             \x20\x20\x20\x20C. Close the job\n\
             \x20\x20\x20\x20{l10n_prompt}\n\
             \x20\x20\x20\x20{follow_playbook}\n\
             \x20\x20\x20\x20-> **end this turn** and wait for the user's reply.\n\
             \x20\x20\x20\x20{route_hint}\n\n\
             \x20\x20**DX-Step 2 - amount sanity check:**\n\
             \x20\x20Compare `amountHuman` from x402-check with `feeAmount` from `services[0]`:\n\
             \x20\x20- Mismatch (delta > 1%) -> enqueue the user decision via `pending-decisions-v2 request`:\n\
             \x20\x20\x20\x20{session_hint}\n\
             \x20\x20\x20\x20```bash\n\
             \x20\x20\x20\x20{cmd_x402_price}\n\
             \x20\x20\x20\x20```\n\
             \x20\x20\x20\x20`--user-content` template (canonical English; 🌐 localize per [Localization] rules):\n\
             \x20\x20\x20\x20Job `{job_id}` — the specified ASP (agentId={dp_id}) actually charges <amountHuman> <tokenSymbol>, which differs from the registered fee <feeAmount> <feeTokenSymbol>. Accept this price?\n\
             \x20\x20\x20\x20A. Accept — continue with this price\n\
             \x20\x20\x20\x20B. Reject — switch to another ASP\n\
             \x20\x20\x20\x20{l10n_prompt}\n\
             \x20\x20\x20\x20{follow_playbook_short}\n\
             \x20\x20\x20\x20-> **end this turn** and wait for the user's reply.\n\
             \x20\x20\x20\x20{route_hint}\n\
             \x20\x20- Match -> continue to DX-Step 3.\n\n\
             \x20\x20**DX-Step 3 - budget check:**\n\
             \x20\x20First call `onchainos agent common context {job_id} --role buyer --agent-id {agent_id}` and extract `paymentMostTokenAmount` (max budget) and the task's `tokenSymbol`.\n\
             \x20\x20⚠️ **Currency check**: compare `tokenSymbol` from x402-check with the task's `tokenSymbol` -\n\
             \x20\x20- Mismatch (e.g. job in USDG, x402 charges USDT) -> since USDT and USDG are both USD stablecoins (~1:1), still compare numerically against the budget.\n\
             \x20\x20\x20\x20`set-payment-mode` will switch the on-chain payment token to **the x402 endpoint's token** (no longer the token used at job creation).\n\
             \x20\x20- Match -> compare directly.\n\
             \x20\x20Compare `amountHuman` with `paymentMostTokenAmount` (**NOT `tokenAmount`; `tokenAmount` is the base budget**):\n\
             \x20\x20- Over -> enqueue the user decision via `pending-decisions-v2 request`:\n\
             \x20\x20\x20\x20{session_hint}\n\
             \x20\x20\x20\x20```bash\n\
             \x20\x20\x20\x20{cmd_over_budget}\n\
             \x20\x20\x20\x20```\n\
             \x20\x20\x20\x20`--user-content` template (canonical English; 🌐 localize per [Localization] rules):\n\
             \x20\x20\x20\x20[Job {short_id} — you are the User Agent] The x402 fee from the designated ASP (agentId={dp_id}) is <amountHuman> <tokenSymbol>, which exceeds your max budget and cannot be used. Choose next step:\n\
             \x20\x20\x20\x20A. Specify another ASP — provide the ASP's agentId\n\
             \x20\x20\x20\x20B. Make the job public — let more ASPs discover it\n\
             \x20\x20\x20\x20C. Close the job\n\
             \x20\x20\x20\x20{l10n_prompt}\n\
             \x20\x20\x20\x20{follow_playbook}\n\
             \x20\x20\x20\x20-> **end this turn** and wait for the user's reply.\n\
             \x20\x20\x20\x20{route_hint}\n\
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
             \x20\x20\x20\x20call `onchainos agent next-action --jobid {job_id} --event job_payment_mode_changed --role buyer --agentId {agent_id}` and follow the returned script (task-402-pay).\n\
             \x20\x20- Output contains `\"confirming\": true` (normal on-chain submission in flight) -> **end this turn** and wait for the `job_payment_mode_changed` system notification.\n\n\
             - **No service or no endpoint (no x402 support)** -> enter **B-Step 1** to create a chat and negotiate.")
}

/// Designated-provider B-Step negotiation protocol (three-step handshake + group creation + multi-round negotiation + persistence + fallback)
pub(crate) fn designated_provider_negotiate(job_id: &str, agent_id: &str, short_id: &str, dp_id: &str, title_display: &str) -> String {
    let l10n_prompt = super::super::flow::L10N_PROMPT;
    let session_hint = super::super::flow::SESSION_STATUS_HINT;
    let follow_playbook = super::super::flow::FOLLOW_PLAYBOOK;
    let route_hint = super::super::flow::ROUTE_VIA_ENVELOPE;
    let title = title_display;
    let cmd_no_asp = super::super::flow::pending_cmd(job_id, agent_id, &format!("[No ASP {short_id}] {title} next-step decision"), "no_asp_found");
    let attachment_file = super::super::content::attachment_file_to_seller(job_id);
    let fallback_cmd = format!("onchainos agent mark-failed {job_id} --provider {dp_id} && onchainos agent recommend {job_id} --agent-id {agent_id}");
    let fallback_lines = format!("First run `onchainos agent mark-failed {job_id} --provider {dp_id}` to flag the failure, then run `onchainos agent recommend {job_id} --agent-id {agent_id}` to fetch a fresh recommendation list.\n\
             \x20\x20If the list is non-empty -> show it to the user via `pending-decisions-v2 request` (same format as the non-designated Step 2: list each ASP's info + pick/next-page/public/close options).\n\
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
             Correct behavior: once negotiation aligns (after the ASP has replied and you have evaluated in Step 2.5), **strictly use** the `[intent:propose]` template (see B-Step 2 Step 4 below) so the handshake parser succeeds. **Even short negotiations must complete all three steps** - even if it's \"can do, original price OK, escrow OK\" three-liner, turn it into [intent:propose] and send it; never skip.\n\
             ⚠️ This rule applies to Step 4 onward — the **first message (Step 1) must always be pure natural language** with no `[intent:*]` markers.\n\n\
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
             🛑 **B-Step 1.5 - SKILL_PREFETCH (mandatory for new sub sessions):**\n\
             Immediately after xmtp_start_conversation returns, call `xmtp_dispatch_session` to pre-load the skill into the newly created sub session:\n\
             \x20\x20sessionKey = <the sessionKey just returned by xmtp_start_conversation>\n\
             \x20\x20content = `[SKILL_PREFETCH] Read the okx-agent-task skill. Pre-load buyer role context. This prefetch message itself requires no action — but when the NEXT inbound message arrives (same turn or later turn), you MUST process it normally via buyer.md §3 routing (#1–#6). Do NOT carry over \"no action\" to business messages.`\n\
             ❌ Do NOT skip this step — the sub session has no context yet; without SKILL_PREFETCH, the first inbound message will be processed without the buyer playbook loaded.\n\
             ⚠️ Do NOT use `xmtp_send` (that would be visible to the ASP). Use `xmtp_dispatch_session` only.\n\n\
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
             1. Call xmtp_send to send the first inquiry (**pure natural language** - let the ASP quote first, then judge):\n\
             \x20\x20content MUST include: job description, expected deliverable, paymentMode preference, budget (base budget).\n\
             \x20\x20content MUST NOT include:\n\
             \x20\x20\x20\x20❌ max_budget / paymentMostTokenAmount / \"最高\" / \"上限\" / \"cap\" / \"maximum\" / \"max\" budget value\n\
             \x20\x20\x20\x20❌ Any number that equals the max_budget value (even without labeling it as such)\n\
             \x20\x20🔴 Real incident: the model included \"最高 0.1 USDT\" in the first inquiry — the ASP immediately quoted 0.1 USDT (the cap), and the user lost all bargaining leverage.\n\
             \x20\x20🛑 The first message MUST be natural language only. Do NOT include `[intent:propose]` or any `[intent:*]` marker — propose is only allowed in Step 4, after the ASP has replied and evaluation (Step 2.5) is complete.\n\
             \x20\x20⚠️ `[intent:propose]` is ALWAYS sent by the buyer (you), NEVER by the ASP. Do NOT ask or instruct the ASP to send `[intent:propose]`.\n\
             \x20\x20-> after sending the first inquiry, proceed to step 1.5 before waiting for the reply.\n\n\
             1.5. **Upload pending attachments (if any)**:\n\
             \x20\x20```bash\n\
             \x20\x20onchainos agent list-attachments {job_id}\n\
             \x20\x20```\n\
             \x20\x20If the output is a non-empty JSON array, iterate over each file path:\n\
             \x20\x20a) `xmtp_file_upload` (filePath=<path>, agentId={agent_id}, jobId={job_id}) → obtain fileKey + 5 decryption-metadata fields (digest/salt/nonce/secret/filename).\n\
             \x20\x20b) `xmtp_send` to the provider with the following content (paste all 6 fields verbatim from xmtp_file_upload):\n\
             \x20\x20{attachment_file}\n\
             \x20\x20⚠️ **Attachment upload failure MUST NOT block the negotiation flow**: if `xmtp_file_upload` fails for any file, skip that file and continue. The negotiation is the critical path; attachment forwarding is best-effort.\n\
             \x20\x20If empty (`[]`) or no attachments were found in the earlier attachment check, skip this step.\n\
             \x20\x20🛑🛑🛑 **MANDATORY — end this turn now.** After the first inquiry (step 1) and attachments (step 1.5) are sent, you **MUST end this turn immediately**.\n\
             \x20\x20The ASP's reply will arrive at the **sub session** (the group created in B-Step 1) as an inbound a2a-agent-chat message; the sub session handles it via buyer.md §3 routing (#6 fallback → `negotiate_reply`).\n\
             \x20\x20❌ Do NOT call `xmtp_get_conversation_history` to poll for the ASP's reply in this turn.\n\
             \x20\x20❌ Do NOT continue to Step 2 / 2.5 / 3 / 4 in this turn — those are executed by the **sub session** when it receives the reply.\n\
             \x20\x20🔴 Real incident: backup session sent the first inquiry, then polled `xmtp_get_conversation_history` in the same turn, saw the ASP's quote, evaluated it, and sent `[intent:propose]` — all from the backup. The sub session had no negotiation context and could not handle subsequent events (ACK / COUNTER / payment-mode-changed).\n\n\
             ━━━━━━━━━ Steps 2–6 below: sub session reference (executed by the sub session, NOT the backup) ━━━━━━━━━\n\n\
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
             \x20\x20call `onchainos agent next-action --jobid {job_id} --event job_payment_mode_changed --role buyer --agentId {agent_id}` and follow the returned script (xmtp_send [intent:confirm]).\n\
             - Output contains `\"confirming\": true` (normal on-chain submission in flight) -> continue to Step 6.3.\n\
             ⚠️ **NEVER** xmtp_send [intent:confirm] while the on-chain call is still confirming - the ASP would apply on seeing [intent:confirm], but the on-chain paymentMode is still in the mempool / unconfirmed, so apply would fail or behave inconsistently. [intent:confirm] must only be sent after the `job_payment_mode_changed` event confirms paymentMode on-chain.\n\n\
             **Step 6.3 - executed only when `confirming`: end this turn** and wait for the `job_payment_mode_changed` system notification.\n\n\
             (New turn) On receiving `job_payment_mode_changed` -> call next-action --event job_payment_mode_changed -> per script, xmtp_send [intent:confirm] to the ASP. The ASP sees CONFIRM -> apply (escrow); on-chain paymentMode is already in place.\n\n\
             ━━━━━━━━━ Negotiation failed / switching ASP ━━━━━━━━━\n\n\
             Current ASP timed out (5 min) / COUNTER rounds exceeded (>=3) / received `[intent:reject]` / negotiation failed -> first xmtp_send `[intent:reject]` (reason: timeout / round limit / failure cause) to the ASP, then switch:\n\
             \x20\x20{fallback_lines}\n\
             ⚠️ **When switching you MUST first send [intent:reject] before switching away** (so the ASP has a clear termination signal), but **do NOT xmtp_delete_conversation**. After switching, ignore any further messages from that ASP.\n\
             No ASPs left on the current page and pagination also returns nothing -> enqueue the user decision via `pending-decisions-v2 request`:\n\
             \x20\x20{session_hint}\n\
             \x20\x20```bash\n\
             \x20\x20{cmd_no_asp}\n\
             \x20\x20```\n\
             \x20\x20`--user-content` template (canonical English; 🌐 localize per [Localization] rules):\n\
             \x20\x20[Job {short_id} — you are the User Agent] None of the recommended ASPs are a fit. Choose next step:\n\
             \x20\x20A. Specify an ASP — provide the ASP's agentId\n\
             \x20\x20B. Make the job public — let more ASPs discover it\n\
             \x20\x20C. Close the job — cancel and refund\n\
             \x20\x20{l10n_prompt}\n\
             \x20\x20{follow_playbook}\n\
             \x20\x20-> **end this turn** and resume execution once the user's reply is relayed back.\n\
             \x20\x20{route_hint}\n\n\
             [Subsequent events]\n\
             - x402 -> set-payment-mode -> job_payment_mode_changed -> task-402-pay (sign + direct/accept + endpoint replay) -> job_accepted -> complete\n\
             - escrow -> set-payment-mode -> job_payment_mode_changed -> notify ASP to apply -> ASP applies on-chain -> ASP xmtp_send notifies user -> user receives a2a-agent-chat -> confirm-accept -> job_accepted\n")
}
