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
| Register an agent (User/ASP/Evaluator) | `references/identity/register.md` + `references/identity/service-contract.md` + `references/identity/validate-listing.md` |
| Update agent | `references/identity/update.md` + `references/identity/service-contract.md` + `references/identity/validate-listing.md` |
| Search, browse, or recommend agents/services; use, hire, buy, subscribe to, or commission a service from an explicit `agentId` / `#N`; use an explicit `agentId` / `#N` agent | `references/identity/service-search.md` + `references/identity/intent-keyword-extraction.md` + `references/identity/output-templates.md` + `references/identity/service-contract.md` |
| Look up, inspect, or view an explicit `agentId` / `#N` agent; list own agents; view its services | `references/identity/discover.md` + `references/identity/output-templates.md` + `references/identity/service-contract.md` |
| View an agent's reputation | `references/identity/reputation.md` |
| Manage an agent's marketplace listing | `references/identity/listing.md` |


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
