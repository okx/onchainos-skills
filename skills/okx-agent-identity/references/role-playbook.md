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

All eight fields (Name / Description / Picture / ServiceName / ServiceDescription / ServiceType / Fee / Endpoint) have standardized four-segment specs — **用途 / 可见范围 / 约束 / 示例**. See `field-specs.md`. When you ask the user a field, inline all four segments with the question.

## STRICT — one question per turn

Applies to every role flow. Applies to every service sub-field. No exceptions.

- Never list "请提供 1. Name 2. Description 3. ..." in one message.
- Never enumerate more than one field per turn.
- If the user volunteered multiple values in one sentence ("叫 Alice，做 DeFi 分析"), you may capture them at parse time — but the confirmation card still renders one row per field.
- The rationale is not just UX; users answer one question more accurately than a list. List format causes dropped fields and typos that force re-prompting, which is worse than the extra turns.

## Pre-check existing agents (normal onboarding only)

Before entering any role flow triggered by the user's own initiative, run `agent get` **once** to see if they already have an agent of the requested role. If yes, reply:

> "你已经有一个 <role> agent (#N <name>)。要继续新建一个，还是更新现有的？"

Do not auto-choose.

**Skip this pre-check entirely for passive onboarding** (`intent=need-requester`) — see `passive-onboarding.md`.

## Confirmation card

Always a table of fields — never a bash blob. See `display-formats.md` §Create/Update Diff for the exact template.

| Field | Value |
|---|---|
| role | <english> (<中文>) |
| name | <...> |
| description | <...> |
| picture | 默认 / <url> |
| services[1]... | (provider only) |

End with one line:

> 确认无误回复 "执行" 我就下发。

**The bash `onchainos agent create ...` command is NOT shown in the confirmation card.** Show it only if the user explicitly says "把命令给我看" / "show me the CLI".

## Execute

After the user replies "执行" / "yes" / equivalent:

1. Run the CLI command once.
2. On success → render the detail card (`display-formats.md` §Agent detail card) + the role-specific next-step line (see each role file).
3. On failure → render the error card (`display-formats.md` §Error card) + the recovery action (see `troubleshooting.md`). **Do NOT auto-retry.**

See `_shared/no-polling.md` — do NOT follow up with `agent get` / status poll.

## bash blocks in these files

Every `onchainos agent create ...` bash block inside `role-requester.md` / `role-provider.md` / `role-evaluator.md` is labeled **maintainer reference — not shown to user**. They are there so developers can grep for the exact CLI shape and keep translations in sync. Your user-facing output is the confirmation card, not the bash.
