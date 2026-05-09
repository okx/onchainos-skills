# Role: requester (买家)

> Registers a buyer identity. No services. The lightest of the three roles.

## STRICT — one question per turn

Every field is asked in its own message. Never list "请提供 1. Name 2. Description 3. ...". If the user volunteered multiple values in one sentence, you may capture them, but the confirmation table still renders each field on its own row.

Field definitions live in `field-specs.md`. When prompting, inline the four segments (`用途 / 可见范围 / 请注意 / 示例` for Chinese users; `Purpose / Visibility / Please note / Example` for English users) in the user's language only.

## Phase preview (render BEFORE Q1)

Once role is confirmed as `requester` and pre-check passed (requester is unique per address — if found, hand off to `update` per `role-playbook.md §Pre-check`), render a short declarative preview, then start Q1.

Chinese:
```
好，开始新 requester 的 create 流程。接下来会收集以下基本信息：
  1. 名称
  2. 描述
  3. 头像（可选）
```

English:
```
Got it — starting a new requester create. We'll collect:
  1. Name
  2. Description
  3. Picture (optional)
```

The preview is **declarative**, not imperative — it describes what's next but does NOT ask for all three at once. See `role-playbook.md §STRICT — Preview ≠ multi-field ask`. Immediately follow the preview with a blank line and `Q1：` / `Q1:`.

## Standard Q&A chain

Questions are labelled `Q1：` / `Q1:` (Chinese / English) in the message to the user. Each Q inlines the four-segment field spec from `field-specs.md` in the user's language only. If §One-shot capture already captured a field, **silently skip that Q** and move to the next.

| Q | Chinese prompt | English prompt | Validation | On failure |
|---|---|---|---|---|
| Q1 | `Q1：这个 requester 叫什么名字？` + 4 segments | `Q1: What's the name of this requester?` + 4 segments | non-empty, ≤ 64 chars | re-ask once with a shorter example |
| Q2 | `Q2：用一句话描述这个 requester。` + 4 segments | `Q2: Describe this requester in a sentence.` + 4 segments | non-empty, ≤ 500 chars | re-ask with a concrete example |
| Q3 | `Q3：要设置头像吗？` + Choice prompt (see `avatar-upload.md`) | `Q3: Want to set an avatar?` + Choice prompt | — | skip → backend default avatar |

