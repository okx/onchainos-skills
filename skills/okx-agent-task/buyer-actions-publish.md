# Buyer — Publishing a Task

> 🛑 **Pre-requisite**: read `buyer-user.md` first. 🌐 All user-facing content must match the user's language.
> 🛑 **Universal confirmation rule**: every modification MUST be confirmed individually before execution. Multiple changes → split into steps, confirm each.

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
4. **Attachment reminder**: if description implies supplementary files (e.g. "see attached" / "参考附件" / "如图" / "详见文件") → ask user whether to attach now or after creation.

### 1.2 Confirmation Form + Create Task

All fields ready → **identity & balance check**:
1. Check whether the current account already has a buyer agent → if yes, use it directly (one account has at most 1 buyer; a wallet may have multiple accounts).
2. No buyer agent → guide the user to create one first (`onchainos agent create --role 1 --name <name> --description <desc>`).
3. Insufficient balance → warn but **do not block**.

⚠️ **Language matching**: the confirmation form field labels **MUST** match the user's conversation language. Chinese conversation → Chinese labels (标题 / 摘要 / 描述 / 支付代币 / 预算 / 最高预算 / 任务过期时间 / 预期工作时长); English conversation → English labels (Title / Summary / Description / Currency / Budget / Max Budget / Acceptance Window / Delivery Window). The playbook is written in English; this does NOT mean the output should be English — always match the **user's** language.

Display the confirmation form (format see **Appendix A** below) → **end this turn** and wait for the user's explicit confirmation of **this form**. Prior confirmations of sub-questions do NOT count.

🛑🛑🛑 **ABSOLUTE PROHIBITION — after displaying the confirmation form, do NOT execute `create-task` or any `onchainos agent` command in the same turn** — the form is a **question**, not an **answer**; the user has not confirmed; you do not have the authority to decide for the user. It must be a **new turn after the user sees the form** before you may execute the CLI. Violation = an unauthorized on-chain operation = funds at risk.

⚠️ **`create-task` does NOT take `--agentId`** — the CLI auto-resolves the buyer identity internally. Do NOT pass `--agentId` or `--agent-id` to `create-task`; that parameter belongs to `next-action`, not `create-task`.

If the user provided attachment file paths, include them in the `create-task` call via `--file <path>` (repeatable for multiple files). The CLI copies files to `~/.onchainos/task/<jobId>/attachments/` after the jobId is obtained.

After success, inform the user of the `jobId`. ⚠️ Do NOT say "published successfully" (not yet confirmed on-chain). ⚠️ Do NOT call `recommend` (wait for `job_created` to trigger it automatically).

### 1.3 Error Handling

| Error | Response |
|---|---|
| Unsupported token / currency mismatch | "Only USDT and USDG supported; budget and max-budget must use the same token." |
| Description < 10 chars | "Add more specifics for better Provider matching." |
| Title > 30 chars | Agent auto re-summarizes. |
| Max budget < budget / missing | "Max budget must be ≥ budget. Please set it (upper limit during negotiation)." |
| Budget decimal > 5 / > 10M | Inform the limit. |
| Deadline out of range | Inform range limits. |
| create-task tx failure | Check network; guide retry. |

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

Displays a table: `jobId` / `Title` / `Budget` / `Status` (all drafts show `📝 Draft`). Budget shows `{amount} {symbol}` or `—` if unset. Empty list → `No drafts found.` / `暂无草稿。`

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

## Appendix A: Task Creation Confirmation Card Template

Display as a `| Field | Value |` table with these rows: **Title**, **Summary**, **Description**, **Currency**, **Budget**, **Max Budget**, **Acceptance Window**, **Delivery Window**. If attachments present, add **Attachments** row with file count + names.

Example (Chinese — translate labels to match user's language):

| 字段 | 值 |
|---|---|
| 标题 | 查询江苏天气 |
| 摘要 | 请查询江苏省当前天气情况，包括温度、湿度等信息。 |
| 描述 | 请查询江苏省当前天气情况，包括温度、湿度、天气状况等信息，并以清晰易懂的格式返回结果。 |
| 支付代币 | USDT |
| 预算 | 0.1 |
| 最高预算 | 0.15 |
| 任务过期时间 | 24h |
| 预期工作时长 | 24h |

> 确认无误？确认后我立即上链创建任务。

Rules: summary always in table; description > 200 chars → `见下方`/`See below` + prose below table; no Visibility row; no acceptance-criteria row; footer = blockquote asking confirmation.
