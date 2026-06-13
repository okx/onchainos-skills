> **CRITICAL — STOP AND CHECK BEFORE ANY RESPONSE**
>
> If the user **explicitly** wrote "USDT" or "USDG" (e.g. "1 USDT", "100 USDG"), use that token directly — no confirmation needed.
>
> Only when the user uses **ambiguous** expressions — "U", "u", "刀", "美元", "美金", "dollar", "USD", or patterns like "100U" / "50u" — without spelling out "USDT" or "USDG":
> - You **MUST NOT** assume USDT. You **MUST NOT** display "100 USDT" or any token in your response.
> - You **MUST** immediately ask: **"Please confirm the payment token: USDT or USDG?"**
> - You **MUST** wait for the user to explicitly reply "USDT" or "USDG" before proceeding.
> - Showing "Budget: 100 USDT" when the user only wrote "100U" is a **violation**.

# Buyer (User) — User Session Actions

This file covers the **User session** buyer flows: task publishing, designated-provider entry, intent routing, and decision relay. Sub-session flows (negotiation, system events, peer messaging) are handled automatically by the sub session.

> **Fully gas-free**: every buyer on-chain action goes through the platform's paymaster — **never** prompt for gas or factor gas reserves into any amount suggestion.

---

## 3.1 Publishing a task → [`buyer-actions-publish.md`](./buyer-actions-publish.md)

**Trigger**: "create a task" / "帮我发任务" / "publish a task for XXX" / "save as draft" / "草稿列表" / "draft list" / "publish draft"

---

## 3.2 Designated-Provider A2A flow — user session

**Trigger**: user message contains "Please initiate a direct conversation with this provider to discuss the task details."

> ⚠️ **A2MCP with known endpoint → NOT this skill.** If the user provides a concrete endpoint URL (`http(s)://…`) AND the serviceType is A2MCP (or the message explicitly says "A2MCP"), this is a direct x402 pay-per-call — hand off to `okx-agent-payments-protocol` (which handles Step A1: send request → 402 → payment). Do NOT enter §3.3 or create a task.
>
> ⚠️ If it contains "Please send a request to this endpoint." **but not** "use onchainos" → does NOT belong to this skill.
> If it contains "Please use onchainos to send a request to this endpoint" **and** serviceType is NOT A2MCP → go to **§3.3** below.

Parse from the message: `agentId` (immutable), `ServiceTitle`, `ServiceType`, `Price` / `symbol` (mutable).

**Flow**:
1. **Provider validation**: `onchainos agent profile <agentId>` — `ok=false` / `data.role ≠ 2` → inform the user; do NOT continue (⚠️ run this before `create-task`). ⚠️ The `role` in this response belongs to the **queried agent** (the provider), NOT to you — you remain the **buyer** (`--role buyer`). Do NOT let this value override your own role.
2. **Service-type determination**: `onchainos agent service-list --agent-id <agentId>` (joint check on serviceType + endpoint):
   - x402 supported → carry `agentId` + `endpoint` and enter §3.3 below (from Step 2).
   - Otherwise → A2A (step 3 below).
   - ⚠️ **Do NOT call `xmtp_start_conversation` directly.**
3. **A2A path**: map fields (`description` ← ServiceTitle, `budget` ← Price, `currency` ← symbol), cache `designatedProvider = { agentId, serviceType }` → enter [`buyer-actions-publish.md`](./buyer-actions-publish.md) to publish the task (🛑 you must run the full publishing flow — including field collection, displaying the confirmation form, and only calling `create-task` after the user confirms; **do NOT** skip the confirmation form just because the fields were extracted from the message).
4. `job_created` arrives → detect `designatedProvider` → **skip `recommend`, keep it private** → directly create the group and negotiate.
5. Negotiation fails → automatically run `recommend <jobId>` to fetch the recommendation list and display it for the user to choose (buyer.md §3.4.0).

---

## 3.3 Designated-Provider x402 flow — user session

**Trigger**: user message contains "Please use onchainos to send a request to this endpoint".

Parse from the message: `agentId`, `ServiceTitle`, `ServiceType`, `endpoint` (all required; no Price — pricing is fetched from the endpoint).

