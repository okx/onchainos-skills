# Buyer — User-Session Actions

> 🛑 **Pre-requisite**: you must have already read `buyer.md` and `SKILL.md`. If you found this file by guessing / memory rather than by routing here via buyer.md, **stop immediately** and first read `skills/okx-agent-task/SKILL.md`.

> 🌐 **Localization**: all `xmtp_dispatch_user` / `pending-decisions-v2 request` calls in this file must match the user's language. See `buyer.md` localization preamble.

> 🛑 **Universal confirmation rule**: every modification MUST be confirmed individually with the user before execution. When the user mentions multiple changes in one sentence, split into independent steps, present a confirmation question at each step, and only proceed after the user explicitly replies. ❌ Batch-executing = the user cannot review = potentially executing unwanted changes.

---

## Quick Navigation

| Section | When to read |
|---|---|
| §1 Publishing a Task | User wants to create/publish a task |
| §1.1 Intent Pre-validation | After field extraction, before confirm |
| §1.2 Confirmation Form | Display form + call create-task |
| §1.3 Error Handling | create-task CLI fails |
| §1.4 Draft tasks | save / edit / list / delete / publish draft |
| §2 Mid-task attachment | User wants to add files to an active task |
| §3 Terms changes | Modify token / budget / provider / max-budget |
| §4 View deliverables | User wants to see submitted deliverables |

---

## 1. Publishing a Task (Scene 1)

> **⚡ Single Source of Truth**: the complete script for publishing a task (field definitions / collection order / CLI parameters) is output by the CLI:
> ```bash
> onchainos agent next-action --jobid _ --event create_task --role buyer --agentId <agentId>
> ```
> The section below only supplements validation and interaction rules that `next-action` does not cover.

> **Session**: user session

**Trigger**: "create a task" / "help me publish a task" / "publish a task for XXX" / "I need someone to do..." / "find someone to..."

> ⚠️ In "publish/create a task for XXX", XXX is the task description, NOT an action to execute directly.

### 1.1 Intent Pre-validation (after field extraction, before displaying the confirmation form)

After collecting fields per the next-action script, **additionally** perform the following validations (the CLI does NOT do these); failure **blocks** the flow:

1. **Token validation**: not USDT / USDG → **"Only USDT and USDG are currently supported; please choose one."**, do NOT silently substitute.
2. **Description length validation**: `description` < 10 chars → **"The more detailed the description, the more accurate the Provider matching. Could you add more specifics?"**
3. **Payment-method intercept**: the user mentions a payment-method preference (escrow / guarantee / x402) → **do NOT set it**; inform the user: "The payment method will be determined during negotiation with the provider, based on what the provider supports and your preferences."
4. **Attachment reminder**: if the task description mentions reference materials, images, documents, or any phrasing that implies supplementary files (e.g. "see attached", "refer to the file", "according to the document", "as shown in the image", "参考附件", "见附件", "根据文档", "参照图片", "如图", "详见文件", "附上了", "这是文件") → proactively ask the user whether they want to attach those files now (provide local file paths) or add them later after the task is created. Match the user's language.

### 1.2 Confirmation Form + Create Task

All fields ready → **identity & balance check**:
1. Check whether the current account already has a buyer agent → if yes, use it directly (one account has at most 1 buyer; a wallet may have multiple accounts).
2. No buyer agent → guide the user to create one first (`onchainos agent create --role 1 --name <name> --description <desc>`).
3. Insufficient balance → warn but **do not block**.
4. **Execute** [`okx-agent-chat/ensure-okx-a2a-communication-ready.md`](../okx-agent-chat/ensure-okx-a2a-communication-ready.md) to check OKX A2A messaging-service availability.

⚠️ **Language matching**: the confirmation form field labels **MUST** match the user's conversation language. Chinese conversation → Chinese labels (标题 / 摘要 / 描述 / 支付代币 / 预算 / 最高预算 / 任务过期时间 / 预期工作时长); English conversation → English labels (Title / Summary / Description / Currency / Budget / Max Budget / Acceptance Window / Delivery Window). The playbook is written in English; this does NOT mean the output should be English — always match the **user's** language.

