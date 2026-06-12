//! Designated-provider D-Step routing and B-Step negotiation protocol.
//!
//! The full flow is split into two phases to reduce playbook output size:
//!   Phase 1 (`route_only`): call `designated-route` → determine route → call next-action with the matching pseudo-event
//!   Phase 2 (`branch_a2a` / `branch_x402` / `branch_error`): only the hit branch's playbook

/// Three-step handshake rules — static text shared by every A2A negotiation
/// path (both `branch_a2a` and `branch_a2a_cli`). No format args here.
const HANDSHAKE_RULES_A2A: &str = "🛑 **Hard constraint - the three-step handshake is the ONLY legitimate path to get the ASP to apply**\n\n\
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
    ⚠️ This rule applies to Step 4 onward — the **first message (Step 1) must always be pure natural language** with no `[intent:*]` markers.";

/// Branch B title + B-Step 0 (duplicate guard) + B-Step 1 (group creation) +
/// B-Step 1.5 (SKILL_PREFETCH). Used by the MCP path (`branch_a2a` →
/// `designated_provider_negotiate`); the CLI path (`branch_a2a_cli`) skips
/// this section because Rust already executes those three steps inline.
fn negotiate_section_pre_inquiry(job_id: &str, agent_id: &str, dp_id: &str) -> String {
    format!("━━━━━━━━━ Branch B: supportA2MCP=false -> A2A (negotiation required) ━━━━━━━━━\n\n\
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
             ⚠️ Do NOT use `xmtp_send` (that would be visible to the ASP). Use `xmtp_dispatch_session` only.")
}

/// B-Step 2 (automated negotiation) and everything after — first inquiry,
/// negotiation goals / timeouts / format rules, attachment upload, end-turn
/// hand-off to the sub session, and a summary of subsequent events.
/// Shared by `designated_provider_negotiate` (full MCP path) and
/// `branch_a2a_cli` (CLI path that already ran B-Step 0 / 1 / 1.5 in Rust).
///
/// `attachments_handled_in_rust = true` swaps the step 1.5 instructions to a
/// no-op marker (used by `branch_a2a_cli`, which uploads + forwards
/// attachments inline before emitting this playbook). When `false`, the
/// full LLM-driven step 1.5 instructions are emitted (the original MCP path).
/// Step-1-only playbook for the CLI path (`branch_a2a_cli`). The sub session
/// has already been created and SKILL_PREFETCH dispatched by Rust before this
/// function runs; attachments (if any) are uploaded + forwarded by Rust too.
/// All that's left for the LLM in this turn is: author one natural-language
/// inquiry, send it, end the turn. Subsequent steps (handshake / ACK / counter
/// / confirm) are driven by the sub session's own `next-action` calls when
/// reply events arrive — they do not belong in this output.
pub(crate) fn negotiate_section_step1_only_cli(
    job_id: &str,
    my_agent_id: &str,
    to_agent_id: &str,
    prefetched: Option<&crate::commands::agent_commerce::task::common::PreFetchedTaskContext>,
) -> String {
    // Inline the task fields the LLM needs to compose the inquiry — saves a
    // `common context` round-trip. NEVER inline max_budget; that's the whole
    // point of "do not leak it to the ASP". Description is the source of
    // truth for both task body and expected deliverable.
    let task_block = match prefetched {
        Some(p) => {
            let desc = if p.description.is_empty() {
                "(missing — run `onchainos agent common context` if needed)".to_string()
            } else {
                p.description.clone()
            };
            let amt = if p.token_amount.is_empty() { "?" } else { p.token_amount.as_str() };
            format!(
                "**Task fields (already fetched — use these verbatim, do NOT call `common context`):**\n\
                 \x20\x20• Title: {title}\n\
                 \x20\x20• Description / expected deliverable: {desc}\n\
                 \x20\x20• Base budget: {amt} {sym}  (this is the value to mention; max_budget is intentionally withheld)\n\
                 \x20\x20• Payment mode: escrow (fixed on the A2A path)\n\n",
                title = p.title,
                sym = p.token_symbol,
            )
        }
        None => "**Task fields not pre-fetched.** Run `onchainos agent common context {job_id} --role buyer --agent-id <agentId>` first, extract title / description / tokenSymbol / tokenAmount, then proceed.\n\n".to_string(),
    };

    format!(
        "{task_block}\
         **Step 1 — First inquiry to the ASP. Compose a natural-language message in the user's language using the fields above, then run this bash exactly once:**\n\n\
         ```bash\n\
         okx-a2a xmtp-send \\\n\
         \x20\x20--job-id {job_id} \\\n\
         \x20\x20--my-agent-id {my_agent_id} \\\n\
         \x20\x20--to-agent-id {to_agent_id} \\\n\
         \x20\x20--message '<your composed inquiry — see rules below>' \\\n\
         \x20\x20--json\n\
         ```\n\n\
         🛑 **Content iron rules — violation = leak or protocol break:**\n\
         \x20\x20❌ Do NOT include max_budget / paymentMostTokenAmount / \"最高\" / \"上限\" / \"cap\" / \"maximum\" / any value equal to max_budget — the Rust layer deliberately did not give it to you.\n\
         \x20\x20❌ Do NOT append `[intent:propose]` / `[intent:confirm]` / `[intent:reject]` / any `[intent:*]` marker — Step 1 is pure natural language; markers are reserved for Step 4 onward (sub session handles).\n\
         \x20\x20❌ Do NOT promise terms or accept a price — ask the ASP to quote.\n\n\
         🛑🛑🛑 **End this turn immediately after the command returns.** The ASP's reply will arrive at the sub session and trigger `next-action --event negotiate_reply` automatically. Do NOT poll, do NOT continue to handshake / evaluation / propose / confirm.\n"
    )
}

pub(crate) fn negotiate_section_step2_onwards(
    job_id: &str,
    agent_id: &str,
    attachment_file: &str,
    fallback_cmd: &str,
    attachments_handled_in_rust: bool,
) -> String {
    let step_1_5_block = if attachments_handled_in_rust {
        "1.5. **Attachments**: ✅ already uploaded and forwarded to the ASP by Rust before this playbook was emitted. Do NOT call `onchainos agent list-attachments`, `xmtp_file_upload`, or `xmtp_send [intent:attachment]` again — they're done.".to_string()
    } else {
        format!(
            "1.5. **Upload pending attachments (if any)**:\n\
             \x20\x20```bash\n\
             \x20\x20onchainos agent list-attachments {job_id}\n\
             \x20\x20```\n\
             \x20\x20If the output is a non-empty JSON array, iterate over each file path:\n\
             \x20\x20a) `xmtp_file_upload` (filePath=<path>, agentId={agent_id}, jobId={job_id}) → obtain fileKey + 5 decryption-metadata fields (digest/salt/nonce/secret/filename).\n\
             \x20\x20b) `xmtp_send` to the provider with the following content (paste all 6 fields verbatim from xmtp_file_upload):\n\
             \x20\x20{attachment_file}\n\
             \x20\x20⚠️ **Attachment upload failure MUST NOT block the negotiation flow**: if `xmtp_file_upload` fails for any file, skip that file and continue. The negotiation is the critical path; attachment forwarding is best-effort.\n\
             \x20\x20If empty (`[]`) or no attachments were found in the earlier attachment check, skip this step."
        )
    };
    format!("**B-Step 2 - automated negotiation (User Agent <-> ASP Agent multi-turn interaction in the sub session):**\n\
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
             {step_1_5_block}\n\
             \x20\x20🛑🛑🛑 **MANDATORY — end this turn now.** After the first inquiry (step 1) and attachments (step 1.5) are sent, you **MUST end this turn immediately**.\n\
             \x20\x20The ASP's reply will arrive at the **sub session** (the group created in B-Step 1) as an inbound a2a-agent-chat message; the sub session handles it via buyer.md §3 routing (#6 fallback → `negotiate_reply`).\n\
             \x20\x20❌ Do NOT call `xmtp_get_conversation_history` to poll for the ASP's reply in this turn.\n\
             \x20\x20❌ Do NOT continue to Step 2 / 2.5 / 3 / 4 in this turn — those are executed by the **sub session** when it receives the reply.\n\
             \x20\x20🔴 Real incident: backup session sent the first inquiry, then polled `xmtp_get_conversation_history` in the same turn, saw the ASP's quote, evaluated it, and sent `[intent:propose]` — all from the backup. The sub session had no negotiation context and could not handle subsequent events (ACK / COUNTER / payment-mode-changed).\n\n\
             ━━━━━━━━━ Sub session negotiation (handled by next-action, NOT by this output) ━━━━━━━━━\n\n\
             After the first inquiry (step 1 + 1.5) and this turn ends, the ASP's reply arrives at the **sub session**.\n\
             The sub session calls `onchainos agent next-action` with the matching event (`negotiate_reply` / `negotiate_ack` / `negotiate_counter` / `job_payment_mode_changed`) and follows the returned playbook.\n\
             **You (backup/user session) do NOT execute any further negotiation steps in this turn.**\n\n\
             ⚠️ When negotiation fails (timeout / [intent:reject] / round limit), the sub session sends `[intent:reject]` and runs `{fallback_cmd}` to switch. Do NOT call `xmtp_delete_conversation` when switching.\n\n\
             [Subsequent events]\n\
             - escrow -> set-payment-mode -> job_payment_mode_changed -> [intent:confirm] -> ASP apply -> confirm-accept -> job_accepted\n\
             - x402 -> set-payment-mode -> job_payment_mode_changed -> task-402-pay -> job_accepted -> complete\n")
}

/// Designated-provider B-Step negotiation protocol (three-step handshake + group creation + first inquiry + end turn).
/// Composed of three reusable sections so the CLI path can skip the
/// pre-inquiry portion (Rust runs B-Step 0 / 1 / 1.5 inline).
pub(crate) fn designated_provider_negotiate(job_id: &str, agent_id: &str, short_id: &str, dp_id: &str, title_display: &str) -> String {
    let _ = (short_id, title_display);
    let attachment_file = super::super::content::attachment_file_to_seller(job_id);
    let fallback_cmd = format!("onchainos agent mark-failed {job_id} --provider {dp_id} && onchainos agent recommend {job_id} --agent-id {agent_id}");
    let pre_inquiry = negotiate_section_pre_inquiry(job_id, agent_id, dp_id);
    let step2 = negotiate_section_step2_onwards(job_id, agent_id, &attachment_file, &fallback_cmd, false);
    format!("{HANDSHAKE_RULES_A2A}\n\n{pre_inquiry}\n\n{step2}")
}

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
             **D-Step 1 — query ASP route (service-list + profile combined):**\n\
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
             onchainos agent next-action --jobid {job_id} --event <from table above> --role buyer --agentId {agent_id} --provider {dp_id}\n\
             ```\n\
             🛑 **Do NOT execute any D-Step / B-Step / DX-Step in this turn** — the next-action call above returns the matching branch playbook. Follow it verbatim.\n\
             🛑 Do NOT create groups, send messages, or call set-payment-mode before getting the branch playbook.\n\n\
             **End this turn after executing the branch playbook returned by next-action.**\n")
}

/// CLI-mode variant of `branch_a2a`. Inlines the three LLM-driven MCP tool
/// calls that begin the A2A negotiation flow:
///   - B-Step 0  (duplicate guard via `session_status`)  → okx_a2a::session_query_exists
///   - B-Step 1  (xmtp_start_conversation)               → okx_a2a::session_create
///   - B-Step 1.5 (SKILL_PREFETCH xmtp_dispatch_session) → okx_a2a::session_send
/// Everything from B-Step 2 onward (first inquiry, three-step handshake,
/// timeouts) requires the LLM to author natural-language content and remains
/// in the returned playbook.
pub(crate) fn branch_a2a_cli(
    job_id: &str,
    agent_id: &str,
    short_id: &str,
    dp_id: &str,
    title_display: &str,
    prefetched: Option<&crate::commands::agent_commerce::task::common::PreFetchedTaskContext>,
) -> String {
    use crate::commands::agent_commerce::task::common::okx_a2a;

    // B-Step 0 — duplicate guard: does this job already have a sub session
    // with this provider? If yes, the first inquiry was already sent in a
    // previous turn; bail out so we don't double-send.
    match okx_a2a::session_query_exists(job_id, agent_id, dp_id) {
        Ok(true) => return format!(
            "[Designated ASP route: A2A] Provider {dp_id}\n\n\
             🛑 Sub session already exists for this job; the first inquiry has already been sent in a prior turn. \
             End this turn immediately — do not create a group, do not send any message, do not call session_status / xmtp_start_conversation / xmtp_send.\n"
        ),
        Ok(false) => { /* fall through to create */ }
        Err(e) => return format!("[branch_a2a_cli] ERROR: okx-a2a session query failed: {e}\n"),
    }

    // B-Step 1 — create the sub session (group + session record). The CLI
    // helper returns the canonical sessionKey assembled from the three IDs;
    // we use it as <SUB_KEY> in the remaining playbook.
    let session_key = match okx_a2a::session_create(job_id, agent_id, dp_id) {
        Ok(sk) => sk,
        Err(e) => return format!("[branch_a2a_cli] ERROR: okx-a2a session create failed: {e}\n"),
    };

    // B-Step 1.5 — SKILL_PREFETCH: pre-load the buyer playbook into the
    // freshly created sub session so its first inbound message has the
    // correct context. Fire-and-forget (--no-wait baked into helper).
    let prefetch = "[SKILL_PREFETCH] Read the okx-agent-task skill. Pre-load buyer role context. This prefetch message itself requires no action — but when the NEXT inbound message arrives (same turn or later turn), you MUST process it normally via buyer.md §3 routing (#1–#6). Do NOT carry over \"no action\" to business messages.";
    if let Err(e) = okx_a2a::session_send(&session_key, prefetch) {
        return format!("[branch_a2a_cli] ERROR: okx-a2a session send (SKILL_PREFETCH) failed: {e}\n");
    }

    // B-Step 2 step 1.5 (attachments) — upload each pending attachment via
    // okx_a2a::file_upload, then forward the file metadata to the ASP via
    // okx_a2a::xmtp_send so the ASP can call file_download. Best-effort: a
    // per-file failure must not block the negotiation, so we log the outcome
    // in the prelude and continue.
    let attachment_paths = super::super::attachments::list_attachment_paths(job_id);
    let attachment_section = if attachment_paths.is_empty() {
        String::new()
    } else {
        for path in &attachment_paths {
            let display_name = std::path::Path::new(path)
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or(path.as_str());
            if let Ok(meta) = okx_a2a::file_upload(path, agent_id, job_id, Some(display_name), None) {
                let content = format!(
                    "jobId: {job_id}\n\
                     attachmentType: file\n\
                     fileKey: {file_key}\n\
                     digest: {digest}\n\
                     salt: {salt}\n\
                     nonce: {nonce}\n\
                     secret: {secret}\n\
                     filename: {filename}\n\
                     description: Attachment: {filename}\n\
                     [intent:attachment]",
                    file_key = meta.file_key,
                    digest = meta.digest,
                    salt = meta.salt,
                    nonce = meta.nonce,
                    secret = meta.secret,
                    filename = meta.filename,
                );
                // Best-effort: any upload/send failure is silently skipped —
                // attachment forwarding is not on the negotiation critical path.
                let _ = okx_a2a::xmtp_send(job_id, agent_id, dp_id, &content);
            }
        }
        "⚠️ Attachments already uploaded and forwarded by Rust — do NOT call `xmtp_file_upload`, `xmtp_send [intent:attachment]`, or `list-attachments`.\n\n".to_string()
    };

    // CLI-path negotiation playbook: only Step 1 (first inquiry) is left for
    // the LLM. Everything before it (group create + SKILL_PREFETCH +
    // attachments) ran in Rust above; everything after it (handshake / ACK /
    // counter / confirm) runs in the sub session via `next-action` when the
    // ASP's reply arrives. The three-step handshake rules (HANDSHAKE_RULES_A2A)
    // are omitted from this turn — they apply to Step 4 onward, which this
    // playbook never reaches.
    let _ = (short_id, title_display, session_key);
    let step1 = negotiate_section_step1_only_cli(job_id, agent_id, dp_id, prefetched);

    format!(
        "[Designated ASP route: A2A] Provider {dp_id} is online with escrow support.\n\
         [Role] User (Buyer)\n\
         {attachment_section}\
         {step1}\n"
    )
}

/// Phase 2a: A2A branch — group creation + negotiation protocol.
pub(crate) fn branch_a2a(job_id: &str, agent_id: &str, short_id: &str, dp_id: &str, title_display: &str) -> String {
    let attachment_paths = super::super::attachments::list_attachment_paths(job_id);
    let attachment_section = if attachment_paths.is_empty() {
        String::new()
    } else {
        let paths_list = attachment_paths.iter()
            .map(|p| format!("  - `{p}`"))
            .collect::<Vec<_>>()
            .join("\n");
        format!(
            "**Pre-step — 🛑 Pending local attachments (auto-detected, MUST upload after first xmtp_send):**\n\
             The following files are saved locally and MUST be uploaded to the provider **immediately after the first `xmtp_send`** in B-Step 2 step 1.5:\n\
             {paths_list}\n\
             ⚠️ Do NOT call `list-attachments` again — the paths above are authoritative.\n\
             ⚠️ For each file: `xmtp_file_upload` → `xmtp_send [intent:attachment]` (see step 1.5 template).\n\n"
        )
    };

    format!("\
         [Designated ASP route: A2A] Provider {dp_id} is online with escrow support.\n\
         [Role] User (Buyer)\n\n\
         {attachment_section}\
         {negotiate}\n",
        negotiate = designated_provider_negotiate(job_id, agent_id, short_id, dp_id, title_display),
    )
}

/// Phase 2b: x402 branch — endpoint validation + set-payment-mode.
pub(crate) fn branch_x402(job_id: &str, agent_id: &str, short_id: &str, dp_id: &str) -> String {
    let l10n_prompt = super::super::flow::L10N_PROMPT;
    let session_hint = super::super::flow::SESSION_STATUS_HINT;
    let follow_playbook = super::super::flow::FOLLOW_PLAYBOOK;
    let follow_playbook_short = super::super::flow::FOLLOW_PLAYBOOK_SHORT;
    let route_hint = super::super::flow::ROUTE_VIA_ENVELOPE;
    let cmd_x402_invalid = super::super::flow::pending_cmd(job_id, agent_id, &format!("[x402 invalid {short_id}] next-step decision"), "x402_invalid");
    let cmd_x402_price = super::super::flow::pending_cmd(job_id, agent_id, &format!("[x402 price {short_id}] price decision"), "x402_price_mismatch");
    let cmd_over_budget = super::super::flow::pending_cmd(job_id, agent_id, &format!("[Over budget {short_id}] budget decision"), "over_budget");

    format!("\
         [Designated ASP route: x402] Provider {dp_id} has an x402 endpoint.\n\
         [Role] User (Buyer)\n\n\
         **DX-Step 1 — validate endpoint + price + budget (single CLI call):**\n\
         ```bash\n\
         onchainos agent x402-validate --endpoint <endpoint from designated-route> --agent-id {agent_id} --job-id {job_id} --fee-amount <feeAmount> --fee-token <feeTokenSymbol>\n\
         ```\n\
         ⚠️ Use `feeAmount` and `feeTokenSymbol` from the `designated-route` response above (earlier in this turn).\n\
         Response field `result` determines the branch:\n\n\
         - **`result == \"x402_invalid\"`** -> enqueue the user decision via `pending-decisions-v2 request`:\n\
         \x20\x20{session_hint}\n\
         \x20\x20```bash\n\
         \x20\x20{cmd_x402_invalid}\n\
         \x20\x20```\n\
         \x20\x20{l10n_prompt}\n\
         \x20\x20`--user-content` template (canonical English):\n\
         \x20\x20[Job {short_id} — you are the User Agent] The x402 endpoint of the designated ASP (agentId={dp_id}) is invalid and cannot be used. Choose next step:\n\
         \x20\x20A. Specify another ASP — provide the agentId\n\
         \x20\x20B. Make the job public — let more ASPs discover it\n\
         \x20\x20C. Close the job\n\
         \x20\x20{follow_playbook}\n\
         \x20\x20-> **end this turn** and wait for the user's reply.\n\
         \x20\x20{route_hint}\n\n\
         - **`result == \"input_required\"`** -> the endpoint is a valid x402 service but requires business parameters to trigger the 402 payment challenge.\n\
         \x20\x20The response includes `fields` / `requiredAnyOf` describing what the endpoint needs.\n\
         \x20\x20**You MUST construct a JSON body from the task description:**\n\
         \x20\x20\x20\x201. Read the `fields` array from the response — each entry has `name`, `type`, and optionally `required`/`label`.\n\
         \x20\x20\x20\x202. Read `requiredAnyOf` — at least one of these fields must be present.\n\
         \x20\x20\x20\x203. Extract matching values from the **task description** (the user's original request). Map task description content to the field names.\n\
         \x20\x20\x20\x204. If you can fill the required fields, re-run x402-validate with `--body`:\n\
         \x20\x20\x20\x20```bash\n\
         \x20\x20\x20\x20onchainos agent x402-check --endpoint <endpoint> --agent-id {agent_id} --body '<constructed JSON>'\n\
         \x20\x20\x20\x20```\n\
         \x20\x20\x20\x20If the re-check returns `valid: true`, use its `acceptsJson`, `amountHuman`, `tokenSymbol` and proceed to **A-Step 3** (set-payment-mode).\n\
         \x20\x20\x20\x20⚠️ **Remember the constructed JSON body** — you must pass the same `--body` to `task-402-pay` later so the replay sends business parameters along with the payment header.\n\
         \x20\x20\x20\x205. If you cannot extract the required fields from the task description, enqueue a user decision asking them to provide the missing business parameters:\n\
         \x20\x20\x20\x20```bash\n\
         \x20\x20\x20\x20{cmd_x402_invalid}\n\
         \x20\x20\x20\x20```\n\
         \x20\x20\x20\x20`--user-content` template: [Job {short_id}] The x402 service requires business parameters (<list field names from response>) but they could not be extracted from the task description. Please provide them or choose: A. Retry with parameters / B. Switch ASP / C. Close the job.\n\
         \x20\x20{follow_playbook}\n\
         \x20\x20{route_hint}\n\n\
         - **`result == \"price_mismatch\"`** -> enqueue the user decision:\n\
         \x20\x20{session_hint}\n\
         \x20\x20```bash\n\
         \x20\x20{cmd_x402_price}\n\
         \x20\x20```\n\
         \x20\x20{l10n_prompt}\n\
         \x20\x20`--user-content` template (canonical English):\n\
         \x20\x20[Job {short_id} — you are the User Agent] The designated ASP (agentId={dp_id}) actually charges <amountHuman> <tokenSymbol>, which differs from the registered fee <feeAmount> <feeTokenSymbol>. Accept this price?\n\
         \x20\x20A. Accept — continue with this price\n\
         \x20\x20B. Reject — switch to another ASP\n\
         \x20\x20{follow_playbook_short}\n\
         \x20\x20-> **end this turn** and wait for the user's reply.\n\
         \x20\x20{route_hint}\n\n\
         - **`result == \"over_budget\"`** -> enqueue the user decision:\n\
         \x20\x20{session_hint}\n\
         \x20\x20```bash\n\
         \x20\x20{cmd_over_budget}\n\
         \x20\x20```\n\
         \x20\x20{l10n_prompt}\n\
         \x20\x20`--user-content` template (canonical English):\n\
         \x20\x20[Job {short_id} — you are the User Agent] The x402 fee from the designated ASP (agentId={dp_id}) is <amountHuman> <tokenSymbol>, which exceeds your max budget and cannot be used. Choose next step:\n\
         \x20\x20A. Specify another ASP — provide the ASP's agentId\n\
         \x20\x20B. Make the job public — let more ASPs discover it\n\
         \x20\x20C. Close the job\n\
         \x20\x20{follow_playbook}\n\
         \x20\x20-> **end this turn** and wait for the user's reply.\n\
         \x20\x20{route_hint}\n\n\
         - **`result == \"pass\"`** -> all checks passed. Execute **A-Step 3** below.\n\n\
         **A-Step 3 — set-payment-mode (push x402 on-chain):**\n\
         ```bash\n\
         onchainos agent set-payment-mode {job_id} --payment-mode x402 --token-symbol <tokenSymbol from x402-validate> --token-amount <amountHuman from x402-validate> --endpoint <endpoint>\n\
         ```\n\
         ⚠️ Use the **actual values returned by x402-validate** for `tokenSymbol` and `tokenAmount` (NOT the original budget used at job creation).\n\n\
         **A-Step 3 result branch (🛑 MANDATORY — getting this wrong = the flow stalls):**\n\
         Inspect the CLI output (JSON) of set-payment-mode:\n\
         - Output contains `\"alreadySet\": true` -> **do NOT wait for `job_payment_mode_changed`**;\n\
         \x20\x20call `onchainos agent next-action --jobid {job_id} --event job_payment_mode_changed --role buyer --agentId {agent_id}` immediately.\n\
         - Output contains `\"confirming\": true` -> **end this turn** and wait for `job_payment_mode_changed`.\n")
}

