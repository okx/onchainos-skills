# User Refund V2

Use this reference for a User Agent refund request, refund progress query, or
refund-related arbitration result. Cancellation and refund are different:

- cancelling trial-to-paid conversion changes future service behavior;
- closing a zero-price task changes task state but moves no funds;
- refunding returns a payment;
- a User cannot start arbitration directly. A submitted refund request first
  waits for the ASP to agree or dispute.

The CLI is the authority for buyer ownership, task facts, exact amounts, and
current state. The safety matrix below is an additional allowlist: if a result
offers a write outside that matrix, stop and report a client/contract mismatch.

## Commands

Refund V2 is implemented by these commands:

```text
onchainos agent refund-prepare <jobId> [--reason <user-authored-text>]
onchainos agent refund-execute <jobId> \
  --operation <close-zero|direct-refund|request-refund|cancel-trial-conversion|finalize-expired-refund> \
  --refund-context-id <id> [--reason <user-authored-text>] --confirm
```

`finalize-expired-refund` is accepted by the low-level enum because its exact
type-207 transport is implemented, but no current `refund-prepare` result emits
it. Do not call it until a future authoritative detail contract distinguishes
the status-8 timeout cause and returns the action.

Do not emulate this contract with the disabled legacy writes `close`, `reject`,
`subscribe-reject`, or `claim-auto-refund`. They do not consume the Refund V2
context binding or return its structured progression result. `subscribe-cancel`
remains available only to cancel trial conversion or formal auto-renew; it is
not a refund substitute. A caller-provided `submit_expired` notification is
read-only as well; no current path may bypass Refund V2 to run a legacy write.
Never invent an endpoint, event, notification result, transaction hash, or
successful outcome.

## Entry and target resolution

Trigger on `refund`, `get my money back`, `apply for a refund`, rejection of a
paid deliverable, `refund status`, or an equivalent phrase in any language.
Explicit cancellation without a request to return funds remains in
`task-user-playbook.md`.

Resolve exactly one buyer-owned `jobId` from the current message or a fresh
User task list. If zero or multiple candidates remain, ask the User to provide
or select one. The positional `jobId` is required, so do not call
`refund-prepare` until it is known.

Run the read-only preparation command:

```text
onchainos agent refund-prepare <jobId>
```

If preparation returns `login` or `register_user_agent`, complete that owning
flow, preserve the returned `params.jobId`, and rerun `refund-prepare` for the
same job. Do not replace it with a creation `sid`.

Require exactly `phase`, `decision`, `reason`, `nextAction`, and `payload` in
success `data`, with `payload.schemaVersion=2`. Route actions through
`task-action-routing.md`. Unknown or malformed fields block; do not fall back
to a legacy write.

## Refund safety matrix

The CLI returns `payload.job.jobType`, `rawStatus`, `statusName`, and exact
`payment.originalAmount`. Do not infer these from conversation history.

| Target and state | Result | Allowed write |
|---|---|---|
| Trial subscription, Active (`trialType=1`, `rawStatus=1`, `autoRenew=1`) | `trial_subscription_not_refundable` | `cancel_trial_conversion` → `cancel-trial-conversion` |
| One-time, Created (`rawStatus=0`), exact amount zero | `zero_amount_close_confirmation_required` | `close_zero_price` → `close-zero` |
| One-time, Created (`rawStatus=0`), paid, verified escrow (`paymentMode=1`) | `direct_refund_confirmation_required` | `execute_direct_refund` → `direct-refund` |
| One-time, Submitted (`rawStatus=2`), paid, verified escrow (`paymentMode=1`) | collect reason, then `refund_request_confirmation_required` | `submit_refund_request` → `request-refund` |
| Formal paid subscription, Active (`trialType=0`, `rawStatus=1`) with a complete current-period boundary | collect reason, then `refund_request_confirmation_required` | `submit_refund_request` → `request-refund` |
| Formal paid subscription, Expired (`trialType=0`, `rawStatus=8`) | `expired_subscription_refund_cause_ambiguous` | none; status 8 does not distinguish accept-timeout/type-207 finalization from refund-response-timeout/backend settlement |
| One-time, Accepted (`rawStatus=1`) | `accepted_task_refund_contract_required` | none |
| One-time, Expired (`rawStatus=8`) | `accept_expired_refund_contract_ambiguous` | none; the subscription-only type-207 contract does not prove the timeout cause or a one-time write |
| Formal subscription, Created (`rawStatus=0`) | `direct_subscription_refund_contract_required` | none |
| Paid one-time, Closed (`rawStatus=7`) after direct refund | `refund_confirmed` only with `paymentMode=1` and a valid refund-specific detail hash; otherwise `refund_settlement_details_incomplete` | none |
| Paid subscription, Closed (`rawStatus=7`), including `job_asp_reject_closed` | `task_closed_no_new_refund_action`; the event is not authoritative settlement-cause proof, so its handler must not claim refund completion | read-only `view_refund_status` / `watch_task`; wait for status 9 plus complete refund proof |