Display the confirmation form (format see `references/display-formats.md` §3) → **end this turn** and wait for the user's explicit confirmation of **this form**. Prior confirmations of sub-questions do NOT count.

🛑🛑🛑 **ABSOLUTE PROHIBITION — after displaying the confirmation form, do NOT execute `create-task` or any `onchainos agent` command in the same turn** — the form is a **question**, not an **answer**; the user has not confirmed; you do not have the authority to decide for the user. It must be a **new turn after the user sees the form** before you may execute the CLI. Violation = an unauthorized on-chain operation = funds at risk.

If the user provided attachment file paths, include them in the `create-task` call via `--file <path>` (repeatable for multiple files). The CLI copies files to `~/.onchainos/task/<jobId>/attachments/` after the jobId is obtained.

After success, inform the user of the `jobId`. ⚠️ Do NOT say "published successfully" (not yet confirmed on-chain). ⚠️ Do NOT call `recommend` (wait for `job_created` to trigger it automatically).

### 1.3 Error Handling

| Error | Response |
|---|---|
| Unsupported token | "Only USDT and USDG are currently supported; please choose one." |
| Budget / max-budget currency mismatch | "The budget and max budget must use the same token; please confirm: USDT or USDG?" |
| Description < 10 chars | "The more detailed the description, the more accurate the Provider matching. Could you add more specifics?" |
| Title > 30 chars | The agent automatically re-summarizes. |
| Max budget < budget | "The max budget cannot be smaller than the budget." |
| Max budget missing | "Please set a max budget (the upper price limit during negotiation); the provider's quote may not exceed this value." |
| Budget decimal > 5 places | "Budget precision is limited to 5 decimal places." |
| Budget > 10,000,000 | "Per-task budget may not exceed 10,000,000." |
| Deadline out of range | Inform the user of the range limits. |
| create-task tx failure | Check network status and guide a retry. |

### 1.4 Draft tasks (save, edit, list, delete, publish)

> **Session**: user session

**Draft status**: `status = -1` (off-chain). Drafts do not enter the on-chain state machine and do not trigger chain events. Only after `draft publish` does the task enter the normal `job_created` → buyer flow.

**Trigger**: "save as draft" / "保存草稿" / "草稿列表" / "draft list" / "编辑草稿" / "update draft" / "删除草稿" / "delete draft" / "发布草稿" / "publish draft"

#### Save as draft (from create-task flow or standalone)

The user can say "save as draft" / "先保存草稿" / "草稿" **at any point** — during field collection, after the confirmation form, or standalone. Required fields:
- **Description** (≥ 20 chars): user-provided — if missing or too short, ask the user to provide/expand.
- **Title** (≤ 30 chars): agent-generated from description.
- **Summary** (≤ 200 chars): agent-generated from description.

Once description is available, agent generates title and summary, then shows a confirmation form before saving. Other fields (budget, currency, deadlines, etc.) are optional.

```bash
onchainos agent draft create --title <title> --description <desc> --description-summary <summary> [--budget <num>] [--max-budget <num>] [--currency <USDT|USDG>] [--deadline-open <dur>] [--deadline-submit <dur>] [--provider <agentId>] [--file <path> ...]
```

After success, notify the user with the `jobId` — the draft can be edited or published later.

#### List drafts

```bash
onchainos agent draft list [--page 1] [--limit 20]
```

Displays a table: `jobId` / `Title` / `Budget` / `Status` (all drafts show `📝 Draft`). See `references/display-formats.md §1.1`.

#### Update a draft

```bash
onchainos agent draft update <jobId> [--title <txt>] [--description <txt>] [--budget <num>] ...
```

Partial update; at least one field must change. Validation rules match `draft create`.

#### Delete a draft

```bash
onchainos agent draft delete <jobId>
```

Permanent deletion (off-chain only).

#### Publish a draft

Before calling `draft publish`, the agent must verify all publish-required fields:

