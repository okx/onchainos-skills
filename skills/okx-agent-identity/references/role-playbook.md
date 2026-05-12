# Role Playbook — Router + Shared Rules

> Entry point for `agent create`. This file is intentionally thin: it routes to the three role-specific files and spells out the rules that apply to all of them. Read the matching role file for the full Q&A chain.

## Route to the right role file

| User intent | Read |
|---|---|
| "注册买家 / requester / buyer / 发任务被系统要求建身份" | `role-requester.md` (includes Passive Onboarding sub-flow) |
| "注册 provider / 服务方 / 上架服务" | `role-provider.md` |
| "注册 evaluator / 验证者 / 仲裁者 / 我想当仲裁" | `role-evaluator.md` (create-first; staking happens separately via `okx-agent-task`) |
| Incoming context `intent=need-requester` | `passive-onboarding.md` → `role-requester.md` |

If the user said "注册一个 agent" without specifying a role, ask the three-option question first using the **numbered-options pattern** (per `SKILL.md §Choice prompts` + `§Core Flow` gate 1) — never the prose `A / B / C` form, which is banned by the choice-prompt rule.

Chinese:
```
你要注册哪种身份？
  1. 买家（requester）— 发任务、付费买服务
  2. 服务方（provider）— 提供服务、接订单
  3. 验证者（evaluator）— 仲裁任务争议
回复数字 1/2/3。
```

English:
```
Which identity do you want to register?
  1. requester — publishes tasks, pays for services
  2. provider — offers services, delivers work
  3. evaluator — arbitrates task disputes
Reply with a number: 1/2/3.
```

Also accept a written role name (`requester` / `provider` / `evaluator` / `买家` / `服务方` / `验证者`) as a fallback, but the primary ask is numeric. CLI accepts `1`/`2`/`3` as `--role` aliases (`utils.rs:162-165`), so the numeric reply can pass straight through.

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

两条路都允许。用编号选项问（参考 `SKILL.md §Choice prompts`）。**Pre-check 返回 K ≥ 1 个 provider 时，列出所有现有 provider**（用户需要看到他们到底有哪些，才能判断是新开还是改其中之一）：

中文（K = 1）：
```
你已经有 1 个 provider 身份：#<N1>（<name1>）。这次是：
  1. 再开一个新的 provider（同一个地址可多开）
  2. 修改 #<N1> 的描述 / 头像 / 服务
回复 1 或 2。
```

中文（K ≥ 2，列出所有）：
```
你已经有 K 个 provider 身份：#<N1>（<name1>）, #<N2>（<name2>）, …, #<NK>（<nameK>）。这次是：
  1. 再开一个新的 provider（同一个地址可多开）
  2. 修改其中某一个
回复 1 或 2。
```

若用户选 2 且 K ≥ 2，**再问一次**让用户指定改哪个，使用单独的 numbered-options 提问：「想改哪个？回复编号 1（#<N1>）/ 2（#<N2>）/ … / K（#<NK>）。」

English (K = 1):
```
You have 1 existing provider identity: #<N1> (<name1>). What would you like to do?
  1. Register a new provider (multiple providers per address are allowed)
  2. Update #<N1> (description / picture / services)
Reply 1 or 2.
```

English (K ≥ 2, list them all):
```
You have K existing provider identities: #<N1> (<name1>), #<N2> (<name2>), …, #<NK> (<nameK>). What would you like to do?
  1. Register a new provider (multiple providers per address are allowed)
  2. Update one of them
Reply 1 or 2.
```

If the user picks 2 and K ≥ 2, ask a follow-up numbered question: "Which one? Reply with a number: 1 (#<N1>) / 2 (#<N2>) / … / K (#<NK>)."

Do not auto-choose for provider. Don't silently default. **Do not collapse the K ≥ 2 case to "one of them" without listing the ids** — the user must see the full list to make an informed pick (and to notice if they have stale providers they forgot about).

### Language

The prompt **must match the user's language**. Follow `SKILL.md §Language Matching`.

**Skip this pre-check entirely for passive onboarding** (`intent=need-requester`) — see `passive-onboarding.md`.

## Confirmation card

> ⛔ The card is **mandatory before every content-creating on-chain write** — `agent create` / `update` / `feedback-submit`. This is enforced by `SKILL.md §⛔ MANDATORY confirmation gate (non-overridable)`; that section is the canonical source. Memory preferences, plan-mode exit, one-shot capture, urgency, and "intent is obvious" all do **NOT** bypass it — see the rationalization list in `SKILL.md §Core Flow` gate 4. State toggles (`agent activate` / `agent deactivate`) are NOT gated and run directly via `SKILL.md §Intent → Sub-flow`.

Always a table of fields — never a bash blob. Match the user's language per `SKILL.md §Language Matching`. Render field labels and row values in one language only. For the `role` row you may show the CLI value once so the user sees what gets sent. See `display-formats.md` §Create/Update Diff for the full template with both language variants.

Chinese variant:

| 字段 | 值 |
|---|---|
| 角色 | 服务方 (`provider`) |
| 名字 | <...> |
| 描述 | <...> |
| 头像 | 默认 — 若用户上传了图片或给了链接，这里**直接贴实际 URL**（例：`https://…/abc.png`），不要写 "已上传" / "uploaded" / 提到 "CDN" 等占位词。 |
| 服务[1] 名称 / 描述 / 类型 / 价格 / 接口地址 | (仅 provider 有) |

End with: `确认无误回复 "执行" 即可。` (English: `Reply "execute" to run.`). Do not promise a verb the model could echo as pre-execution chatter — see `SKILL.md §Step 3 — No narration between confirmation and result`.

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

> Before invoking the CLI, run the **3-question pre-execute self-check** defined in `SKILL.md §Step 3: Execute` — externalize your answers (pre-check ran? confirm token in latest turn? card values byte-equal to CLI values?). **If any answer ≠ yes, render the confirmation card and wait — do NOT call the tool.** The canonical wording, command-specific reinterpretations, and full remediation table all live in `SKILL.md §Step 3`; do not maintain a parallel summary here.

After the user replies "执行" / "yes" / equivalent:

1. Run the CLI command once.
2. On success → render the detail card (`display-formats.md` §Agent detail card) + the role-specific next-step line (see each role file). **Exception — passive onboarding** (`intent=need-requester`): render **only one line** and **no detail card** per `passive-onboarding.md §Messages to the user` + `role-requester.md §Passive Onboarding → After success`. **For the writes in the `SKILL.md §Step 4` same-turn handoff whitelist** (`requester` / `provider` / `evaluator` create, plus `activate` / `deactivate`), the visible line is followed by the same-turn handoff to the downstream file specified in that whitelist (chat post-hook is a silent no-op outside an OpenClaw runtime). Do not stop between visible line and handoff. See each role file's §Post-success "Agent directive" block. **Passive onboarding does NOT trigger the same-turn chat handoff** either (`role-requester.md §Passive Onboarding` "Do NOT load after-agent-list-changed.md" rule).
3. On failure → render the error card (`display-formats.md` §Error card) + the recovery action (see `troubleshooting.md`). **Do NOT auto-retry.**

See `_shared/no-polling.md` — do NOT follow up with `agent get` / status poll. Same-turn skill handoffs in the §Step 4 whitelist are explicitly allowed (they are not polling).

## bash blocks in these files

Every `onchainos agent create ...` bash block inside `role-requester.md` / `role-provider.md` / `role-evaluator.md` is labeled **maintainer reference — not shown to user**. They are there so developers can grep for the exact CLI shape and keep translations in sync. Your user-facing output is the confirmation card, not the bash.