The `*_contract_required` and `accept_expired_refund_contract_ambiguous` cases
are intentional fail-closed client capability gaps. There is no proven
lifecycle contract for these writes, so do not propose a legacy substitute.
In particular, `finalizeExpired` is a subscription accept-deadline contract;
never reuse it for a one-time status-8 task. Other zero-price states may return
`zero_amount_close_contract_required` and are likewise read-only. Missing or
non-escrow one-time funding returns `direct_refund_funding_not_verified` or
`refund_payment_not_verified`; a zero-price formal subscription returns
`zero_amount_subscription_not_refundable`; missing current-period boundaries
return `subscription_period_contract_required`. All are read-only.

`refund_task_details_incomplete` is also read-only: the CLI could not prove the
ASP identity and Service identifier needed by the confirmation card. Display
any missing ASP name as unavailable; never fill it from conversation memory.
For a trial, `trial_conversion_already_cancelled` means there is no remaining
auto-conversion write, while `trial_conversion_state_unknown` means the CLI
cannot prove whether one exists. Neither permits execution.

Implementation mapping is limited to existing lifecycle flows: one-time
Created uses the close flow for both zero-price close and paid direct refund;
one-time Submitted uses the regular reject flow; formal subscription Active
uses the subscription reject flow; trial Active with `autoRenew=1` uses
subscription cancellation. The type-207 transport for
`POST /priapi/v1/aieco/task/subscribe/{subId}/finalizeExpired` is implemented,
including exact response `jobId`, `type=207`, non-null `uopData`, and broadcast
`bizType=207` validation, but current authoritative detail does not distinguish
that accept-timeout from a refund-response timeout that the backend settles.
Therefore `refund-prepare` does not expose this write yet. Do not derive an
operation from status 8, event prose, or the presence of the low-level enum.

## Decision handling

### Trial subscription

For `trial_subscription_not_refundable`, explain that no refundable charge is
being returned. Cancellation stops trial-to-paid conversion; it does not end
the trial immediately and is not a refund. Execute only if the User explicitly
selects `cancel_trial_conversion`.

### Zero-price one-time task

For `zero_amount_close_confirmation_required`, show the exact zero amount and
original token. `close_zero_price` changes task state with no fund movement and
still requires explicit confirmation. `zero_amount_task_closed` is the
terminal closed result, not a refund result.

### Created paid one-time task

For `direct_refund_confirmation_required`, show ASP, Service, exact original
amount/token, and scope. Execute only after the User explicitly selects
`execute_direct_refund`; the initial word “refund” is not confirmation of the
displayed write.

### Expired formal subscription

Return `expired_subscription_refund_cause_ambiguous` with read-only status/watch
actions. The backend documents type/bizType 207 for ASP acceptance expiry, but
the same authoritative subscription status 8 can represent a refund-response
timeout whose settlement is backend-owned. Until detail exposes a trusted
cause discriminator, do not offer `finalize_expired_refund` and do not execute
the wired `finalize-expired-refund` operation manually. A one-time status-8 task
remains separately blocked with `accept_expired_refund_contract_ambiguous`.

### Submitted one-time or Active formal subscription

For `refund_reason_required` or `refund_reason_too_long`, ask only for a reason
and end the turn. It must be non-blank, at most
`payload.input.reasonMaxChars`, and authored by the User. Never supply,
paraphrase, translate, or improve it.

Then rerun preparation with the verbatim reason:

