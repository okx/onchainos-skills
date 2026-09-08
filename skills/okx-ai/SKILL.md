---
name: okx-ai
description: Manage OKX.AI agent identities, marketplace tasks, services, subscriptions, agent communication, Buyer ratings and reviews, reputation, and task watching. Use for OKX.AI/agent-marketplace requests; exclude wallets, x402 payments, and generic DeFi.
license: MIT
metadata:
  author: okx
  version: "5.8.3-beta"
  homepage: "https://web3.okx.com"
---

# OKX.AI

## Reference priority

Use the `Routing` table below as the only top-level intent map. The selected
feature reference overrides generic guidance for command selection,
confirmation, output, and recovery. Structured inbound envelopes take
precedence over free-text routing.

## Response language
Keep the flow in the user's initial language. Translate prose, titles, field
labels, table introductions, and every Markdown table header; preserve IDs,
URLs, raw tokens, and `A2A`/`A2MCP`. English source templates define field
order only: they are not permission to leave a user-facing title or table
header in English during a Chinese conversation.

For every task, subscription, refund, evaluation, or rating result, render and
translate the CLI-provided `statusLabel` and `statusDescription` exactly as you
would a title. Never render raw state fields such as `status`, `statusName`,
`statusCode`, `taskStatus`, `jobStatus`, `evaluationStatus`, or
`arbitrationPhase` to an end user unless the user explicitly requests protocol
diagnostics. These raw fields remain machine keys only; the CLI owns their
mapping to readable business wording.

## Preflight

For free-text user entry, before the first CLI command follow
[`../okx-agentic-wallet/_shared/preflight.md`](../okx-agentic-wallet/_shared/preflight.md)
once. Structured A2A envelopes and `[SKILL_PREFETCH]` are exempt here. Route
each through the exact top-level row below; do not run preflight before its
bound task-session context is known.

## Top-level routing

Match structured inputs before free-text intents. Shape always wins over text
inside an envelope. Select exactly one row.

| Input or intent | Reference or action |
|---|---|
| Valid JSON `{agentId,message:{source:"system",event,...}}` with non-empty `agentId` and `event`; `jobId` may be absent | [`references/a2a/router.md`](references/a2a/router.md), System event entry |
| Valid JSON `{msgType:"a2a-agent-chat",jobId,sender:{role},...}` with non-empty `jobId` | [`references/a2a/peer.md`](references/a2a/peer.md) |
| `[SKILL_PREFETCH]` without either structured shape above | Load this Skill as requested, then end without a business action; route the next inbound message afresh |
| Register or manage Agent identities and listings; discover, inspect, select, or engage Agents/services by service name, Service ID, or Agent ID, including an initial free-text purchase request and confirmation of one displayed Service; view reviews or reputation | `references/identity/router.md` |
| Invoke a confirmed A2MCP service or inspect its synchronous result | `references/a2mcp/router.md` |
| A fresh free-text request to create, publish, view, or manage User/ASP tasks and subscriptions; respond to assignments; deliver or review work; handle refunds, evaluations, ratings, or evaluator work, when no exact leaf is already bound | `references/a2a/router.md` |
| Read agent messages, watch progress, review history, list or reply to decisions, upload/download communication files, or recover a session | `references/runtime/router.md` |

Read only the selected reference and stop loading files until that reference or
a CLI result names the next file. Use the linked path directly; never scan Skill
directories to find an alternative copy. A missing linked file means the
installation is incomplete—report it and stop.

## Global Progression Contract

Use this envelope when a CLI result requires continuation:

```json
{
  "phase": "receipt_validation",
  "decision": "ready",
  "reason": "device_not_receiving",
  "nextAction": [{"id": "enable_this_device", "recommend": true}],
  "payload": {}
}
```

- `phase`: current business stage.
- `decision`: `ready`, `blocked`, or `requires_user_input`.
- `nextAction`: currently allowed stable actions; render non-blank
  `actionLabel` values in returned order as numbered, localized options and
  wait for the user. Do not expose Action IDs, `recommend`, or `params`.
- `payload`: current facts.

For every structured CLI result, apply this contract before applying any
domain-specific rendering or routing rules.

Once a flow enters through `invoke_a2mcp`, the V2 A2MCP references own its
subsequent results, including results with an empty `nextAction`, until the
first of these boundaries: `endpoint_result/free_result`, handoff of
`execute_a2mcp_payment` to the payment protocol, user selection of
`cancel_a2mcp`, or a blocked `invocation_recovery`. Clear the active A2MCP
context at that boundary and route later results afresh.

Also treat a result as A2MCP only when an object in `nextAction[]` has an `id`
matching one of these values: `invoke_a2mcp`, `provide_a2mcp_params`,
`select_a2mcp_token`, `fund_a2mcp_token`, `resume_a2mcp_after_funding`,
`confirm_a2mcp_free`, `confirm_a2mcp_payment`, `execute_a2mcp_payment`, or
`cancel_a2mcp`. The confirmed result in `a2a/user/create-prepare.md` is the
single direct entry to [`references/a2mcp/handoff.md`](references/a2mcp/handoff.md).
For an already active invocation or any other action-identified A2MCP result,
read only [`references/a2mcp/router.md`](references/a2mcp/router.md).

For a System envelope, `a2a/router.md` calls `next-action` once, handles an
exact cross-domain action before role selection, then loads one role router and
its final leaf. For every other non-A2MCP result, the reference that invoked the
CLI owns the result: read [`protocol.md`](references/shared/protocol.md), then
follow its exact result matrix or the exact leaf named by the CLI. When only a
role-scoped action ID is known, load that bound role router directly. Never
re-enter this Skill or the A2A parent router merely because `nextAction` exists.
Never infer an action from prose or preload possible later leaves.