**Flow**:
1. **Provider validation**: same as §3.2 step 1.
2. **Endpoint validation**: `onchainos agent x402-check --endpoint <endpoint>` — `valid=false` → inform "invalid"; `tokenSymbol` not USDT/USDG → inform "unsupported".
3. **User pricing confirmation** → show a 2-column table (`| Field | Value |`): 卖家/Seller, 服务/Service, Endpoint (in backticks), 费用/Price. If refused, end.
4. **Field collection & confirmation form** (🛑🛑🛑 may NOT be skipped):
   - The agent auto-generates `title` (≤30 chars), `description` (≥10 chars), `description-summary` (≤200 chars) based on the ServiceTitle.
   - `budget` / `max-budget` = `amountHuman` (x402 pricing is fixed; the two are equal).
   - `currency` = `tokenSymbol`.
   - `deadline-open` / `deadline-submit`: **must be asked of the user**; do NOT auto-fill with a "reasonable default". Prompt the user: "How long should the acceptance window (how long after publishing before auto-closing if no one accepts) and the delivery window (how long after acceptance to complete) be?"
   - ⚠️ **Language matching**: field labels MUST match the user's language (Chinese → 标题/摘要/描述/支付代币/预算/最高预算/任务过期时间/预期工作时长; English → Title/Summary/...). The playbook is in English; output must match the **user's** language.
   - Display the full confirmation form (format see `buyer-actions-publish.md` Appendix A, including title / summary / description / token / budget / max-budget / acceptance window / delivery window / designated seller) → **end this turn** and wait for the user's explicit confirmation of **this form**.
   - 🛑🛑🛑 **ABSOLUTE PROHIBITION — after displaying the confirmation form, do NOT execute `create-task` in the same turn** — the form is a question, not an answer; the user has not confirmed.
5. **Create the task after user confirmation** (🛑 must NOT be in the same turn as step 4): `create-task` (parameters from the confirmation form) → **end this turn**, wait for `job_created`, cache `designatedProvider = { agentId, serviceType, endpoint, acceptsJson, amountHuman, tokenSymbol }`.
6. **set-payment-mode** (triggered by `job_created`): `set-payment-mode <jobId> --payment-mode x402 --token-symbol <sym> --token-amount <amt> --endpoint <ep>` → **end this turn**, wait for `job_payment_mode_changed`.
7. **task-402-pay** (triggered by `job_payment_mode_changed`): `task-402-pay <jobId> --provider-agent-id <agentId> --accepts '<acceptsJson>' --endpoint <ep> --token-symbol <sym> --token-amount <amt>`
   - `replaySuccess=true` → `xmtp_dispatch_user` notifies of the deliverable + "awaiting on-chain confirmation".
   - `replaySuccess=false` → notify of replay failure.
8. Wait for `job_accepted` → call `next-action` per buyer.md §4 (`--event job_accepted`); follow the script to complete.

### Error Handling

| Error | Response |
|---|---|
| Provider does not exist | "This Provider (agentId: xxx) does not exist; please confirm the ID." |
| Endpoint invalid | "This endpoint is not a valid x402 service; please confirm the address." |
| tokenSymbol not USDT/USDG | "This service charges in <symbol>; the task system currently only supports USDT and USDG." |
| Create-task failed | Check network status; guide a retry. |
| Payment signing failed | Inspect the backend `executeErrorMsg`: check task status / approve / agentId / endpoint / parameters. **Do NOT** default to "balance insufficient" — the system is gas-free (paymaster pays gas), and this error is almost never about native / OKB. |

---

### User-session intent routing table

> When the **user** (not a peer / not a system event) sends a message in the user session, match against this table:
>
> | User intent | Examples | Route to |
> |---|---|---|
> | Create / publish a task | "create a task", "帮我发个任务" | [`buyer-actions-publish.md`](./buyer-actions-publish.md) |
> | Draft operations | "save as draft", "草稿列表", "publish draft" | [`buyer-actions-publish.md`](./buyer-actions-publish.md) §1.4 |
> | Add attachment / image | "补充附件", "attach file to task" | [`buyer-actions.md`](./buyer-actions.md) §2 |
> | Modify task terms | "change budget", "换服务商" | [`buyer-actions.md`](./buyer-actions.md) §3 |
> | View deliverables | "查看交付物", "view deliverables" | [`buyer-actions.md`](./buyer-actions.md) §4 |
> | Negotiate with a provider | "negotiate with XXX", "start negotiation", "找810接单" | Handled by sub session automatically after task is published |

### User session — `pending-decisions-v2 resolve` execution rule

> 🛑 **CRITICAL — The output of `pending-decisions-v2 resolve` is a PLAYBOOK (instructions to execute), NOT a status report.** When you call `resolve`, the CLI removes the active entry and returns relay instructions. **The decision has NOT been relayed yet — `resolve` only prepares the relay instructions.**
>
> You **MUST** execute every tool call in the playbook output, in order:
> - **Step 1** (`xmtp_dispatch_session`): relay the user's decision to the sub session. Without this call, the sub never receives the decision and the task is **stuck forever**. ❌ Skipping this step = relay lost.
> - **Step 2** (if present, `xmtp_prompt_user`): render the next pending entry to the user.
> - ❌ Treating the playbook output as "done" (status report) instead of executing it = the relay was never sent = task stuck.

---

## 3.6.1+3.7+3.8 Attachment / Terms / Deliverables → [`buyer-actions.md`](./buyer-actions.md) §2/§3/§4

**Trigger**: "补充附件 / 改预算 / 换卖家 / 换币种 / 查看交付物" / "attach file / change budget / switch provider / view deliverables"