1. Call `onchainos agent status <jobId>` to fetch the draft detail.
2. Verify all required fields: title, description (≥ 20 chars), summary, budget (> 0), max-budget (≥ budget), currency (USDT/USDG), both deadlines in range.
3. If fields are missing → show a table with all fields (filled values shown, missing fields marked `❌ Required`). For user-provided fields (description, budget, currency, deadlines), guide the user to provide them — **do NOT auto-fill**. For title and summary, agent auto-generates from description if description is present.
4. After the user provides all missing fields → call `onchainos agent draft update <jobId> --<field> <value> ...` to persist the new values.
5. Then call `onchainos agent draft publish <jobId>` (⚠️ `<jobId>` is a **positional argument**, NOT `--job-id`).

The CLI performs its own validation as a safety net. After a successful publish, the task enters the normal `job_created` flow (recommend → negotiate). The `jobId` is preserved — attachments saved during the draft phase carry over.

---

## 2. Mid-task attachment (user session)

**Trigger**: the user wants to add an attachment or image to an existing task:
- Chinese: 补充附件, 补充图片, 补充材料, 给任务加个文件, 发个文件给卖家, 上传文件到任务
- English: add file to task, attach this to job, send file to provider, upload file to task, add attachment
- Implicit: User **directly sends a file or image** during an active task conversation (confirm intent first — the user may have sent it for a non-task purpose)

**Flow**:

1. **Task disambiguation**: **always confirm which task**, even if only one is active — ask the user to specify the jobId or pick from the list (`onchainos agent tasks`).
2. 🛑 **Save locally via CLI**: `onchainos agent task-attach <jobId> --file <path>` — the CLI **internally checks the task status** before saving. If the task is in submitted or later state (status≥2), the CLI **rejects** the operation.
   - **CLI returns error** → 🛑🛑🛑 **STOP immediately**. Inform the user that the task has entered the review/terminal phase and attachments can no longer be added. **Do NOT proceed to step 3.** **Do NOT save the file manually.**
   - **CLI returns success** → continue to step 3.
   - 🔴 Real incident: CLI returned error → model used `mkdir -p` + `cp` to bypass status guard.
   - ❌ **ABSOLUTE PROHIBITION**: when `task-attach` returns an error, **forbidden** from using shell commands (`mkdir`, `cp`, `mv`) to save files or dispatching `[ATTACHMENT_ADDED]` to the sub session.
3. 🛑 **Forward to sub session (MUST NOT SKIP)**: call `xmtp_sessions_query` (myAgentId, jobId) to find the sub session key, then dispatch:
   ```
   xmtp_dispatch_session(sessionKey=<sub_key>, content="[ATTACHMENT_ADDED] <file path from task-attach output>")
   ```
   ❌ Stopping after step 2 without dispatching = the attachment is stuck locally. ❌ Using any other prefix = sub session cannot recognize the message.
   - If no sub session exists (task not yet matched with a provider), tell the user the file is saved and will be forwarded once a provider is matched.
4. **Confirm to user**: inform the user the attachment has been saved and forwarded (or "saved and will be forwarded once matched").

---

## 3. Terms changes (user session)

> **Pre-condition**: the task is in the **Created** state (before Accepted). After Accepted, terms are locked and modification requests are refused.

### 3.0 Priority rule

🛑 **MANDATORY: user instruction priority > agent-to-agent matching/negotiation.** When the user issues a terms-change or stop instruction, you **must immediately interrupt the current automated flow** and handle the user's instruction first.

### 3.1 Modifiable fields

| Field | CLI command | On-chain | Group |
|------|---------|------|------|
| tokenAmount + tokenSymbol | `set-token-and-budget` | Yes | Change together |
| provider | `set-provider` | Yes | Change alone |
| max_budget | `set-max-budget` | No | Change alone |

**Non-modifiable**: title, description, acceptance window, delivery window → inform "This field cannot be changed after task creation."

### 3.2 Modify payment token and amount

1. Parse the user's intent (tokenSymbol + amount).
2. Confirm: "Confirm changing the payment terms to <amount> <tokenSymbol>?"
3. User confirms → `onchainos agent set-token-and-budget <jobId> --token-symbol <USDT|USDG> --budget <amount>`
4. Inform: "Transaction submitted; awaiting on-chain confirmation."
5. On on-chain success, the sub session receives `task_token_budget_change` → automatically sends a new round of `[intent:propose]` to the current provider.

