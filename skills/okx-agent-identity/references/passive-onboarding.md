# Passive Onboarding — entry from `okx-agent-task`

> When another skill (currently `okx-agent-task`) needs the user to have an identity before it can proceed, it hands control over to `okx-agent-identity` with an `intent=...` context. This file describes how to behave in that mode so the user experiences a single smooth flow instead of a restart.

## Entry signals

| Incoming context | Meaning | Jump to |
|---|---|---|
| `intent=need-requester` | User tried to publish a task but has no requester agent. | `role-requester.md` §Passive Onboarding (simplified sub-flow) |
| (future) `intent=need-evaluator` | User tried to accept a dispute but has no evaluator agent. | TBD — currently route through normal `role-evaluator.md` including the staking card. |

> The `okx-agent-task` skill is maintained separately. This file documents the contract from `okx-agent-identity`'s side so both sides can stay aligned.

## Contract (identity side)

When you detect `intent=need-requester` (either from an explicit context field, or from the user's own phrasing "发任务系统说我没买家身份"), you MUST:

1. **Skip role selection.** Role is fixed as `requester`.
2. **Skip the "check existing agents" pre-step.** The handoff implied none exist — if one does show up later (e.g., user returns mid-flow), short-circuit without creating.
3. **Skip the picture prompt.** Use backend default.
4. **Skip the phase preview.** Passive mode is deliberately lean — the three-step flow (name → description → confirm) is short enough that a preamble would add more noise than signal. Go straight to Q1. (This diverges from the normal requester flow in `role-requester.md §Phase preview`; that's intentional.)
5. **Ask only two fields**: `name`, `description`, one per turn — rendered in natural language with **no `Q1：` / `Q1:` prefix** in the user-visible text (see `SKILL.md §UX Output Red Lines Red line 3` and the natural-language prompt wording in `references/role-requester.md §Standard Q&A chain`). Field-spec inlining still per `field-specs.md`.
6. **Render the confirmation card** and wait for the user's `执行` / `execute` token. Passive mode does **NOT** bypass the confirmation gate — `agent create` is a content-creating on-chain write and falls under `SKILL.md §⛔ MANDATORY confirmation gate (non-overridable)`. The card is the standard requester confirmation card (4 rows: 角色 / 名字 / 描述 / 头像) per `references/role-requester.md §Confirmation`. The user's mental model being "发任务" does NOT bypass the gate; "we already collected the fields" does NOT bypass the gate. Render the card. Always.
7. **Execute** `create --role requester` with the minimum flags, only after the in-turn confirm token.
8. **Hand back** — do NOT offer a follow-up like "要不要发任务？"; `okx-agent-task` already has that intent queued.

## Messages to the user

When entering passive mode, announce it in one line so the user isn't confused about which skill is speaking:

> "发布任务需要先有一个买家身份。我帮你快速建一个（两个问题加一次确认就好），完成后直接回到发任务的流程。"

After success — **only one line** in the user's language, following the `#<id>` placeholder rule in `display-formats.md`. Include `#<id>` only when the post-create response actually surfaced an id; if the id is not yet available (e.g. the create CLI returned `{txHash}` only with no `agent` block), omit `#<id>` and use the no-id variant below. **Do NOT render the agent detail card** in passive mode — the user just confirmed all fields on the confirmation card a turn ago, and the goal here is a lean handoff back to `okx-agent-task`. Detail card is reserved for the normal (non-passive) requester flow per `role-requester.md §Post-success`.

With id (Chinese): "已为你创建买家身份 #<id>。现在继续发布任务。"
Without id (Chinese): "已为你创建买家身份。现在继续发布任务。"
With id (English): "Requester identity #<id> created. Resuming the task-publish flow."
Without id (English): "Requester identity created. Resuming the task-publish flow."

(The `<name>` is intentionally omitted from the success line — the user just confirmed `name` on the previous turn's confirmation card, so echoing it again is redundant in the lean handoff. This matches `SKILL.md §Passive Onboarding` and `role-requester.md §Passive Onboarding → After success`.)

No other chatter. No "要不要再 activate 一下" / "want to activate it too?", no "要不要查余额" / "want to check your balance?" — any extra question breaks the handoff.

## Edge cases

| Situation | Action |
|---|---|
| User asks to cancel mid-flow ("算了不注册了") | Confirm cancellation, tell the task skill the identity is not available: "已取消创建，发布任务需要买家身份，等你想好再来。" |
| User volunteers a service mid-flow ("顺便加个 MCP 服务") | Explain: requester 不带 service；如果想对外收费请后续再注册 provider。不要在被动子流程里混入 service。 |
| Pre-existing requester is found (e.g., user was mistaken about not having one) | Skip create. Echo in user's language: Chinese "你已经有买家身份 #<N>（<name>），直接用它继续发布任务。" / English "You already have requester identity #<N> (<name>) — using it to continue publishing the task." Hand back. |
| Backend rejects create | Render the error card (`display-formats.md` §Error card). Do NOT auto-retry. The task skill will see the failure and decide. |

## Why passive mode matters

The user's mental model was "发任务" — they didn't ask to go through identity registration. Every extra question in this sub-flow is friction that may cause them to abandon. The goal is: **minimum viable identity, then straight back to the intent they started with**.

Normal onboarding (user says "我要注册一个买家") still uses the full flow in `role-requester.md` — 4 turns (name + description + avatar choice + confirm). Passive onboarding is the lean variant (3 turns: name + description + confirm; avatar uses backend default, no choice prompt). The confirmation step is the same in both — passive does NOT skip it, per `SKILL.md §⛔ MANDATORY confirmation gate`.
