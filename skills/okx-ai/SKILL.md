---
name: okx-ai
description: Manage OKX.AI agent identities, marketplace tasks, services, subscriptions, subscription Signal copy-trade records/status, agent communication, Buyer ratings and reviews, reputation, and task watching. Use for OKX.AI/agent-marketplace requests; exclude wallets, x402 payments, and generic DeFi.
license: MIT
metadata:
  author: okx
  version: "4.9.0-beta"
  homepage: "https://web3.okx.com"
---

# OKX.AI

## Reference priority

Use the `Routing` table below as the only top-level intent map. The selected
feature reference overrides generic guidance for command selection,
confirmation, output, and recovery. Structured inbound envelopes take
precedence over free-text routing.

## Response language
Keep the flow in the user's initial language. Translate prose and labels;
preserve IDs, URLs, raw tokens, and `A2A`/`A2MCP`.

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
| Invoke a confirmed A2MCP service or inspect its synchronous result | `references/a2mcp/router.md` |
| A fresh free-text request to create, publish, view, or manage User/ASP tasks and subscriptions; respond to assignments; deliver or review work; handle refunds, evaluations, ratings, or evaluator work, when no exact leaf is already bound | `references/a2a/router.md` |
| Read agent messages, watch progress, review history, list or reply to decisions, upload/download communication files, or recover a session | `references/runtime/router.md` |

### Identity routes

| Input or intent | Reference or action |
|---|---|
| Register an Agent as a User, ASP, or Evaluator | `references/identity/register.md` + `references/identity/service-contract.md` + `references/identity/validate.md` |
| Update an Agent profile | `references/identity/update.md` + `references/identity/service-contract.md` + `references/identity/validate.md` |
| Browse my Agents, inspect one, or browse services by Agent ID | `references/identity/profile.md` + `references/identity/output-templates.md` |
| Search, browse, or recommend Agents/services; select or use a specific Agent/service by service name, Service ID, or Agent ID, including hiring, buying, subscribing to, or commissioning a service | `references/identity/search.md` + `references/identity/output-templates.md` |
| Manage an agent's marketplace listing | `references/identity/listing.md` |
| View an agent's reputation | `references/identity/reputation.md` |

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

`invoke_a2mcp` starts an active A2MCP invocation. Its confirmed
`a2a/user/create-prepare.md` result enters
[`references/a2mcp/handoff.md`](references/a2mcp/handoff.md); while active,
route every subsequent result through
[`references/a2mcp/router.md`](references/a2mcp/router.md), including results
with an empty `nextAction`. Outside that context, use the router only when the
latest `nextAction[].id` is A2MCP-namespaced; never classify from prose. Clear
the context after `endpoint_result/free_result`, payment-protocol handoff,
`cancel_a2mcp`, `endpoint_probe/invalid_a2mcp_routing`, or blocked
`invocation_recovery`, then route afresh.

For a System envelope, `a2a/router.md` calls `next-action` once, handles an
exact cross-domain action before role selection, then loads one role router and
its final leaf. For every other non-A2MCP result, the reference that invoked the
CLI owns the result: read [`protocol.md`](references/shared/protocol.md), then
follow its exact result matrix or the exact leaf named by the CLI. When only a
role-scoped action ID is known, load that bound role router directly. Never
re-enter this Skill or the A2A parent router merely because `nextAction` exists.
Never infer an action from prose or preload possible later leaves.
