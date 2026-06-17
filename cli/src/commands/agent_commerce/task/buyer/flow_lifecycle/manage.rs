//! Task creation, attachment forwarding, and term-change event prompt generators.

// --- User-action: create task ------------------------------------------

pub(crate) fn create_task() -> String {
    "\
🔒 **Pre-flight check**: have you read `skills/okx-agent-task/buyer-user.md`?\n\
If not → **stop executing this playbook immediately**; first load buyer-user.md per the CLAUDE.md routing rules → then come back here.\n\
Skipping skill loading = not knowing the publishing flow / field rules / confirmation form = create-task will fail.\n\n\
[Current Operation] Publish task (create_task)
[Role] User (User Agent)
[Session Type] user session (talking directly to the user)

🛑 **No skipping**: you MUST finish collecting all fields → show the confirmation form → wait for an explicit user confirmation before calling the CLI.
❌ **Do NOT use `draft create` + `draft publish` as a substitute for `create-task`** — they are completely different flows. `create-task` publishes the task on-chain in one step. The draft flow (`draft create` → `draft update` → `draft publish`) is ONLY for when the user explicitly says \"save as draft\". If the user says \"publish a task\" / \"发布任务\" / \"create a task\", you MUST use `create-task` (Step 6), NOT the draft path.
💡 **Draft shortcut**: if the user says \"save as draft\" / \"先保存草稿\" / \"草稿\" at ANY point during field collection, **jump to Step 6-D**. Draft requires `--description` (≥20 chars, user-provided); `--title` and `--description-summary` are agent-generated from description. If description is missing or <20 chars, ask the user to provide/expand it before saving.

================================================
Step 1 -- Field collection (collect progressively in conversation; **only enter Step 2 when all fields are ready**)
================================================

🛑🛑🛑 **ABSOLUTE RULE — No auto-fill for user-provided fields**:
The following fields MUST come from the user's explicit input: **Description, Budget, Max budget, Currency, Acceptance window, Delivery window**.
If the user has NOT explicitly stated a field's value, you MUST ask for it — do NOT guess, infer, generate a default, or extract an implied value from the task description.
Even if the user's description hints at a price range or timeline (e.g. \"大概50块\" / \"一两天\"), you MUST confirm with the user before filling.
Only **Title** and **Summary** are agent-generated (from the user's description).
🔴 Real incident: a user said \"翻译2000字文档\", the agent auto-filled budget, deadline-open, and deadline-submit without asking — the user did not agree to those values, and the task was published with wrong terms.

| Field | CLI flag | Constraint | How to collect |
|---|---|---|---|
| Description | --description | 20-2000 chars | Consolidate the user's words. If <20 → \"A more detailed description helps match a better Provider. Could you add more specifics?\" |
| Title | --title | <=30 chars | Agent-generated; **must count chars after generating**, shorten if >30 |
| Summary | --description-summary | <=200 chars | Agent-generated; **must count chars after generating**, shorten if >200 |
| Payment token | --currency | Only USDT / USDG | ⚠️ See token rules below |
| Budget | --budget | number; <=5 decimal places; max 10,000,000 | **MUST ask the user; do NOT auto-fill or guess.** Extract the number only after the user states it explicitly |
| Max budget | --max-budget | **Required**; >= budget; <=5 decimal places; max 10,000,000 | ⚠️ **You MUST ask the user explicitly**, do not auto-fill or guess. This is the negotiation price cap; the ASP's quote cannot exceed it |
| Acceptance window | --deadline-open | 10 min - 6 months; format `<n>h` / `<n>m` | **MUST ask the user; do NOT auto-fill or guess.** How long the task stays open before auto-closing if no ASP accepts |
| Delivery window | --deadline-submit | 1 min - 6 months; format `<n>h` / `<n>m` | **MUST ask the user; do NOT auto-fill or guess.** How long after acceptance the ASP must deliver |
| Designated provider | --provider | optional; provider agentId | If the user names a specific provider, extract the agentId. **Do not ask proactively** -- if the user does not bring it up, omit it |

🛑 **Token rules (top priority)**:
- User writes \"USDT\" or \"USDG\" explicitly → use it directly, no confirmation
- User uses fuzzy expressions (\"U\" / \"u\" / \"buck\" / \"dollar\" / \"USD\" / \"100U\" / \"50u\") → **you MUST first ask \"Please confirm the payment token: USDT or USDG?\"**, fill it in only after the user replies explicitly
- **Do not default to USDT**: rendering \"100 USDT\" when the user only said \"100U\" is a violation

================================================
Step 2 -- Validation (after all fields collected, before showing the form)
================================================

1. Token is neither USDT nor USDG → \"Only USDT and USDG are supported. Please choose one.\"
2. **Currency consistency between budget and max budget**: if the user mentions different tokens for budget and max budget (e.g. \"budget 10 USDT, max 20 USDG\") → **block**, \"Budget and max budget must use the same token. Please confirm: USDT or USDG?\". The task has a single --currency, the two must match.
3. Description < 20 chars → ask the user to expand
4. max_budget < budget → \"Max budget cannot be less than the budget.\"
5. max_budget missing → \"Please set the max budget (the negotiation price cap); the ASP's quote cannot exceed it.\"
6. budget > 10,000,000 or > 5 decimal places → tell the user the limits

================================================
Step 3 -- Identity & balance check
================================================

1. `onchainos agent get` to check whether the current account has buyer identity (role=1)
2. Has buyer → tell the user which account is being used
3. No buyer → guide registration via `onchainos agent register`
4. Insufficient balance → warn but do not block creation

================================================
Step 4 -- 🛑 Communication availability check (must not be skipped)
================================================

🛑 **MANDATORY -- complete this before showing the confirmation form**.
All post-creation negotiation, notifications, and review depend on the messaging service; messaging down = task created and immediately stuck.

1. **Read** the **entire content** of `skills/okx-agent-chat/ensure-okx-a2a-communication-ready.md`
2. **Fully execute** the flow inside ensure-okx-a2a-communication-ready.md (start from Step 0; walk the decision tree to completion)
3. After it finishes, proceed to Step 5

================================================
Step 4.5 -- ASP matching (after communication check, before confirmation form)
================================================

🛑 This step runs AFTER Step 4 (communication check) and BEFORE Step 5 (confirmation form).

**Branch by whether the user designated a provider:**

**A. User designated a provider** (`--provider` is set):

```bash
onchainos agent asp-match --task-desc \"<description>\" --provider-agent-id <agentId>
```

Handle the result:
- Empty (ASP has no service) → tell the user: \"This ASP has no registered services. Please choose another ASP or remove the designation.\" → wait for the user to decide.
- Non-empty → extract the top service from the output:
  - `serviceId`, `serviceName`, `serviceDescription`, `serviceType`
  - `feeAmount` (→ `serviceTokenAmount`), `feeToken` (→ `serviceTokenAddress`), `feeTokenSymbol`
  - `endpoint` (if A2MCP)

**Validation (designated):**
1. Currency consistency: task `currency` must match `feeTokenSymbol`. Mismatch → \"Task payment token ({currency}) differs from the service fee token ({feeTokenSymbol}). Please change the task token or choose another ASP.\"
2. Budget check: `max-budget` ≥ `feeAmount`. Fail → \"Task max budget ({max-budget}) is lower than the service price ({feeAmount} {feeTokenSymbol}). Please increase the max budget.\"

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

Reply with a number to pick an ASP, or \"more\" for the next page.
```

→ **End this turn** and wait for the user's reply.

**User reply routing:**
- Number → select that ASP; extract its service fields; run the same validation as Branch A (currency + budget). Pass → continue to Step 4.6. Fail → show the error and wait.
- \"more\" / \"下一页\" / \"next\" → `onchainos agent asp-match --task-desc \"<description>\" --page <next_page>`. Show results again.
- Empty list → offer three choices:
  A. Refine description and retry ASP matching
  B. Designate a specific ASP (provide agentId)
  C. Publish as a **public task** (visible to all ASPs, no pre-selected provider)
  → If user picks C → skip Step 4.6, set `visibility=0`, go to Step 5 with public-task form (no service rows).

================================================
Step 4.6 -- serviceParams inference
================================================

Using the selected service's `serviceDescription` + `serviceName` + the user's task `description`, infer a `serviceParams` JSON string.

Rules:
- Identify what input the service expects from its description
- Extract matching values from the user's task description
- Output a JSON object (e.g. `{\"contractAddress\": \"0x1234...\", \"chain\": \"ETH\"}`)
- If nothing can be inferred → use empty string `\"\"`, do not block the flow

Do NOT ask the user for serviceParams — infer silently and show it in the confirmation form (Step 5). The user can correct it there.

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
| Acceptance window | <Nh> |
| Delivery window | <Nh> |
| --- | --- |
| Provider | Agent <providerAgentId> (or \"Public — no designated provider\" if public) |
| Service | <serviceName> (<serviceType>) |
| Service ID | <serviceId> |
| Service price | <feeAmount> <feeTokenSymbol> |
| Service params | <serviceParams readable display, or \"None\"> |
| Payment mode | escrow (A2A) or x402 (A2MCP) |

⚠️ **Payment mode**: determined by `serviceType` from asp-match — A2A → `escrow`, A2MCP → `x402`. Do NOT ask the user to choose.
⚠️ **Public task**: if user chose \"public\" in Step 4.5, omit the Service / Service ID / Service price / Service params / Payment mode rows. Show Provider row as \"Public — no designated provider\".

> Confirm and publish? Or save as draft?

⚠️ Use Chinese field labels for Chinese conversations, English labels for English conversations.

→ **End this turn**; wait for the user's reply.
🛑 Earlier sub-question confirmations (e.g. token confirmation) do NOT count as confirming the form.

================================================
Step 5.5 -- Route by user decision (🛑 must NOT be in the same turn as Step 5)
================================================

🛑🛑🛑 You MUST show the confirmation form (Step 5) AND wait for the user's reply before entering this step.
NEVER skip directly to Step 6 or Step 6-D.
🔴 Real incident: an agent auto-filled all fields from the user's description, skipped the confirmation form, and called `create-task` directly — the task was published on-chain with terms the user never agreed to.

After the user replies, determine which path to take:

- **User confirms / says publish / approves** → go to Step 6
- **User says \"save as draft\" / \"draft\" / \"先保存\" / \"草稿\"** → go to Step 6-D
- **User asks to edit a basic field** (description/budget/currency/deadlines) → update the field, re-run Step 4.5 validation (currency + budget check against the selected service) if currency or max-budget changed, show the form again (return to Step 5)
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
  --deadline-open <deadline_open> --deadline-submit <deadline_submit> \\
  --provider <providerAgentId> \\
  --service-id <serviceId> \\
  --service-params '<serviceParams JSON>' \\
  --service-token-address <feeToken> \\
  --service-token-amount <feeAmount> \\
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
  --deadline-open <deadline_open> --deadline-submit <deadline_submit> \\
  --visibility 0
```
⚠️ Public tasks: NO `--provider` / `--service-*` / `--payment-mode` flags. `--visibility 0` is required.

🛑 Private tasks: `--provider`, `--service-*`, and `--payment-mode` flags are **all required**. Omitting `--visibility` defaults to 1 (PRIVATE).
⚠️ **Payment mode** is derived from `serviceType`: A2A → `escrow`, A2MCP → `x402`. Do NOT ask the user to choose.
🛑 **Error handling**: if the CLI returns a validation error, relay it to the user. **Do NOT auto-modify the user's content.** After the user fixes, return to Step 5.

================================================
Step 6.5 -- Save attachments (only if the user included files with the task request)
================================================

If the user's **original message** included file(s) or image(s) (e.g. Telegram documents `[document telegram:file/...]`, local file paths, inline images) that are intended as task reference material (e.g. 原图, reference image, 附件, sample):

For each file, call:
```bash
onchainos agent task-attach --file \"<local file path>\" <jobId>
```

The file will be stored locally under `~/.onchainos/task/<jobId>/attachments/` and automatically picked up by the sub session during negotiation (Step 1.5 checks `list-attachments`).

⚠️ Only save files the user explicitly mentioned as task-related. Do not save unrelated files.
⚠️ If the file hasn't been downloaded to a local path yet, download it first (e.g. via the platform's file download mechanism) before calling `task-attach`.
⚠️ If `task-attach` fails, skip it and proceed to the notification — attachment failure must NOT block task creation.

If the user's message did NOT include any files, skip this step entirely.

================================================

After success, tell the user directly (do NOT call `xmtp_dispatch_user` — you are already in the user session):\n\
".to_string()
    + &format!("\
- Private task (has provider): \"{create_designated}\"\n\
- Public task (no provider): \"{create_public}\"\n\
⚠️ If the CLI output contains a `⚠️ Insufficient ... balance` warning line, append it to the message above.\n\
🌐 Localize to the user's language.\n\n\
===============================================================\n\
🛑🛑🛑 STOP -- after create-task + task-attach (if any) + watch (if prompted), you **MUST end this turn**\n\
===============================================================\n\
✅ **Exception: `[Watch]` hint** -- if the CLI output contains a `[Watch]` block, run the emitted `okx-a2a user watch ...` command before ending the turn. Read `skills/okx-task-watch/SKILL.md` first if you haven't in this session.\n\
❌ **Do not say \"task published\" or \"publish succeeded\"** -- create-task only submits the transaction; it is not yet confirmed on-chain.\n\
❌ **Do not call any other onchainos agent commands** (except `task-attach` in Step 6.5 and `okx-a2a user watch` above) -- all further actions are driven by on-chain events.\n\
❌ **Do not describe the subsequent flow** (negotiation / payment) in the notification — the payment path is determined downstream, not here.\n\
===============================================================\n\n\
================================================\n\
Step 6-D -- Draft path: save as draft (off-chain)\n\
================================================\n\
🛑 **ONLY enter this step if the user EXPLICITLY said \"save as draft\" / \"草稿\" / \"先保存\"**. If the user said \"publish\" / \"发布\" / \"confirm\" / confirmed the form → you are in the WRONG step; go back to Step 6.\n\n\
Step 6-D.1 -- Check required fields for draft creation\n\n\
Draft creation requires `--description` (≥ 20 chars, user-provided), `--title` (agent-generated from description, ≤ 30 chars), and `--description-summary` (agent-generated from description, ≤ 200 chars).\n\n\
Check whether the user has provided a description (≥ 20 chars). If not, ask the user to provide or expand it.\n\
Once description is ready, generate title and summary from it, then show a draft confirmation form:\n\n\
| Field | Value |\n\
|---|---|\n\
| Title | <agent-generated, ≤30 chars> |\n\
| Summary | <agent-generated, ≤200 chars> |\n\
| Description | <user-provided content> |\n\
| Budget | <value or \"—\"> |\n\
| Max budget | <value or \"—\"> |\n\
| Currency | <value or \"—\"> |\n\
| Acceptance window | <value or \"—\"> |\n\
| Delivery window | <value or \"—\"> |\n\
| Provider | <Agent agentId or \"—\"> |\n\
| Service | <serviceName or \"—\"> |\n\
| Service price | <feeAmount feeTokenSymbol or \"—\"> |\n\
| Service params | <serviceParams or \"—\"> |\n\n\
> Save as draft? Fields marked — are optional and can be added later.\n\n\
⚠️ Use Chinese field labels for Chinese conversations, English labels for English conversations.\n\
🛑 **Description**: must come from the user — do NOT auto-generate or invent content. You may consolidate the user's words, but the substance must be theirs.\n\
🛑 **Title & Summary**: agent-generated from the user's description. Must count chars after generating — shorten title if >30, summary if >200.\n\
→ After the user confirms, proceed to Step 6-D.2.\n\n\
Step 6-D.2 -- Call draft create CLI\n\n\
Once description + title + summary are ready, call the CLI with all fields the user has provided:\n\n\
```bash\n\
onchainos agent draft create \\\\\n\
  --title \"<title>\" \\\\\n\
  --description \"<description>\" \\\\\n\
  --description-summary \"<summary>\" \\\\\n\
  [--budget <budget>] [--max-budget <max_budget>] \\\\\n\
  [--currency <USDT|USDG>] \\\\\n\
  [--deadline-open <deadline_open>] [--deadline-submit <deadline_submit>] \\\\\n\
  [--provider <provider agentId>] \\\\\n\
  [--service-id <serviceId>] [--service-params '<serviceParams>'] \\\\\n\
  [--service-token-address <feeToken>] [--service-token-amount <feeAmount>]\n\
```\n\n\
🛑 **Error handling**: if the CLI returns a validation error (e.g. \"description is too short\"), relay the error message to the user and ask them to fix it. **Do NOT auto-modify, expand, or rewrite the user's content** — the user must provide the corrected value themselves.\n\
⚠️ If the user included file(s), save them after draft creation:\n\
```bash\n\
onchainos agent task-attach --file \"<local file path>\" <jobId>\n\
```\n\n\
After success, tell the user directly (do NOT call `xmtp_dispatch_user` — you are already in the user session):\n\
- content: \"{draft_saved}\"\n\
🌐 Localize to the user's language.\n\n\
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
Step 1 -- Pre-publish field check
================================================

Query the draft detail to verify all required fields are populated:
```bash
onchainos agent status {job_id}
```

Check the following required fields:
| Field | API field | Requirement |
|---|---|---|
| Title | title | non-empty |
| Description | description | >= 20 characters |
| Summary | descriptionSummary | non-empty |
| Currency | paymentTokenSymbol | USDT or USDG |
| Budget | tokenAmount | > 0, <= 10,000,000 |
| Max budget | paymentMostTokenAmount | >= budget |
| Acceptance window | expireConfig.acceptDeadline | 10m ~ 180d (in seconds) |
| Delivery window | expireConfig.submittedDeadline | 1m ~ 180d (in seconds) |

If any field is missing or invalid → show a table listing ALL fields with their current values (filled fields show the value, missing fields show `❌ Required`). Then:
- **Description, Budget, Max budget, Currency, Acceptance window, Delivery window**: these are user-provided fields — ask the user to provide them. **Do NOT auto-fill.**
- **Title** (≤30 chars) and **Summary** (≤200 chars): agent-generated from description. If description is present but title/summary are missing, **auto-generate them** from the description (count chars, shorten if needed). Do NOT ask the user to write these.

→ After the user provides field(s), **do not call `draft update` yet** — update the in-memory values and show the table again until all required fields are filled.

⚠️ The CLI `draft publish` has a built-in validation safety net; this step is the first line of defense.
🛑 **Error handling**: if the user provides a value that fails validation (e.g. description too short), relay the error and ask them to fix it. **Do NOT auto-modify the user's content** (description, budget, etc.).

================================================
Step 2 -- Update draft with collected fields
================================================

Once ALL required fields are verified, call `draft update` to persist any fields the user provided during Step 1:
```bash
onchainos agent draft update {job_id} --<field1> <value1> --<field2> <value2> ...
```

Only include fields that were missing or changed during Step 1. If no fields were updated (all were already present), skip this step.

================================================
Step 3 -- Call draft publish CLI
================================================

```bash
onchainos agent draft publish {job_id}
```
⚠️ `{job_id}` is a **positional argument**, NOT a flag. Do NOT use `--job-id`.

This command validates all required fields, checks balance (blocking), signs the transaction, and broadcasts on-chain.

================================================
Step 4 -- Notify user
================================================

After success, tell the user directly (do NOT call `xmtp_dispatch_user` — you are already in the user session):
- No designated provider → \"{publish_public}\"
- With designated provider → \"{publish_designated}\"
⚠️ If the CLI output contains a `⚠️ Insufficient ... balance` warning line, append it to the message above.
🌐 Localize to the user's language.

===============================================================
🛑🛑🛑 STOP -- after draft publish + watch (if prompted), you **MUST end this turn**
===============================================================
✅ **Exception: `[Watch]` hint** -- if the CLI output contains a `[Watch]` block, run the emitted `okx-a2a user watch ...` command before ending the turn. Read `okx-task-watch/SKILL.md` first if you haven't in this session.
❌ **Do not say \"task published\" or \"publish succeeded\"** -- draft publish only submits the transaction; it is not yet confirmed on-chain.
❌ **Do not call any other onchainos agent commands** (except `okx-a2a user watch` above) -- all further actions are driven by on-chain events.
===============================================================\n",
        publish_public = super::super::content::draft_publish_public_user_notify(),
        publish_designated = super::super::content::draft_publish_designated_user_notify(),
    )
}

// --- Attachment forwarding ---------------------------------------------

pub(crate) fn attachment_added(ctx: &super::super::flow::FlowContext<'_>) -> String {
    let l10n_short = super::super::flow::L10N_DISPATCH_SHORT;
    let job_id = ctx.job_id;
    let agent_id = ctx.agent_id;
    let short_id = ctx.short_id;
    let attachment_template = super::super::content::attachment_file_to_seller(job_id);
    let att_sent = super::super::content::attachment_sent_user_notify();
    let att_saved = super::super::content::attachment_saved_user_notify();
    let att_blocked = super::super::content::attachment_phase_blocked_user_notify();

    format!(
    "[Trigger] attachment_added (user session dispatched `[ATTACHMENT_ADDED]` — a file was saved locally and must be uploaded + forwarded to the provider)\n\
     [Role] User (User Agent)\n\n\
     🛑 **This is the ONLY correct path for forwarding attachments. Do not improvise.**\n\
     🔴 Real incident: a Minimax model received `[ATTACHMENT_ADDED]`, skipped `xmtp_file_upload`, and sent the raw local file path via `xmtp_send` — \
     the provider received a path like `/Users/.../attachments/photo.jpg` which it cannot access. Then the model called next-action with `event=job_submitted` in --message (wrong event) and the task got stuck.\n\n\
     [Your next actions (strict order)]\n\n\
     **Step 1 — Extract the file path:**\n\
     The `[ATTACHMENT_ADDED]` message content has the format: `[ATTACHMENT_ADDED] <absolute file path>`.\n\
     Extract the file path (everything after the prefix). This is a **local** file that has NOT been sent to the provider yet.\n\n\
     **Step 2 — Check task status:**\n\
     ```bash\n\
     onchainos agent status {job_id}\n\
     ```\n\
     Read `status` (int) and branch:\n\n\
     --------- Branch A: status=1 (accepted) OR status=0 (created) with an active sub session ---------\n\n\
     **A-Step 1 — Upload the file (encrypted):**\n\
     Call `xmtp_file_upload`:\n\
     \x20\x20- filePath: <extracted path from Step 1>\n\
     \x20\x20- agentId: {agent_id}\n\
     \x20\x20- jobId: {job_id}\n\
     → On success you receive 6 fields: `fileKey`, `digest`, `salt`, `nonce`, `secret`, `filename`.\n\n\
     ⚠️ If `xmtp_file_upload` fails → call xmtp_dispatch_user to notify the user that the attachment failed to send; **do NOT retry or block** — end the turn.\n\n\
     **A-Step 2 — Forward to the provider via `xmtp_send`:**\n\
     Send to the provider with **all 6 fields** + `[intent:attachment]` suffix (exact format — paste all fields verbatim):\n\
     ```\n\
     {attachment_template}\n\
     ```\n\
     ❌ Do NOT send the local file path — the provider cannot access your filesystem.\n\
     ❌ Do NOT omit any of the 6 encryption fields — the provider needs all of them to decrypt the file.\n\
     🛑 **VERBATIM COPY — every field value (especially `secret`, `digest`, `salt`, `nonce`) must be pasted in FULL, character-for-character from `xmtp_file_upload` output. These are base64/hex strings that can be 40-200+ characters long. Do NOT truncate, abbreviate, or replace any part with `...` or ellipsis — even a single missing character = decryption failure.**\n\
     🔴 Real incident: a model abbreviated `secret: SHUJoA...dqE=` (replaced the middle with `...`), the provider could not decrypt the file and the task stalled.\n\n\
     **A-Step 3 — Notify the user:**\n\
     Call xmtp_dispatch_user:\n\
     \x20\x20content: {att_sent}\n\
     Fill: `<short_jobId>` = {short_id}\n\
     {l10n_short}\n\n\
     → **End this turn.**\n\n\
     --------- Branch B: status=0 (created) and NO active sub session ---------\n\n\
     The file is already stored locally under `~/.onchainos/task/<jobId>/attachments/`.\n\
     It will be uploaded to the provider automatically when the negotiation session starts.\n\n\
     Call xmtp_dispatch_user:\n\
     \x20\x20content: {att_saved}\n\
     Fill: `<short_jobId>` = {short_id}\n\
     {l10n_short}\n\n\
     → **End this turn.**\n\n\
     --------- Branch C: status≥2 (submitted / rejected / disputed / terminal) ---------\n\n\
     Call xmtp_dispatch_user:\n\
     \x20\x20content: {att_blocked}\n\
     Fill: `<short_jobId>` = {short_id}\n\
     {l10n_short}\n\n\
     → **End this turn.**\n"
    )
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
     Rationale: price is locked at accept time, not negotiated in chat. The on-chain tokenSymbol / tokenAmount update is visible to the ASP via task-detail queries; no `xmtp_send` propagation is needed.\n\n\
     ❌ Do not xmtp_send to the provider (price talk is forbidden in chat).\n\
     ❌ Do not xmtp_dispatch_user (the user already knows about the change in the user session).\n\
     ❌ Do not call set-token-and-budget / set-asp / set-max-budget (the user session already did).\n"
    )
}