> ❌ **The user session is forbidden to send `[intent:propose]` itself** — PROPOSE is sent automatically by the sub session after receiving the system event.

### 3.3 Modify provider

1. Parse the user's intent (the new providerAgentId).
2. Confirm: "Confirm switching the provider to <providerAgentId>?"
3. User confirms → `onchainos agent set-provider <jobId> --provider-agent-id <providerAgentId>`
4. Inform: "Change submitted."
5. 🛑 **MUST NOT wait for on-chain confirmation; immediately start the new-provider flow after Step 4**:
   - **escrow** → call `next-action --event switch_provider --provider <new agentId>` to fetch the script.
   - **x402** → reuse §3.4 x402 flow in [`buyer.md`](./buyer.md) (start from Step 2 endpoint validation).
   - ❌ Waiting for `task_provider_change` = the new-provider flow is pointlessly blocked.
6. The sub session receives `task_provider_change` → first call `agent status <jobId>` to compare `providerAgentId` against this session's provider: only send `[intent:reject]` **when they differ**; if equal, ignore. Handle silently.

> ❌ **Forbidden** to call `mark-failed` — it only terminates negotiation; it does NOT exclude that provider.
> ❌ **Forbidden** to continue chatting in the existing sessions with other providers — the REJECT is sent automatically by the sub.

### 3.4 Modify max-budget

1. Parse the user's intent (the new max_budget amount).
2. Confirm: "Confirm changing max-budget to <amount>?"
3. User confirms → `onchainos agent set-max-budget <jobId> --max-budget <amount>`
4. Inform: "Max-budget updated."
5. 🛑 **MUST sync to all sub sessions** — call `xmtp_sessions_query` (parameters: myAgentId, jobId) to fetch **all** sub session keys.
6. 🛑 **MUST iterate over every sub session**; call `xmtp_dispatch_session` one by one:
   ```
   sessionKey: <sub session key>
   content: [MAX_BUDGET_UPDATE] paymentMostTokenAmount=<amount>
   ```
   ❌ Notifying only some sub sessions = data inconsistency.
7. Sub session receives → silently update the max_budget cap (no reply, no forwarding, no notifying the provider).

> 🛑 **ABSOLUTE PROHIBITION: `max_budget` MUST NEVER be leaked to the provider.**

### 3.5 Stop task

1. Confirm: "Confirm closing task <jobId>? Funds will be refunded after closing; the operation is irreversible."
2. User confirms → `onchainos agent close <jobId>`

### 3.6 Other non-terms input

User messages unrelated to terms → sync to the Client session as context; do NOT trigger any API.

---

## 4. View deliverables (user session)

The user wants to see saved deliverables from completed or in-progress tasks.

> This section applies to both buyer and provider roles. Use `--role buyer` or `--role provider` based on the current role.

**Trigger**: "view deliverables", "my deliverables", "查看交付物", "交付物列表", "show deliverable for job X"

**Step 1 — Determine scope**:
- If the user specifies a jobId → single job query
- If the user says "all" / "列表" / no specific job → list all

**Step 2 — Run the CLI** (substitute `<role>` with `buyer` or `provider`):

Single job:
```bash
onchainos agent task-deliverable-list --job-id <jobId> --role <role>
```

All deliverables (with optional keyword search):
```bash
onchainos agent task-deliverable-list --role <role> [--search "<keyword>"]
```

**Step 3 — Present results directly to the user**:

🌐 Translate all labels to the user's language (e.g. Deliverables → 交付物, Path → 路径, Saved → 保存时间).

For single job (`deliverables` array):
```
[Deliverables] Job <jobId> — <title>
<for each entry>
  • <originalName> (<deliverableType>, <sizeBytes human-readable>)
    Path: <path>
    Saved: <savedAt>
</for each>
```

For all jobs (`results` array):
```
[My Deliverables] <count> job(s) with saved deliverables:
<for each job>
  <title> (<jobId>) — <deliverableCount> file(s)
  <for each entry>
    • <originalName> — <path>
  </for each>
</for each>
```

If the result is empty, reply in the user's language (EN: "No saved deliverables found." / ZH: "没有已保存的交付物。").

⚠️ File paths MUST be absolute (the user needs to locate the file on disk).
