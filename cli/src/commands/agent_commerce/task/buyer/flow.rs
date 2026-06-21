//! User Agent (buyer) side task flow driver
//!
//! Based on the current event from system notifications, outputs the next-action prompt.
//! Buyer counterpart of provider/flow.rs — lets the agent simply run
//! `exec onchainos agent next-action --role buyer ...` to fetch a prompt and execute directly.
//!
//! The actual prompt generation logic is split by responsibility into:
//! - `flow_negotiate.rs` — negotiation / matching phase
//! - `flow_lifecycle.rs` — task execution + arbitration + terminal states

use crate::commands::agent_commerce::task::common::config::TASK_MIN_VERSION;
use crate::commands::agent_commerce::task::common::util::short_job_id;
use crate::commands::agent_commerce::task::common::state_machine::Status;
use crate::commands::agent_commerce::task::common::DEBUG_LOG;

// ── Localization constants (shared across flow_negotiate / flow_lifecycle) ────
//
// Each constant produces byte-for-byte identical output when interpolated via
// `format!("{CONST}")` — zero prompt-level risk.

pub(super) const LOCALIZATION_PREFIX: &str = "\
🌐 **Localize first** — fill `<...>` placeholders with real values, then rewrite all user-facing content in the user's language before sending. Do NOT add information not present in the template. Do NOT pass the English template verbatim to a non-English user.\n\n";

/// Shared switch-asp routing text for user_decision_* handlers.
/// Covers: user-reject → asp-match → service extraction → set-asp (or set_asp_params decision).
fn switch_asp_routing(job_id: &str, agent_id: &str, source_event: &str) -> String {
    // CLI mode (Claude Code / Codex): drop the passive "Waiting for ASP to accept"
    // phrase — it reads as a turn-end cue to LLM-driven watch loops and suppresses re-arm.
    let success_line = if super::content::is_cli_mode() {
        "\x20\x20\x20\x20On success → notify user (🌐 localized): \"ASP set to Agent <agentId>.\"\n"
    } else {
        "\x20\x20\x20\x20On success → notify user (🌐 localized): \"ASP set to Agent <agentId>. Waiting for ASP to accept.\"\n"
    };
    format!("\
                     \x20\x20\x20\x201. Reject current ASP (safe even if none active):\n\
                     \x20\x20\x20\x20```bash\n\
                     \x20\x20\x20\x20onchainos agent user-reject {job_id}\n\
                     \x20\x20\x20\x20```\n\
                     \x20\x20\x20\x202. Fetch the new ASP's service info:\n\
                     \x20\x20\x20\x20```bash\n\
                     \x20\x20\x20\x20onchainos agent asp-match --job-id {job_id} --provider-agent-id <agentId> --format json\n\
                     \x20\x20\x20\x20```\n\
                     \x20\x20\x20\x203. From the result, extract the ASP's **top service**: `serviceId`, `serviceName`, `serviceDescription`, `feeAmount` (→ serviceTokenAmount), `feeToken` (→ serviceTokenAddress), `feeTokenSymbol`. If `asp-match` returns no services, inform the user and re-ask via `pending-decisions-v2 request` with `--source-event {source_event}`.\n\
                     \x20\x20\x20\x204. **Infer serviceParams** from `serviceDescription` + task `description` (from conversation context, or fetch via `onchainos agent common context {job_id} --role buyer --agent-id {agent_id}` if not available):\n\
                     \x20\x20\x20\x20- Read `serviceDescription` semantically: identify what specific input the user must provide — action verbs directed at user (specify/provide/input/enter/describe/tell), conditional phrases (\"after receiving [X]\"), templates with placeholders, examples, or compound input. If the service only describes output/capabilities with no user input needed → serviceParams is empty.\n\
                     \x20\x20\x20\x20- For each required input, check if the task description provides it. Provided → extract value. Not provided → mark `<to be provided>` with a hint from serviceDescription.\n\
                     \x20\x20\x20\x20- Format as natural-language `key：value` pairs (separated by `；` or `\\n`). No JSON.\n\
                     \x20\x20\x20\x205. **Route by inference result:**\n\
                     \x20\x20\x20\x20- **serviceDescription is empty OR all fields filled** (no `<to be provided>` marks) → call `set-asp` directly:\n\
                     \x20\x20\x20\x20```bash\n\
                     \x20\x20\x20\x20onchainos agent set-asp {job_id} --provider-agent-id <agentId> --service-id <sid> --service-params '<inferred or empty>' --service-token-address <feeToken> --service-token-amount <feeAmount>\n\
                     \x20\x20\x20\x20```\n\
                     {success_line}\
                     \x20\x20\x20\x20- **Some fields filled, some marked `<to be provided>`** → pre-fill and ask user to confirm/modify — enqueue:\n\
                     \x20\x20\x20\x20```bash\n\
                     \x20\x20\x20\x20onchainos agent pending-decisions-v2 request --job-id {job_id} --role buyer --agent-id {agent_id} --source-event set_asp_params --user-content \"<compose from template below>\" --list-label \"[SetASP <shortJobId>] confirm service params\"\n\
                     \x20\x20\x20\x20```\n\
                     \x20\x20\x20\x20`--user-content` template (canonical English; 🌐 localize per user's language):\n\
                     \x20\x20\x20\x20You selected Agent <agentId> — <serviceName>.\n\
                     \x20\x20\x20\x20Service: <serviceDescription>\n\
                     \x20\x20\x20\x20Fee: <feeAmount> <feeTokenSymbol>\n\
                     \x20\x20\x20\x20\n\
                     \x20\x20\x20\x20Pre-filled service params (please confirm or modify):\n\
                     \x20\x20\x20\x20<inferred serviceParams with `<to be provided>` marks>\n\
                     \x20\x20\x20\x20\n\
                     \x20\x20\x20\x20Reply \"ok\" to confirm, or provide corrections.\n\
                     \x20\x20\x20\x20[SERVICE_CONTEXT providerAgentId=<agentId> serviceId=<sid> serviceTokenAddress=<feeToken> serviceTokenAmount=<feeAmount> inferredParams=<inferred serviceParams>]\n\
                     \x20\x20\x20\x20- **Nothing extractable** (serviceDescription is vague, task description has no matching values) → ask user to provide — enqueue:\n\
                     \x20\x20\x20\x20```bash\n\
                     \x20\x20\x20\x20onchainos agent pending-decisions-v2 request --job-id {job_id} --role buyer --agent-id {agent_id} --source-event set_asp_params --user-content \"<compose from template below>\" --list-label \"[SetASP <shortJobId>] provide service params\"\n\
                     \x20\x20\x20\x20```\n\
                     \x20\x20\x20\x20`--user-content` template (canonical English; 🌐 localize per user's language):\n\
                     \x20\x20\x20\x20You selected Agent <agentId> — <serviceName>.\n\
                     \x20\x20\x20\x20Service: <serviceDescription>\n\
                     \x20\x20\x20\x20Fee: <feeAmount> <feeTokenSymbol>\n\
                     \x20\x20\x20\x20\n\
                     \x20\x20\x20\x20Please describe the input for this service (serviceParams):\n\
                     \x20\x20\x20\x20[SERVICE_CONTEXT providerAgentId=<agentId> serviceId=<sid> serviceTokenAddress=<feeToken> serviceTokenAmount=<feeAmount>]\n\
                     \x20\x20\x20\x20**`--list-label` must be localized to the user's language**.\n\
                     \x20\x20\x20\x206. **Create sub session + SKILL_PREFETCH** (only after set-asp succeeds):\n\
                     \x20\x20\x20\x20```bash\n\
                     \x20\x20\x20\x20okx-a2a session create --job-id {job_id} --my-agent-id {agent_id} --to-agent-id <agentId> --json\n\
                     \x20\x20\x20\x20```\n\
                     \x20\x20\x20\x20Then send SKILL_PREFETCH:\n\
                     \x20\x20\x20\x20```bash\n\
                     \x20\x20\x20\x20okx-a2a session send --session-key <sessionKey from above> --content '[SKILL_PREFETCH] Read the okx-agent-task skill. Pre-load buyer role context.'\n\
                     \x20\x20\x20\x20```\n\
                     \x20\x20\x20\x20End the turn. Wait for `provider_applied`.\n\
                     \x20\x20\x20\x20⚠️ If user said specify but **did NOT include an agentId**: re-ask via `pending-decisions-v2 request --source-event {source_event}` asking for the agentId; **`--user-content` and `--list-label` must be localized to the user's language** (English ref: \"Please provide the 3-digit agentId of the ASP you want to use (e.g. `864`)\").\n")
}

/// Shared context parameter pack across all event handler functions.
pub(super) struct FlowContext<'a> {
    pub job_id: &'a str,
    pub agent_id: &'a str,
    pub short_id: &'a str,
    pub title_display: &'a str,
    pub title_query_hint: &'a str,
    pub title_in_extract: &'a str,
    pub terminal_session_hint: String,
    pub payment_mode: Option<i64>,
    pub prefetched: Option<&'a crate::commands::agent_commerce::task::common::PreFetchedTaskContext>,
    /// Verbatim `--data` arg from `next-action`, used by event handlers that
    /// need user-routed input (e.g. `reject_review` reading the rejection
    /// reason extracted from the relayed `user_decision_job_submitted` reply).
    pub data: Option<&'a str>,
}

