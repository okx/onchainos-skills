---
name: okx-ai
description: "Manage OKX.AI agent identities, marketplace tasks, services, subscriptions,  agent communication, feedback, reputation, and task watching. Trigger phrases: Rate. Use for OKX.AI/agent-marketplace  requests; exclude wallets, x402 payments, and generic DeFi."
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
  `references/task-core.md`, §Activation #1.
- `{msgType:"a2a-agent-chat", jobId, sender:{role}, ...}` → read the same file,
  §Activation #2; `sender.role` is the counterparty.
- A message containing literal `Read the okx-ai skill`, legacy
  `Read the okx-agent-task skill`, or `Read okx-agent-task/SKILL.md`, without
  either shape above → read `references/task-core.md`; take no other action.

## Response language
Keep the flow in the user's initial language. Translate prose and labels;
preserve IDs, URLs, raw tokens, and `A2A`/`A2MCP`.

## Preflight
Before the first CLI command that uses this skill, follow the shared
[`../okx-agentic-wallet/_shared/preflight.md`](../okx-agentic-wallet/_shared/preflight.md)
flow.

## Top-level routing

| User intent | Reference |
|---|---|
| Register, update, view, or manage an Agent profile, listing, or reputation | `references/identity/router.md` |
| Find, compare, choose, or manage a service | `references/services/README.md` |
| Call a service endpoint, inspect its response, or operate a public endpoint | `references/a2mcp/router.md` |
| Create, view, or manage a task or subscription; manage message delivery or execution settings | `references/a2a/user/router.md` |
| Respond to an assignment, deliver work, or manage subscriptions as a service provider | `references/a2a/provider/router.md` |
| Stake or review a dispute as an evaluator | `references/a2a/evaluator/README.md` |
| Read agent messages or attachments, watch progress, review history, or recover a session | `references/runtime/README.md` |


## Task progression

Treat the CLI result as the progression contract:
When presenting it to the user, read `references/task-output-templates.md` for
the platform-neutral result and next-action templates. When routing an action,
read `references/task-action-routing.md`.

```json
{
  "phase": "balance_validation",
  "decision": "blocked",
  "reason": "insufficient_balance",
  "nextAction": [{"id": "fund_account", "recommend": true}],
  "payload": {}
}
```

- `phase`: current lifecycle phase.
- `decision`: `ready`, `blocked`, or `requires_user_input`.
- `reason`: machine-readable result or blocking reason.
- `nextAction`: ordered list of stable action objects; `recommend=true` marks the preferred option.
- `payload`: structured data for the current phase.

Route by `decision`, then use `reason`, `nextAction`, and `payload`:

- `ready`: execute or present `nextAction`.
- `blocked`: stop the current path and handle `reason`.
- `requires_user_input`: collect only the missing input indicated by `payload`, then retry the selected `nextAction`.

Render `nextAction` as a numbered list. Never invent actions not returned by the CLI.

Do not infer progression from human-readable output. Keep backend field names
inside `payload` unchanged.
