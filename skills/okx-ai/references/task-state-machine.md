# Task State Machine (Shared Blueprint)

> **The single source of truth** — aligned with `cli/src/commands/agent_commerce/task/common/state_machine.rs`. All role skill files reference this diagram.
>
> The state machine itself is payment-mode-agnostic — for payment details see [`payment-modes.md`](./payment-modes.md); for entry differences see [`entry-points.md`](./entry-points.md).
>
> **Important layering**: this system strictly distinguishes between **task status** (Status, 11 real enums) and **system events** (Event, 58 total). **Events are not states** — some events are transient (don't change status, e.g. `provider_applied` / `dispute_approved`), some trigger state transitions, and some are entirely decoupled from task status (e.g. staking events).

---

## Task Status (11 real enums)

Backend `status` int field → local `Status` enum mapping (`state_machine.rs::Status::from_int`):

| int | string | enum | Meaning | Entry event |
|---|---|---|---|---|
| `-1` | `init` | `Status::Init` | Internal initialization state | — |
| `0` | `created` | `Status::Created` | Task on-chain, awaiting acceptance | `job_created` |
| `1` | `accepted` | `Status::Accepted` | Designated ASP accepted the buyer-created-and-funded task; execution starts | `job_accepted` |
| `2` | `submitted` | `Status::Submitted` | ASP deliverable on-chain | `job_submitted` |
| `3` | `rejected` | `Status::Rejected` | User Agent rejected deliverable; 24h decision window (dispute / agree-refund) | `job_rejected` |
| `4` | `disputed` | `Status::Disputed` | Dispute in progress (evidence period + commit/reveal) | `job_disputed` |
| `5` | `admin_stopped` | `Status::AdminStopped` | Terminal: admin-stopped by the platform | — |
| `6` | `completed` | `Status::Completed` | Terminal: task completed (normal acceptance / dispute won by ASP / review timeout auto-complete) | `job_completed` or `job_auto_completed` |
| `7` | `close` | `Status::Close` | Service-lifecycle terminal. A zero-price one-time task needs no refund; a fresh backend chain-projected one-time positive-amount paid escrow close (`paymentMode=1`) means escrow was returned even if no Tx Hash is exposed. Subscription Closed(7) is not refund proof. | `job_closed` or `job_asp_reject_closed` |
| `8` | `expired` | `Status::Expired` | Non-terminal for buyer funds: an acceptance/refund-response deadline elapsed. The current backend detail does not distinguish those timeout causes, so Refund V2 remains read-only and cannot choose a claim/finalize write from status alone. | `job_expired`, `job_asp_accept_expire`, or `job_asp_reject_expire` |
| `9` | `failed` | `Status::Failed` | One-time: backend chain-projected successful refund transition. Subscription: bare/event-only status is overloaded; durable local `request-refund` provenance with the core job/Buyer/formal-job-type/exact-positive-amount/token-address binding plus fresh Buyer-owned Failed(9) identifies refund. `sub_failed_notify` merely labels a possible charge failure and is non-terminal without trustworthy event provenance/cause. | `job_refunded`, `job_auto_refunded`, `sub_asp_agree`, `sub_reject_refund_notify`, `dispute_resolved`, or `sub_failed_notify` |

> ⚠️ **`Status::Failed` (int 9) is task-kind dependent.** For a one-time task, the backend writes this chain-projected state only for successful refund transitions, so fresh matching Refund V2 detail may say "refund completed" even when Tx Hash is unavailable. For a subscription, bare or event-only `FAILED` also represents terminal charge failure and remains ambiguous. Confirmation requires durable local Refund V2 `request-refund` provenance bound to the same job, Buyer, formal `jobType=1` subscription, exact positive original amount, and token address plus fresh Buyer-owned Failed(9). Provider/Service, period, token symbol, and `paymentMode` are optional comparisons: a mismatch vetoes only when both sides exist, while absence merely reduces detail/display. Legacy events may describe the ASP-agree, timeout, or dispute branch but cannot create proof. `dispute_resolved` is stricter for both terminal branches: durable local refund-request provenance plus fresh composed job type, Buyer ownership, and exact status 9 (User wins/refund) or 6 (ASP wins/no refund) are required before any verdict, rating, notification, or cleanup. With the current lack of trustworthy event provenance/cause, `sub_failed_notify` remains fail-closed, non-terminal, and read-only even when no refund intent is found; emit no terminal marker and perform no cleanup. No new backend cause query, typed source, or refund-specific Tx Hash field is required for the proven refund paths. The Mermaid diagram below uses `refunded` only as the one-time state-machine-friendly name.
>
> ⚠️ **`Status::Expired` (int 8) is not a buyer-side settlement terminal.** Do not emit a terminal watch marker or clean up the User task session from `job_expired`, `job_asp_accept_expire`, or `job_asp_reject_expire`. Because status 8 overloads multiple timeout causes, do not select `claimAutoRefund` or subscription type/bizType 207 from status or event prose. Reconcile with `refund-prepare`; only its fresh `reason=refund_confirmed` result proves that a paid-refund watch may end.
>
> Backend semantics define `job_closed`, `job_refunded`, and `job_auto_refunded` as transaction-result notifications, not `uopData.executeResult` preflight. They may omit Tx Hash. For subscription finality, re-read fresh task-kind/status/ownership/payment facts and require matching durable local `request-refund` provenance; the event itself supplies branch wording only. Only explicit boolean `executeResult=false` blocks the legacy signer; missing, null, and non-boolean values remain compatible but never prove transaction success.
>
> ⚠️ **There is no `applied` status** — `provider_applied` is an event; when it fires, status is still `created`. Similarly when `dispute_approved` fires, status is still `rejected` (dispute phase 1 approve). Events are just "what just happened" — they don't necessarily change status.

---
