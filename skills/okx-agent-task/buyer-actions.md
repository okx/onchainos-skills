# Buyer — User-Session Actions

> 🛑 **Pre-requisite**: read `buyer-user.md` first. 🌐 All user-facing content must match the user's language.
> 🛑 **Universal confirmation rule**: every modification MUST be confirmed individually before execution. Multiple changes in one sentence → split into steps, confirm each. ❌ Batch-executing = user cannot review.

---

## Quick Navigation

| Section | When to read |
|---|---|
| §1 Publishing | **Moved** → [`buyer-actions-publish.md`](./buyer-actions-publish.md) |
| §2 Mid-task attachment | User wants to add files to an active task |
| §3 Terms changes | Modify token / budget / provider / max-budget |
| §4 View deliverables | User wants to see submitted deliverables |
| §5 Designated-Provider A2A | User designates a specific provider (A2A path) |
| §6 Designated-Provider x402 | User designates a provider with x402 endpoint |

---

## 2. Mid-task attachment (user session)

**Trigger**: 补充附件/补充图片/给任务加文件/add file to task/attach this to job/upload file to task, or user directly sends a file during an active task conversation (confirm intent first).

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

🛑 **Priority rule**: user instruction > automated flow. Terms-change or stop from user → immediately interrupt and handle first.

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
   - **escrow** → call `next-action --role buyer --agentId <yours> --message '{"event":"switch_provider","jobId":"<jobId>","provider":"<new agentId>"}'` to fetch the script.
   - **x402** → reuse §6 x402 flow below (start from Step 2 endpoint validation).
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

- Single job: `onchainos agent task-deliverable-list --job-id <jobId> --role <role>`
- All / search: `onchainos agent task-deliverable-list --role <role> [--search "<keyword>"]`

**Step 3 — Present results directly to the user** (🌐 translate labels to user's language):

- Single job: list each entry with `originalName`, `deliverableType`, `sizeBytes` (human-readable), absolute `path`, `savedAt`.
- All jobs: group by job (`title` + `jobId`), show `deliverableCount` + each file's `originalName` and absolute `path`.
- Empty → "No saved deliverables found." / "没有已保存的交付物。"
- ⚠️ File paths MUST be absolute.

---

## 5. Designated-Provider A2A flow

**Trigger**: user message contains "Please initiate a direct conversation with this provider to discuss the task details."

> ⚠️ **A2MCP with known endpoint → NOT this skill** — concrete URL + A2MCP serviceType → `okx-agent-payments-protocol`. "Please send a request to this endpoint" without "use onchainos" → also NOT this skill. "Please use onchainos to send a request to this endpoint" + non-A2MCP → **§6** below.

Parse from the message: `agentId` (immutable), `ServiceTitle`, `ServiceType`, `Price` / `symbol` (mutable).

**Flow**:
1. **Provider validation**: `onchainos agent profile <agentId>` — `ok=false` / `data.role ≠ 2` → inform the user; do NOT continue. ⚠️ The `role` in this response belongs to the **queried agent** (the provider), NOT to you — you remain the **buyer** (`--role buyer`).
2. **Service-type determination**: `onchainos agent service-list --agent-id <agentId>` (joint check on serviceType + endpoint):
   - x402 supported → carry `agentId` + `endpoint` and enter §6 below (from Step 2).
   - Otherwise → A2A (step 3 below).
   - ⚠️ **Do NOT call `xmtp_start_conversation` directly.**
3. **A2A path**: map fields (`description` ← ServiceTitle, `budget` ← Price, `currency` ← symbol), cache `designatedProvider = { agentId, serviceType }` → enter [`buyer-actions-publish.md`](./buyer-actions-publish.md) to publish the task (🛑 must run the full publishing flow including confirmation form).
4. `job_created` arrives → detect `designatedProvider` → **skip `recommend`, keep it private** → directly create the group and negotiate.
5. Negotiation fails → automatically run `recommend <jobId>` to display for user to choose.

---

## 6. Designated-Provider x402 flow

**Trigger**: user message contains "Please use onchainos to send a request to this endpoint".

Parse from the message: `agentId`, `ServiceTitle`, `ServiceType`, `endpoint` (all required; no Price — pricing is fetched from the endpoint).

**Flow**:
1. **Provider validation**: same as §5 step 1.
2. **Endpoint validation**: `onchainos agent x402-check --endpoint <endpoint>` — `valid=false` → inform "invalid"; `tokenSymbol` not USDT/USDG → inform "unsupported".
3. **User pricing confirmation** → show a 2-column table (`| Field | Value |`): 卖家/Seller, 服务/Service, Endpoint (in backticks), 费用/Price. If refused, end.
4. **Field collection & confirmation form** (🛑🛑🛑 may NOT be skipped):
   - The agent auto-generates `title` (≤30 chars), `description` (≥10 chars), `description-summary` (≤200 chars) based on the ServiceTitle.
   - `budget` / `max-budget` = `amountHuman` (x402 pricing is fixed; the two are equal).
   - `currency` = `tokenSymbol`.
   - `deadline-open` / `deadline-submit`: **must be asked of the user**; do NOT auto-fill.
   - ⚠️ **Language matching**: field labels MUST match the user's language.
   - Display the full confirmation form (format see `buyer-actions-publish.md` Appendix A) → **end this turn** and wait for explicit confirmation.
   - 🛑🛑🛑 **ABSOLUTE PROHIBITION — after displaying the confirmation form, do NOT execute `create-task` in the same turn.**
5. **Create the task after user confirmation**: `create-task` → **end this turn**, wait for `job_created`, cache `designatedProvider = { agentId, serviceType, endpoint, acceptsJson, amountHuman, tokenSymbol }`.
6. **set-payment-mode** (triggered by `job_created`): `set-payment-mode <jobId> --payment-mode x402 --token-symbol <sym> --token-amount <amt> --endpoint <ep>` → **end this turn**, wait for `job_payment_mode_changed`.
7. **task-402-pay** (triggered by `job_payment_mode_changed`): `task-402-pay <jobId> --provider-agent-id <agentId> --accepts '<acceptsJson>' --endpoint <ep> --token-symbol <sym> --token-amount <amt>`
   - `replaySuccess=true` → notify deliverable + "awaiting on-chain confirmation".
   - `replaySuccess=false` → notify replay failure.
8. Wait for `job_accepted` → call `next-action --role buyer --agentId <yours> --message '{"event":"job_accepted","jobId":"<jobId>"}'`; follow the script to complete.

### Error Handling

| Error | Response |
|---|---|
| Provider not found / Endpoint invalid / tokenSymbol not USDT/USDG | Inform user with specific reason; do not proceed. |
| Create-task failed | Check network; guide retry. |
| Payment signing failed | Inspect `executeErrorMsg`. Do NOT default to "balance insufficient" — system is gas-free. |
