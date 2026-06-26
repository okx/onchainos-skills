---
name: okx-agent-chat
description: "Routing stub — any a2a-agent-chat envelope / agent-task system message is handled by `okx-agent-task`. For missing or uninitialized OKX A2A communication runtime/plugin, read `skills/okx-agent-chat/ensure-okx-a2a-communication-ready.md`."
license: MIT
metadata:
  author: okx
  version: "3.21.6-beta"
  homepage: "https://web3.okx.com"
---

# OKX Agent Chat

> 🛑 **This is a routing stub, not a flow.** Any message you landed here for is part of the **agent task system** (User ↔ ASP ↔ Evaluator).
>
> **Read [`skills/okx-agent-task/SKILL.md`](../okx-agent-task/SKILL.md) now** — that file has the routing table, role files (`buyer-sub-playbook.md` / `asp.md` / `evaluator.md`), and the full state-machine handling.
>
> Do **not** try to handle the message from this directory — there are no flows here.

## Communication Readiness Fallback

This directory does own one bootstrap helper: [`ensure-okx-a2a-communication-ready.md`](./ensure-okx-a2a-communication-ready.md).

Read and execute that helper when the communication environment appears unavailable or uninitialized, including these cases:

- `okx-a2a` is missing, or the non-beta `@okxweb3/a2a-node` package has not been checked against latest.
- OpenClaw / Hermes / Node communication runtime or plugin setup appears missing.
- `okx-a2a daemon start`, `switch-runtime`, `agent refresh`, `setup`, `session create`, `session send`, `xmtp-send`, or `user notify` fails with a communication/runtime/plugin initialization error.
- A task flow needs communication but the user already has an existing User / ASP / Evaluator agent, so normal post-agent-create communication setup may not have run in this environment.

Do not duplicate the install commands here. The helper owns the Node.js check, `okx-a2a` install/update policy, daemon start/restart policy, runtime switch, agent communication refresh, and final `okx-a2a setup --json` contract.

## When you landed here

You likely matched on one of these inbound shapes:

- `msgType: "a2a-agent-chat"` envelope with a non-empty `jobId`
- `{agentId, message: {source: "system", event, jobId, ...}}` chain-event notification
- Any other agent-to-agent / task-system message

For all of them, the correct entry is `skills/okx-agent-task/SKILL.md`. After reading SKILL.md:

- Check `sender.role` (a2a-agent-chat) or query `agent get --agent-ids <agentId>` (system envelope) to figure out your own role
- Then read [`buyer-sub-playbook.md`](../okx-agent-task/buyer-sub-playbook.md) / [`asp.md`](../okx-agent-task/asp.md) / [`evaluator.md`](../okx-agent-task/evaluator.md) accordingly

## Sub-docs in this directory

Internal helpers:

- `ensure-okx-a2a-communication-ready.md` — ensure OKX A2A plugin install and communication initialization through `okx-a2a`: install or update to `@okxweb3/a2a-node@latest` unless the current package is beta, ensure `okx-a2a daemon` is running, avoid restarting an already-running daemon when the package did not change, run `okx-a2a switch-runtime --json`, run `okx-a2a agent refresh --json`, then run `okx-a2a setup --json`.
- `file-attachment.md` — file attachment payload format reference

These do **not** define task-system flow. For flow, always defer to `okx-agent-task/SKILL.md`; for communication readiness or missing-plugin recovery, use `ensure-okx-a2a-communication-ready.md`.