No service questions. No staking. (Signing address is never asked — the CLI always uses the current wallet's selected XLayer address; `--address` does not exist.)

## Good / bad cases

| User input | Action |
|---|---|
| "叫 Alice" | Record `name=Alice`; next turn asks description only. |
| "描述你帮我来一个" | Decline politely — description is publicly searchable, the user should own the wording. Offer an example to anchor: "可以写成 `做 DeFi 研究，经常雇佣数据服务`，你改成合适的再发我。" |
| "我要一个 buyer 叫 Alice，做 DeFi 研究" | Capture `name=Alice` + `description=做 DeFi 研究` in one turn. Still render the confirmation table with each field on its own row. |
| "给我加个 5 USDT 的服务" | Explain: requester 不带 service；如果要对外收费请改注册 provider。不要把 service 拼进 requester 的 create。 |

## Confirmation

> ⛔ Mandatory before invoking the CLI. See `SKILL.md §⛔ MANDATORY confirmation gate (non-overridable)` — that section enumerates the rationalizations (`auto-execute` / plan-mode exit / one-shot capture / urgency / "intent obvious") that do **NOT** bypass it.

Show the two-column table (`display-formats.md` §Create/Update Diff → Create variant) in the user's language. Render ONE variant — never bilingual.

Chinese variant:

| 字段 | 值 |
|---|---|
| 角色 | 买家 (`requester`) |
| 名字 | Alice |
| 描述 | 做 DeFi 研究，经常雇佣数据服务 |
| 头像 | 默认 |

> 确认无误回复 "执行" 即可。

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

Render **one visible line** in the user's language (one of the four variants below), then follow the **agent directive** block (internal — not rendered to the user). Follow the `#<id>` placeholder rule in `display-formats.md` (top) — if the id is known from pre-check, include it; otherwise omit.

### Visible line (four variants)

All four variants are **declarative**, never a question — the same-turn handoff below continues without waiting for a user reply, so a question mark would create an unanswerable prompt. Also do **not** pre-announce the chat handoff (e.g. "preparing messaging layer next"): the chat flow is a silent no-op outside an OpenClaw runtime, so any pre-announce would mislead users in Claude Code / Claude Desktop / etc.

With known id (from pre-check `agent get` lookup), Chinese:
> 买家身份 #<id> 已注册，可以去 `okx-agent-task` 发任务。

Without id (current CLI only returns txHash), Chinese:
> 买家身份已注册，可以去 `okx-agent-task` 发任务。

English equivalents:
> Requester identity #<id> is live — head to `okx-agent-task` to publish a task.
> Requester identity registered — head to `okx-agent-task` to publish a task.

Do NOT mention the `okx-agent-chat/after-agent-list-changed.md` path to the user in the visible line — the same-turn handoff below loads that skill's own prompt, which decides on its own whether to surface anything (silent in non-OpenClaw runtimes).

### Agent directive (internal — do NOT render to the user)

After emitting the visible line above, **do not stop the turn**. In the same response, load `/skills/okx-agent-chat/after-agent-list-changed.md` and continue with its `Execution Flow` inside the same response — the local a2a agent list just changed, so the chat skill keeps the OpenClaw runtime side in sync (refresh-agents fast path or first-time install). The flow self-gates on `OPENCLAW_CLI` / `OPENCLAW_SHELL`, so it is a **silent no-op** in non-OpenClaw runtimes; loading it unconditionally is safe and is the documented contract.

**Skip the handoff** (render visible line only, then stop) if the user has already declined chat / messaging setup earlier in this conversation (e.g., "不用聊天 / no chat / skip messaging" or similar). Also skip on the **passive onboarding** path — see §Passive Onboarding `After success`.

This is one of the documented exceptions to `SKILL.md §Step 4`'s "Stop. Wait for the user." rule; see that section's whitelist for the carve-out.

**Do NOT** chase with `agent get` / status poll (that is about querying chain state — different from the same-turn handoff above, which just loads the next skill's prompt). See `_shared/no-polling.md`.

## Passive Onboarding — entry from `okx-agent-task`

When `okx-agent-task` hands control over with context `intent=need-requester` (the user tried to publish a task but has no requester agent yet), enter the **simplified requester sub-flow**.

### Simplified sub-flow

Skip these normally-required steps:

- Do **not** ask for `--role` — it's fixed as `requester`.
- Do **not** pre-check existing agents — the handoff already implied none exist.
- Do **not** ask for `picture` — use backend default.
- Do **not** render the Phase preview — passive mode is deliberately lean, go straight to Q1 (see `passive-onboarding.md`).

Keep these:

- Ask `name` as `Q1：` / `Q1:`.
- Ask `description` as `Q2：` / `Q2:`.
- Show confirmation table (still field-per-row, still mandatory).
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

Do NOT load `/skills/okx-agent-chat/after-agent-list-changed.md` here — passive mode is contracted to hand strictly back to `okx-agent-task` with the single line above (see `passive-onboarding.md` "No other chatter"). The task skill triggers the chat post-hook itself when its flow needs it.

### When user already has a requester

If a pre-existing requester agent happens to be found (e.g., the user returns mid-flow), **skip create** (requester is unique per address — see `role-playbook.md §Pre-check`). Echo in the user's language:
- 中文："你已经有买家身份 #<N>（<name>），直接用它继续发布任务。"
- English: "You already have requester identity #<N> (<name>) — using it to continue publishing the task."

Hand back.
