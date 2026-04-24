# Role Playbook — Router + Shared Rules

> Entry point for `agent create`. This file is intentionally thin: it routes to the three role-specific files and spells out the rules that apply to all of them. Read the matching role file for the full Q&A chain.

## Route to the right role file

| User intent | Read |
|---|---|
| "注册买家 / requester / buyer / 发任务被系统要求建身份" | `role-requester.md` (includes Passive Onboarding sub-flow) |
| "注册 provider / 服务方 / 上架服务" | `role-provider.md` |
| "注册 evaluator / 验证者 / 仲裁者 / 我想当仲裁" | `role-evaluator.md` (create-first; staking happens separately via `okx-agent-task`) |
| Incoming context `intent=need-requester` | `passive-onboarding.md` → `role-requester.md` |

If the user said "注册一个 agent" without specifying a role, ask the three-option question first:

> "你要注册哪种身份？买家 (requester) / 服务方 (provider) / 验证者 (evaluator)？"

Do NOT default. Do NOT guess from the name / description fields.

## Field prompts

All eight fields (Name / Description / Picture / `name` / `servicedescription` / `servicetype` / `fee` / `endpoint`) have standardized four-segment specs — **用途 / 可见范围 / 请注意 / 示例** (Chinese) or **Purpose / Visibility / Please note / Example** (English). See `field-specs.md`. When you ask the user a field, inline all four segments with the question in the user's language only (never mix languages in one message).

## STRICT — one question per turn

Applies to every role flow. Applies to every service sub-field. No exceptions.

- Never list "请提供 1. Name 2. Description 3. ..." / "Please provide 1. Name 2. Description 3. ..." in one message as an **imperative ask**.
- Never enumerate more than one field per turn in an **asking** message.
- If the user volunteered multiple values in one sentence ("叫 Alice，做 DeFi 分析"), you may capture them at parse time (see `SKILL.md §One-shot capture`) — but the confirmation card still renders one row per field, and any still-unanswered fields are still asked one at a time.
- The rationale is not just UX; users answer one question more accurately than a list. List format causes dropped fields and typos that force re-prompting, which is worse than the extra turns.

### Preview ≠ multi-field ask

Showing a **declarative preview** at the start of each phase ("接下来会问你：名称、描述、头像（可选）。" / "Next we'll collect: Name, Description, Picture (optional).") is **allowed and encouraged** — it sets expectations and lets users decide whether to one-shot. Previews are statements, not asks; they are always followed by a single `Q1：` / `Q1:` asking exactly one field.

The distinction is verb mood:

- ❌ Banned (imperative, multi-field): "请提供 1. 名称 2. 描述 3. 头像" / "Please provide: 1. Name 2. Description 3. Picture"
- ✅ Allowed (declarative preamble + single Q): "接下来会收集：名称、描述、头像（可选）。\n\nQ1：这个 provider 叫什么名字？" / "Next we'll collect: Name, Description, Picture (optional).\n\nQ1: What's the name of this provider?"

If in doubt: the preamble describes what will happen; the Q asks for exactly one thing.

## Pre-check existing agents (normal onboarding only)

Before entering any role flow triggered by the user's own initiative, run `agent get` **once** to see if they already have an agent of the requested role.

**每个地址下 requester 和 evaluator 只能各有一个**（产品约定，后端兜底拒绝第二次）。Provider 不限——同一个地址可以有多个 provider 身份（便于分别提供不同服务）。Pre-check 结果按 role 分两条支路：

### requester / evaluator（唯一身份）

如果已存在同 role 的 agent，**不要**提供"新建"选项，不要进入 create 流程。直接告知并指向 update：

> 中文："你已经有 <role> 身份 #<N>（<name>）。同一个地址只能注册一个 <role>，想改描述 / 头像就说 `更新 #<N>`，或者直接拿这个身份继续用。"
>
> English: "You already have a <role> identity #<N> (<name>). Each address can only register one <role> — say `update #<N>` if you want to edit description / picture, or just keep using this one."

用户如果坚持要另一个，重申产品限制，不要绕开（后端会拒）。

### provider（可多开）

两条路都允许。用编号选项问（参考 `SKILL.md §Choice prompts`）：

中文：
```
你已经有一个 provider 身份 #<N>（<name>）。这次是：
  1. 再开一个新的 provider（同一个地址可多开）
  2. 修改现有这个的描述 / 头像 / 服务
回复 1 或 2。
```

English:
```
You already have a provider identity #<N> (<name>). What would you like to do?
  1. Register a new provider (multiple providers per address are allowed)
  2. Update the existing one (description / picture / services)
Reply 1 or 2.
```

Do not auto-choose for provider. Don't silently default.

### Language

The prompt **must match the user's language**. Follow `SKILL.md §Language matching`.

**Skip this pre-check entirely for passive onboarding** (`intent=need-requester`) — see `passive-onboarding.md`.

## Confirmation card

Always a table of fields — never a bash blob. Match the user's language per `SKILL.md §Language matching`. Render field labels and row values in one language only. For the `role` row you may show the CLI value once so the user sees what gets sent. See `display-formats.md` §Create/Update Diff for the full template with both language variants.

Chinese variant:

| 字段 | 值 |
|---|---|
| 角色 | 服务方 (`provider`) |
| 名字 | <...> |
| 描述 | <...> |
| 头像 | 默认 — 若用户上传了图片或给了链接，这里**直接贴实际 URL**（例：`https://…/abc.png`），不要写 "已上传" / "uploaded" / 提到 "CDN" 等占位词。 |
| 服务[1] 名称 / 描述 / 类型 / 价格 / 接口地址 | (仅 provider 有) |

End with: `确认无误回复 "执行" 我就下发。`

English variant:

| Field | Value |
|---|---|
| Role | provider |
| Name | <...> |
| Description | <...> |
| Picture | default — if the user uploaded an image or supplied a link, render the **actual URL verbatim** here (e.g. `https://…/abc.png`). Never write "uploaded" / "已上传" / mention "CDN" as a placeholder. |
| Service [1] Name / Description / Type / Fee / Endpoint | (provider only) |

End with: `Reply "execute" to run it.`

**The bash `onchainos agent create ...` command is NOT shown in the confirmation card.** Show it only if the user explicitly says "把命令给我看" / "show me the CLI".

## Execute

After the user replies "执行" / "yes" / equivalent:

1. Run the CLI command once.
2. On success → render the detail card (`display-formats.md` §Agent detail card) + the role-specific next-step line (see each role file).
3. On failure → render the error card (`display-formats.md` §Error card) + the recovery action (see `troubleshooting.md`). **Do NOT auto-retry.**

See `_shared/no-polling.md` — do NOT follow up with `agent get` / status poll.

## bash blocks in these files

Every `onchainos agent create ...` bash block inside `role-requester.md` / `role-provider.md` / `role-evaluator.md` is labeled **maintainer reference — not shown to user**. They are there so developers can grep for the exact CLI shape and keep translations in sync. Your user-facing output is the confirmation card, not the bash.
