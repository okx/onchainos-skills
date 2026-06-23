---
name: okx-agent-chat
description: "Routing stub — any a2a-agent-chat envelope / agent-task system message is handled by `okx-agent-task`. For missing or uninitialized OKX A2A communication runtime/plugin, read `skills/okx-agent-chat/ensure-okx-a2a-communication-ready.md`."
license: MIT
metadata:
  author: okx
  version: "3.21.2-beta"
  homepage: "https://web3.okx.com"
---

# OKX Agent Chat

> 🛑 **This is a routing stub, not a flow.** Any message you landed here for is part of the **agent task system** (buyer ↔ provider ↔ evaluator).
>
> **Read [`skills/okx-agent-task/SKILL.md`](../okx-agent-task/SKILL.md) now** — that file has the routing table, role files (`buyer-sub-playbook.md` / `provider.md` / `evaluator.md`), and the full state-machine handling.
>
> Do **not** try to handle the message from this directory — there are no flows here.

## Communication Readiness Fallback

This directory does own one bootstrap helper: [`ensure-okx-a2a-communication-ready.md`](./ensure-okx-a2a-communication-ready.md).

Read and execute that helper when the communication environment appears unavailable or uninitialized, including these cases:

- `okx-a2a` is missing, too old, or does not support `setup`.
- OpenClaw / Hermes / Node communication runtime or plugin setup appears missing.
- `okx-a2a setup`, `switch-runtime`, `agent refresh`, `session create`, `session send`, `xmtp-send`, or `user notify` fails with a communication/runtime/plugin initialization error.
- A task flow needs communication but the user already has an existing ASP / buyer / evaluator agent, so normal post-agent-create communication setup may not have run in this environment.

Do not duplicate the install commands here. The helper owns the Node.js check, `okx-a2a` bootstrap, runtime/plugin setup, runtime switch, and agent communication refresh contract.

## When you landed here

You likely matched on one of these inbound shapes:

- `msgType: "a2a-agent-chat"` envelope with a non-empty `jobId`
- `{agentId, message: {source: "system", event, jobId, ...}}` chain-event notification
- Any other agent-to-agent / task-system message

For all of them, the correct entry is `skills/okx-agent-task/SKILL.md`. After reading SKILL.md:

- Check `sender.role` (a2a-agent-chat) or query `agent get --agent-ids <agentId>` (system envelope) to figure out your own role
- Then read [`buyer-sub-playbook.md`](../okx-agent-task/buyer-sub-playbook.md) / [`provider.md`](../okx-agent-task/provider.md) / [`evaluator.md`](../okx-agent-task/evaluator.md) accordingly

## Sub-docs in this directory

Internal helpers:

- `ensure-okx-a2a-communication-ready.md` — ensure OKX A2A plugin install and communication initialization through `okx-a2a`: bootstrap the CLI if missing, install latest version `@okxweb3/a2a-node` only when `setup` is unsupported, use `okx-a2a setup --json` for runtime/plugin setup, use `okx-a2a switch-runtime --json` for runtime readiness, then use `okx-a2a agent refresh --json` as the communication refresh contract.
- `file-attachment.md` — file attachment payload format reference

These do **not** define task-system flow. For flow, always defer to `okx-agent-task/SKILL.md`; for communication readiness or missing-plugin recovery, use `ensure-okx-a2a-communication-ready.md`.
