//! Task creation, attachment forwarding, and term-change event prompt generators.

// --- User-action: create task ------------------------------------------

pub(crate) fn create_task() -> String {
    "\
[Current Operation] Publish task (create_task)
[Role] User (User Agent)
[Session Type] user session (talking directly to the user)

🛑 Collect all fields → show confirmation form → wait for explicit user confirmation → call CLI. Default: `create-task` (Step 6). If user says \"save as draft\" / \"草稿\" → jump to Step 6-D.

================================================
Step 1 -- Field collection (collect progressively in conversation; **only enter Step 2 when all fields are ready**)
================================================

⚠️ Description, Budget, Max budget, Currency: MUST come from user's explicit input — no guessing/auto-fill. Title and Summary: agent-generated. Acceptance/Delivery window: system defaults, do not ask or show.

| Field | CLI flag | Constraint | How to collect |
|---|---|---|---|
| Description | --description | 20-2000 chars | Consolidate user's words. If <20 → ask to expand |
| Title | --title | <=30 chars | Agent-generated; count chars, shorten if >30 |
| Summary | --description-summary | <=200 chars | Agent-generated; count chars, shorten if >200 |
| Payment token | --currency | Only USDT / USDG | Fuzzy input (\"U\"/\"USD\") → ask \"USDT or USDG?\" |
| Budget | --budget | number; <=5 decimals; max 10M | Ask user explicitly |
| Max budget | --max-budget | Required; >= budget; <=5 decimals; max 10M | Ask user explicitly (negotiation price cap) |
| Designated provider | --provider | optional; provider agentId | Extract if user names one; do not ask proactively |

================================================
Step 2 -- Validation (after all fields collected, before showing the form)
================================================

1. Token not USDT/USDG → reject
2. Budget & max budget use different tokens → block, ask which one
3. Description < 20 chars → ask to expand
4. max_budget < budget → reject
5. max_budget missing → ask
6. budget > 10M or > 5 decimal places → reject

================================================
Step 4.5 -- ASP matching (after communication check, before confirmation form)
================================================

**A. User designated a provider** (`--provider` is set):

```bash
onchainos agent asp-match --task-desc \"<description>\" --provider-agent-id <agentId> --format json
```

- Empty → \"This ASP has no registered services. Choose another or remove designation.\"
- Non-empty → extract top service: `serviceId`, `serviceName`, `serviceDescription`, `serviceType`, `feeAmount`→serviceTokenAmount, `feeToken`→serviceTokenAddress, `feeTokenSymbol`, `endpoint` (if A2MCP).
- Validate: task `currency` must match `feeTokenSymbol`. Mismatch → ask user to change token or ASP.

**B. User did NOT designate a provider:**

```bash
onchainos agent asp-match --task-desc \"<description>\"
```

Show as numbered list: `Agent <id> — security/feedback/sold | Service: <name> (<type>) — <fee> | 「<desc>」`. End turn; wait for reply.

**User reply routing:**
- Number → select ASP, extract service fields, validate currency match. Pass → Step 4.6. Fail → show error.
- \"more\" / \"更多\" → `asp-match --page <next>`.
- Empty list → offer: A. Refine description, B. Designate ASP by agentId, C. Publish as public task (`visibility=0`, skip Step 4.6).

================================================
Step 4.6 -- serviceParams inference
================================================

Using the selected service's `serviceDescription` + `serviceName` + the user's task `description`, infer a `serviceParams` plain-text string.

**Step 1 — Identify required user input** from `serviceDescription`:
Read the serviceDescription semantically and determine what specific input the user must provide to use this service. Common patterns:
- Action verbs directed at the user (specify / provide / input / enter / describe / tell / set up)
- Conditional phrases implying expected input (\"after receiving [X]\", \"given [X]\", \"just say [X]\")
- Templates with placeholders (\"from A to B\", \"some [X]\", \"a specific [X]\")
- Examples showing expected input format (after \"example\" / \"e.g.\")
- Compound input (\"a one-line description + an image\")
If the serviceDescription only describes the service's **output or capabilities** without indicating any user-provided input → no serviceParams needed, skip to Step 4.

**Step 2 — Match against user's task description**:
For each required input from Step 1, check if the user's task description already provides it:
- **Provided** → extract the concrete value
- **Not provided** → mark as `<to be provided>`, with a hint derived from the serviceDescription (e.g. serviceDescription says \"input an EVM address\" but user didn't specify → `EVM address: <to be provided>`)

**Step 3 — Format**: natural-language `key：value` pairs separated by `；` or `\\n`. Do NOT use JSON.

**Step 4 — Confidence routing:**
- All fields filled (no `<to be provided>` marks) → use inferred serviceParams directly in the confirmation form
- Some fields marked `<to be provided>` → show in confirmation form with marks; user can edit before confirming
- No input required (Step 1 found nothing) → serviceParams is empty

Do NOT ask the user for serviceParams separately — always show in the confirmation form (Step 5). The user can correct it there.

================================================
Step 5 -- Show the confirmation form
================================================

| Field | Value |
|---|---|
| Title | <short title, <=30 chars> |
| Summary | <brief summary of the task, <=200 chars> |
| Description | <full content> (if <=200 chars, put it in the table; if >200, write `see below` and render full content below) |
| Payment token | <USDT or USDG> |
| Budget | <number> |
| Max budget | <number> (negotiation price cap) |
| Provider | Agent <providerAgentId> (or \"Public — no designated provider\" if public) |
| Service | <serviceName> |
| Service desc | <serviceDescription> |
| Service price | <feeAmount> <feeTokenSymbol> (only show this row if feeAmount has a value) |
| Service params | <serviceParams readable display, or \"None\"> |
| Payment mode | escrow (A2A) or x402 (A2MCP) |

Payment mode: A2A → `escrow`, A2MCP → `x402` (from serviceType; do not ask user).
Public task: omit Service/Service desc/Service price/Service params/Payment mode rows.

> Confirm and publish? Or save as draft?

→ **End this turn**; wait for the user's reply.

================================================
Step 5.5 -- Route by user decision (separate turn from Step 5)
================================================

- Confirm / publish → Step 6
- \"save as draft\" / \"草稿\" → Step 6-D
- Edit description → update → **re-run Step 4.5** (new description may match different ASPs) → Step 4.6 → Step 5
- Edit budget/max-budget/serviceParams → update → Step 5
- Edit currency → update → re-validate currency consistency → Step 5
- Change ASP → Step 4.5 Branch B
- Ambiguous → ask: publish or draft?

================================================
Step 6 -- Publish (create-task)
================================================

```bash
onchainos agent create-task \\
  --description \"<description>\" --description-summary \"<summary>\" --title \"<title>\" \\
  --budget <budget> --max-budget <max_budget> --currency <USDT|USDG> \\
  --provider <agentId> --service-id <sid> --payment-mode <escrow|x402> \\  # private task
  [--service-params '<params>'] [--service-token-address <addr>] [--service-token-amount <amt>]
```
- Private task (default): `--provider`, `--service-id`, `--payment-mode` required. Payment mode: A2A→escrow, A2MCP→x402.
- Public task: replace provider/service flags with `--visibility 0`.
- CLI error → relay to user, do NOT auto-modify → return to Step 5.

================================================
Step 6.5 -- Save attachments
================================================

If the user included file(s)/image(s) as task material → for each: `onchainos agent task-attach --file \"<path>\" <jobId>`. Download to local path first if needed. Failure → skip (do not block). No files → skip this step.

================================================

After success, tell the user directly (you are in the user session, no `okx-a2a user notify` needed):\n\
".to_string()
    + &format!("\
- Private: \"{create_designated}\"\n\
- Public: \"{create_public}\"\n\
⚠️ Append `⚠️ Insufficient ... balance` warning from CLI output if present. 🌐 Localize.\n\n\
🛑 **STOP** — after create-task + task-attach (if any), end this turn. Exception: if CLI output contains `[Watch]` block → read `skills/okx-task-watch/SKILL.md`, execute watch, then end. Do not say \"published\"/\"succeeded\" (only submitted). No other commands; no describing subsequent flow.\n\n\
================================================\n\
Step 6-D -- Draft path\n\
================================================\n\
Required: `--title` (≤30), `--description` (≥20), `--description-summary` (≤200). With provider → also `--service-id`.\n\n\
```bash\n\
onchainos agent draft create --title \"<title>\" --description \"<desc>\" --description-summary \"<summary>\" [--budget <n>] [--max-budget <n>] [--currency <sym>] [--provider <agentId> --service-id <id>] [--service-params '<params>'] [--service-token-address <addr>] [--service-token-amount <amt>]\n\
```\n\
Error → relay, do NOT auto-modify. After success → \"{draft_saved}\" 🌐 Localize. End turn.\n",
        create_designated = super::super::content::create_task_designated_user_notify(),
        create_public = super::super::content::create_task_public_user_notify(),
        draft_saved = super::super::content::draft_saved_user_notify(),
    )
}

// --- User-action: publish draft ----------------------------------------

pub(crate) fn draft_publish(job_id: &str) -> String {
    format!("\
[Current Operation] Publish draft (draft_publish)
[Role] User (User Agent)
[Session Type] user session (talking directly to the user)

================================================
Step 1 -- Call draft publish CLI
================================================

```bash
onchainos agent draft publish {job_id}
```
⚠️ `{job_id}` is a **positional argument**, NOT a flag. Do NOT use `--job-id`.

Backend validates all required fields, checks balance, signs the transaction, and broadcasts on-chain.

🛑 **Error handling**: if the CLI returns a validation error (missing fields, invalid values, insufficient balance, etc.), relay the error message to the user verbatim. **Do NOT auto-fix.** The user can update the draft via `draft update` and retry.

================================================
Step 2 -- Notify user
================================================

After success, tell the user directly (do NOT call `okx-a2a user notify` — you are already in the user session):
- No designated provider → \"{publish_public}\"
- With designated provider → \"{publish_designated}\"
⚠️ If the CLI output contains a `⚠️ Insufficient ... balance` warning line, append it to the message above.
🌐 Localize to the user's language.

===============================================================
🛑🛑🛑 STOP -- after draft publish + watch (if prompted), you **MUST end this turn**
===============================================================
✅ **Exception: `[Watch]` hint** -- if the CLI output contains a `[Watch]` block, you MUST first read `skills/okx-task-watch/SKILL.md` (if not already read this session), then execute the watch per its §Run watch using the jobId in the `[Watch]` block, before ending the turn. Do NOT short-circuit by guessing the bash command.
❌ **Do not say \"task published\" or \"publish succeeded\"** -- draft publish only submits the transaction; it is not yet confirmed on-chain.
❌ **Do not call any other onchainos agent commands** (except the watch above) -- all further actions are driven by on-chain events.
===============================================================\n",
        publish_public = super::super::content::draft_publish_public_user_notify(),
        publish_designated = super::super::content::draft_publish_designated_user_notify(),
    )
}

// --- Attachment forwarding ---------------------------------------------

/// Upload + forward a single attachment file in Rust. Returns Ok(()) on
/// success or Err with a human message on failure.
fn upload_and_forward_one(
    file_path: &str,
    agent_id: &str,
    job_id: &str,
    to_agent_id: &str,
) -> Result<(), String> {
    use crate::commands::agent_commerce::task::common::okx_a2a;

    let upload = okx_a2a::file_upload(file_path, agent_id, job_id, None, None)
        .map_err(|e| format!("file upload failed for {file_path}: {e}"))?;

    let msg = format!(
        "jobId: {job_id}\n\
         attachmentType: file\n\
         fileKey: {file_key}\n\
         digest: {digest}\n\
         salt: {salt}\n\
         nonce: {nonce}\n\
         secret: {secret}\n\
         filename: {filename}\n\
         description: This is an attachment/reference material for the task. The ASP should download it for task execution.\n\
         [intent:attachment]",
        file_key = upload.file_key,
        digest = upload.digest,
        salt = upload.salt,
        nonce = upload.nonce,
        secret = upload.secret,
        filename = upload.filename,
    );

    okx_a2a::xmtp_send(job_id, to_agent_id, &msg)
        .map_err(|e| format!("xmtp-send failed for {file_path}: {e}"))
}

/// Upload + forward ALL pending attachments for a job. Best-effort: failures
/// are logged but do not block the caller. Returns the count of successfully
/// forwarded files.
pub(crate) fn upload_and_forward_all_attachments(
    job_id: &str,
    agent_id: &str,
    to_agent_id: &str,
) -> usize {
    use crate::commands::agent_commerce::task::common::DEBUG_LOG;

    let files = super::super::attachments::list_attachment_paths(job_id);
    if files.is_empty() {
        return 0;
    }
    let mut ok_count = 0usize;
    for fp in &files {
        match upload_and_forward_one(fp, agent_id, job_id, to_agent_id) {
            Ok(()) => {
                ok_count += 1;
                if DEBUG_LOG {
                    eprintln!("[attachment_cli] ✓ forwarded: {fp}");
                }
            }
            Err(e) => {
                eprintln!("[attachment_cli] ⚠ skipped: {e}");
            }
        }
    }
    ok_count
}

/// Rust fast-path for `attachment_added`: upload + xmtp-send in-process,
/// then return a notify-only prompt for the LLM.
pub(crate) fn attachment_added_cli(
    ctx: &super::super::flow::FlowContext<'_>,
    message: Option<&serde_json::Value>,
) -> String {
    let job_id = ctx.job_id;
    let agent_id = ctx.agent_id;
    let short_id = ctx.short_id;

    let file_path = message
        .and_then(|m| m.get("filePath"))
        .and_then(|v| v.as_str())
        .unwrap_or("");
    if file_path.is_empty() {
        return format!(
            "[attachment_added_cli] ERROR: filePath missing in --message JSON.\n\n\
             [Your next action] Notify the user:\n\
             ```bash\n\
             okx-a2a user notify --content '<localized: Attachment forwarding failed — file path was not provided. Please retry via task-attach.>'\n\
             ```\n"
        );
    }

    let to_agent_id = ctx.prefetched
        .and_then(|p| p.provider_agent_id.as_deref())
        .unwrap_or("");
    if to_agent_id.is_empty() {
        return format!(
            "[attachment_added_cli] ERROR: provider not assigned — cannot forward attachment.\n\n\
             [Your next action] Notify the user:\n\
             ```bash\n\
             okx-a2a user notify --content '<localized: [Job {short_id}] Attachment saved locally but no provider assigned yet. It will be forwarded automatically once a provider accepts the task.>'\n\
             ```\n"
        );
    }

    match upload_and_forward_one(file_path, agent_id, job_id, to_agent_id) {
        Ok(()) => {
            let att_sent = super::super::content::attachment_sent_user_notify()
                .replace("<short_jobId>", short_id);
            format!(
                "[attachment_added_cli] ✓ Attachment uploaded and forwarded to provider in-process.\n\n\
                 [Your next action] Notify the user and end turn.\n\n\
                 Content:\n\
                 \x20\x20{att_sent}\n\n\
                 ```bash\n\
                 okx-a2a user notify --content '<localized content>'\n\
                 ```\n\
                 **End this turn.**\n"
            )
        }
        Err(e) => {
            eprintln!("[attachment_added_cli] upload/forward failed: {e}");
            format!(
                "[attachment_added_cli] ERROR: upload/forward failed: {e}\n\n\
                 [Your next action] Notify the user that the attachment could not be sent.\n\n\
                 ```bash\n\
                 okx-a2a user notify --content '<translate: [Job {short_id}] Attachment forwarding failed. Please retry later.>'\n\
                 ```\n\n\
                 **End this turn.**\n"
            )
        }
    }
}

// --- Term-change events ------------------------------------------------

pub(crate) fn task_token_budget_change(ctx: &super::super::flow::FlowContext<'_>) -> String {
    let _job_id = ctx.job_id;

    format!(
    "[System Notification] task_token_budget_change (payment token / amount change settled on-chain)\n\
     [Role] User (User Agent)\n\n\
     ⚠️ This event is triggered by the user session calling `set-token-and-budget`. The terms are now updated on-chain.\n\n\
     [Receiving-scenario decision -- 🛑 MANDATORY]\n\
     This event is broadcast to all user-side sub sessions.\n\
     - If you are the **backup session** → **ignore this event, end the turn immediately, do not call any tool**\n\
     - If you are a **sub session (a negotiation session with a specific provider)** → **also ignore this event, end the turn**\n\n\
     Rationale: price is locked at accept time, not negotiated in chat. The on-chain tokenSymbol / tokenAmount update is visible to the ASP via task-detail queries; no `okx-a2a xmtp-send` propagation is needed.\n\n\
     ❌ Do not run `okx-a2a xmtp-send` to the provider (price talk is forbidden in chat).\n\
     ❌ Do not run `okx-a2a user notify` (the user already knows about the change in the user session).\n\
     ❌ Do not call set-token-and-budget / set-asp / set-max-budget (the user session already did).\n"
    )
}
