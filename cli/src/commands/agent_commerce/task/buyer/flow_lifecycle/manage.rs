//! Task creation, attachment forwarding, and term-change event prompt generators.

// --- User-action: create task ------------------------------------------

pub(crate) fn create_task() -> String {
    "\
[Current Operation] Publish task (create_task)
[Role] User (User Agent)
[Session Type] user session (talking directly to the user)

🛑 **No skipping**: you MUST finish collecting all fields → show the confirmation form → wait for an explicit user confirmation before calling the CLI.
❌ **Do NOT use `draft create` + `draft publish` as a substitute for `create-task`** — they are completely different flows. `create-task` publishes the task on-chain in one step. The draft flow (`draft create` → `draft update` → `draft publish`) is ONLY for when the user explicitly says \"save as draft\". If the user says \"publish a task\" / \"发布任务\" / \"create a task\", you MUST use `create-task` (Step 6), NOT the draft path.
💡 **Draft shortcut**: if the user says \"save as draft\" / \"先保存草稿\" / \"草稿\" at ANY point during field collection, **jump to Step 6-D**. Draft only requires `--title`, `--description` (≥20 chars), `--description-summary`. If a provider is designated, `--service-id` is also required. Other fields are optional.

================================================
Step 1 -- Field collection (collect progressively in conversation; **only enter Step 2 when all fields are ready**)
================================================

🛑🛑🛑 **ABSOLUTE RULE — No auto-fill for user-provided fields**:
The following fields MUST come from the user's explicit input: **Description, Budget, Max budget, Currency**.
If the user has NOT explicitly stated a field's value, you MUST ask for it — do NOT guess, infer, generate a default, or extract an implied value from the task description.
Even if the user's description hints at a price range (e.g. \"大概50块\"), you MUST confirm with the user before filling.
Only **Title** and **Summary** are agent-generated (from the user's description).
Acceptance window and Delivery window are system defaults — do NOT ask the user for these, and do NOT show them in the confirmation form.

| Field | CLI flag | Constraint | How to collect |
|---|---|---|---|
| Description | --description | 20-2000 chars | Consolidate the user's words. If <20 → \"A more detailed description helps match a better Provider. Could you add more specifics?\" |
| Title | --title | <=30 chars | Agent-generated; **must count chars after generating**, shorten if >30 |
| Summary | --description-summary | <=200 chars | Agent-generated; **must count chars after generating**, shorten if >200 |
| Payment token | --currency | Only USDT / USDG | ⚠️ Fuzzy input (\"U\"/\"USD\") → ask \"USDT or USDG?\" (see buyer-user.md) |
| Budget | --budget | number; <=5 decimal places; max 10,000,000 | **MUST ask the user; do NOT auto-fill or guess.** Extract the number only after the user states it explicitly |
| Max budget | --max-budget | **Required**; >= budget; <=5 decimal places; max 10,000,000 | ⚠️ **You MUST ask the user explicitly**, do not auto-fill or guess. This is the negotiation price cap; the ASP's quote cannot exceed it |
| Designated provider | --provider | optional; provider agentId | If the user names a specific provider, extract the agentId. **Do not ask proactively** -- if the user does not bring it up, omit it |

================================================
Step 2 -- Validation (after all fields collected, before showing the form)
================================================

1. Token is neither USDT nor USDG → \"Only USDT and USDG are supported. Please choose one.\"
2. **Currency consistency between budget and max budget**: if the user mentions different tokens for budget and max budget (e.g. \"budget 10 USDT, max 20 USDG\") → **block**, \"Budget and max budget must use the same token. Please confirm: USDT or USDG?\". The task has a single --currency, the two must match.
3. Description < 20 chars → ask the user to expand
4. max_budget < budget → \"Max budget cannot be less than the budget.\"
5. max_budget missing → \"Please set the max budget (the negotiation price cap); the ASP's quote cannot exceed it.\"
6. budget > 10,000,000 or > 5 decimal places → tell the user the limits

🛑 Preflight (identity + communication) must have passed before entering this playbook.

================================================
Step 4.5 -- ASP matching (after communication check, before confirmation form)
================================================

🛑 This step runs AFTER Step 4 (communication check) and BEFORE Step 5 (confirmation form).

**Branch by whether the user designated a provider:**

**A. User designated a provider** (`--provider` is set):

```bash
onchainos agent asp-match --task-desc \"<description>\" --provider-agent-id <agentId> --format json
```

Handle the result:
- Empty (ASP has no service) → tell the user: \"This ASP has no registered services. Please choose another ASP or remove the designation.\" → wait for the user to decide.
- Non-empty → extract the top service from the output:
  - `serviceId`, `serviceName`, `serviceDescription`, `serviceType`
  - `feeAmount` (→ `serviceTokenAmount`), `feeToken` (→ `serviceTokenAddress`), `feeTokenSymbol`
  - `endpoint` (if A2MCP)

**Validation (designated):**
1. Currency consistency: task `currency` must match `feeTokenSymbol`. Mismatch → \"Task payment token ({currency}) differs from the service fee token ({feeTokenSymbol}). Please change the task token or choose another ASP.\"

**B. User did NOT designate a provider:**

```bash
onchainos agent asp-match --task-desc \"<description>\"
```

Format the output as a numbered list for the user:
```
Matched ASPs:
1. Agent <id> — security: X | feedback: Y | sold: Z
   Service: <name> (<type>) — <feeAmount> <feeTokenSymbol>
   「<serviceDescription>」
2. ...

Reply with a number to pick, or \"more\" / \"更多\" for the next page.
```

🌐 **Footer keywords must match the user's language** — e.g. Chinese: 回复\"更多\"查看更多; English: reply \"more\" for the next page.

→ **End this turn** and wait for the user's reply.

**User reply routing:**
- Number → select that ASP; extract its service fields; run the same validation as Branch A (currency + budget). Pass → continue to Step 4.6. Fail → show the error and wait.
- \"more\" / \"更多\" / \"下一页\" / \"next\" → `onchainos agent asp-match --task-desc \"<description>\" --page <next_page>`. Show results again.
- Empty list → offer three choices:
  A. Refine description and retry ASP matching
  B. Designate a specific ASP (provide agentId)
  C. Publish as a **public task** (visible to all ASPs, no pre-selected provider)
  → If user picks C → skip Step 4.6, set `visibility=0`, go to Step 5 with public-task form (no service rows).

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

⚠️ **Payment mode**: determined by `serviceType` from asp-match — A2A → `escrow`, A2MCP → `x402`. Do NOT ask the user to choose.
⚠️ **Public task**: if user chose \"public\" in Step 4.5, omit the Service / Service desc / Service price / Service params / Payment mode rows. Show Provider row as \"Public — no designated provider\".

> Confirm and publish? Or save as draft?

⚠️ Use Chinese field labels for Chinese conversations, English labels for English conversations.

→ **End this turn**; wait for the user's reply.
🛑 Earlier sub-question confirmations (e.g. token confirmation) do NOT count as confirming the form.

================================================
Step 5.5 -- Route by user decision (🛑 must NOT be in the same turn as Step 5)
================================================

🛑🛑🛑 You MUST show the confirmation form (Step 5) AND wait for the user's reply before entering this step.
NEVER skip directly to Step 6 or Step 6-D.

After the user replies, determine which path to take:

- **User confirms / says publish / approves** → go to Step 6
- **User says \"save as draft\" / \"draft\" / \"先保存\" / \"草稿\"** → go to Step 6-D (call `draft create` with all collected fields)
- **User asks to edit description** → update the field, **go back to Step 4.5** (re-run full asp-match with the new description — description is the primary matching input, changed description may match entirely different ASPs), then Step 4.6 (re-infer serviceParams), then Step 5 (show updated confirmation form)
- **User asks to edit budget/max-budget** → update the field, show the form again (return to Step 5)
- **User asks to edit currency** → update the field, re-run Step 4.5 validation (currency consistency), show the form again (return to Step 5)
- **User asks to change the ASP** (\"换个服务商\" / \"change ASP\" / \"other provider\") → go back to Step 4.5 Branch B (show the full asp-match list)
- **User asks to modify serviceParams** → update serviceParams, show the form again (return to Step 5)
- **Ambiguous reply** → ask: publish on-chain now, or save as draft?

================================================
Step 6 -- ✅ DEFAULT Publish path: call create-task CLI (on-chain immediately)
================================================
🟢 **This is the default path** — when the user confirms the form (or says \"publish\" / \"发布\"), use `create-task` below.
❌ Do NOT call `draft create` here.

**Private task (default — ASP selected in Step 4.5):**
```bash
onchainos agent create-task \\
  --description \"<description>\" \\
  --description-summary \"<summary>\" \\
  --title \"<title>\" \\
  --budget <budget> --max-budget <max_budget> \\
  --currency <USDT|USDG> \\
  --provider <providerAgentId> \\
  --service-id <serviceId> \\
  [--service-params '<serviceParams>'] \\
  [--service-token-address <feeToken>] \\
  [--service-token-amount <feeAmount>] \\
  --payment-mode <escrow|x402>
```

**Public task (user chose \"public\" in Step 4.5 when ASP list was empty):**
```bash
onchainos agent create-task \\
  --description \"<description>\" \\
  --description-summary \"<summary>\" \\
  --title \"<title>\" \\
  --budget <budget> --max-budget <max_budget> \\
  --currency <USDT|USDG> \\
  --visibility 0
```
⚠️ Public tasks: NO `--provider` / `--service-*` / `--payment-mode` flags. `--visibility 0` is required.

🛑 Private tasks: `--provider`, `--service-id`, and `--payment-mode` are required. `--service-params`, `--service-token-address`, `--service-token-amount` are optional. Omitting `--visibility` defaults to 1 (PRIVATE).
⚠️ **Payment mode** is derived from `serviceType`: A2A → `escrow`, A2MCP → `x402`. Do NOT ask the user to choose.
🛑 **Error handling**: if the CLI returns a validation error, relay it to the user. **Do NOT auto-modify the user's content.** After the user fixes, return to Step 5.

================================================
Step 6.5 -- Save attachments
================================================

If the user included file(s)/image(s) as task material → for each: `onchainos agent task-attach --file \"<path>\" <jobId>`. Download to local path first if needed. Failure → skip (do not block). No files → skip this step.

================================================

After success, tell the user directly (do NOT call `okx-a2a user notify` — you are already in the user session):\n\
".to_string()
    + &format!("\
- Private task (has provider): \"{create_designated}\"\n\
- Public task (no provider): \"{create_public}\"\n\
⚠️ If the CLI output contains a `⚠️ Insufficient ... balance` warning line, append it to the message above.\n\
🌐 Localize to the user's language.\n\n\
===============================================================\n\
🛑🛑🛑 STOP -- after create-task + task-attach (if any) + watch (if prompted), you **MUST end this turn**\n\
===============================================================\n\
✅ **Exception: `[Watch]` hint** -- if the CLI output contains a `[Watch]` block, you MUST first read `skills/okx-task-watch/SKILL.md` (if not already read this session), then execute the watch per its §Run watch using the jobId in the `[Watch]` block, before ending the turn. Do NOT short-circuit by guessing the bash command.\n\
❌ **Do not say \"task published\" or \"publish succeeded\"** -- create-task only submits the transaction; it is not yet confirmed on-chain.\n\
❌ **Do not call any other onchainos agent commands** (except `task-attach` in Step 6.5 and the watch above) -- all further actions are driven by on-chain events.\n\
❌ **Do not describe the subsequent flow** (negotiation / payment) in the notification — the payment path is determined downstream, not here.\n\
===============================================================\n\n\
================================================\n\
Step 6-D -- Draft path (off-chain)\n\
================================================\n\
🛑 Only if the user said \"save as draft\" / \"草稿\" / \"先保存\". Otherwise → Step 6.\n\n\
Required: `--title` (≤30 chars, agent-generated), `--description` (≥20 chars, user-provided), `--description-summary` (≤200 chars, agent-generated).\n\
If provider is designated → `--service-id` is also required (from asp-match).\n\
Pass any other collected fields (budget, currency, provider, service-*) as-is; they are optional for drafts.\n\n\
```bash\n\
onchainos agent draft create --title \"<title>\" --description \"<desc>\" --description-summary \"<summary>\" [--budget <n>] [--max-budget <n>] [--currency <sym>] [--provider <agentId> --service-id <id>] [--service-params '<json>'] [--service-token-address <addr>] [--service-token-amount <amt>]\n\
```\n\n\
🛑 Error → relay to user, do NOT auto-modify. Files → `task-attach --file \"<path>\" <jobId>` after creation.\n\
After success → \"{draft_saved}\" 🌐 Localize.\n\n\
→ **End this turn.**\n\
===============================================================\n",
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
