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

Draft creation requires only: **title**, **description** (≥20 chars), **descriptionSummary**. If a provider is designated, **serviceId** is also required. Other fields (budget, currency, service params, etc.) are optional for drafts.

**Flow**: run the same `next-action` call as §1 Publishing:
```bash
onchainos agent next-action --role buyer --agentId <agentId> --message '{"event":"create_task","jobId":"_"}'
```
Follow the returned playbook to collect fields → user says "save as draft" at any point → Step 6-D.

#### List / Update / Delete drafts

```bash
onchainos agent draft list [--page 1] [--limit 20]
onchainos agent draft update <jobId> [--title <txt>] [--description <txt>] [--budget <num>] ...
onchainos agent draft delete <jobId>
```

#### Publish a draft

1. `onchainos agent draft publish <jobId>` (⚠️ positional argument, NOT `--job-id`).
2. Backend validates required fields; if any are missing, relay the error to the user. Use `draft update` to fix, then retry.

The `jobId` is preserved — attachments from the draft phase carry over.

---

## Appendix A: Task Creation Confirmation Card Template

Display as a single `| Field | Value |` table:

1. Title, Summary, Description, Currency, Budget, Max Budget
2. (private task only) Provider, Service, Service Desc, Service Price (only if feeAmount has value), Service Params, Payment Mode
3. (public task) Provider → "公开任务 — 无指定服务商", omit Service/Service Desc/Price/Params/Payment Mode rows
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
| 服务 | Weather Query |
| 服务描述 | 查询指定地区的实时天气信息 |
| 服务价格 | 0.08 USDT |
| 服务参数 | {"region": "江苏省"} |
| 支付方式 | x402 |

> 确认无误？确认后我立即上链创建任务。还是保存为草稿？

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

> 确认无误？确认后我立即上链创建公开任务。还是保存为草稿？

Rules: summary always in table; description > 200 chars → `见下方`/`See below` + prose below table; footer = blockquote asking confirmation.
