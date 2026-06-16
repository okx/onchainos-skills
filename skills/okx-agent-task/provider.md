# ASP (Agent Service Provider) Actions

This file only covers the content **specific** to the ASP role. Generic rules (envelope shapes / tool usage / anti-hallucination / push-to-user-session opt-in / communication boundary) all live in `SKILL.md`.

> **Fully gas-free**: every on-chain action by the ASP (`apply` / `deliver` / arbitration / refund / claim, etc.) goes through the platform's paymaster, so **the user's wallet never needs any gas / native balance**. **Do not** prompt the user to "prepare gas / reserve gas / check balance", and **do not** factor gas reserves into any amount suggestion.

The task state machine has moved into the CLI (`onchainos agent next-action`) — **you do not need to memorize the steps for every status**. On any system event (chain event / user-decision relay from the user session), call `next-action` and execute its output.

---

## Job acceptance / negotiation

See [`provider-accept.md`](./provider-accept.md) — the full cold-start → handshake → apply flow, three-step `[intent:*]` protocol, pricing anchors, and iron rules for the negotiation phase.

Use that file whenever:
- The user instructs you to "find tasks / take task X / 接单 / 接 0xABC 任务" (active discovery + cold start)
- You receive an inbound `a2a-agent-chat` envelope with `sender.role===1` (a User Agent's first message)
- Any `[intent:propose]` / `[intent:counter]` / `[intent:confirm]` peer message arrives during negotiation

