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
| `8` | `expired` | `Status::Expired` | Terminal. Fresh authoritative Expired(8) proves that the backend automatic refund has arrived for a paid non-trial task after ASP acceptance or delivery timeout. No later status, Tx Hash, local provenance, or Buyer claim/finalize write is required. Trial and zero-amount tasks are also terminal but use no-refund outcomes because no refundable payment existed. | `job_expired` or `job_asp_accept_expire` |
| `9` | `failed` | `Status::Failed` | Refund or failure terminal. ASP refund-decision timeout enters this state, but its event is not proof: refund confirmation requires durable `request-refund` provenance plus fresh matching owner/type/payment facts. Other subscription Failed(9) remains overloaded; `sub_failed_notify` is non-terminal without trustworthy cause provenance. | `job_refunded`, `job_auto_refunded`, `job_asp_reject_expire`, `sub_asp_agree`, `sub_reject_refund_notify`, `dispute_resolved`, or `sub_failed_notify` |

> ⚠️ **`Status::Failed` (int 9) is task-kind and provenance dependent.** For a one-time task, fresh Failed(9) remains a chain-projected successful-refund state. For `job_asp_reject_expire` on either task kind, require durable local Refund V2 `request-refund` provenance bound to the same job, Buyer, task type, exact positive original amount, and token address plus fresh matching Failed(9); the caller-supplied event cannot replace that proof. For other subscription refund paths, durable `request-refund` provenance remains required because bare/event-only Failed(9) also represents terminal charge failure. Provider/Service, period, token symbol, and `paymentMode` are optional comparisons: a mismatch vetoes only when both sides exist, while absence merely reduces detail/display. `dispute_resolved` remains stricter for both terminal branches, and `sub_failed_notify` remains fail-closed without trustworthy cause provenance. No refund-specific Tx Hash is required for a proven terminal path.
>
> ⚠️ **`Status::Expired` (int 8) is a terminal backend lifecycle fact.** For a paid non-trial `job_asp_accept_expire` or ASP delivery timeout (`job_expired` / legacy `submit_expired`), fresh Buyer-owned Expired(8), task kind, and exact positive original payment are sufficient to return `refund_confirmed`: the refund has arrived. No Failed(9), Tx Hash, local request/observation provenance, or Buyer claim/finalize action is required. A direct read follows its returned `stop`; scoped lifecycle/watch handling emits the terminal marker and cleans up. Trial or zero-amount expiry returns `expired_without_refundable_payment` with `settlement.state=not_required`; the same direct-read versus scoped-event boundary applies, without claiming fund movement. Both status-8 results set `rules.providerTimeoutRefundExpected=false` because timeout is no longer a future outcome.
>
> Backend semantics define `job_closed`, `job_refunded`, and `job_auto_refunded` as transaction-result notifications, not `uopData.executeResult` preflight. They may omit Tx Hash. For subscription Failed(9) finality, re-read fresh task-kind/status/ownership/payment facts and require durable local `request-refund` provenance for a User-requested/provider-decision-timeout refund. Expired(8) finality is independent and comes directly from the fresh authoritative lifecycle projection. The event itself supplies branch context only. Only explicit boolean `executeResult=false` blocks the legacy signer; missing, null, and non-boolean values remain compatible but never prove transaction success.
>
> ⚠️ **There is no `applied` status** — `provider_applied` is an event; when it fires, status is still `created`. Similarly when `dispute_approved` fires, status is still `rejected` (dispute phase 1 approve). Events are just "what just happened" — they don't necessarily change status.

---