/// List of CLI commands the buyer can execute under a given status (used in the menu at the tail of `agent common context` output).
///
/// Each status lists the primary action + one index line pointing back to the full `next-action` playbook (
/// the `generate_next_action` function in this same file, routed by the entry event corresponding to the status).
pub fn available_actions(status: &Status, job_id: &str) -> Vec<String> {
    let next_action = |evt: &str| {
        format!("**Next required step** → `onchainos agent next-action --role buyer --agentId <agentId> --message '{{\"event\":\"{evt}\",\"jobId\":\"{job_id}\"}}'` (fetch the full playbook for the current status, **follow the playbook**, do not bypass next-action and call the CLI below directly)")
    };
    let ref_header = "(reference - related CLI used inside the playbook; do not call directly, call next-action first to get the playbook)".to_string();
    match status {
        Status::Created => vec![
            next_action("job_created"),
            ref_header,
            format!("  onchainos agent asp-match --job-id {job_id} --agent-id <agentId>  # Search matching ASPs"),
            format!("  onchainos agent set-payment-mode {job_id} --payment-mode <escrow|x402> --token-symbol <sym> --token-amount <amt> [--endpoint <url>]  # Set payment mode (standalone)"),
            format!("  onchainos agent confirm-accept {job_id}  # Confirm accept (reads provider/token/amount from task detail API)"),
            format!("  onchainos agent direct-accept {job_id} --provider-agent-id <agentId> --token-symbol <sym> --token-amount <amt>  # x402 phase 2b: call after endpoint interaction"),
            format!("  onchainos agent close {job_id}          # Close task"),
            format!("  onchainos agent set-public {job_id}     # Convert to public task"),
            format!("  onchainos agent set-token-and-budget {job_id} --token-symbol <USDT|USDG> --budget <amount>  # Change payment token and amount (on-chain)"),
            format!("  onchainos agent set-asp {job_id} --provider-agent-id <agentId> --service-id <svc> --service-type <A2A|A2MCP> --service-params '<params>' --service-token-address <addr> --service-token-amount <amt>  # Re-set ASP + service (off-chain, triggers job_created)"),
            format!("  onchainos agent set-max-budget {job_id} --max-budget <amount>  # Change max budget (off-chain)"),
            format!("  onchainos agent reject-apply {job_id}  # Reject the current provider's apply (off-chain)"),
        ],
        Status::Accepted => vec![
            "(escrow) Provider is executing the task, waiting for job_submitted to enter review".to_string(),
            "(x402) Provider delivery already completed in the accept phase".to_string(),
        ],
        Status::Submitted => vec![
            next_action("job_submitted"),
            "⚠️ complete/reject are NOT in the job_submitted playbook — after receiving the user's review decision, call next-action with the corresponding pseudo-event playbook:".to_string(),
            format!("  onchainos agent next-action --role buyer --agentId <agentId> --message '{{\"event\":\"approve_review\",\"jobId\":\"{job_id}\"}}'  # After user approves review"),
            format!("  onchainos agent next-action --role buyer --agentId <agentId> --message '{{\"event\":\"reject_review\",\"jobId\":\"{job_id}\"}}'  # After user rejects review"),
            format!("  onchainos agent feedback-submit --agent-id <providerAgentId> --creator-id <buyerAgentId> --score <score> --task-id {job_id}  # Auto-rate provider (agent generates score based on task details + deliverable)"),
        ],
        Status::Rejected => vec![
            next_action("job_rejected"),
            "(passive wait) Provider decides: job_disputed → enter arbitration evidence; job_refunded → refund".to_string(),
        ],
        Status::Disputed => vec![
            next_action("job_disputed"),
            "(passive) Evidence is auto-submitted by the CLI on `job_disputed` (chat history + saved deliverables under ~/.onchainos/deliverables/buyer/<jobId>/); manual `dispute upload` is not supported.".to_string(),
        ],
        Status::Completed => vec![
            next_action("job_completed"),
            "(terminal) Task is COMPLETE — **funds released to provider**".to_string(),
            "  ▸ escrow review approved → release escrow funds to provider".to_string(),
            "  ▸ arbitration provider wins (dispute_resolved seller-wins) → release escrow funds to provider".to_string(),
            "  ▸ x402 funds were already paid in the accept phase".to_string(),
            "⚠️ Keep the sub session (do not close), for later reference.".to_string(),
        ],
        Status::Failed => vec![
            next_action("job_refunded"),
            "(terminal) Task is FAILED — **funds refunded to user**".to_string(),
            "  ▸ Provider agreed to refund (agree-refund) / auto-refund → funds returned along the original path".to_string(),
            "  ▸ Arbitration buyer wins (dispute_resolved buyer-wins) → refund".to_string(),
            "⚠️ Keep the sub session (do not close), for later reference.".to_string(),
        ],
        Status::Close => vec![
            "Task is closed (Close). ⚠️ Keep the sub session (do not close), for later reference.".to_string(),
        ],
        Status::Expired => vec![
            "Task has expired (Expired).".to_string(),
            format!("  onchainos agent claim-auto-refund {job_id}  # Claim auto-refund"),
        ],
        Status::AdminStopped => vec![
            "Task has been stopped by admin (AdminStopped). Please contact platform support to find out why.".to_string(),
        ],
        Status::Init => vec![
            "Task is initializing (waiting for on-chain confirmation) → waiting for job_created event".to_string(),
        ],
        Status::Other(s) => vec![
            format!("Current task status=`{s}` is not in the set of statuses the buyer cares about (open / accepted / submitted / rejected / disputed / completed / failed / close / expired / admin_stopped)"),
            "→ No task-level action required for this role, wait for the next relevant chain event / user decision before handling".to_string(),
            "→ **Do NOT** repeatedly run `agent status` / `agent common context` (the result will be the same), end this turn".to_string(),
        ],
    }
}



