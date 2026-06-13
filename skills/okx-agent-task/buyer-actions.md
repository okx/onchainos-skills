# Buyer — User-Session Actions

> 🛑 **Pre-requisite**: you must have already read `buyer-user.md`. If you arrived here by guessing, **stop** and read it first.

> 🌐 **Localization**: all `xmtp_dispatch_user` / `pending-decisions-v2 request` calls in this file must match the user's language. See `buyer-user.md` localization preamble.

> 🛑 **Universal confirmation rule**: every modification MUST be confirmed individually with the user before execution. When the user mentions multiple changes in one sentence, split into independent steps, present a confirmation question at each step, and only proceed after the user explicitly replies. ❌ Batch-executing = the user cannot review = potentially executing unwanted changes.

---

## Quick Navigation

| Section | When to read |
|---|---|
| §1 Publishing | **Moved** → [`buyer-actions-publish.md`](./buyer-actions-publish.md) |
| §2 Mid-task attachment | User wants to add files to an active task |
| §3 Terms changes | Modify token / budget / provider / max-budget |
| §4 View deliverables | User wants to see submitted deliverables |

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
   - **x402** → reuse §3.3 x402 flow in [`buyer-user.md`](./buyer-user.md) (start from Step 2 endpoint validation).
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
