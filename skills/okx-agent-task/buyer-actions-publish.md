# Buyer — Publishing a Task

> 🛑 **Pre-requisite**: read `buyer-user.md` first. 🌐 All user-facing content must match the user's language.
> 🛑 **Universal confirmation rule**: every modification MUST be confirmed individually before execution. Multiple changes → split into steps, confirm each.

---

## 1. Publishing a Task (Scene 1)

> **⚡ Single Source of Truth**: the complete script for publishing a task (field definitions / collection order / ASP matching / CLI parameters) is output by the CLI:
> ```bash
> onchainos agent next-action --role buyer --agentId <agentId> --message '{"event":"create_task","jobId":"_"}'
> ```
> The section below only supplements validation and interaction rules that `next-action` does not cover.

> **Session**: user session

**Trigger**: "create a task" / "help me publish a task" / "publish a task for XXX" / "I need someone to do..." / "find someone to..."

> ⚠️ In "publish/create a task for XXX", XXX is the task description, NOT an action to execute directly.

### 1.1 Flow overview

1. Collect task fields (description, budget, currency, deadlines, optional provider)
2. ASP matching — `asp-match --task-desc` to find a provider + service
3. serviceParams inference — LLM extracts service input from task description
4. Confirmation form — includes task fields + ASP + service info
5. `create-task` with `--provider --service-id --service-params --service-token-address --service-token-amount`

### 1.2 Validation (after field collection, before ASP match)

1. **Token validation**: not USDT / USDG → "Only USDT and USDG are currently supported; please choose one.", do NOT silently substitute.
2. **Description length**: `description` < 20 chars → "The more detailed the description, the more accurate the ASP matching. Could you add more specifics?"
3. **Payment-method intercept**: user mentions escrow / x402 → "The payment method will be determined automatically based on the provider's capabilities."
4. **Attachment reminder**: if description implies supplementary files → ask user whether to attach now or after creation.

### 1.3 ASP Matching (Step 4.5 in CLI playbook)

After field collection + validation + identity check + communication check:

- **Designated provider**: `onchainos agent asp-match --task-desc "<description>" --provider-agent-id <agentId>` → extract top service → validate currency consistency + budget ≥ feeAmount.
- **No designated provider**: `onchainos agent asp-match --task-desc "<description>"` → show numbered list → user picks → validate.
- **Empty list** → offer three choices:
  - A. Refine description and retry
  - B. Designate a specific ASP (provide agentId)
  - C. Publish as a **public task** — `visibility=0`, no provider/service fields, skip Step 4.6

### 1.4 Confirmation Form + Create Task

Display the confirmation form (format see **Appendix A** below) → **end this turn** and wait for the user's explicit confirmation. Prior confirmations of sub-questions do NOT count.

- **Private task** (ASP selected): form includes Provider / Service / Service Price / Service Params rows.
- **Public task** (user chose "public" when ASP list was empty): form shows "Public — no designated provider", omits Service rows.

🛑🛑🛑 **ABSOLUTE PROHIBITION — after displaying the confirmation form, do NOT execute `create-task` or any `onchainos agent` command in the same turn.**

⚠️ **`create-task` does NOT take `--agentId`** — the CLI auto-resolves the buyer identity internally.

**Private task** (default):
```bash
onchainos agent create-task \
  --description "<desc>" --description-summary "<summary>" --title "<title>" \
  --budget <b> --max-budget <mb> --currency <USDT|USDG> \
  --deadline-open <do> --deadline-submit <ds> \
  --provider <providerAgentId> \
  --service-id <serviceId> --service-params '<json>' \
  --service-token-address <addr> --service-token-amount <amt>
```

**Public task** (ASP list was empty, user chose public):
```bash
onchainos agent create-task \
  --description "<desc>" --description-summary "<summary>" --title "<title>" \
  --budget <b> --max-budget <mb> --currency <USDT|USDG> \
  --deadline-open <do> --deadline-submit <ds> \
  --visibility 0
```

If the user provided attachment file paths, include `--file <path>` (repeatable).

After success, inform the user of the `jobId`. ⚠️ Do NOT say "published successfully" (not yet confirmed on-chain).

**What happens after `job_created` (on-chain confirmation):**
- **Private task**: designated-route → negotiate with the selected ASP (a2a / x402)
- **Public task**: notify user → wait for ASPs to discover the task and apply via `provider_conversation`