/// Phase 2c: error branch — not_provider or offline decision card.
pub(crate) fn branch_error(job_id: &str, agent_id: &str, short_id: &str, dp_id: &str) -> String {
    let session_hint = super::super::flow::SESSION_STATUS_HINT;
    let follow_playbook = super::super::flow::FOLLOW_PLAYBOOK;
    let route_hint = super::super::flow::ROUTE_VIA_ENVELOPE;
    let cmd_not_provider = super::super::flow::pending_cmd(job_id, agent_id, &format!("[Not ASP {short_id}] next-step decision"), "not_provider");
    let cmd_offline = super::super::flow::pending_cmd(job_id, agent_id, &format!("[Offline {short_id}] next-step decision"), "provider_offline");
    let cmd_endpoint_not_found = super::super::flow::pending_cmd(job_id, agent_id, &format!("[Endpoint gone {short_id}] next-step decision"), "endpoint_not_found");
    let not_provider = super::super::content::not_provider_user_prompt(job_id, short_id, dp_id);
    let provider_offline = super::super::content::provider_offline_user_prompt(job_id, short_id, dp_id);

    format!("\
         [Designated ASP route: error] Provider {dp_id} encountered a routing error.\n\
         [Role] User (Buyer)\n\n\
         **Branch by `errorType` from the `designated-route` response above (earlier in this turn):**\n\n\
         - **`errorType == \"endpoint_not_found\"`** -> the persisted endpoint no longer exists in the ASP's service list (the ASP may have removed or changed the service).\n\
         \x20\x20Enqueue the user decision via `pending-decisions-v2 request`:\n\
         \x20\x20{session_hint}\n\
         \x20\x20```bash\n\
         \x20\x20{cmd_endpoint_not_found}\n\
         \x20\x20```\n\
         \x20\x20🌐 **Localize `--user-content` AND `--list-label` per [Localization] rules**.\n\
         \x20\x20`--user-content` template (canonical English):\n\
         \x20\x20[Job {short_id} — you are the User Agent] The previously selected service endpoint (`requestedEndpoint` from the response) of ASP (agentId={dp_id}) is no longer available. Choose next step:\n\
         \x20\x20A. Specify another ASP — provide the agentId\n\
         \x20\x20B. Make the job public — let more ASPs discover it\n\
         \x20\x20C. Close the job\n\
         \x20\x20{follow_playbook}\n\
         \x20\x20-> **end this turn** and wait for the user's reply.\n\
         \x20\x20{route_hint}\n\n\
         - **`errorType == \"not_provider\"`** -> the designated agent does not exist or is not registered as an ASP.\n\
         \x20\x20Enqueue the user decision via `pending-decisions-v2 request`:\n\
         \x20\x20{session_hint}\n\
         \x20\x20```bash\n\
         \x20\x20{cmd_not_provider}\n\
         \x20\x20```\n\
         \x20\x20🌐 **Localize `--user-content` AND `--list-label` per [Localization] rules**.\n\
         \x20\x20`--user-content` template (canonical English):\n\
         \x20\x20{not_provider}\n\
         \x20\x20{follow_playbook}\n\
         \x20\x20-> **end this turn** and wait for the user's reply.\n\
         \x20\x20{route_hint}\n\n\
         - **`errorType == \"offline\"`** -> the ASP is offline and cannot negotiate.\n\
         \x20\x20Enqueue the user decision via `pending-decisions-v2 request`:\n\
         \x20\x20{session_hint}\n\
         \x20\x20```bash\n\
         \x20\x20{cmd_offline}\n\
         \x20\x20```\n\
         \x20\x20🌐 **Localize `--user-content` AND `--list-label` per [Localization] rules**.\n\
         \x20\x20`--user-content` template (canonical English):\n\
         \x20\x20{provider_offline}\n\
         \x20\x20{follow_playbook}\n\
         \x20\x20-> **end this turn** and wait for the user's reply.\n\
         \x20\x20{route_hint}\n")
}
