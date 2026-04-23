# Role: requester (买家)

> Registers a buyer identity. No services. The lightest of the three roles.

## STRICT — one question per turn

Every field is asked in its own message. Never list "请提供 1. Name 2. Description 3. ...". If the user volunteered multiple values in one sentence, you may capture them, but the confirmation table still renders each field on its own row.

Field definitions live in `field-specs.md`. When prompting, inline the four segments (`用途 / 可见范围 / 请注意 / 示例` for Chinese users; `Purpose / Visibility / Please note / Example` for English users) in the user's language only.

## Standard Q&A chain

| Turn | Ask | Validation | On failure |
|---|---|---|---|
| 1 | `Name` (see `field-specs.md`) | non-empty, ≤ 64 chars | re-ask once with a shorter example |
| 2 | `Description` (see `field-specs.md`) | non-empty, ≤ 500 chars | re-ask with a concrete example |
| 3 | (optional) `Picture` — "要上传头像吗？" branch to `avatar-upload.md` | — | skip → backend default avatar |

No service questions. No `--address` prompt. No staking.

## Good / bad cases

| User input | Action |
|---|---|
| "叫 Alice" | Record `name=Alice`; next turn asks description only. |
| "描述你帮我来一个" | Decline politely — description is publicly searchable, the user should own the wording. Offer an example to anchor: "可以写成 `做 DeFi 研究，经常雇佣数据服务`，你改成合适的再发我。" |
| "我要一个 buyer 叫 Alice，做 DeFi 研究" | Capture `name=Alice` + `description=做 DeFi 研究` in one turn. Still render the confirmation table with each field on its own row. |
| "给我加个 5 USDT 的服务" | Explain: requester 不带 service；如果要对外收费请改注册 provider。不要把 service 拼进 requester 的 create。 |

## Confirmation

Show the two-column table (`display-formats.md` §Create/Update Diff → Create variant) in the user's language. Render ONE variant — never bilingual.

Chinese variant:

| 字段 | 值 |
|---|---|
| 角色 | 买家 (`requester`) |
| 名字 | Alice |
| 描述 | 做 DeFi 研究，经常雇佣数据服务 |
| 头像 | 默认 |

> 确认无误回复 "执行" 我就下发。

English variant:

| Field | Value |
|---|---|
| Role | requester |
| Name | Alice |
| Description | Independent DeFi researcher, frequently buys data services. |
| Picture | default |

> Reply "execute" to run it.

**Do NOT show the bash command** unless the user explicitly asks ("把命令给我看" / "show me the CLI"). Confirmation cards are field-only.

## Execute (maintainer reference — not shown to user)

```bash
onchainos agent create \
  --role requester \
  --name "<name>" \
  --description "<description>" \
  [--picture "<url>"]
```

## Post-success suggestion

Single-line next step, in the user's language. Follow the `#<id>` placeholder rule in `display-formats.md` (top) — if the id is known from pre-check, include it; otherwise omit.

With known id (from pre-check `agent get` lookup), Chinese:
> 注册完成，买家身份 #<id> 已生效。要不要去 `okx-agent-task` 发布任务？

Without id (current CLI only returns txHash), Chinese:
> 买家身份已注册。要不要去 `okx-agent-task` 发布任务？

English equivalents:
> Requester identity #<id> is live. Want to head to `okx-agent-task` to publish a task?
> Requester identity registered. Want to head to `okx-agent-task` to publish a task?

**Do NOT** chase with `agent get` / status poll. See `_shared/no-polling.md`.

## Passive Onboarding — entry from `okx-agent-task`

When `okx-agent-task` hands control over with context `intent=need-requester` (the user tried to publish a task but has no requester agent yet), enter the **simplified requester sub-flow**.

### Simplified sub-flow

Skip these normally-required steps:

- Do **not** ask for `--role` — it's fixed as `requester`.
- Do **not** pre-check existing agents — the handoff already implied none exist.
- Do **not** ask for `picture` — use backend default.

Keep these:

- Ask `name` (turn 1).
- Ask `description` (turn 2).
- Show confirmation table (still field-per-row).
- Execute.

### After success

Return control to the caller. The response to the user contains:

1. The detail card of the new requester agent (follow §Language matching + `#<id>` rule — omit the Agent ID row if the id isn't available yet).
2. One line, in the user's language. With id available:
   - 中文："已为你创建买家身份 #<id>。现在继续回到发布任务的流程。"
   - English: "Requester identity #<id> created for you. Resuming the task-publish flow."
3. One line, without id:
   - 中文："已为你创建买家身份。现在继续回到发布任务的流程。"
   - English: "Requester identity created. Resuming the task-publish flow."

Do NOT ask "要不要发任务" / "want to publish a task?" — the task skill already has the pending intent; it will resume.

### When user already has a requester

If a pre-existing requester agent happens to be found (e.g., the user returns mid-flow), **skip create** (requester is unique per address — see `role-playbook.md §Pre-check`). Echo in the user's language:
- 中文："你已经有买家身份 #<N>（<name>），直接用它继续发布任务。"
- English: "You already have requester identity #<N> (<name>) — using it to continue publishing the task."

Hand back.