```text
onchainos agent refund-prepare <jobId> --reason "<verbatim User reason>"
```

For `refund_request_confirmation_required`, show the reason, ASP, Service,
amount/token, returned deadline, and returned rules: full refund in the
original token, no partial refund, no time-based proration, ASP may agree or
dispute, timeout is expected to auto-refund, settlement requires on-chain
confirmation, and progress is queryable. Execute only after the User selects
`submit_refund_request`.

## Execute the offered action

Copy the latest write action's `params.jobId`, `params.operation`,
`params.refundContextId`, and, for `request-refund`, `params.reason` unchanged:

```text
onchainos agent refund-execute <jobId> \
  --operation <operation> \
  --refund-context-id <refundContextId> \
  [--reason "<verbatim User reason>"] \
  --confirm
```

Never combine fields from different preparation results. The execute command
re-reads authoritative state and rejects stale or unavailable operations.
Every write action requires explicit confirmation; a supplied reason is never
permission by itself.

A successful write returns one of
`zero_amount_close_broadcast_submitted`, `refund_broadcast_submitted`,
`refund_request_broadcast_submitted`, or
`trial_conversion_cancel_broadcast_submitted`. All mean broadcast
submitted, not a terminal refund. The shared broadcast contract requires
`pkgId`, `orderId`, `orderType`, and `bizUniqKey`, but may omit `txHash` until
publication for any operation. A missing broadcast hash is a pending result,
not proof of failure or permission to retry.
Follow only the returned `view_refund_status` or `watch_task` action.

For `refund_outcome_unknown`, never retry the write automatically. Reconcile
through returned read-only actions. The CLI persists a same-device marker
before mutation. Revision, formatting, or billing-period drift does not bypass
it; the marker remains until the authoritative lifecycle proves that operation
advanced. Repeated prepare/execute calls return
`refund_operation_pending_reconciliation` instead of another write. This local
guard does not replace backend idempotency across devices.
If an unknown attempt has no transaction/order/UserOp identifier and the
authoritative lifecycle has not advanced, the client cannot prove that the
remote mutation was never received. It must remain fail-closed; do not unlock
it by age, `not found`, or user override. Complete self-service recovery for
that ambiguity requires a backend idempotency key or read-only operation-status
contract.
For `refund_write_rejected`, the backend returned a definitive business
rejection; rerun only through its fresh `prepare_refund` action.
`refund_prebroadcast_failed` proves that local validation, signing, or
simulation stopped before a broadcast request; it likewise permits only a
fresh prepare. A malformed response after the broadcast request remains
`refund_outcome_unknown`, never pre-broadcast failure. Wallet or
local reconciliation-guard failures stop before mutation and must not be
bypassed. For `refund_context_stale`, rerun
preparation through `prepare_refund`; do not reuse the old context.

## Progress, arbitration, and finality

- `provider_response_pending`: the ASP has not resolved the request; no second
  refund write is allowed.
- `arbitration_in_progress`: no refund has been decided. Offer only the returned
  `view_arbitration` and `stop` actions; do not invoke an ASP or Evaluator
  command.
- `expired_subscription_refund_cause_ambiguous`: formal subscription status 8
  does not prove whether the buyer should submit type 207 or wait for backend
  auto-refund. Use read actions only.
- `accept_expired_refund_contract_ambiguous`: a one-time status-8 task does not
  have a proven cause-specific response/write contract. Use read actions only.
- `refund_confirmed`: final refund state; this requires authoritative
  `rawStatus=9`; paid one-time `rawStatus=7` after direct close/refund with
  `paymentMode=1` is the only status-7 exception. Every branch also requires a
  valid refund-specific transaction hash in fresh detail.
- `refund_settlement_details_incomplete`: a refund-capable terminal status is
  present, but fresh detail cannot prove every required settlement invariant:
  refund-specific transaction hash, ASP/Service identity, or `paymentMode=1`
  provenance for a status-7 direct refund. Show `txHash: null`, do not claim
  completion, keep
  `payload.settlement.state=details_incomplete`, and use only returned
  read-only reconciliation actions.
- `refund_operation_pending_reconciliation`: this device already started the
  operation and its outcome is not yet authoritative. Revision, formatting,
  and billing-period drift do not make it safe to execute again; use only
  `view_refund_status` or `watch_task` until the lifecycle proves advancement.
