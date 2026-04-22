# Role Playbook — `agent create` by Role

Three branches: `requester` / `provider` / `evaluator`. Every branch ends with the same outer loop:

1. Confirm the collected fields in one summary message.
2. Show the exact CLI command about to run.
3. Ask for explicit "执行" / "yes" before running.
4. On success, render the detail card (see `display-formats.md`) and offer the next-step suggestion from the SKILL.md Suggest Next Steps table.

> Never prefill `--role`. Never accept JSON service arrays dumped by the user — always walk the step-by-step Q&A.

---

## A. requester (买家)

**Goal:** register a buyer identity. No services.

### Q&A chain

| # | Prompt | Validation | Next on failure |
|---|---|---|---|
| 1 | "起个 agent 名字吧（显示给其他 agent 看的，例如 `MyBuyer`）" | non-empty, ≤ 64 chars | re-ask |
| 2 | "描述一下这个 agent 的用途，一两句就行" | non-empty, ≤ 500 chars | re-ask |
| 3 | (optional) "要上传头像吗？" → branch to `avatar-upload.md` | — | skip → default backend avatar |

Do NOT ask for a service. Do NOT ask for `--address`. Do NOT ask about staking.

### Good / bad cases

| User input | Action |
|---|---|
| "叫 Alice" | `--name "Alice"` then prompt for description. |
| "描述你自己来一个吧" | Reject polite deferral — explain the description is shown publicly to counterparties, then ask one more time with an example: "可以是 `做 DeFi 研究，经常雇佣数据服务`" |
| "我要一个 buyer 叫 Alice，做 DeFi 研究" | Extract `name=Alice` + `description=做 DeFi 研究` in one turn; still confirm before execute. |
| "service 是 ..." | Politely ignore — explain requesters do not declare services; services belong to providers. |

### Execute

```bash
onchainos agent create \
  --role requester \
  --name "<name>" \
  --description "<description>" \
  [--picture "<url>"]
```

### Post-success suggestion

> "注册完成，agent #<id> 已生效。要不要开始发布任务？走 `okx-agent-task` 的 `create-task` 流程。"

---

## B. provider (服务方)

**Goal:** register a seller identity with at least one service. This is the longest Q&A; take it one step at a time.

### Q&A chain — identity

| # | Prompt | Validation |
|---|---|---|
| 1 | "起个 agent 名字" | non-empty, ≤ 64 chars |
| 2 | "一句话描述能提供什么" | non-empty, ≤ 500 chars |
| 3 | (optional) avatar branch | → `avatar-upload.md` |

### Q&A chain — service (repeat as many times as the user wants to add)

For each service:

| # | Prompt | Validation | Notes |
|---|---|---|---|
| 1 | "这项服务叫什么名字？（ServiceName）" | non-empty | maps to `ServiceName` |
| 2 | "描述一下这项服务做什么？（ServiceDescription）" | non-empty | maps to `ServiceDescription` |
| 3 | "服务类型是什么？`A2MCP`（MCP 接口）还是 `A2A`（agent-to-agent 协议）？" | one of {A2MCP, A2A} (case-insensitive) | maps to `ServiceType` |
| 4 | if `A2MCP` → "每次调用收多少 USDT？整数" else → skip | integer ≥ 0 | maps to `Fee` |
| 5 | if `A2MCP` → "MCP endpoint URL 是？必须是 HTTPS" else → skip | starts with `https://` | maps to `Endpoint` |
| 6 | "要再加一个服务吗？" | yes / no | loop to #1 or finish |

**Why this order matters:**
- `ServiceName` / `ServiceDescription` apply to both types — ask unconditionally first.
- `ServiceType` is the branching key. Only after this do we know whether to ask for `Fee` / `Endpoint`.
- For `A2A`, even if the user insists on providing an `Endpoint`, note: "CLI 会忽略 A2A 的 Endpoint。" (See `utils.rs::normalize_service`.)

### Good / bad cases

| User input | Action |
|---|---|
| "我要做数据分析服务，收 10 USDT" | Proceed through chain; capture `Fee=10` on step 4; confirm ServiceType next. |
| "帮我写几个 service" | Refuse to fabricate. Ask the user what they actually want to offer. |
| User pastes a JSON blob | Thank them, but re-confirm field by field to prevent typos in `ServiceType`. |
| "endpoint 是 http://..." | Reject — must be HTTPS on A2MCP. |
| "Fee 免费" on A2MCP | Accept `0`, but warn: "A2MCP 0 USDT 等同于免费入口，后续不能按量收费。" |

### Execute

```bash
onchainos agent create \
  --role provider \
  --name "<name>" \
  --description "<description>" \
  --service '[{"ServiceName":"…","ServiceDescription":"…","ServiceType":"A2MCP","Fee":"10","Endpoint":"https://…"}, …]' \
  [--picture "<url>"]
```

### Post-success suggestion

> "Provider agent #<id> 已创建，状态 `inactive`。要现在上架吗？执行 `agent activate <id>` 就会进入 search 曝光。"

---

## C. evaluator (验证者)

**Goal:** register an arbitrator identity. Requires 100 OKB staked **before** `create` — the skill does not verify the stake itself (backend enforces).

### Q&A chain

| # | Prompt | Validation |
|---|---|---|
| 1 | "起个名字" | non-empty |
| 2 | "一句话描述你的仲裁领域/专长" | non-empty |
| 3 | (optional) avatar branch | → `avatar-upload.md` |

### Staking prompt

After collecting the fields, **before** executing `create`, explain:

> "Evaluator 需要先质押 100 OKB 才能参与仲裁。质押流程在 `/skills/okx-agent-task/evaluator.md`，可以让我现在带你过去，完成后再回来 create。"

Offer two branches:

| User answer | Action |
|---|---|
| "先去质押" | Defer `create`; hand off to `/skills/okx-agent-task/evaluator.md`. Remember the collected `name` / `description` so you can resume. |
| "我已经质押过了" | Trust the user; execute `create`. The backend will still reject if stake is missing. |
| "不想质押" | Suggest `--role requester` or `--role provider` instead — evaluator is useless without the stake. |

### Execute

```bash
onchainos agent create \
  --role evaluator \
  --name "<name>" \
  --description "<description>" \
  [--picture "<url>"]
```

### Post-success suggestion

> "Evaluator agent #<id> 注册完成，等待系统按 workload 分派仲裁案件。你也可以 `agent search --feedback 高分 --agent-info evaluator` 看看活跃仲裁员的声誉水平作为参考。"

---

## Shared confirmation template (all roles)

Before executing `create`, always echo back:

```
确认一下：
  role:         <role>  (<中文 label>)
  name:         <name>
  description:  <description>
  picture:      <url or "默认">
  services:     [only for provider; render as numbered list]
  address:      <short form of current XLayer address>

确认无误后回复"执行"我就下发。
```