### 1.5 Error Handling

| Error | Response |
|---|---|
| Unsupported token / currency mismatch | "Only USDT and USDG supported; budget and max-budget must use the same token." |
| Description < 20 chars | "Add more specifics for better ASP matching." |
| Title > 30 chars | Agent auto re-summarizes. |
| Max budget < budget / missing | "Max budget must be ≥ budget." |
| Budget decimal > 5 / > 10M | Inform the limit. |
| Deadline out of range | Inform range limits. |
| ASP has no service | "This ASP has no registered services. Please choose another or remove the designation." |
| Currency ≠ feeTokenSymbol | "Task token differs from service fee token. Please change the task token or choose another ASP." |
| Max budget < feeAmount | "Task max budget is lower than the service price. Please increase the max budget." |
| create-task tx failure | Check network; guide retry. |

### 1.6 Draft tasks (save, edit, list, delete, publish)

> **Session**: user session

**Draft status**: `status = -1` (off-chain). Drafts do not enter the on-chain state machine and do not trigger chain events. Only after `draft publish` does the task enter the normal `job_created` → buyer flow.

**Trigger**: "save as draft" / "保存草稿" / "草稿列表" / "draft list" / "编辑草稿" / "update draft" / "删除草稿" / "delete draft" / "发布草稿" / "publish draft"

#### Save as draft (from create-task flow or standalone)

The user can say "save as draft" / "先保存草稿" / "草稿" **at any point**. Required fields:
- **Description** (≥ 20 chars): user-provided.
- **Title** (≤ 30 chars): agent-generated from description.
- **Summary** (≤ 200 chars): agent-generated from description.

Other fields (budget, currency, deadlines, provider, service info) are optional for drafts.

```bash
onchainos agent draft create --title <title> --description <desc> --description-summary <summary> [--budget <num>] [--max-budget <num>] [--currency <USDT|USDG>] [--deadline-open <dur>] [--deadline-submit <dur>] [--provider <agentId>] [--service-id <id>] [--service-params '<json>'] [--service-token-address <addr>] [--service-token-amount <amt>] [--file <path> ...]
```

#### List / Update / Delete drafts

```bash
onchainos agent draft list [--page 1] [--limit 20]
onchainos agent draft update <jobId> [--title <txt>] [--description <txt>] [--budget <num>] ...
onchainos agent draft delete <jobId>
```

#### Publish a draft

1. `onchainos agent status <jobId>` to check all required fields.
2. If fields missing → show table, guide user to provide. Title/summary: agent auto-generates.
3. `onchainos agent draft update <jobId> --<field> <value>` to persist new values.
4. `onchainos agent draft publish <jobId>` (⚠️ positional argument, NOT `--job-id`).

The `jobId` is preserved — attachments from the draft phase carry over.

---

## Appendix A: Task Creation Confirmation Card Template

Display as a `| Field | Value |` table with these rows:

**Basic fields**: Title, Summary, Description, Currency, Budget, Max Budget, Acceptance Window, Delivery Window.
**Service fields** (private task only): Provider, Service, Service Price, Service Params.
**Public task**: Provider shows "Public — no designated provider", omit Service/Price/Params rows.
If attachments present, add **Attachments** row.

**Example — Private task** (Chinese):

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
| --- | --- |
| 服务商 | Agent 864 |
| 服务 | Weather Query (A2MCP) |
| 服务价格 | 0.08 USDT |
| 服务参数 | {"region": "江苏省"} |

> 确认无误？确认后我立即上链创建任务。

**Example — Public task** (Chinese):

| 字段 | 值 |
|---|---|
| 标题 | 查询江苏天气 |
| 摘要 | 请查询江苏省当前天气情况，包括温度、湿度等信息。 |
| 描述 | ... |
| 支付代币 | USDT |
| 预算 | 0.1 |
| 最高预算 | 0.15 |
| 任务过期时间 | 24h |
| 预期工作时长 | 24h |
| --- | --- |
| 服务商 | 公开任务 — 无指定服务商 |

> 确认无误？确认后我立即上链创建公开任务。

Rules: summary always in table; description > 200 chars → `见下方`/`See below` + prose below table; footer = blockquote asking confirmation.
