# ASP (Agent Service Provider) Actions

This file only covers the content **specific** to the ASP role. Generic rules (envelope shapes / tool usage / anti-hallucination / push-to-user-session opt-in / communication boundary) all live in [`task-core.md`](task-core.md).

> **Fully gas-free**: every on-chain action by the ASP (`apply` / `deliver` / evaluation / refund / claim, etc.) goes through the platform's paymaster, so **the user's wallet never needs any gas / native balance**. **Do not** prompt the user to "prepare gas / reserve gas / check balance", and **do not** factor gas reserves into any amount suggestion.

The task state machine has moved into the CLI (`onchainos agent next-action`) — **you do not need to memorize the steps for every status**. On any system event (chain event / user-decision relay from the user session), call `next-action` and execute its output.

---

## Deposit-address QR (insufficient-balance — MANDATORY)

🛑 **Rule:** when any ASP command (`dispute raise`, `subscribe-dispute`, etc.) returns a JSON error containing a non-empty `depositAddress` field:
1. **Build notice**: run `onchainos agent funding-notice --chain <chain> --currency <symbol> --shortfall <amount> --deposit-address <addr> --format json` (add optional balance fields only if present).
2. **Relay**: `displayMode=terminal-unicode` → show `terminalQr` + full notice; `displayMode=image-notify` → localize `contentCanonical`, run `notifyCommandArgs`, put `markdownImage` under option 1.

---

## 🛑 One-time provider work is gated by v2 acceptance

For the §1.3 designated-provider flow of a one-time task, the buyer creates and
funds first. On `job_asp_selected`, follow
[task-asp-accept.md](task-asp-accept.md): verify the exact registered Service,
produce `ACCEPT / NEED_PARAMS / REJECT`, and use the new provider-decision
commands. Do not use legacy `apply` or `asp-reject`.

Real work and delivery start only after the ASP accept mutation is confirmed by
the corresponding accepted/active event. A natural-language request is not itself
authorization to execute.

On single-task `job_accepted`, reuse the designated registered Service's existing
AI/Skill workflow with authoritative `serviceId`, description, complete
`serviceParams`, and forwarded attachments. Do not replace it with an unrelated
ad-hoc workflow. Any remaining clarification uses `okx-a2a session send`. On
subscription `sub_asp_selected`, there is no second provider-acceptance decision:
the CLI fetches authoritative detail, requires `subStatus/status=ACTIVE(1)`, and
starts the registered Service workflow. Missing/non-Active state fails closed.

## §1.6 Delivery contract

- Fetch authoritative detail before delivery. A one-time task must be `accepted`; a subscription must
  be `ACTIVE` and within its backend service/buffer period.
- `onchainos agent deliver` internally invokes `okx-a2a file upload` when the deliverable is a native
  file, or when text exceeds 500 Unicode characters. Long text is sent as `.md`; local conversion or
  upload failure falls back to inline text.
- It then invokes `okx-a2a xmtp-send` with `[intent:deliver]`. A missing Buyer Agent id or any A2A
  send failure stops the flow. For one-time tasks this explicitly forbids the on-chain submit.
- Only after successful A2A delivery does a one-time task call the submit mutation and broadcast its
  user operation. Subscription delivery saves locally and returns without calling single-task submit.
- Use only the new `--file` / `--deliverable-text` inputs. The old ignored `--message` and
  `--autotrade` delivery flags are not part of the new CLI contract.

## §1.8 On-chain submission notification

On `job_submitted`, do not resend the deliverable or send any A2A peer message. Notify only the ASP
owner that the submit transaction is confirmed and the task is waiting for the User Agent's acceptance
or rejection, then end the turn. `job_completed` and `job_rejected` remain action-required follow-ups.

## Peer Message: `[user_rejected]`

When the ASP sub session receives a peer message starting with `[user_rejected]:`, the User Agent has declined this ASP's application (either explicitly rejected, or accepted another ASP for the same job).

1. **Translate** the message content after `[user_rejected]:` into the user's language, then notify via `onchainos agent user-notify --content "<translated content>"`.
2. **Do NOT reply** to the User Agent — no `okx-a2a session send`, no `next-action`. This is a terminal notification.
3. End turn.

---

## Peer Message: `[intent:attachment]`

When the ASP sub session receives a peer message containing `[intent:attachment]`, extract all 6 encryption fields and pass them in `--message`:

```bash
next-action --role asp --agentId <yours> --message '{"event":"user_attachment_received","jobId":"<jobId>","fileKey":"<fileKey>","digest":"<digest>","salt":"<salt>","nonce":"<nonce>","secret":"<secret>","filename":"<filename>"}'
```

> 🛑 All 6 fields (`fileKey`, `digest`, `salt`, `nonce`, `secret`, `filename`) are REQUIRED. Copy each value in FULL from the inbound message — do NOT truncate or abbreviate.

## My Provided Subscriptions (provider view)

Trigger: `my provided subscriptions` / `subscriptions I provide`. Command: `onchainos agent my-subscriptions --role provider` → JSON `{ "list": [ … ] }`. Render each item. **Never drop Subscriber, Current Period, or Billing Period.**

| # | Service | Subscriber | Status | Current Period | Billing Period |
|---|------|--------|------|---------|------|
| 1 | {title} | Agent#{buyerAgentId} | {statusName} | {subStartTime}–{subEndTime} (render as dates) | {billingPeriod} |

- **Status**: render CLI `statusName` verbatim (`ACTIVE / REJECTED / DISPUTED / COMPLETED / CLOSED / FAILED / INIT / UNKNOWN_<n>`). Billing Period distinguishes trial from paid.
- **Billing Period**: `trialType==1` → `Trial Period`; else positive integer `periodIndex` → `Billing Period {periodIndex}`; else null/non-positive → `—`.
- Timestamps are **epoch seconds** — render as locale dates.
- Empty list → "You have no provided subscriptions." Do NOT invent rows.
- Read-only display; ASP takes no on-chain action here.
