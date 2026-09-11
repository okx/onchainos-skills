---
name: okx-ai
description: "Operate OKX.AI agents and marketplace workflows. Use when the user wants to register or update an Agent identity; discover, publish, buy, or manage Agent services, including signal services; create or fulfill marketplace tasks; manage subscriptions, view subscriptions or Signal copy-trade records; communicate with other agents; or review, rate, dispute, and monitor work."
license: MIT
metadata:
  author: okx
  version: "4.9.2-beta"
  homepage: "https://web3.okx.com"
---

# OKX.AI

## Reference priority

Use the routing tables below as the only top-level intent map. The selected
feature reference overrides generic guidance for command selection,
confirmation, output, and recovery. Structured inbound envelopes take
precedence over free-text routing.

## Response language
Keep the flow in the user's initial language. Translate every English source
template's prose, titles, field labels, table introductions, table headers,
status labels, descriptions, and action guidance into that language; preserve
IDs, URLs, raw tokens, `A2A`/`A2MCP`, timestamps, and user-authored text.
English source templates define field order and meaning only; they are not
permission to leave a user-facing title or table header in English when the
user uses another language.

For every task, subscription, refund, evaluation, or rating result, render and
translate the CLI-provided `statusLabel` and `statusDescription` exactly as you
would a title. Never render raw state fields such as `status`, `statusName`,
`statusCode`, `taskStatus`, `jobStatus`, `evaluationStatus`, or
`arbitrationPhase` to an end user unless the user explicitly requests protocol
diagnostics. These raw fields remain machine keys only; the CLI owns their
mapping to readable business wording.

## Preflight

Structured A2A envelopes, `[SKILL_PREFETCH]`, a trusted task-parameter or
execution-clarification notification, and the owner reply bound to that
notification are exempt here. Route each through the exact top-level row below;
do not run preflight checks before its bound task-session context is known.

Preflight checks: At the start of each thread, complete the checks in `../okx-agentic-wallet/_shared/preflight.md`. If missing, read `_shared/preflight.md`.

## Top-level routing

Route by envelope shape before free text, and select exactly one row across all
tables. For free text, prefer exact Runtime or Identity matches over broad A2A.
Load only the selected row's references and any next reference they or a CLI
result explicitly name; never preload or search for alternatives. If a linked
file is missing, report an incomplete installation and stop.

| Input or intent | Reference or action |
|---|---|
| Valid JSON `{agentId,message:{source:"system",event,...}}` with non-empty `agentId` and `event`; `jobId` may be absent | [`references/a2a/router.md`](references/a2a/router.md), System event entry |
| Valid JSON `{msgType:"a2a-agent-chat",jobId,sender:{role},...}` with non-empty `jobId` | [`references/a2a/peer.md`](references/a2a/peer.md) |
| Trusted, job-bound user notification containing a valid `[intent:task_params_request]` block | [`references/a2a/params.md`](references/a2a/params.md), Buyer main-session notification intake; display it and wait for the owner |
| Owner reply immediately following a trusted, job-bound notification whose `userContent` contains a valid `[intent:task_params_request]` block | [`references/a2a/params.md`](references/a2a/params.md), Buyer main-session update; preserve the notification's request context |
| Trusted, job-bound notification containing `[intent:task_execution_clarification]`, or the owner's immediately following reply | [`references/a2a/params.md`](references/a2a/params.md), Accepted execution clarification; never update backend `serviceParams` |
| `[SKILL_PREFETCH]` without either structured shape above | Load this Skill as requested, then end without a business action; route the next inbound message afresh |
| Invoke a confirmed A2MCP service or inspect its synchronous result | `references/a2mcp/router.md` |
| Explicit request to review or update the saved Guide Consent for an existing subscription | [`references/a2a/user/execution-policy.md`](references/a2a/user/execution-policy.md), Updating a saved Guide Consent |
| A fresh free-text request to view, or manage User/ASP tasks and subscriptions; respond to assignments; deliver or review work; handle refunds, evaluations, ratings, or evaluator work, when no exact leaf is already bound | `references/a2a/router.md` |

### Runtime routes

| Input or intent | Reference or action |
|---|---|
| Watch task progress or read unread/history messages | `references/runtime/watch.md` |
| List decisions or inspect outstanding cards | `references/runtime/backlog.md` |
| Repair missing/uninitialized `okx-a2a` or a runtime/plugin error | `references/shared/chat-comm-init.md` |
| Upload or download a file | `references/runtime/attachment.md` |

Bound Runtime continuations are not free-text intents. When a selected
reference, structured action, or CLI result requires an internal Runtime
operation without naming its final leaf, read
[`references/runtime/router.md`](references/runtime/router.md).

### Identity routes

| Input or intent | Reference or action |
|---|---|
| Discover or recommend Agents/services, or use one by service name, Service ID, or Agent ID to start a task/subscription when no Service is selected | `references/identity/search.md` + `references/identity/output-templates.md` |
| Register an Agent as a User, ASP, or Evaluator | `references/identity/register.md` + `references/identity/service-contract.md` + `references/identity/validate.md` |
| Update an Agent profile | `references/identity/update.md` + `references/identity/service-contract.md` + `references/identity/validate.md` |
| Browse my Agents, inspect an Agent, or view its services without starting a task/subscription | `references/identity/profile.md` + `references/identity/output-templates.md` |
| Manage an agent's marketplace listing | `references/identity/listing.md` |
| View an agent's reputation | `references/identity/reputation.md` |

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
