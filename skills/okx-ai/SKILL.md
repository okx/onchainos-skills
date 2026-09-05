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
| Register or manage Agent identities and listings; discover, inspect, select, or engage Agents/services by service name, Service ID, or Agent ID; view reviews or reputation | `references/identity/router.md` |
| Browse tasks or start accepting jobs as ASP | `references/task-asp-accept.md`, §1; passive guidance only, do not run a command |
| Request a refund, reject a paid deliverable, check refund progress, or handle a refund-related arbitration result | `references/task-user-refund.md` + `references/task-cli-reference.md`; Refund V2 is distinct from cancellation |
| Auto-renew, trial cancel, or deliver | §Task Marketplace |
| View an existing User task list or ASP task list | `references/task-user-playbook.md` → `references/task-user-intent-routing.md`, §Task list |
| Query rejected tasks that can be arbitrated (`哪些可以仲裁` / `可以仲裁的任务`) or pending-arbitration tasks (`可仲裁` / `待仲裁`) | `references/task-arbitration.md`, §Query intent mapping; run `onchainos agent tasks --status rejected --agent-id <aspAgentId> --page 1 --limit 20`; results are tasks rejected by the User and eligible for an arbitration decision |
| Query filed arbitration cases (`仲裁列表` / `已发起仲裁` / `仲裁案件`) | `references/task-arbitration.md`, §Query arbitration cases; run `arbitration-list` |
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
  "phase": "funding_required",
  "decision": "blocked",
  "reason": "insufficient_balance",
  "nextAction": [],
  "payload": {
    "operation": "task_creation",
    "fundingTarget": {},
    "qr": {},
    "fundingNeed": {}
  }
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

A structured `phase=funding_required`, `decision=blocked`,
`reason=insufficient_balance` result has no action-routing entry. Open
[`funding.md`](../okx-agentic-wallet/references/funding.md) immediately; that
Reference owns balance verification and the handoff back to a fresh task
preview.

Render `nextAction` as a numbered list when user choice is required. Execute a
single safe action directly when its routing reference requires no confirmation.
Never invent actions not returned by the CLI.

Do not infer progression from human-readable output. Keep backend field names
inside `payload` unchanged.
