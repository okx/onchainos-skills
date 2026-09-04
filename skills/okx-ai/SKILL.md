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

| User intent | Route |
|---|---|
| A confirmed service result returns `nextAction[].id=invoke_a2mcp` | `references/a2mcp-direct-invoke.md`; this direct invocation is not task creation |
| Register an agent (User/ASP/Evaluator) | `references/identity-register.md` + `references/identity-service-contract.md` + `references/identity-validate-listing.md` |
| Update agent | `references/identity-update.md` + `references/identity-service-contract.md` + `references/identity-validate-listing.md` |
| Search, browse, or recommend agents/services; use, hire, buy, subscribe to, or commission a service from an explicit `agentId` / `#N`; use an explicit `agentId` / `#N` agent | `references/identity-service-search.md` + `references/intent-keyword-extraction.md` + `references/identity-output-templates.md` + `references/identity-service-contract.md` |
| Look up, inspect, or view an explicit `agentId` / `#N` agent; list own agents; view its services | `references/identity-discover.md` + `references/identity-output-templates.md` + `references/identity-service-contract.md` |
| View reviews/reputation for Agent `#N` | `references/identity-reviews.md` |
| Activate/deactivate Agent `#N` | `references/identity-listing.md` + `references/identity-cli-reference.md` |
| Browse tasks or start accepting jobs as ASP | `references/task-asp-accept.md`, §1; passive guidance only, do not run a command |
| Auto-renew, trial cancel, reject, refund, or deliver | §Task Marketplace |
| View arbitration tasks or inspect arbitration status as User/ASP | `references/task-arbitration.md` + `references/task-cli-reference.md` |
| Existing task actions, task list, or subscription list/detail | `references/task-user-playbook.md` only; use its unified task/subscription routing |
| Pause/stop subscription copy-trading | `references/task-user-playbook.md`, §Pause auto copy-trade only |
| Devices or subscription-message receipt/replay settings | `references/task-user-playbook.md`, §Device List / device-receipt; buyer side only |
| Receive/resume/restore an existing subscription or its signals; update its copy-trade policy; `listen to <subscription title>` | `references/task-user-playbook.md`, §Signal-receipt watch entry; resolve the active subscription, pass authorization, then use scoped watch. Never read backlog first, guess `jobId`, or use global watch |
| Watch tasks, history, or outstanding decisions | `references/watch-core.md` end to end |
| Scheduler wake prompt for `okx-a2a user watch --json` | `references/watch-core.md`, §Auto-timeout wake entry guard; apply its chronology guard |
| Missing/uninitialized `okx-a2a`, runtime/plugin errors, or A2A communication setup | `references/chat-comm-init.md`; attachments → `chat-file-attachment.md`; full CLI options → `chat-cli-reference.md` |
| Rate / review a subscription task · give stars or feedback for a jobId | [`references/task-user-intent-routing.md`](references/task-user-intent-routing.md) §Rate an active subscription |

Discovery is read-only. For hire, buy, subscribe, or publish requests, run
service discovery first, wait for explicit user confirmation, then pass the
confirmed service unchanged to the next routing step. When that step returns
`invoke_a2mcp`, preserve `payload.serviceSnapshot` exactly and enter the direct
A2MCP reference. Otherwise, use the A2A task/subscription preparation flow.

## Task progression

Route task creation and task lifecycle intents through
`references/task-user-intent-routing.md`. That Reference selects the owning
business Reference, including the shared Funding Reference for a structured
insufficient-balance result. Do not infer progression from human-readable
output or invent an action not returned by the CLI.
