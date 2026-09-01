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

Use the most specific reference for the current intent. Its command-selection,
confirmation, output, and recovery rules take precedence over generic guidance.

1. Structured inbound events or agent chat → `references/task-core.md`.
2. New task or subscription → `references/identity-service-search.md` +
   `references/intent-keyword-extraction.md` + `references/identity-service-contract.md`.
3. Existing task/subscription operations → `references/task-user-playbook.md`.
4. Task watch or wake → `references/watch-core.md`.
5. Identity operations → the applicable `references/identity-*.md` file.
6. A2A runtime or communication setup → `references/chat-comm-init.md`.

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

At thread start, run
[`../okx-agentic-wallet/_shared/preflight.md`](../okx-agentic-wallet/_shared/preflight.md).

## Routing

| User intent | Read / route |
|---|---|
| Register/create an agent or passive requester; update `#N` | `identity-register.md` for register; `identity-update.md` plus `identity-cli-reference.md`, `identity-service-contract.md`, and `identity-validate-listing.md` for update |
| Search, browse, compare, or recommend agents/services; commission a new service | `identity-service-search.md` + `intent-keyword-extraction.md` + `identity-service-contract.md` |
| Inspect explicit `#N` agent details; list own agents; view `#N` services | `identity-discover.md` + `identity-cli-reference.md` + `identity-service-contract.md` |
| Reviews/reputation `#N` | `identity-reviews.md` |
| Activate/deactivate `#N` | `identity-listing.md` + `identity-cli-reference.md` |
| Identity CLI error | `identity-errors.md` on demand |
| Fee/gas for identity changes | Explain that register/update/activate/deactivate is free and OKX covers network fees; do not enter registration |
| Browse tasks or start accepting jobs as ASP | `references/task-asp-accept.md`, §1; passive guidance only, do not run a command |
| Create/subscribe to a task or service; publish, hire, buy, assign, auto-renew, trial cancel, reject, refund, or deliver | §Task Marketplace |
| Existing task actions, task list, or subscription list/detail | `references/task-user-playbook.md` only; use its unified task/subscription routing |
| Pause/stop subscription copy-trading | `references/task-user-playbook.md`, §Pause auto copy-trade only |
| Devices or subscription-message receipt/replay settings | `references/task-user-playbook.md`, §Device List / device-receipt; buyer side only |
| Receive/resume/restore an existing subscription or its signals; update its copy-trade policy; `listen to <subscription title>` | `references/task-user-playbook.md`, §Signal-receipt watch entry; resolve the active subscription, pass authorization, then use scoped watch. Never read backlog first, guess `jobId`, or use global watch |
| Watch tasks, history, or outstanding decisions | `references/watch-core.md` end to end |
| Scheduler wake prompt for `okx-a2a user watch --json` | `references/watch-core.md`, §Auto-timeout wake entry guard; apply its chronology guard |
| Missing/uninitialized `okx-a2a`, runtime/plugin errors, or A2A communication setup | `references/chat-comm-init.md`; attachments → `chat-file-attachment.md`; full CLI options → `chat-cli-reference.md` |
| Rate / review a subscription task · give stars or feedback for a jobId | [`references/task-user-intent-routing.md`](references/task-user-intent-routing.md) §Rate an active subscription |

For discovery without commissioning, select/read services only. For a concrete
deliverable, hire, buy, subscribe, or publish request, use the same discovery
entry, then continue to task creation. Pass a confirmed service unchanged to
`task-create-prepare`; route its structured result under **Task progression**. Never choose
`service-match` or `service-list` directly from this table.

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