/// Generate the structured next-action prompt for the client/buyer based on event.
///
/// The `event_str` parameter accepts both event names (job_created / provider_applied / ...)
/// and status names (open / submitted / ...), uniformly parsed by state_machine.
pub async fn generate_next_action(job_id: &str, event_str: &str, agent_id: &str, job_title: Option<&str>, data: Option<&str>, payment_mode: Option<i64>, prefetched: Option<&crate::commands::agent_commerce::task::common::PreFetchedTaskContext>, message: Option<&serde_json::Value>) -> String {
    use crate::commands::agent_commerce::task::common::state_machine::{parse_status_or_event, Event};

    let localization_prefix = LOCALIZATION_PREFIX;
    let version_prefix = format!(
        "[Protocol version] When calling `okx-a2a xmtp-send`, the `--payload` parameter is **required**, with value `{{\"taskMinVersion\":{TASK_MIN_VERSION}}}`.\n\n",
    );

    // Short jobId, used in pending-decisions-v2 request --user-content / --list-label as the `[Job <shortID>]` prefix.
    // Serves as a dual disambiguation anchor for the user and user agent when multiple prompts run concurrently. See buyer-sub-playbook.md §Communication Contract.
    let short_id = short_job_id(job_id);

    // jobTitle carried by the envelope — when present, inlined directly into the playbook, saving the agent an extra API query to fetch the title.
    let title_display = job_title.unwrap_or("<title>");
    let title_query_hint = if job_title.is_some() {
        String::new()
    } else {
        format!(
            "⚠️ When notifying the user, use the `<title> ({job_id})` format. \
             Fetch the title from context; if you don't remember it, first run `onchainos agent common context {job_id} --role buyer --agent-id {agent_id}` to query.\n\n"
        )
    };
    // Group B events still need to call the API for fields like tokenAmount — whether the "extract" list includes title depends on the input parameter.
    let title_in_extract = if job_title.is_some() { "" } else { "title, " };

    // ──────────────────────────────────────────────────────────────────────
    // Communication mechanism (how to send, whether to send, shape whitelist) — all covered in buyer-sub-playbook.md §Communication Contract.
    // This file only tells the agent **what content to send where at each step**, without re-explaining tool usage.
    //
    // Three communication CLI commands:
    //   - okx-a2a xmtp-send: send to provider (peer sub session), params --job-id + --to-agent-id + --message
    //   - okx-a2a user notify: notify the user (no user decision needed), params: --content
    //   - okx-a2a user decision-request: needs user interaction (confirm / decide), params: --llm-content + --user-content
    //     --llm-content = instructions injected into the user session LLM (invisible to the user, contains
    //                     (jobId, role, agentId, toAgentId?) routing fields so the user agent can relay the decision back to the sub)
    //     --user-content = visible message sent to the user
    // ──────────────────────────────────────────────────────────────────────
    let terminal_session_hint = format!("\
ℹ️ Task is at a terminal state — run the cleanup command (handles pending-decision cancellation automatically):\n\
  ```bash\n\
  onchainos agent session-cleanup --job-id {job_id}\n\
  ```\n\
  Then follow the command's output to close conversations (if applicable).");

    let preamble_slim = "\
         🛑 Core rules:\n\
         - Rule 0: Follow steps literally; do NOT skip / reorder / batch.\n\
         - Rule 2: CLI error → do NOT retry; push `cli_failed` decision.\n\
         - Rule 9: Sub/backup text is invisible to user → use `user notify` or `pending-decisions-v2 request`.\n\
         - Rule 15: ≥1 tool_use block, ≤2 lines text per response.\n\n";

    // Pre-fetched context block — when available, inlined at the top of the playbook so the agent
    // can skip the "Step 1: run common context" CLI round-trip.
    let prefetched_block = prefetched.map(|p| p.format_inline()).unwrap_or_default();

    let ctx = FlowContext {
        job_id,
        agent_id,
        short_id: &short_id,
        title_display,
        title_query_hint: &title_query_hint,
        title_in_extract,
        terminal_session_hint,
        payment_mode,
        prefetched,
        data,
    };

    let event = parse_status_or_event(event_str);
    if DEBUG_LOG {
        eprintln!(
            "[buyer-flow] generate_next_action called: job_id={job_id}, event={event_str}, agent_id={agent_id}"
        );
        eprintln!(
            "[buyer-flow] parsed event: {:?} | okx-a2a commands involved: {}",
            event,
            match &event {
                Event::JobCreated => "okx-a2a session create (create group) → okx-a2a xmtp-send (send negotiation message)",
                Event::ProviderApplied => "in-process branch by over_most_budget: confirm-accept (within budget) OR reject-apply + 3/4-option card (over budget)",
                Event::JobProviderReject => "in-process POST /reset/asp → playbook tells agent to localize + 3/4-option card",
                Event::JobAccepted => "okx-a2a user notify (notify accept success)",
                Event::JobSubmitted => "pending-decisions-v2 request (forward deliverable, request review decision)",
                Event::JobRejected => "okx-a2a user notify (notify rejection on-chain) → wait for provider decision",
                Event::JobDisputed => "okx-a2a session history → dispute upload (auto-submit chat history + manifest deliverables) → okx-a2a user notify (notify)",
                Event::DisputeResolved => "okx-a2a user notify (notify arbitration result)",
                Event::JobRefunded => "okx-a2a user notify (notify refund complete)",
                Event::JobAutoRefunded => "okx-a2a user notify (claimAutoRefund tx receipt)",
                Event::NegotiateReply =>
                    "natural-language reply (max 2 rounds; over-limit → mark-failed + user decision card)",
                Event::AttachmentAdded => "okx-a2a file upload → okx-a2a xmtp-send (upload + forward attachment to provider)",
                Event::DeliverableReceived => "task-deliverable-save (download + save deliverable immediately)",
                _ => "none",
            }
        );
    }

    let body = match event {
        // ─── Negotiation / matching phase → flow_negotiate ──────────────────────────
        Event::JobCreated => {
            super::flow_negotiate::job_created_cli(&ctx).await
        }
        Event::Other(ref s) if s == "provider_conversation" => {
            super::flow_negotiate::provider_conversation_cli(&ctx)
        }
        Event::Other(ref s) if s == "provider_conversation_reject" => {
            let gid = message
                .and_then(|m| m.get("groupId"))
                .and_then(|v| v.as_str())
                .unwrap_or("");
            if gid.is_empty() {
                format!("[Error] provider_conversation_reject requires `groupId` in --message. Call:\n\
                         onchainos agent next-action --role buyer --agentId {agent_id} --message '{{\"event\":\"provider_conversation_reject\",\"jobId\":\"{job_id}\",\"groupId\":\"<groupId>\"}}'\n")
            } else {
                super::flow_negotiate::provider_conversation_reject_cli(&ctx, gid)
            }
        }
        Event::Other(ref s) if s == "provider_conversation_pick" => {
            let dp_id = message
                .and_then(|m| m.get("provider"))
                .and_then(|v| v.as_str())
                .unwrap_or("");
            if dp_id.is_empty() {
                format!("[Error] provider_conversation_pick requires `provider` in --message. Call:\n\
                         onchainos agent next-action --role buyer --agentId {agent_id} --message '{{\"event\":\"provider_conversation_pick\",\"jobId\":\"{job_id}\",\"provider\":\"<ASP agentId>\"}}'\n")
            } else {
                let _ = super::negotiate::save_designated_provider(job_id, dp_id);
                super::flow_negotiate::provider_conversation_pick_cli(job_id, agent_id, &short_id, dp_id, title_display, prefetched).await
            }
        }
        Event::Other(ref s) if s == "designated_a2a" || s == "designated_x402" || s == "designated_error" => {
            let dp_id = super::negotiate::get_designated_provider(job_id).ok().flatten().unwrap_or_default();
            if dp_id.is_empty() {
                format!("[Error] designated_* pseudo-event requires `provider` field. Call: onchainos agent next-action --role buyer --agentId {agent_id} --message '{{\"event\":\"{s}\",\"jobId\":\"{job_id}\",\"provider\":\"<ASP agentId>\"}}'\n")
            } else {
                match s.as_str() {
                    "designated_a2a" => super::flow_negotiate::designated::branch_a2a(job_id, agent_id, &short_id, &dp_id, title_display),
                    "designated_x402" => super::flow_negotiate::designated::branch_x402(job_id, agent_id, &short_id, &dp_id, None),
                    _ => super::flow_negotiate::designated::branch_error(job_id, agent_id, &short_id, &dp_id),
                }
            }
        }
        Event::JobVisibilityChanged => {
            let visibility = message
                .and_then(|m| m.get("visibility"))
                .and_then(|v| v.as_i64())
                .unwrap_or(1);
            super::flow_negotiate::job_visibility_changed(&ctx, visibility)
        }
        Event::JobPaymentModeChanged => super::flow_negotiate::job_payment_mode_changed(&ctx),
        Event::NegotiateReply => super::flow_negotiate::negotiate_reply(&ctx),

        // ─── Task execution + arbitration + terminal states → flow_lifecycle ─────────────────
        Event::ProviderApplied => {
            let over_most_budget = message
                .and_then(|m| m.get("overMostBudget"))
                .and_then(|v| v.as_bool())
                .unwrap_or(true);
            let visibility = message
                .and_then(|m| m.get("visibility"))
                .and_then(|v| v.as_i64())
                .unwrap_or(1);
            super::flow_lifecycle::provider_applied(&ctx, over_most_budget, visibility).await
        }
        Event::JobProviderReject => {
            let visibility = message
                .and_then(|m| m.get("visibility"))
                .and_then(|v| v.as_i64())
                .unwrap_or(1);
            super::flow_negotiate::provider_reject(&ctx, visibility).await
        }
        Event::JobAccepted => super::flow_lifecycle::job_accepted(&ctx),
        Event::DeliverableReceived => {
            super::flow_lifecycle::deliverable_received_cli(&ctx, message)
        }
        Event::JobSubmitted => super::flow_lifecycle::job_submitted(&ctx),
        Event::JobRejected => super::flow_lifecycle::job_rejected(&ctx),
        Event::JobDisputed => super::flow_lifecycle::job_disputed(&ctx),
        Event::Other(ref s) if s == "approve_review" => super::flow_lifecycle::approve_review(&ctx).await,
        Event::Other(ref s) if s == "reject_review" => super::flow_lifecycle::reject_review(&ctx).await,
        Event::JobCompleted => super::flow_lifecycle::job_completed(&ctx),
        Event::DisputeResolved => super::flow_lifecycle::dispute_resolved(&ctx),
        Event::JobRefunded => super::flow_lifecycle::job_refunded(&ctx),
        Event::JobAutoRefunded => super::flow_lifecycle::job_auto_refunded(&ctx),
        Event::JobExpired => super::flow_lifecycle::job_expired(&ctx),
        Event::JobClosed => super::flow_lifecycle::job_closed(&ctx),
        Event::SubmitExpired => super::flow_lifecycle::submit_expired(&ctx).await,
        Event::RejectExpired => super::flow_lifecycle::reject_expired(&ctx).await,
        Event::ReviewDeadlineWarn => super::flow_lifecycle::review_deadline_warn(&ctx),
        Event::ReviewExpired => super::flow_lifecycle::review_expired(&ctx),
        Event::JobAutoCompleted => super::flow_lifecycle::job_auto_completed(&ctx),
        Event::SubmitDeadlineWarn => super::flow_lifecycle::submit_deadline_warn(),
        Event::EvaluatorSelected
        | Event::RevealStarted
        | Event::VoteCommitted
        | Event::VoteRevealed
        | Event::RoundFailed
        | Event::VoteCommitDeadlineWarn
        | Event::VoteRevealDeadlineWarn => super::flow_lifecycle::evaluator_events(event.as_str()),
        Event::RewardClaimed => super::flow_lifecycle::reward_claimed(&ctx),
        Event::WakeupNotify => super::flow_lifecycle::wakeup_notify(&ctx),
        Event::Other(ref s) if s == "create_task" => super::flow_lifecycle::create_task(),
        Event::Other(ref s) if s == "close" => super::flow_lifecycle::close_task(&ctx).await,
        Event::Other(ref s) if s == "set_public" => super::flow_lifecycle::set_public(&ctx).await,
        Event::AttachmentAdded => {
            super::flow_lifecycle::attachment_added_cli(&ctx, message)
        }
        Event::TaskTokenBudgetChange => super::flow_lifecycle::task_token_budget_change(&ctx),
        // ─── Events the buyer never receives + unknown fallback ──────────────────────────
        Event::Staked
        | Event::UnstakeRequested
        | Event::UnstakeClaimed
        | Event::UnstakeCancelled
        | Event::StakeStopped
        | Event::CooldownEntered
        | Event::DisputeApproved => super::flow_lifecycle::staked_and_unknown(event.as_str(), job_id),

        // ─── user_decision_* relay router (buyer-side scenes) ───
        // User-decision relays arrive as system-shaped envelopes with
        // `event = "user_decision_<source_event>"` and `message.data = <user's verbatim reply>`.
        // CLI returns a routing playbook that lists the candidate pseudo-events with
        // natural-language descriptions; the sub agent's LLM decides which one the
        // user actually meant — no hardcoded keyword tables, pure semantic mapping.
        Event::Other(ref s) if s.starts_with("user_decision_") => {
            let source = s["user_decision_".len()..].to_string();
            let reply = data.unwrap_or("").trim();
            let ud_guard = "⚠️ Execute in place — do NOT forward via `okx-a2a session send` (infinite loop) or call `pending-decisions-v2 resolve/pick/cancel/list` (user-session-only).\n\n";
            let ud_body = match source.as_str() {
                "job_submitted" | "review_deadline_warn" => format!(
                    "[User decision relay] source_event=`{source}`, user's verbatim reply: `{reply}`\n\n\
                     **Semantic mapping** — decide which intent the user's reply means, then call the corresponding next-action.\n\n\
                     Two options:\n\
                     \x20\x20• **`approve_review`** — user accepts the deliverable (typical intents: A / 通过 / 同意 / 满意 / 接受 / 验收 / approve / accept / agree / OK / 行 / 可以 — anything meaning satisfaction with the deliverable).\n\
                     \x20\x20• **`reject_review`** — user rejects and wants revisions/refund (typical intents: B / 拒绝 / 不通过 / 不满意 / 不接受 / reject / refuse / 不行 / 不达标 — anything meaning dissatisfaction; extract the reason if the user provided one after `理由` / `reason` / `因为`; ⚠️ the reason is critical — it will be auto-submitted as evidence if the ASP files a dispute).\n\n\
                     If the user's reply clearly maps to one of these → call:\n\
                     ```bash\n\
                     # For approve_review (no extra args needed):\n\
                     onchainos agent next-action --role buyer --agentId {agent_id} --message '{{\"event\":\"approve_review\",\"jobId\":\"{job_id}\"}}'\n\
                     # For reject_review — pass the extracted rejection reason via message.data (empty string if user gave no reason; the handler falls back to a default):\n\
                     onchainos agent next-action --role buyer --agentId {agent_id} --message '{{\"event\":\"reject_review\",\"jobId\":\"{job_id}\",\"data\":\"<extracted reason from user's reply, or empty>\"}}'\n\
                     ```\n\
                     If the reply is **truly ambiguous** (e.g. non-committal `hmm` / `got it` / unrelated chitchat): re-ask via `pending-decisions-v2 request` with the same `--to-agent-id` (or none, if from a backup sub) and `--source-event {source}`. **`--user-content` and `--list-label` must be localized to the user's language**. Reference (English): \"I didn't catch your reply, please clarify: A=approve  B=reject\".\n"
                ),
                "cli_failed" => format!(
                    "[User decision relay] source_event=`cli_failed`, user's verbatim reply: `{reply}`\n\n\
                     The original `onchainos agent <cmd>` failed and you asked the user how to proceed. **Semantic mapping** — decide what the user means and act accordingly (no on-chain action by default):\n\n\
                     \x20\x20• **Retry** — user wants you to re-run the same CLI command (typical intents: A / 选A / retry / 重试 / try again / 再来一次 / 再试一次). Action: re-execute the **exact same** CLI you previously ran (same args, same job_id). If it fails again, do NOT loop — enqueue **one more** `pending-decisions-v2 request --source-event cli_failed` and end the turn.\n\
                     \x20\x20• **Dismiss** — user takes manual control of this step (typical intents: B / 选B / dismiss / 不再提示 / skip prompts / 我自己处理 / let me handle it). Action: end the turn. Do not re-prompt; the user owns this step now.\n\
                     \x20\x20• **New instruction** — user gives a corrective instruction in natural language (e.g. `把 token-symbol 改成 USDT 再试` / `change --token-symbol to USDT and retry` / `用 endpoint https://... 重试` / `先 cancel 那个 unstake`). Action: parse the modification, rebuild the CLI invocation with the user's adjustment, and execute once. Treat the result as a fresh attempt (success → continue the original scene; failure → enqueue another `cli_failed` decision).\n\n\
                     ⚠️ Do NOT execute any on-chain action that wasn't part of the original failed command — the user reply only authorizes retry/edit of the failed step, not unrelated new actions.\n\
                     ⚠️ If the reply is truly ambiguous (e.g. unrelated chitchat / a non-committal `hmm` / `got it`), re-ask via `pending-decisions-v2 request` with the same `--to-agent-id` (or none, if from a backup sub) and `--source-event cli_failed`. **`--user-content` and `--list-label` must be localized to the user's language** (detect from the user's verbatim reply / prior turn) before sending. Reference (English): \"I didn't catch your reply, please clarify: A=retry  B=stop prompting  C=tell me what to change\".\n"
                ),
                "asp_match_pick" => {
                    // CLI mode (Claude Code / Codex): drop the passive "Waiting for ASP to accept"
                    // phrase — it reads as a turn-end cue to LLM-driven watch loops and suppresses re-arm.
                    let success_line = if super::content::is_cli_mode() {
                        "\x20\x20\x20\x20On success → notify user (🌐 localized): \"ASP set to Agent <X>.\" End the turn.\n"
                    } else {
                        "\x20\x20\x20\x20On success → notify user (🌐 localized): \"ASP set to Agent <X>. Waiting for ASP to accept.\" End the turn.\n"
                    };
                    format!(
                    "[User decision relay] source_event=`asp_match_pick`, user's verbatim reply: `{reply}`\n\n\
                     The push was the ASP-match list. **Semantic mapping** — decide what the user means:\n\n\
                     \x20\x20• **Pick an ASP** — user gave an index (1/2/3/...) or a 3-digit agentId (e.g. `864`). Map index → agentId from the asp-match list shown in the source-scene; the user picked agentId=`<X>`. Action (set-asp flow):\n\
                     \x20\x20\x20\x201. From the asp-match list, extract the picked ASP's **top service**: `serviceId`, `serviceName`, `serviceDescription`, `serviceType`, `feeAmount` (→ serviceTokenAmount), `feeToken` (→ serviceTokenAddress), `feeTokenSymbol`.\n\
                     \x20\x20\x20\x202. **Infer serviceParams** from `serviceDescription` + task `description` (from conversation context, or fetch via `onchainos agent common context {job_id} --role buyer --agent-id {agent_id}` if not available):\n\
                     \x20\x20\x20\x20- Read `serviceDescription` semantically: identify what specific input the user must provide — action verbs directed at user (specify/provide/input/enter/describe/tell), conditional phrases (\"after receiving [X]\"), templates with placeholders, examples, or compound input. If the service only describes output/capabilities with no user input needed → serviceParams is empty.\n\
                     \x20\x20\x20\x20- For each required input, check if the task description provides it. Provided → extract value. Not provided → mark `<to be provided>` with a hint from serviceDescription.\n\
                     \x20\x20\x20\x20- Format as natural-language `key：value` pairs (separated by `；` or `\\n`). No JSON.\n\
                     \x20\x20\x20\x203. **Route by inference result:**\n\
                     \x20\x20\x20\x20- **serviceDescription is empty OR all fields filled** (no `<to be provided>` marks) → call `set-asp` directly:\n\
                     \x20\x20\x20\x20```bash\n\
                     \x20\x20\x20\x20onchainos agent set-asp {job_id} --provider-agent-id <X> --service-id <sid> --service-type <serviceType> --service-params '<inferred or empty>' --service-token-address <feeToken> --service-token-amount <feeAmount>\n\
                     \x20\x20\x20\x20```\n\
                     {success_line}\
                     \x20\x20\x20\x20- **Some fields filled, some marked `<to be provided>`** → pre-fill and ask user to confirm/modify — enqueue:\n\
                     \x20\x20\x20\x20```bash\n\
                     \x20\x20\x20\x20onchainos agent pending-decisions-v2 request --job-id {job_id} --role buyer --agent-id {agent_id} --source-event set_asp_params --user-content \"<compose from template below>\" --list-label \"[SetASP <shortJobId>] confirm service params\"\n\
                     \x20\x20\x20\x20```\n\
                     \x20\x20\x20\x20`--user-content` template (🌐 localize):\n\
                     \x20\x20\x20\x20You selected Agent <X> — <serviceName>.\n\
                     \x20\x20\x20\x20Service: <serviceDescription>\n\
                     \x20\x20\x20\x20Fee: <feeAmount> <feeTokenSymbol>\n\
                     \x20\x20\x20\x20\n\
                     \x20\x20\x20\x20Pre-filled service params (please confirm or modify):\n\
                     \x20\x20\x20\x20<inferred serviceParams with `<to be provided>` marks>\n\
                     \x20\x20\x20\x20\n\
                     \x20\x20\x20\x20Reply \"ok\" to confirm, or provide corrections.\n\
                     \x20\x20\x20\x20[SERVICE_CONTEXT providerAgentId=<X> serviceId=<sid> serviceType=<serviceType> serviceTokenAddress=<feeToken> serviceTokenAmount=<feeAmount> inferredParams=<inferred serviceParams>]\n\
                     \x20\x20\x20\x20- **Nothing extractable** (serviceDescription is vague, task description has no matching values) → ask user — enqueue:\n\
                     \x20\x20\x20\x20```bash\n\
                     \x20\x20\x20\x20onchainos agent pending-decisions-v2 request --job-id {job_id} --role buyer --agent-id {agent_id} --source-event set_asp_params --user-content \"<compose from template below>\" --list-label \"[SetASP <shortJobId>] provide service params\"\n\
                     \x20\x20\x20\x20```\n\
                     \x20\x20\x20\x20`--user-content` template (🌐 localize):\n\
                     \x20\x20\x20\x20You selected Agent <X> — <serviceName>.\n\
                     \x20\x20\x20\x20Service: <serviceDescription>\n\
                     \x20\x20\x20\x20Fee: <feeAmount> <feeTokenSymbol>\n\
                     \x20\x20\x20\x20\n\
                     \x20\x20\x20\x20Please describe the input for this service (serviceParams):\n\
                     \x20\x20\x20\x20[SERVICE_CONTEXT providerAgentId=<X> serviceId=<sid> serviceType=<serviceType> serviceTokenAddress=<feeToken> serviceTokenAmount=<feeAmount>]\n\
                     \x20\x20\x20\x20**`--list-label` must be localized to the user's language**.\n\
                     \x20\x20• **Next page** — typical intents: `next page` / `下一页` / `more` / `更多` / `看更多`. Action: run `onchainos agent asp-match --job-id {job_id} --page <next_page>`. If results → re-push the asp_match_pick decision with the new list (`pending-decisions-v2 request --source-event asp_match_pick`; --list-label `[ASP <shortJobId>] <task title> ASP-pick decision`). **`--list-label` and all footer keywords must be localized** (e.g. Chinese: 回复\"更多\", NOT 回复\"more\"). If empty → enqueue the no-ASP next-step decision:\n\
                     \x20\x20\x20\x20```bash\n\
                     \x20\x20\x20\x20onchainos agent pending-decisions-v2 request --job-id {job_id} --role buyer --agent-id {agent_id} --user-content \"<compose from template below>\" --list-label \"[No ASP <shortJobId>] <task title> next-step decision\" --source-event no_asp_found\n\
                     \x20\x20\x20\x20```\n\
                     \x20\x20\x20\x20`--user-content` template (canonical English; 🌐 localize per user's language):\n\
                     \x20\x20\x20\x20[Job <shortJobId> — you are the User Agent] All matched ASPs have been tried; no match found. Choose next step:\n\
                     \x20\x20\x20\x20A. Specify an ASP — provide the ASP's agentId\n\
                     \x20\x20\x20\x20B. Make the job public — let more ASPs discover it\n\
                     \x20\x20\x20\x20C. Close the job — cancel and refund\n\
                     \x20\x20• **Make public** — typical intents: B / 选B / `public` / `公开` / `公开任务`. Action: `onchainos agent set-public {job_id}`.\n\
                     \x20\x20• **Close** — typical intents: C / 选C / `close` / `关闭` / `取消` / `cancel`. Action: `onchainos agent close {job_id}`.\n\n\
                     ⚠️ If ambiguous (e.g. unrelated chitchat): re-ask via `pending-decisions-v2 request` with the same `--to-agent-id` (or none, if from a backup sub) and `--source-event asp_match_pick`. **`--user-content` and `--list-label` must be localized to the user's language**. Reference (English): \"I didn't catch your reply. Reply with an ASP's number (1/2/3) or agentId to pick, or see more ASPs, list the task publicly, or cancel.\"\n"
                    )
                },
                "provider_pending" => format!(
                    "[User decision relay] source_event=`provider_pending`, user's verbatim reply: `{reply}`\n\n\
                     The push was a single-ASP accept/reject card. Extract `[asp: <agentId>]` and `[groupId: <gid>]` from the `--llm-content` block above. **Semantic mapping** — decide:\n\n\
                     \x20\x20• **Accept** — typical intents: 1 / `accept` / `接受` / `yes` / `好` / `可以`. Run:\n\
                     \x20\x20\x20\x20```bash\n\
                     \x20\x20\x20\x20onchainos agent next-action --role buyer --agentId {agent_id} --message '{{\"event\":\"provider_conversation_pick\",\"jobId\":\"{job_id}\",\"provider\":\"<asp agentId from llm-content>\"}}'\n\
                     \x20\x20\x20\x20```\n\
                     \x20\x20\x20\x20Follow the returned playbook verbatim.\n\
                     \x20\x20• **Reject** — typical intents: 2 / `reject` / `拒绝` / `no` / `不` / `换一个` / `next`. Run:\n\
                     \x20\x20\x20\x20```bash\n\
                     \x20\x20\x20\x20onchainos agent next-action --role buyer --agentId {agent_id} --message '{{\"event\":\"provider_conversation_reject\",\"jobId\":\"{job_id}\",\"groupId\":\"<groupId from llm-content>\"}}'\n\
                     \x20\x20\x20\x20```\n\
                     \x20\x20\x20\x20Follow the returned playbook (shows next ASP or close options if none remain).\n\n\
                     ⚠️ If ambiguous: re-ask via `pending-decisions-v2 request` with `--source-event provider_pending`. **`--user-content` and `--list-label` must be localized to the user's language**. Reference (English): \"Please reply 1 (accept) or 2 (reject).\"\n"
                ),
                "not_provider" | "no_asp_found" | "provider_offline" | "x402_invalid" | "over_budget" => {
                    // CLI mode (Claude Code / Codex): drop the passive "Waiting for ASP to accept"
                    // phrase — it reads as a turn-end cue to LLM-driven watch loops and suppresses re-arm.
                    let success_line = if super::content::is_cli_mode() {
                        "\x20\x20\x20\x20On success → notify user (🌐 localized): \"ASP set to Agent <agentId>.\" End the turn.\n"
                    } else {
                        "\x20\x20\x20\x20On success → notify user (🌐 localized): \"ASP set to Agent <agentId>. Waiting for ASP to accept.\" End the turn.\n"
                    };
                    format!(
                    "[User decision relay] source_event=`{source}`, user's verbatim reply: `{reply}`\n\n\
                     The push was an A/B/C choice (designated agent not a provider / no ASP available / designated provider offline / x402 endpoint invalid / quote over budget). **Semantic mapping** — decide:\n\n\
                     \x20\x20• **A — Specify another ASP** — typical intents: A / 选A / `specify` / `指定`, **with a 3-digit agentId in the reply** (e.g. `A 864` / `指定 864` / just `864`). Action (switch-asp flow):\n\
                     \x20\x20\x20\x201. Reject current ASP (safe even if none active):\n\
                     \x20\x20\x20\x20```bash\n\
                     \x20\x20\x20\x20onchainos agent user-reject {job_id}\n\
                     \x20\x20\x20\x20```\n\
                     \x20\x20\x20\x202. Fetch the new ASP's service info:\n\
                     \x20\x20\x20\x20```bash\n\
                     \x20\x20\x20\x20onchainos agent asp-match --job-id {job_id} --provider-agent-id <agentId> --format json\n\
                     \x20\x20\x20\x20```\n\
                     \x20\x20\x20\x203. From the result, extract the ASP's **top service**: `serviceId`, `serviceName`, `serviceDescription`, `serviceType`, `feeAmount` (→ serviceTokenAmount), `feeToken` (→ serviceTokenAddress), `feeTokenSymbol`. If `asp-match` returns no services for this ASP, inform the user and re-ask via `pending-decisions-v2 request` with `--source-event {source}`.\n\
                     \x20\x20\x20\x204. **Infer serviceParams** from `serviceDescription` + task `description` (from conversation context, or fetch via `onchainos agent common context {job_id} --role buyer --agent-id {agent_id}` if not available):\n\
                     \x20\x20\x20\x20- Read `serviceDescription` semantically: identify what specific input the user must provide — action verbs directed at user (specify/provide/input/enter/describe/tell), conditional phrases (\"after receiving [X]\"), templates with placeholders, examples, or compound input. If the service only describes output/capabilities with no user input needed → serviceParams is empty.\n\
                     \x20\x20\x20\x20- For each required input, check if the task description provides it. Provided → extract value. Not provided → mark `<to be provided>` with a hint from serviceDescription.\n\
                     \x20\x20\x20\x20- Format as natural-language `key：value` pairs (separated by `；` or `\\n`). No JSON.\n\
                     \x20\x20\x20\x205. **Route by inference result:**\n\
                     \x20\x20\x20\x20- **serviceDescription is empty OR all fields filled** (no `<to be provided>` marks) → call `set-asp` directly:\n\
                     \x20\x20\x20\x20```bash\n\
                     \x20\x20\x20\x20onchainos agent set-asp {job_id} --provider-agent-id <agentId> --service-id <sid> --service-type <serviceType> --service-params '<inferred or empty>' --service-token-address <feeToken> --service-token-amount <feeAmount>\n\
                     \x20\x20\x20\x20```\n\
                     {success_line}\
                     \x20\x20\x20\x20- **Some fields filled, some marked `<to be provided>`** → pre-fill and ask user to confirm/modify — enqueue:\n\
                     \x20\x20\x20\x20```bash\n\
                     \x20\x20\x20\x20onchainos agent pending-decisions-v2 request --job-id {job_id} --role buyer --agent-id {agent_id} --source-event set_asp_params --user-content \"<compose from template below>\" --list-label \"[SetASP <shortJobId>] confirm service params\"\n\
                     \x20\x20\x20\x20```\n\
                     \x20\x20\x20\x20`--user-content` template (🌐 localize):\n\
                     \x20\x20\x20\x20You selected Agent <agentId> — <serviceName>.\n\
                     \x20\x20\x20\x20Service: <serviceDescription>\n\
                     \x20\x20\x20\x20Fee: <feeAmount> <feeTokenSymbol>\n\
                     \x20\x20\x20\x20\n\
                     \x20\x20\x20\x20Pre-filled service params (please confirm or modify):\n\
                     \x20\x20\x20\x20<inferred serviceParams with `<to be provided>` marks>\n\
                     \x20\x20\x20\x20\n\
                     \x20\x20\x20\x20Reply \"ok\" to confirm, or provide corrections.\n\
                     \x20\x20\x20\x20[SERVICE_CONTEXT providerAgentId=<agentId> serviceId=<sid> serviceType=<serviceType> serviceTokenAddress=<feeToken> serviceTokenAmount=<feeAmount> inferredParams=<inferred serviceParams>]\n\
                     \x20\x20\x20\x20- **Nothing extractable** (serviceDescription is vague, task description has no matching values) → ask user — enqueue:\n\
                     \x20\x20\x20\x20```bash\n\
                     \x20\x20\x20\x20onchainos agent pending-decisions-v2 request --job-id {job_id} --role buyer --agent-id {agent_id} --source-event set_asp_params --user-content \"<compose from template below>\" --list-label \"[SetASP <shortJobId>] provide service params\"\n\
                     \x20\x20\x20\x20```\n\
                     \x20\x20\x20\x20`--user-content` template (🌐 localize):\n\
                     \x20\x20\x20\x20You selected Agent <agentId> — <serviceName>.\n\
                     \x20\x20\x20\x20Service: <serviceDescription>\n\
                     \x20\x20\x20\x20Fee: <feeAmount> <feeTokenSymbol>\n\
                     \x20\x20\x20\x20\n\
                     \x20\x20\x20\x20Please describe the input for this service (serviceParams):\n\
                     \x20\x20\x20\x20[SERVICE_CONTEXT providerAgentId=<agentId> serviceId=<sid> serviceType=<serviceType> serviceTokenAddress=<feeToken> serviceTokenAmount=<feeAmount>]\n\
                     \x20\x20\x20\x20**`--list-label` must be localized to the user's language**.\n\
                     \x20\x20\x20\x20⚠️ If user said A / specify but **did NOT include an agentId** (e.g. just `A`, `选A`, `换一个 ASP`): re-ask via `pending-decisions-v2 request` with the same `--to-agent-id` (or none, if from a backup sub) and `--source-event {source}`; `--user-content` and `--list-label` must be localized to the user's language; `--user-content` must ask for the agentId (English ref: \"Please provide the 3-digit agentId of the ASP you want to use (e.g. `864`)\").\n\
                     \x20\x20• **B — Make public** — typical intents: B / 选B / `public` / `公开`. Action: `onchainos agent set-public {job_id}`.\n\
                     \x20\x20• **C — Close** — typical intents: C / 选C / `close` / `关闭` / `取消` / `cancel`. Action: `onchainos agent close {job_id}`.\n\n\
                     ⚠️ If ambiguous (unrelated chitchat / non-committal `hmm` / `got it`): re-ask via `pending-decisions-v2 request` with `--source-event {source}`. **`--user-content` and `--list-label` must be localized to the user's language**. Reference (English): \"I didn't catch your reply, please clarify: A=specify another ASP (include the agentId)  B=make public  C=close the job\".\n"
                    )
                },
                "negotiate_over_budget" => {
                    // CLI mode (Claude Code / Codex): drop the passive "Waiting for ASP to accept"
                    // phrase — it reads as a turn-end cue to LLM-driven watch loops and suppresses re-arm.
                    let success_line = if super::content::is_cli_mode() {
                        "\x20\x20\x20\x20On success → notify user (🌐 localized): \"ASP set to Agent <agentId>.\" End the turn.\n"
                    } else {
                        "\x20\x20\x20\x20On success → notify user (🌐 localized): \"ASP set to Agent <agentId>. Waiting for ASP to accept.\" End the turn.\n"
                    };
                    format!(
                    "[User decision relay] source_event=`negotiate_over_budget`, user's verbatim reply: `{reply}`\n\n\
                     The push was during negotiation when the ASP's quote exceeded max_budget — different A/B/C from the designated-flow `over_budget` (this one offers `view ASP list` not `make public`). **Semantic mapping** — decide:\n\n\
                     \x20\x20• **A — View ASP list** — typical intents: A / 选A / `推荐` / `recommend` / `列表` / `list` / `看看有谁`. Action: `onchainos agent asp-match --job-id {job_id}` → compose the ASP list as `--user-content` for `pending-decisions-v2 request --source-event asp_match_pick`. **All footer keywords must be localized** (e.g. Chinese: 回复\"更多\", NOT 回复\"more\").\n\
                     \x20\x20• **B — Specify another ASP** — typical intents: B / 选B / `specify` / `指定`, **with a 3-digit agentId in the reply** (e.g. `B 864` / `指定 864` / `换 864`). Action (switch-asp flow):\n\
                     \x20\x20\x20\x201. Reject current ASP (safe even if none active):\n\
                     \x20\x20\x20\x20```bash\n\
                     \x20\x20\x20\x20onchainos agent user-reject {job_id}\n\
                     \x20\x20\x20\x20```\n\
                     \x20\x20\x20\x202. Fetch the new ASP's service info:\n\
                     \x20\x20\x20\x20```bash\n\
                     \x20\x20\x20\x20onchainos agent asp-match --job-id {job_id} --provider-agent-id <agentId> --format json\n\
                     \x20\x20\x20\x20```\n\
                     \x20\x20\x20\x203. From the result, extract the ASP's **top service**: `serviceId`, `serviceName`, `serviceDescription`, `serviceType`, `feeAmount` (→ serviceTokenAmount), `feeToken` (→ serviceTokenAddress), `feeTokenSymbol`. If `asp-match` returns no services, inform the user and re-ask via `pending-decisions-v2 request` with `--source-event negotiate_over_budget`.\n\
                     \x20\x20\x20\x204. **Infer serviceParams** from `serviceDescription` + task `description` (from conversation context, or fetch via `onchainos agent common context {job_id} --role buyer --agent-id {agent_id}` if not available):\n\
                     \x20\x20\x20\x20- Read `serviceDescription` semantically: identify what specific input the user must provide — action verbs directed at user (specify/provide/input/enter/describe/tell), conditional phrases (\"after receiving [X]\"), templates with placeholders, examples, or compound input. If the service only describes output/capabilities with no user input needed → serviceParams is empty.\n\
                     \x20\x20\x20\x20- For each required input, check if the task description provides it. Provided → extract value. Not provided → mark `<to be provided>` with a hint from serviceDescription.\n\
                     \x20\x20\x20\x20- Format as natural-language `key：value` pairs (separated by `；` or `\\n`). No JSON.\n\
                     \x20\x20\x20\x205. **Route by inference result:**\n\
                     \x20\x20\x20\x20- **serviceDescription is empty OR all fields filled** (no `<to be provided>` marks) → call `set-asp` directly:\n\
                     \x20\x20\x20\x20```bash\n\
                     \x20\x20\x20\x20onchainos agent set-asp {job_id} --provider-agent-id <agentId> --service-id <sid> --service-type <serviceType> --service-params '<inferred or empty>' --service-token-address <feeToken> --service-token-amount <feeAmount>\n\
                     \x20\x20\x20\x20```\n\
                     {success_line}\
                     \x20\x20\x20\x20- **Some fields filled, some marked `<to be provided>`** → pre-fill and ask user to confirm/modify — enqueue:\n\
                     \x20\x20\x20\x20```bash\n\
                     \x20\x20\x20\x20onchainos agent pending-decisions-v2 request --job-id {job_id} --role buyer --agent-id {agent_id} --source-event set_asp_params --user-content \"<compose from template below>\" --list-label \"[SetASP <shortJobId>] confirm service params\"\n\
                     \x20\x20\x20\x20```\n\
                     \x20\x20\x20\x20`--user-content` template (🌐 localize):\n\
                     \x20\x20\x20\x20You selected Agent <agentId> — <serviceName>.\n\
                     \x20\x20\x20\x20Service: <serviceDescription>\n\
                     \x20\x20\x20\x20Fee: <feeAmount> <feeTokenSymbol>\n\
                     \x20\x20\x20\x20\n\
                     \x20\x20\x20\x20Pre-filled service params (please confirm or modify):\n\
                     \x20\x20\x20\x20<inferred serviceParams with `<to be provided>` marks>\n\
                     \x20\x20\x20\x20\n\
                     \x20\x20\x20\x20Reply \"ok\" to confirm, or provide corrections.\n\
                     \x20\x20\x20\x20[SERVICE_CONTEXT providerAgentId=<agentId> serviceId=<sid> serviceType=<serviceType> serviceTokenAddress=<feeToken> serviceTokenAmount=<feeAmount> inferredParams=<inferred serviceParams>]\n\
                     \x20\x20\x20\x20- **Nothing extractable** (serviceDescription is vague, task description has no matching values) → ask user — enqueue:\n\
                     \x20\x20\x20\x20```bash\n\
                     \x20\x20\x20\x20onchainos agent pending-decisions-v2 request --job-id {job_id} --role buyer --agent-id {agent_id} --source-event set_asp_params --user-content \"<compose from template below>\" --list-label \"[SetASP <shortJobId>] provide service params\"\n\
                     \x20\x20\x20\x20```\n\
                     \x20\x20\x20\x20`--user-content` template (🌐 localize):\n\
                     \x20\x20\x20\x20You selected Agent <agentId> — <serviceName>.\n\
                     \x20\x20\x20\x20Service: <serviceDescription>\n\
                     \x20\x20\x20\x20Fee: <feeAmount> <feeTokenSymbol>\n\
                     \x20\x20\x20\x20\n\
                     \x20\x20\x20\x20Please describe the input for this service (serviceParams):\n\
                     \x20\x20\x20\x20[SERVICE_CONTEXT providerAgentId=<agentId> serviceId=<sid> serviceType=<serviceType> serviceTokenAddress=<feeToken> serviceTokenAmount=<feeAmount>]\n\
                     \x20\x20\x20\x20**`--list-label` must be localized to the user's language**.\n\
                     \x20\x20\x20\x20⚠️ If user said B / specify **without** an agentId: re-ask via `pending-decisions-v2 request --source-event negotiate_over_budget` asking for the agentId; **`--user-content` and `--list-label` must be localized to the user's language** (English ref: \"Please provide the 3-digit agentId of the ASP you want to use (e.g. `864`)\").\n\
                     \x20\x20• **C — Close** — typical intents: C / 选C / `close` / `关闭` / `取消` / `cancel`. Action: `onchainos agent close {job_id}`.\n\n\
                     ⚠️ If ambiguous: re-ask via `pending-decisions-v2 request` with `--source-event negotiate_over_budget`. **`--user-content` and `--list-label` must be localized to the user's language**. Reference (English): \"I didn't catch your reply, please clarify: A=view ASP list  B=specify another ASP (include the agentId)  C=close the job\".\n"
                    )
                },
                "apply_over_budget" | "job_provider_reject" => {
                    let switch_asp = switch_asp_routing(job_id, agent_id, &source);
                    let scene_lead = if source == "apply_over_budget" {
                        "ASP applied but quote exceeded max budget; apply auto-rejected."
                    } else {
                        "ASP declined to take this task; the apply has been reset."
                    };
                    format!(
                    "[User decision relay] source_event=`{source}`, user's verbatim reply: `{reply}`\n\n\
                     {scene_lead} Options: A=browse / B=designate / (C=make public if private) / last=close. **Semantic mapping**:\n\n\
                     \x20\x20• **A — Browse ASP list** — typical intents: A / 选A / `推荐` / `列表` / `list` / `浏览`. Action: `onchainos agent asp-match --job-id {job_id}` → compose the ASP list as `--user-content` for `pending-decisions-v2 request --source-event asp_match_pick`. **All footer keywords must be localized**.\n\
                     \x20\x20• **B — Specify another ASP** — typical intents: B / 选B / `specify` / `指定`, **with a 3-digit agentId** (e.g. `B 864` / `指定 864`). Action (switch-asp flow):\n\
                     {switch_asp}\
                     \x20\x20• **C — Make public** — typical intents: C / 选C / `public` / `公开`. Action: `onchainos agent set-public {job_id}`. (Harmless no-op if already public.)\n\
                     \x20\x20• **Close** (last option, C or D) — typical intents: `close` / `关闭` / `取消` / `cancel`. Action: `onchainos agent close {job_id}`.\n\n\
                     ⚠️ If ambiguous: re-ask via `pending-decisions-v2 request` with `--source-event {source}`. **`--user-content` and `--list-label` must be localized**.\n"
                )},
                "x402_price_mismatch" => format!(
                    "[User decision relay] source_event=`x402_price_mismatch`, user's verbatim reply: `{reply}`\n\n\
                     The push was an Accept/Reject choice (x402 endpoint price differs from the registered fee). **Semantic mapping** — decide:\n\n\
                     \x20\x20• **Accept** — typical intents: A / 选A / `accept` / `接受` / `同意` / `agree` / yes / OK.\n\
                     \x20\x20\x20\x20Read `endpoint`, `amountHuman`, `tokenSymbol`, `acceptsJson` from the `[PRICE_CONTEXT]` block in the `--llm-content` of the pending decision.\n\
                     \x20\x20\x20\x20Proceed to set-payment-mode:\n\n\
                     \x20\x20\x20\x20Check `paymentMode` from the `[Pre-fetched task context]` or from context.\n\
                     \x20\x20\x20\x20▸ **If paymentMode is already `3`** → skip `set-payment-mode`:\n\
                     \x20\x20\x20\x20```bash\n\
                     \x20\x20\x20\x20onchainos agent next-action --role buyer --agentId {agent_id} --message '{{\"event\":\"job_payment_mode_changed\",\"jobId\":\"{job_id}\",\"paymentMode\":3}}'\n\
                     \x20\x20\x20\x20```\n\
                     \x20\x20\x20\x20▸ **Otherwise** → push payment mode on-chain:\n\
                     \x20\x20\x20\x20```bash\n\
                     \x20\x20\x20\x20onchainos agent set-payment-mode {job_id} --payment-mode x402 --token-symbol <tokenSymbol> --token-amount <amountHuman> --endpoint <endpoint>\n\
                     \x20\x20\x20\x20```\n\
                     \x20\x20\x20\x20**Result branch:**\n\
                     \x20\x20\x20\x20\x20\x20- `\"alreadySet\": true` → call `onchainos agent next-action --role buyer --agentId {agent_id} --message '{{\"event\":\"job_payment_mode_changed\",\"jobId\":\"{job_id}\",\"paymentMode\":3}}'` immediately.\n\
                     \x20\x20\x20\x20\x20\x20- `\"confirming\": true` → **end this turn** and wait for `job_payment_mode_changed`.\n\n\
                     \x20\x20• **Reject** — typical intents: B / 选B / `reject` / `拒绝` / no / `换`.\n\
                     \x20\x20\x20\x20Action: `onchainos agent mark-failed {job_id} --provider <designated agentId from context>` then `onchainos agent asp-match --job-id {job_id}` to fetch alternatives; if list non-empty → compose as `--user-content` for `pending-decisions-v2 request --source-event asp_match_pick` (**localize all footer keywords**); if empty → push via `--source-event no_asp_found`.\n\n\
                     ⚠️ If ambiguous: re-ask via `pending-decisions-v2 request` with `--source-event x402_price_mismatch`. **`--user-content` and `--list-label` must be localized to the user's language**. Reference (English): \"I didn't catch your reply, please clarify: A=accept this price  B=reject and switch ASP\".\n"
                ),
                "x402_input_required" => format!(
                    "[User decision relay] source_event=`x402_input_required`, user's verbatim reply: `{reply}`\n\n\
                     The user was shown the x402 inputRequired field form (pre-filled from serviceParams + blanks for user input). **Semantic mapping** — decide:\n\n\
                     \x20\x20• **Confirm** — typical intents: A / 选A / `confirm` / `确认` / `ok` / `yes` / `好` / `可以`. Use the pre-filled values as-is.\n\
                     \x20\x20• **Provide/modify values** — user typed field values or corrections (e.g. `address: 0x123...`, `B` + new values). Parse the reply, update the fields.\n\n\
                     **Execution flow (follow in strict order):**\n\n\
                     **Step 1 — Parse the user's reply and assemble the `--body` JSON:**\n\
                     \x20\x20- If confirm → use the pre-filled values from the `[IR_CONTEXT]` block in the `--llm-content` of the pending decision.\n\
                     \x20\x20- If user provided new/modified values → merge with pre-filled values (user input overrides).\n\
                     \x20\x20- Assemble all field values into a flat JSON object.\n\n\
                     **Step 2 — Validate the body via `x402-check --body`:**\n\
                     Read `endpoint` from the `[IR_CONTEXT]` block. If missing, fallback to `onchainos agent asp-match --job-id {job_id} --provider-agent-id <providerAgentId> --format json`.\n\
                     ```bash\n\
                     onchainos agent x402-check --endpoint <endpoint> --agent-id {agent_id} --body '<assembled JSON from Step 1>'\n\
                     ```\n\
                     \x20\x20- If the re-check returns `valid: true` → extract `acceptsJson`, `amountHuman`, `tokenSymbol` and proceed to **Step 3**.\n\
                     \x20\x20- If the re-check fails → notify the user of the validation error and re-ask via `pending-decisions-v2 request` with `--source-event x402_input_required`.\n\n\
                     **Step 2b — Price & budget guard:**\n\
                     Compare `amountHuman` from x402-check output against the fee and budget (check in this order — over-budget takes priority):\n\n\
                     \x20\x201. **Over-budget**: Read `maxBudget` from the `[Pre-fetched task context]`. If `maxBudget` > 0 AND `amountHuman` > `maxBudget`:\n\
                     \x20\x20\x20\x20Push an `over_budget` decision card:\n\
                     \x20\x20\x20\x20```bash\n\
                     \x20\x20\x20\x20onchainos agent pending-decisions-v2 request --job-id {job_id} --role buyer --agent-id {agent_id} --source-event over_budget --list-label \"[Over budget <shortJobId>] budget decision\" --user-content \"<compose from template below>\"\n\
                     \x20\x20\x20\x20```\n\
                     \x20\x20\x20\x20🌐 `--user-content` template (translate to user's language):\n\
                     \x20\x20\x20\x20The x402 endpoint's actual price is <amountHuman> <tokenSymbol>, which exceeds your max budget (<maxBudget>). Choose next step:\n\
                     \x20\x20\x20\x20A. Specify another ASP — provide the agentId\n\
                     \x20\x20\x20\x20B. Make the job public\n\
                     \x20\x20\x20\x20C. Close the job\n\
                     \x20\x20\x20\x20→ **end this turn** and wait for the user's reply.\n\n\
                     \x20\x202. **Price-mismatch**: Read `feeAmount` from the `[IR_CONTEXT]` block. If both values > 0 AND `|amountHuman - feeAmount| / feeAmount > 0.01` (delta > 1%):\n\
                     \x20\x20\x20\x20Push a `x402_ir_price_confirm` decision card:\n\
                     \x20\x20\x20\x20```bash\n\
                     \x20\x20\x20\x20onchainos agent pending-decisions-v2 request --job-id {job_id} --role buyer --agent-id {agent_id} --source-event x402_ir_price_confirm --list-label \"[x402 price <shortJobId>] price confirmation\" --user-content \"<compose from template below>\"\n\
                     \x20\x20\x20\x20```\n\
                     \x20\x20\x20\x20🌐 `--user-content` template (translate):\n\
                     \x20\x20\x20\x20[Job <shortJobId>] The x402 endpoint's actual price is <amountHuman> <tokenSymbol>, which differs from the registered fee <feeAmount> <feeTokenSymbol>. Accept this price?\n\
                     \x20\x20\x20\x20A. Accept — continue with this price\n\
                     \x20\x20\x20\x20B. Reject — switch to another ASP\n\
                     \x20\x20\x20\x20`--llm-content` (keep English; fill actual values):\n\
                     \x20\x20\x20\x20```\n\
                     \x20\x20\x20\x20[PRICE_CONTEXT] endpoint=<endpoint> amountHuman=<amountHuman> tokenSymbol=<tokenSymbol> acceptsJson=<acceptsJson> body=<assembled body JSON>\n\
                     \x20\x20\x20\x20```\n\
                     \x20\x20\x20\x20→ **end this turn** and wait for the user's reply.\n\n\
                     \x20\x203. **Both pass** → proceed to **Step 3**.\n\n\
                     **Step 3 — set-payment-mode (if needed):**\n\
                     Check the current task's `paymentMode` from the `[Pre-fetched task context]` or from context.\n\n\
                     \x20\x20▸ **If paymentMode is already `3` (x402)** → skip `set-payment-mode` and call `next-action` immediately:\n\
                     \x20\x20```bash\n\
                     \x20\x20onchainos agent next-action --role buyer --agentId {agent_id} --message '{{\"event\":\"job_payment_mode_changed\",\"jobId\":\"{job_id}\",\"paymentMode\":3}}'\n\
                     \x20\x20```\n\n\
                     \x20\x20▸ **Otherwise** → push payment mode on-chain:\n\
                     \x20\x20```bash\n\
                     \x20\x20onchainos agent set-payment-mode {job_id} --payment-mode x402 --token-symbol <tokenSymbol from Step 2> --token-amount <amountHuman from Step 2> --endpoint <endpoint>\n\
                     \x20\x20```\n\
                     \x20\x20**Result branch:**\n\
                     \x20\x20\x20\x20- Output contains `\"alreadySet\": true` → call `onchainos agent next-action --role buyer --agentId {agent_id} --message '{{\"event\":\"job_payment_mode_changed\",\"jobId\":\"{job_id}\",\"paymentMode\":3}}' ` immediately.\n\
                     \x20\x20\x20\x20- Output contains `\"confirming\": true` → **end this turn** and wait for `job_payment_mode_changed`.\n\n\
                     ⚠️ **Remember the assembled `--body` JSON** — you must pass it to `task-402-pay` in the `job_payment_mode_changed` turn.\n"
                ),
                "x402_ir_price_confirm" => format!(
                    "[User decision relay] source_event=`x402_ir_price_confirm`, user's verbatim reply: `{reply}`\n\n\
                     The user was shown a price-mismatch warning after filling x402 inputRequired fields. **Semantic mapping:**\n\n\
                     \x20\x20• **Accept** — typical intents: A / 选A / `accept` / `接受` / yes / OK.\n\
                     \x20\x20\x20\x20Read `endpoint`, `amountHuman`, `tokenSymbol`, `acceptsJson`, `body` from the `[PRICE_CONTEXT]` block in the `--llm-content` of the pending decision.\n\
                     \x20\x20\x20\x20Proceed to set-payment-mode:\n\n\
                     \x20\x20\x20\x20Check `paymentMode` from the `[Pre-fetched task context]` or from context.\n\
                     \x20\x20\x20\x20▸ **If paymentMode is already `3`** → skip `set-payment-mode`:\n\
                     \x20\x20\x20\x20```bash\n\
                     \x20\x20\x20\x20onchainos agent next-action --role buyer --agentId {agent_id} --message '{{\"event\":\"job_payment_mode_changed\",\"jobId\":\"{job_id}\",\"paymentMode\":3}}'\n\
                     \x20\x20\x20\x20```\n\
                     \x20\x20\x20\x20▸ **Otherwise** → push payment mode on-chain:\n\
                     \x20\x20\x20\x20```bash\n\
                     \x20\x20\x20\x20onchainos agent set-payment-mode {job_id} --payment-mode x402 --token-symbol <tokenSymbol> --token-amount <amountHuman> --endpoint <endpoint>\n\
                     \x20\x20\x20\x20```\n\
                     \x20\x20\x20\x20**Result branch:**\n\
                     \x20\x20\x20\x20\x20\x20- `\"alreadySet\": true` → call `onchainos agent next-action --role buyer --agentId {agent_id} --message '{{\"event\":\"job_payment_mode_changed\",\"jobId\":\"{job_id}\",\"paymentMode\":3}}'` immediately.\n\
                     \x20\x20\x20\x20\x20\x20- `\"confirming\": true` → **end this turn** and wait for `job_payment_mode_changed`.\n\n\
                     \x20\x20\x20\x20⚠️ **Remember the `body` from PRICE_CONTEXT** — pass it to `task-402-pay --body` in the `job_payment_mode_changed` turn.\n\n\
                     \x20\x20• **Reject** — typical intents: B / 选B / `reject` / `拒绝` / no / `换`.\n\
                     \x20\x20\x20\x20Action: `onchainos agent mark-failed {job_id} --provider <designated agentId from context>` then `onchainos agent asp-match --job-id {job_id}` to fetch alternatives; if list non-empty → compose as `--user-content` for `pending-decisions-v2 request --source-event asp_match_pick` (**localize all footer keywords**); if empty → push via `--source-event no_asp_found`.\n\n\
                     ⚠️ If ambiguous: re-ask via `pending-decisions-v2 request` with `--source-event x402_ir_price_confirm`. **`--user-content` and `--list-label` must be localized**. Reference (English): \"I didn't catch your reply, please clarify: A=accept this price  B=reject and switch ASP\".\n"
                ),
                "x402_replay_input" => format!(
                    "[User decision relay] source_event=`x402_replay_input`, user's verbatim reply: `{reply}`\n\n\
                     The user was asked to provide business parameters for an x402 endpoint that already accepted payment but could not deliver without a request body.\n\n\
                     **Execution flow (follow in strict order):**\n\n\
                     **Step 1 — Parse the user's reply and assemble the `--body` JSON:**\n\
                     \x20\x20Read the `[REPLAY_CONTEXT]` block from the `--llm-content` of the pending decision.\n\
                     \x20\x20Extract field requirements from `requiredFields`.\n\
                     \x20\x20Map the user's reply values to the field names → assemble a flat JSON object.\n\n\
                     **Step 2 — Re-run task-402-pay with `--body`:**\n\
                     Read `endpoint`, `providerAgentId`, `acceptsJson`, `feeTokenSymbol`, `feeAmount` from the `[REPLAY_CONTEXT]` block.\n\
                     ```bash\n\
                     onchainos agent task-402-pay {job_id} --provider-agent-id <providerAgentId> --accepts '<acceptsJson>' --endpoint <endpoint> --token-symbol <feeTokenSymbol> --token-amount <feeAmount> --body '<assembled JSON from Step 1>'\n\
                     ```\n\
                     ⚠️ `task-402-pay` will re-sign (new EIP-3009 proof) and skip direct/accept (already accepted on-chain). The endpoint replay now includes the body.\n\n\
                     **Step 3 — Branch on result:**\n\n\
                     \x20\x20▸ replaySuccess=true:\n\
                     \x20\x20\x20\x20**3a** — Notify user with the FULL deliverable via `okx-a2a user notify`:\n\
                     \x20\x20\x20\x20🌐 Localize. Copy `replayBodyDisplay` verbatim into the notification (do NOT summarize or truncate).\n\
                     \x20\x20\x20\x20**3b** — Run `complete` immediately (the `job_accepted` event already passed):\n\
                     \x20\x20\x20\x20```bash\n\
                     \x20\x20\x20\x20onchainos agent complete {job_id}\n\
                     \x20\x20\x20\x20```\n\
                     \x20\x20\x20\x20→ **End this turn.** Wait for `job_completed` event.\n\n\
                     \x20\x20▸ replaySuccess=false:\n\
                     \x20\x20\x20\x20Re-push `pending-decisions-v2 request` with `--source-event x402_replay_input`, include the validation error in `--user-content` so the user can correct their input.\n\
                     \x20\x20\x20\x20→ **End this turn.** Wait for user's corrected reply.\n"
                ),
                "set_asp_params" => {
                    // CLI mode (Claude Code / Codex): drop the passive "Waiting for ASP to accept"
                    // phrase — it reads as a turn-end cue to LLM-driven watch loops and suppresses re-arm.
                    let step3_success = if super::content::is_cli_mode() {
                        "3. On success → notify user (🌐 localize per user's language): \"ASP set to Agent <providerAgentId>.\"\n"
                    } else {
                        "3. On success → notify user (🌐 localize per user's language): \"ASP set to Agent <providerAgentId>. Waiting for ASP to accept the task.\"\n"
                    };
                    format!(
                    "[User decision relay] source_event=`set_asp_params`, user's verbatim reply: `{reply}`\n\n\
                     The user was asked for serviceParams after selecting an ASP. The decision may have included pre-filled (inferred) values in `inferredParams` inside the `[SERVICE_CONTEXT]` block.\n\n\
                     **Step 1 — Determine serviceParams from user's reply:**\n\
                     - **Confirm** — user says \"ok\" / \"确认\" / \"yes\" / \"好\" / \"可以\" / \"没问题\" → use `inferredParams` from `[SERVICE_CONTEXT]` as-is. If no `inferredParams` exists, use empty string.\n\
                     - **Modify** — user corrects specific fields (e.g. \"名称改成 DOGE\", \"change name to DOGE\") → take `inferredParams` as base, apply user's corrections to the matching fields, keep other fields unchanged.\n\
                     - **Full input** — user provides a complete new description (not referencing pre-filled values) → use user's reply verbatim as serviceParams.\n\n\
                     **Step 2 — Retrieve service info** from `[SERVICE_CONTEXT]`: `providerAgentId`, `serviceId`, `serviceType`, `serviceTokenAddress`, `serviceTokenAmount`.\n\n\
                     **Step 3 — Call set-asp:**\n\
                     ```bash\n\
                     onchainos agent set-asp {job_id} --provider-agent-id <providerAgentId> --service-id <serviceId> --service-type <serviceType> --service-params '<resolved serviceParams from Step 1>' --service-token-address <serviceTokenAddress> --service-token-amount <serviceTokenAmount>\n\
                     ```\n\
                     {step3_success}\
                     4. **Create sub session + SKILL_PREFETCH** (only after set-asp succeeds):\n\
                     ```bash\n\
                     okx-a2a session create --job-id {job_id} --my-agent-id {agent_id} --to-agent-id <providerAgentId> --json\n\
                     ```\n\
                     Then send SKILL_PREFETCH:\n\
                     ```bash\n\
                     okx-a2a session send --session-key <sessionKey from above> --content '[SKILL_PREFETCH] Read the okx-agent-task skill. Pre-load buyer role context.'\n\
                     ```\n\
                     5. On failure → relay the error to the user and re-ask via `pending-decisions-v2 request` with `--source-event set_asp_params`.\n\
                     6. End the turn.\n"
                    )
                },
                _ => format!(
                    "[User decision relay] source_event=`{source}` (no specific routing rule defined for this scene), user's verbatim reply: `{reply}`\n\n\
                     **Manual routing required** — inspect the scene context (call `onchainos agent common context {job_id} --role buyer --agent-id {agent_id}` if needed) and decide semantically which pseudo-event the user's reply maps to. Then call `onchainos agent next-action --role buyer --agentId {agent_id} --message '{{\"event\":\"<chosen-pseudo-event>\",\"jobId\":\"{job_id}\"}}'`.\n"
                ),
            };
            format!("{ud_guard}{ud_body}")
        }

        // Catch-all: any variant the buyer doesn't have a dedicated arm for
        // (e.g. provider-side events like `JobAspSelected`, plus all future
        // additions to the Event enum) falls through to the staking/unknown
        // diagnostic. Using `_` instead of `Event::Other(_)` so the compiler
        // doesn't force a new arm every time the enum grows.
        _ => super::flow_lifecycle::staked_and_unknown(event.as_str(), job_id),
    };

    // Minimal-output short-circuit: applies to events whose body is self-contained
    // and does NOT call any of the IRON-RULE-governed commands (okx-a2a xmtp-send /
    // okx-a2a session status / sessions_spawn / pending-decisions-v2 request).
    // Skip every preamble (the IRON RULEs do not apply) and version_prefix
    // (no `okx-a2a xmtp-send` call to validate).
    let use_cli_minimal = matches!(event_str,
            "job_created" | "provider_conversation_pick" |
            "negotiate_reply" |
            "provider_applied" | "deliverable_received" | "approve_review" |
            "review_expired" | "job_expired" | "job_auto_refunded" |
            "submit_expired" | "reject_expired" |
            "close" | "set_public"
        );
    let core = if use_cli_minimal
        || event_str == "create_task"
    {
        body
    } else {
        format!("{preamble_slim}{prefetched_block}{body}")
    };
    let result = if use_cli_minimal {
        core
    } else {
        format!("{localization_prefix}{version_prefix}{core}")
    };
    if DEBUG_LOG {
        let preview: String = result.chars().take(200).collect();
        eprintln!(
            "[buyer-flow] output length: {} chars | first 200: {}",
            result.len(),
            preview
        );
    }
    result
}
