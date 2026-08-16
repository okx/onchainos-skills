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

## 🛑 `deliver` is gated by `job_accepted`

`apply` going on-chain does NOT advance the task status — it stays `created`. The User Agent then has to run `confirm-accept`, which triggers the `job_accepted` system event. **Only after `job_accepted` arrives** may the ASP run `onchainos agent deliver` / `okx-a2a xmtp-send` the deliverable.

Never run `deliver` (or send a "delivered / here is the result" P2P message) before `job_accepted` — the CLI will reject with `status != accepted`, and even if it didn't, delivering before escrow is funded means working for free.

Real work execution (calling external tools / generating output / etc.) ALSO waits for `job_accepted`. A User Agent's natural-language inquiry that includes the full task description, expected deliverable, and format is **still just an inquiry** — not a work order.

> **Deprecated `--autotrade`:** the CLI still accepts this argument so older ASP scripts do not fail,
> but ignores its value completely. It is never parsed, validated, appended to the XMTP message, or used
> to drive User-side execution. Put the complete signal in `--deliverable-text` (or the delivered file).

---

## Peer Message: `[user_rejected]`

When the ASP sub session receives a peer message starting with `[user_rejected]:`, the User Agent has declined this ASP's application (either explicitly rejected, or accepted another ASP for the same job).

1. **Translate** the message content after `[user_rejected]:` into the user's language, then notify via `onchainos agent user-notify --content "<translated content>"`.
2. **Do NOT reply** to the User Agent — no `okx-a2a xmtp-send`, no `next-action`. This is a terminal notification.
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

## Subscription events (`sub_*`)

For the ASP, **most** subscription events are display-only notifications: call `next-action --role asp`
and render the returned message; don't push a decision, don't wait, don't transition state. The **one
exception is `sub_user_reject`** — it requires an ASP refund/dispute decision (see its row below), so do
NOT treat it as display-only or ignore it.

| Event | Action |
|---|---|
| `sub_asp_selected` | Render the CLI's canonical `Content:` per the language rule below. End turn. |
| `sub_complete_notify` / `sub_close_notify` / `sub_failed_notify` | Render the CLI's canonical terminal `Content:` per the language rule below, then follow `session-cleanup`. End turn. |
| `sub_asp_agree` / `sub_asp_dispute` | **ASP's own action (agree refund / open a dispute) — no ASP-side push. Silently ignore. End turn.** Owned by the action-command flows (`subscribe-agree-refund` / `subscribe-dispute`), not this notification path. |
| `sub_user_reject` | **Decision — NOT display-only, do NOT ignore.** The buyer rejected the current period. Call `next-action --role asp`; the CLI returns a `pending-decisions-v2 request-prompt` decision (A = file a dispute for evaluation / B = confirm the refund — ASP-3 copy: `[Action Needed: User Rejection]` with the rejected period, the precise response deadline `{rejectWindowEndsAt}`, and the auto-refund amount). Push that decision to the user per the returned guidance. Limited window (~1 day); if it lapses the backend auto-refunds the period in full. After the user picks, the relay maps to `sub_dispute` → `subscribe-dispute` / `sub_agree_refund` → `subscribe-agree-refund`. |
| `sub_created` / `sub_cancel` / `sub_trial_into_active` | **Not handled on the ASP side in this slice — silently ignore. End turn.** Buyer-only. |
| `sub_renew` | Renewal → the **previous period's income is now claimable**. Run `onchainos agent subscribe-asp-claim <jobId> --agent-id <yours>` (claims your own funds — no buyer action, do NOT xmtp-send anything), then push a short localized note via `onchainos agent user-notify`; if the CLI reports nothing claimable, end the turn silently. |

#### ASP `sub_*` language rule

- **English ASP** → send the CLI `Content:` **verbatim**.
- **Any other language** → translate the CLI English content faithfully, preserving every field and omitted-clause behavior.
