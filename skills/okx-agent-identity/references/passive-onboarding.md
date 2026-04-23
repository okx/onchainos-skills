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
4. **Ask only two fields**: `name`, `description`, one per turn (see `field-specs.md`).
5. **Execute** `create --role requester` with the minimum flags.
6. **Hand back** — do NOT offer a follow-up like "要不要发任务？"; `okx-agent-task` already has that intent queued.

## Messages to the user

When entering passive mode, announce it in one line so the user isn't confused about which skill is speaking:

> "发布任务需要先有一个买家身份。我帮你快速建一个（两个问题就好），完成后直接回到发任务的流程。"

After success:

> "已为你创建买家身份 #<N>（<name>）。现在继续发布任务。"

No other chatter. No "要不要再 activate 一下", no "要不要查余额" — any extra question breaks the handoff.

## Edge cases

| Situation | Action |
|---|---|
| User asks to cancel mid-flow ("算了不注册了") | Confirm cancellation, tell the task skill the identity is not available: "已取消创建，发布任务需要买家身份，等你想好再来。" |
| User volunteers a service mid-flow ("顺便加个 MCP 服务") | Explain: requester 不带 service；如果想对外收费请后续再注册 provider。不要在被动子流程里混入 service。 |
| Pre-existing requester is found (e.g., user was mistaken about not having one) | Skip create. Echo: "你已经有买家身份 #<N>（<name>），直接用它继续发布任务。" Hand back. |
| Backend rejects create | Render the error card (`display-formats.md` §6). Do NOT auto-retry. The task skill will see the failure and decide. |

## Why passive mode matters

The user's mental model was "发任务" — they didn't ask to go through identity registration. Every extra question in this sub-flow is friction that may cause them to abandon. The goal is: **minimum viable identity, then straight back to the intent they started with**.

Normal onboarding (user says "我要注册一个买家") still uses the full flow in `role-requester.md` — 3 turns including avatar option. Passive onboarding is the stripped-down 2-turn variant.
