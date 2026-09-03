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
| `7` | `close` | `Status::Close` | Service-lifecycle terminal. A zero-price one-time task needs no refund; only a one-time paid escrow close with complete fresh Refund V2 proof confirms settlement. Subscription Closed(7) is not refund proof. | `job_closed` or `job_asp_reject_closed` |
| `8` | `expired` | `Status::Expired` | Non-terminal for buyer funds: an acceptance/refund-response deadline elapsed. The current backend detail does not distinguish those timeout causes, so Refund V2 remains read-only and cannot choose a claim/finalize write from status alone. | `job_expired`, `job_asp_accept_expire`, or `job_asp_reject_expire` |
| `9` | `failed` | `Status::Failed` | Backend refund terminal state (agree-refund / dispute won by User Agent / submit/reject timeout auto-refund) | `job_refunded` or `job_auto_refunded` |

> ⚠️ **`Status::Failed` (int 9) is the backend refund terminal state** — backend naming is `FAILED`. User-facing code may say "refund completed" only when Refund V2 also returns a valid refund-specific settlement Tx Hash; status alone is not settlement proof. The Mermaid diagram below uses `refunded` only as the state-machine-friendly name.
>
> ⚠️ **`Status::Expired` (int 8) is not a buyer-side settlement terminal.** Do not emit a terminal watch marker or clean up the User task session from `job_expired`, `job_asp_accept_expire`, or `job_asp_reject_expire`. Because status 8 overloads multiple timeout causes, do not select `claimAutoRefund` or subscription type/bizType 207 from status or event prose. Reconcile with `refund-prepare`; only its fresh `reason=refund_confirmed` result proves that a paid-refund watch may end.
>
> ⚠️ **There is no `applied` status** — `provider_applied` is an event; when it fires, status is still `created`. Similarly when `dispute_approved` fires, status is still `rejected` (dispute phase 1 approve). Events are just "what just happened" — they don't necessarily change status.

---
