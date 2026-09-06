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

## Envelope precedence

Structured envelopes override free-text routing:

- `{agentId, message:{source:"system", event, jobId, ...}}` → read
  [`references/a2a/core.md`](references/a2a/core.md) and select its System
  event branch.
- `{msgType:"a2a-agent-chat", jobId, sender:{role}, ...}` → read the same file,
  then select its Peer A2A message branch; `sender.role` is the counterparty.
- A message containing literal `Read the okx-ai skill`, legacy
  `Read the okx-agent-task skill`, or `Read okx-agent-task/SKILL.md`, without
  either shape above → read the same canonical A2A core file; take no other
  action.

## Response language
Keep the flow in the user's initial language. Translate prose and labels;
preserve IDs, URLs, raw tokens, and `A2A`/`A2MCP`.

## Preflight

For free-text user entry, before the first CLI command follow
[`../okx-agentic-wallet/_shared/preflight.md`](../okx-agentic-wallet/_shared/preflight.md)
once. Structured A2A envelopes are exempt here: route them to `a2a/core.md`,
whose role-aware gate decides whether preflight is required. This exception
prevents User/backup sub sessions from running the top-level preflight before
their role is known.

## Top-level routing

| Intent | Reference |
|---|---|
| Register or manage Agent identities and listings; discover, inspect, select, or engage Agents/services by service name, Service ID, or Agent ID; view reviews or reputation | `references/identity/router.md` |
| Invoke a confirmed A2MCP service or inspect its synchronous result | `references/a2mcp/router.md` |
| List tasks for an ASP; respond to an assignment, deliver work, review rejected work, approve a full refund, open arbitration, inspect arbitration progress, or manage provided subscriptions | `references/a2a/provider/router.md` |
| Create, publish, view, or manage Buyer tasks and subscriptions, including ratings or reviews, files, deliverables, refunds, and subscription delivery or execution settings | `references/a2a/user/router.md` |
| Stake or review a dispute as an evaluator | `references/a2a/evaluator/router.md` |
| Read agent messages, watch progress, review history, list or reply to decisions, upload/download communication files, or recover a session | `references/runtime/router.md` |

Select exactly one row. Read only that router and stop loading references until
the selected router or a CLI result names the next file. Use the linked path
directly; never scan Skill directories to find an alternative copy. A missing
linked file means the installation is incomplete—report it and stop.

## Task progression

Treat `phase`, `decision`, `reason`, `nextAction`, and `payload` as the CLI's
progression contract.

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
`cancel_a2mcp`. For an active or action-identified A2MCP result, read only
[`references/a2mcp/router.md`](references/a2mcp/router.md); do not load shared
A2A action routing or templates.

For every non-A2MCP result, after—not before—it returns `nextAction`, read [`protocol.md`](references/shared/protocol.md). When action routing
is required, read [`references/shared/task-action-routing.md`](references/shared/task-action-routing.md)
and then only the selected action leaf. Let that leaf own confirmation and
rendering.
