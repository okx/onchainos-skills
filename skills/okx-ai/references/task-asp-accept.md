# ASP — Job Acceptance / Negotiation Flow

> This file covers how an ASP comes to work on a job. Since the public task type
> was removed, an ASP no longer discovers or proactively contacts users about a
> job — tasks are **designated** by the User Agent and arrive via system events.
> Generic ASP role rules + system event handling live in [`task-asp.md`](task-asp.md).

---

## 1. There is no proactive-accept path

An ASP cannot take, search for, or cold-start a job on its own. The only way an ASP
becomes attached to a job is for a User Agent to **designate** it on-chain; that
designation reaches the ASP as a `JobAspSelected` system event, which drives the
apply/negotiation flow automatically (see [`task-asp.md`](task-asp.md)).

When the user asks the ASP to "take / accept / 接 {jobId}", respond with
passive-readiness guidance and STOP:

| User intent | Agent action |
|---|---|
| "接 {jobId} / 承接任务 X / take task 0xABC / contact the User Agent of {jobId}" | Explain the ASP is passive: "Agent X is online; a task is worked only after its User Agent designates X on-chain — that arrives as a system event. There is no way to take a job proactively." Then STOP. |
| "已激活 / activated / 上线 / 在线" | **Passive readiness only** — say "agent X is online; private tasks targeted at X will arrive via system events" and STOP. |

> 🛑🛑🛑 **ABSOLUTE PROHIBITION — DO NOT call `onchainos agent apply`**: `apply` is
> **system-event-triggered only** — it runs from the `JobAspSelected` playbook (Rust
> code) when the User Agent has designated this ASP on-chain. **Manually invoking
> `onchainos agent apply` is always wrong.** Bypassing the designation = state machine
> corruption + potential escrow loss. 🔴 Real incident: agent received "接 0xABC 任务"
> and called `agent apply 0xABC ...` directly → User Agent had never designated this
> ASP → apply rejected / task stuck.

> 🛑 **Same-wallet multi-agent (self-trading) must still follow the full protocol** —
> even when User Agent and ASP are the same wallet, the User Agent must designate the
> ASP and the apply must run from the system-event-triggered path. Do NOT short-circuit.
> Do NOT batch-loop across multiple jobIds.

---

## 2. Pre-flight Agent disambiguation

Only relevant for read-only status queries the user may ask about a job the ASP is
already designated on. When the user did NOT include an explicit `agentId`:

- Wallet has 0 ASPs → **STOP**. Tell the user "You don't have an ASP identity yet — you need to register one before you can be designated for jobs." then route to `okx-ai` with the intent "Register an ASP identity".
- Wallet has only 1 ASP → use it directly.
- Multiple ASPs → list the candidates and ask the user "which one?" — they must pick **exactly one**.

---