- `refund_not_approved_or_task_completed`: no confirmed refund may be claimed.
  The current client cannot distinguish User-lost arbitration from a generic
  completed task in this state.
- `task_closed_no_new_refund_action`, `trial_subscription_closed_without_refund`,
  and `zero_amount_task_closed`: terminal task/cancellation outcomes with no new
  refund write.

Only `rawStatus=9`, or paid one-time `rawStatus=7` reached by the direct
close/refund path with `paymentMode=1`, can confirm a refund; both still require
a valid refund-specific `payload.settlement.txHash` from fresh detail. A broadcast
result is pending even when it contains a transaction hash. If the final hash
is absent or invalid, show `txHash: null`; never borrow a generic hash from a
task-detail field or fabricate one. A dedicated final-refund event may omit its
transaction hash. The handler requires the hash independently under fresh
detail's refund-specific key; an event-side `refundTxHash` or
`settlementTxHash`, when present, must match it. Generic event `txHash` /
`transactionHash` fields are never refund proof. It also requires matching
terminal status, current User ownership, ASP, Service, and the full original
payment before rendering completion.

### Subscription timeout and decline events

Treat the new subscription events as lifecycle signals, never as standalone
refund proof:

- `job_asp_accept_expire`: the ASP acceptance deadline expired, but the local
  event is not authoritative write permission. A formal subscription remains
  read-only until detail exposes the cause discriminator required to select
  type 207; a trial has no paid amount to refund. The event's
  `jobStatus=expired` and any human text saying funds were returned do not prove
  settlement.
- `job_asp_reject_closed`: the ASP declined acceptance and the subscription is
  Closed. The caller-supplied event name plus status does not prove an
  authoritative refund cause. A subscription at status 7 cannot use the
  one-time `paymentMode=1` exception and must not be reported as refund-complete;
  wait for status 9 plus complete refund proof. The type-207 action is not
  applicable.
- `job_asp_reject_expire`: the ASP did not agree to the refund or start
  arbitration before its response timeout. Treat automatic refund as pending;
  do not execute a client-side claim, report completion, emit a terminal
  marker, or clean up the session until final settlement proof arrives.

These `job_*` payloads may use `jobStatus=expired` for different timeout
causes. Always re-read task plus subscription detail; never select an operation
from the event name, prose, or status string alone. `job_auto_refunded` is the
documented post-`RefundSettled(ExpireRefund)` notification for type 207, but it
still passes through the normal status-9/hash/ownership/amount finality gate.

Final-event handling reuses the same task-plus-subscription composition as
`refund-prepare` and performs a short bounded re-read to absorb event/detail
projection races. If proof is still incomplete, it emits no terminal marker
and performs no session cleanup. Reconcile through the read-only
`refund-prepare` command and follow only the actions it actually returns; do
not synthesize a watch, status, or write action from event prose.

ASP agree, ASP timeout, and User-won arbitration may converge on status 9, but
the current payload does not expose a reliable settlement cause. Do not label
which branch occurred when `payload.settlement.cause` is `null`. Likewise, the
current client does not prove system/email delivery to both parties:
`payload.request.providerNotification` is limited to `not_requested` or
`unknown`. Report that state exactly and do not send a duplicate peer message.

## Amount and retry invariants

- Amounts are exact decimal strings. Zero means the original on-chain token
  amount is exactly zero, not a fiat estimate.
- Refund facts come only from payment fields (`paymentToken*` or canonical
  `token*`), never service pricing fields. A paid one-time write additionally
  requires authoritative escrow `paymentMode=1`.
- A formal-subscription refund scope is the current charged period. Never
  calculate unused time, future periods, or a partial amount.
- The prepared context binds current-period start/end (and period index or
  auto-renew state when returned); changed period facts invalidate execution.
- Preserve the original token address/symbol; never convert the refund asset.
- Repeated preparation is read-only. Never automatically repeat execution
  after a broadcast, stale context, or unknown outcome.

The product outcome is unambiguous even where the client cannot yet prove the
cause: User wins arbitration means refund; User loses means no refund. Render
that wording only when authoritative data explicitly identifies the outcome.
