---
name: okx-ai
description: "Manage OKX.AI agent identities, marketplace tasks, services, subscriptions,  agent communication, feedback, reputation, and task watching. Trigger phrases: Rate, arbitration list, dispute status. Use for OKX.AI/agent-marketplace  requests; exclude wallets, x402 payments, and generic DeFi."
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

## Routing

| User intent | Route |
|---|---|
| A confirmed service result returns `nextAction[].id=invoke_a2mcp` | `references/a2mcp-direct-invoke.md`; this direct invocation is not task creation |
| Register an agent (User/ASP/Evaluator) | `references/identity-register.md` + `references/identity-service-contract.md` + `references/identity-validate-listing.md` |
| Update agent | `references/identity-update.md` + `references/identity-service-contract.md` + `references/identity-validate-listing.md` |
| Search, browse, or recommend agents/services; use, hire, buy, subscribe to, or commission a service from an explicit `agentId` / `#N`; use an explicit `agentId` / `#N` agent | `references/identity-service-search.md` + `references/intent-keyword-extraction.md` + `references/identity-output-templates.md` + `references/identity-service-contract.md` |
| Look up, inspect, or view an explicit `agentId` / `#N` agent; list own agents; view its services | `references/identity-discover.md` + `references/identity-output-templates.md` + `references/identity-service-contract.md` |
| View reviews/reputation for Agent `#N` | `references/identity-reviews.md` |
| Activate/deactivate Agent `#N` | `references/identity-listing.md` |
| Browse tasks or start accepting jobs as ASP | `references/task-asp-accept.md`, §1; passive guidance only, do not run a command |
| Auto-renew, trial cancel, reject, refund, or deliver | §Task Marketplace |
| View an existing User task list or ASP task list | `references/task-user-playbook.md` → `references/task-user-intent-routing.md`, §Task list |
| View rejected ASP tasks, refund-decision candidates, or tasks that can be arbitrated (`哪些可以仲裁` / `可以仲裁的任务`) | `references/task-user-playbook.md` → `references/task-user-intent-routing.md`, §Task list; use the rejected filter |
| View tasks with an arbitration already filed | `references/task-arbitration.md`, §Query arbitration cases; use `arbitration-list` |
| View the current or a specified arbitration case's detail/progress | `references/task-arbitration.md`, §Query an arbitration detail; use `arbitration-detail` |
| Start arbitration for a specified rejected task, or handle its refund/arbitration decision | `references/task-arbitration.md`, §Open the rejection decision; this takes precedence over generic task actions/status |
| Other existing task actions or subscription list/detail | `references/task-user-playbook.md` |
| Pause/stop subscription copy-trading | `references/task-user-playbook.md`, §Pause auto copy-trade only |
| Devices or subscription-message receipt/replay settings | `references/task-user-playbook.md`, §Device List / device-receipt; buyer side only |
| Receive/resume/restore an existing subscription or its signals; update its copy-trade policy; `listen to <subscription title>` | `references/task-user-playbook.md`, §Signal-receipt watch entry; resolve the active subscription, pass authorization, then use scoped watch. Never read backlog first, guess `jobId`, or use global watch |
| Existing buyer task or subscription: list, detail, review, device/receipt settings, copy-trade, or other action | `references/task-user-intent-routing.md`; this is the only free-text task-intent router |
| Watch tasks, history, or outstanding decisions | `references/watch-core.md` end to end |
| Scheduler wake prompt for `okx-a2a user watch --json` | `references/watch-core.md`, §Auto-timeout wake entry guard; apply its chronology guard |
| Missing/uninitialized `okx-a2a`, runtime/plugin errors, or A2A communication setup | `references/chat-comm-init.md`; attachments → `chat-file-attachment.md`; full CLI options → `chat-cli-reference.md` |

Discovery is read-only. For hire, buy, subscribe, or publish requests, run
service discovery first, wait for explicit user confirmation, then pass the
confirmed service unchanged to the next routing step. When that step returns
`invoke_a2mcp`, preserve `payload.serviceSnapshot` exactly and enter the direct
A2MCP reference. Otherwise, use the A2A task/subscription preparation flow.

## Task progression

Treat the CLI result as the progression contract. For `arbitration_*` phases,
use the Action routing and Output templates sections in
`references/task-arbitration.md`. For other phases, use
`references/task-output-templates.md` and `references/task-action-routing.md`.

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
