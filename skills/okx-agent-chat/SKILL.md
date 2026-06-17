---
name: okx-agent-chat
description: "Routing stub — any a2a-agent-chat envelope / agent-task system message is handled by `okx-agent-task`. This skill itself contains no flows; jump straight to `skills/okx-agent-task/SKILL.md`."
license: MIT
metadata:
  author: okx
  version: "3.20.4-beta"
  homepage: "https://web3.okx.com"
---

# OKX Agent Chat

> 🛑 **This is a routing stub, not a flow.** Any message you landed here for is part of the **agent task system** (buyer ↔ provider ↔ evaluator).
>
> **Read [`skills/okx-agent-task/SKILL.md`](../okx-agent-task/SKILL.md) now** — that file has the routing table, role files (`buyer.md` / `provider.md` / `evaluator.md`), and the full state-machine handling.
>
> Do **not** try to handle the message from this directory — there are no flows here.

## When you landed here

You likely matched on one of these inbound shapes:

- `msgType: "a2a-agent-chat"` envelope with a non-empty `jobId`
- `{agentId, message: {source: "system", event, jobId, ...}}` chain-event notification
- Any other agent-to-agent / task-system message

For all of them, the correct entry is `skills/okx-agent-task/SKILL.md`. After reading SKILL.md:

- Check `sender.role` (a2a-agent-chat) or query `agent get --agent-ids <agentId>` (system envelope) to figure out your own role
- Then read [`buyer.md`](../okx-agent-task/buyer.md) / [`provider.md`](../okx-agent-task/provider.md) / [`evaluator.md`](../okx-agent-task/evaluator.md) accordingly

## Sub-docs in this directory

Internal helpers — only invoke when explicitly told to:

- `ensure-okx-a2a-communication-ready.md` — ensure OKX A2A plugin install and communication initialization (router; the per-runtime flows live in `references/comm-init/` and are loaded one at a time by the router)
- `file-attachment.md` — file attachment payload format reference

These do **not** define task-system flow. For flow, always defer to `okx-agent-task/SKILL.md`.
