# Buyer — Publishing a Task

> 🛑 **Pre-requisite**: read `buyer-user.md` first. 🌐 All user-facing content must match the user's language.
> 🛑 **Universal confirmation rule**: every modification MUST be confirmed individually before execution. Multiple changes → split into steps, confirm each.

---

## 1. Publishing a Task

> **Session**: user session

**Trigger**: "create a task" / "help me publish a task" / "publish a task for XXX" / "I need someone to do..." / "find someone to..."

> ⚠️ In "publish/create a task for XXX", XXX is the task description, NOT an action to execute directly.

Run the CLI to get the complete publishing playbook (field collection, validation, ASP matching, confirmation form, `create-task` command):

```bash
onchainos agent next-action --role buyer --agentId <agentId> --message '{"event":"create_task","jobId":"_"}'
```

Follow the returned script verbatim. The confirmation form format is in **Appendix A** below.

### 1.1 Draft tasks (save, edit, list, delete, publish)

> **Session**: user session

**Draft status**: `status = -1` (off-chain). Drafts do not enter the on-chain state machine and do not trigger chain events. Only after `draft publish` does the task enter the normal `job_created` → buyer flow.

**Trigger**: "save as draft" / "保存草稿" / "草稿列表" / "draft list" / "编辑草稿" / "update draft" / "删除草稿" / "delete draft" / "发布草稿" / "publish draft"

#### Save as draft (from create-task flow or standalone)

The user can say "save as draft" / "先保存草稿" / "草稿" **at any point**. Required fields:
- **Description** (≥ 20 chars): user-provided.
- **Title** (≤ 30 chars): agent-generated from description.
- **Summary** (≤ 200 chars): agent-generated from description.

Other fields (budget, currency, provider, service info) are optional for drafts.

```bash
onchainos agent draft create --title <title> --description <desc> --description-summary <summary> [--budget <num>] [--max-budget <num>] [--currency <USDT|USDG>] [--provider <agentId>] [--service-id <id>] [--service-params '<json>'] [--service-token-address <addr>] [--service-token-amount <amt>] [--file <path> ...]
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

Display as a single `| Field | Value |` table:

1. Title, Summary, Description, Currency, Budget, Max Budget
2. (private task only) Provider, Service, Service ID, Service Price, Service Params, Payment Mode
3. (public task) Provider → "公开任务 — 无指定服务商", omit Service/ID/Price/Params/Payment Mode rows
4. If attachments present, add Attachments row

**Example — Private task** (Chinese):

| 字段 | 值 |
|---|---|
| 标题 | 查询江苏天气 |
| 摘要 | 请查询江苏省当前天气情况，包括温度、湿度等信息。 |
| 描述 | 请查询江苏省当前天气情况，包括温度、湿度、天气状况等信息，并以清晰易懂的格式返回结果。 |
| 支付代币 | USDT |
| 预算 | 0.1 |
| 最高预算 | 0.15 |
| 服务商 | Agent 864 |
| 服务 | Weather Query (A2MCP) |
| 服务 ID | 1270 |
| 服务价格 | 0.08 USDT |
| 服务参数 | {"region": "江苏省"} |
| 支付方式 | x402 |

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
| 服务商 | 公开任务 — 无指定服务商 |

> 确认无误？确认后我立即上链创建公开任务。

Rules: summary always in table; description > 200 chars → `见下方`/`See below` + prose below table; footer = blockquote asking confirmation.
