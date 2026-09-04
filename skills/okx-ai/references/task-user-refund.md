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

Refund V2 is client-side orchestration over the unchanged refund backend
contract. It reuses the existing lifecycle endpoints, result events, and fresh
task/subscription status projection. To disambiguate overloaded subscription
Failed(9) without adding a backend field, the client durably records its own
Refund V2 `request-refund` provenance and reuses it across polling and process
restart. It does not require a new refund cause query, typed settlement source,
or fields named `refundTxHash` or `settlementTxHash`.

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
| Paid one-time, Closed (`rawStatus=7`) with verified escrow (`paymentMode=1`) | `refund_confirmed` from the fresh backend chain projection; a Tx Hash may be unavailable and is not required for the refund conclusion | none |
| Paid one-time, Failed (`rawStatus=9`) | `refund_confirmed`; for one-time tasks this chain-projected state is reserved for successful refund transitions. A Tx Hash may be unavailable | none |
| Paid subscription, Closed (`rawStatus=7`), including `job_asp_reject_closed` | `task_closed_no_new_refund_action`; Closed alone is not a subscription-refund terminal | retain read-only refund reconciliation only when matching durable local `request-refund` intent exists |
| Subscription, bare Failed (`rawStatus=9`), with no matching durable local `request-refund` provenance | `refund_settlement_details_incomplete`; subscription Failed also represents terminal charge failure | no refund-complete claim; event-free polling cannot infer the cause |
| Subscription legacy result event (`sub_asp_agree`, `sub_reject_refund_notify`, `job_refunded`, `job_auto_refunded`, or `dispute_resolved`) plus Failed (`rawStatus=9`), but no matching durable local `request-refund` provenance | `refund_settlement_details_incomplete`; the event may describe a branch but cannot create proof | read-only only; event-only Failed(9) remains ambiguous |
| Matching durable local `request-refund` provenance bound to the same job, Buyer, formal `jobType=1` subscription, exact positive original amount, and token address plus fresh Buyer-owned Failed (`rawStatus=9`) | `refund_confirmed`; polling and restart recovery use this client provenance plus fresh lifecycle contract | none; Tx Hash is optional, and a legacy event may refine branch wording |
| Subscription `sub_failed_notify` plus Failed (`rawStatus=9`), without trustworthy event provenance/cause, whether or not matching durable refund intent exists | `refund_settlement_details_incomplete`; neither refund nor charge/conversion failure is proven | read-only reconciliation only; emit no terminal marker and perform no cleanup |

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

`refund_task_details_incomplete` is also read-only for a pre-write flow: the CLI
could not prove the ASP identity and Service identifier needed by the
confirmation card. Terminal refund finality does not depend on those display
labels; if a confirmed terminal response lacks them, render them as unavailable
instead of downgrading or filling them from conversation memory.
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

For `direct_refund_confirmation_required`, render the ordered
`refund_task_details` presentation contract from `task-output-templates.md`.
It contains task name, Job ID, task type, Provider name plus Agent ID, current
status, and exact original amount/token. Do not add a Service row, internal
receipt handle, or localized hard-coded copy. Execute only after the User
explicitly selects `execute_direct_refund`; the initial word “refund” is not
confirmation of the displayed write.

For this Created-task path, the on-chain `close` UserOperation is itself the
operation that closes the job and returns escrow to the User. A fresh backend
chain projection of one-time Closed(7), paid amount, and `paymentMode=1`
therefore confirms the refund outcome. Do not wait for or invent a second
refund transaction. Render an authoritative Tx Hash when one is available;
otherwise render it as unavailable without downgrading the confirmed refund.
The same-device receipt remains useful for pending reconciliation and audit,
but it is not required once the backend has projected the terminal state.

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

For `refund_request_confirmation_required`, render the ordered
`refund_task_details` section, the verbatim `refund_reason` row, and the four
semantic `refund_rules` items defined in `task-output-templates.md`. Do not add
a Service row, deadline row, internal receipt handle, or localized hard-coded
copy to those sections. Execute only after the User selects
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

For the immediate post-submit display, preserve the same language-neutral
presentation contract used by the corresponding confirmation:

- `refund_broadcast_submitted` renders only the ordered
  `refund_task_details` section before the returned actions;
- `refund_request_broadcast_submitted` renders the ordered
  `refund_task_details` section, the verbatim `refund_reason`, and the four
  `refund_rules` items before the returned actions.

Keep the result explicitly pending; these sections do not claim that the
refund has settled. Do not expose internal receipt handles or turn an
unconfirmed broadcast hash into refund-completion evidence.

For reconciliation, a persisted broadcast receipt is evidence only for the
exact operation that produced it. Its provenance must bind the current User,
`jobId`, prepared context/snapshot, operation, validated lifecycle response
`type`, the same value carried unchanged as broadcast `bizType`, and durable
receipt handles. The current `/close` contract does not publish a fixed numeric
type allowlist, so the client must not invent one. A receipt
for `direct-refund` identifies that submitted operation while it is pending;
the later fresh one-time Closed(7) or Failed(9) chain projection proves the
refund outcome even when the backend does not expose a Tx Hash. A
`request-refund` journal entry proves only that exact client refund
intent/submission by itself; it never proves a later ASP-agreed, timeout, or
arbitration refund outcome alone. Keep valid request provenance durable across
Rejected(3), Disputed(4), process restart, polling, and terminal-result
recovery. Its required core binding is the same job, Buyer, formal `jobType=1`
subscription, exact positive original amount, and token address; a later fresh
Buyer-owned subscription Failed(9) supplies the backend lifecycle half of the
confirmation. Provider/Service, period, token-symbol, and `paymentMode` values
are optional comparisons: when both recorded and fresh values exist, a mismatch
vetoes; absence does not invalidate provenance or finality and only reduces
available detail/display. Definitively rejected or core-mismatched local records
must not qualify.

`executeResult` inside backend `uopData` is the backend's pre-broadcast
preflight result. Preserve the established signer compatibility contract: only
an explicit boolean `false` blocks before signing/broadcast and surfaces the
backend error when present. Boolean `true`, null, a missing field, or a
non-boolean value continues through the existing signing/broadcast validation;
none of those values is itself a broadcast receipt, transaction-success result,
terminal state, or refund proof.

For `refund_outcome_unknown`, never retry the write automatically. Reconcile
through returned read-only actions. The CLI persists a local marker before
mutation. Revision, formatting, or billing-period drift does not bypass it.
For `request-refund`, retain matching provenance through intermediate lifecycle
advancement, polling, process restart, and terminal-result recovery so a later
fresh subscription Failed(9) remains disambiguated. Invalidate it only after a
definitive rejection or core-binding mismatch. After confirmed terminal
handling, retain this provenance or persist an equivalent durable terminal fact;
do not erase the only recovery evidence. Repeated prepare/execute calls return
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
- `refund_confirmed`: final refund state. For a one-time task, a fresh backend
  chain projection of Failed(9) identifies one of the successful refund
  transitions (ASP agreement, refund-response timeout, or User-won
  arbitration); a fresh paid-escrow Closed(7) with positive amount and
  `paymentMode=1` also confirms that escrow was returned. A Tx Hash or
  same-device receipt is useful audit data but is not a prerequisite.
  For a subscription, matching durable local Refund V2 `request-refund`
  provenance bound to job, Buyer, formal `jobType=1`, exact positive original
  amount, and token address plus fresh Buyer-owned Failed(9) confirms the refund.
  Provider/Service, period, token-symbol, and `paymentMode` values veto only on
  a two-sided mismatch; absence affects detail/display only.
  This works for later polling and after process restart. Bare and event-only
  subscription Failed(9) remain cause-ambiguous. Legacy result events may
  describe the branch but cannot create proof.
- `refund_settlement_details_incomplete`: a refund-capable lifecycle status is
  present, but matching durable request provenance, its core job/Buyer/formal-
  job-type/exact-positive-amount/token-address binding, required fresh
  status/ownership facts, or the one-time direct-refund `paymentMode=1`
  invariant is missing or conflicting. Optional Provider/Service, period,
  token-symbol, or subscription `paymentMode` absence and a missing Tx Hash do
  not alone select this reason. Keep the unavailable normalized
  `settlement.txHash` null, do not claim completion, keep
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

Transaction evidence is optional display/audit data after the established
provenance-plus-fresh-lifecycle contract has confirmed a refund outcome. There is no
required `refundTxHash` or `settlementTxHash` property. When a Tx Hash is
returned, display it only in its returned operation/event context; never
relabel an unrelated task hash as refund evidence. When no hash is returned,
`settlement.txHash` remains null/unavailable while
`settlement.state=confirmed` and `reason=refund_confirmed` may still be valid.
In that case `settlement.confirmationSource=backend_onchain_lifecycle` records
why the business outcome is confirmed; it is not transaction provenance.
`settlement.provenance` remains reserved for optional operation-scoped wallet
order correlation. The durable local `request-refund` record is an internal
classification/recovery input; it need not create a Tx Hash or populate
`settlement.provenance`.

For subscription Failed(9), the state alone is not enough because the backend
also uses it for terminal charge failure. Pair fresh Buyer/status facts with
durable local `request-refund` provenance whose core binding covers job, Buyer,
formal job type, exact positive original amount, and token address; no future cause query or
typed backend source is required. A legacy event is optional branch context,
not proof. A Tx Hash may be absent in a confirmed result. A request/finalize
broadcast remains proof only of that submitted client operation by itself, not
of the later backend-owned refund outcome.
During pending reconciliation, any submitted candidate remains confined to
`settlement.broadcastReceipt.txHash` and must be labelled pending.

### Refund lifecycle events

The backend contract is unchanged. `job_closed`, `job_refunded`, and
`job_auto_refunded` are transaction-result notifications, not `executeResult`
preflight, and their payloads may omit a Tx Hash. Subscription events
`sub_asp_agree`, `sub_reject_refund_notify`, `job_refunded`,
`job_auto_refunded`, and `dispute_resolved` retain their legacy branch
semantics. However, the CLI event argument can be caller-supplied or replayed,
so the event may select branch wording but cannot create settlement proof. The
standard system-envelope gate still applies: nonzero `message.code` is failure.

An event never authorizes a new write. Re-read fresh composed detail and apply
the independent proof gate:

- for a one-time task, matching fresh positive-amount paid-escrow Closed(7) or
  Failed(9), plus User/payment binding, may produce `refund_confirmed` without a
  Tx Hash; missing ASP/Service labels are rendered unavailable;
- for a subscription, durable local `request-refund` provenance must bind the
  same job, Buyer, formal `jobType=1`, exact positive original amount, and token
  address, while fresh detail proves Buyer ownership and Failed(9). Optional
  Provider/Service, period, token-symbol, and `paymentMode` values veto only
  when both sides exist and conflict. This produces `refund_confirmed` during event handling,
  polling, or restart recovery. Bare Failed(9) and event-only Failed(9) remain
  ambiguous.

For `dispute_resolved`, always require matching durable local Refund V2
`request-refund` provenance plus fresh composed job type, Buyer ownership, and
exact terminal status. Only then does status 9 identify the User-winning/refund
branch or status 6 identify the ASP-winning/no-refund branch; the event itself
proves neither. Without that proof, do not announce a verdict, auto-rate,
notify, emit a terminal marker, or clean up.

`sub_failed_notify` is never refund proof, and its current caller-supplied/
replayable envelope is not trustworthy charge-failure cause provenance either.
With or without durable refund intent, fail closed: make neither a refund nor
charge-failure final claim, emit no terminal marker, perform no cleanup, and
keep only read-only reconciliation. A terminal charge-failure result requires
an independently trustworthy provenance/cause result from the CLI.

Treat the other new subscription events as lifecycle signals, never as
standalone refund proof:

- `job_asp_accept_expire`: the ASP acceptance deadline expired, but the local
  event is not authoritative write permission. A formal subscription remains
  read-only until detail exposes the cause discriminator required to select
  type 207; a trial has no paid amount to refund. The event's
  `jobStatus=expired` and any human text saying funds were returned do not prove
  settlement.
- `job_asp_reject_closed`: the ASP declined acceptance and the subscription is
  Closed. A subscription at status 7 cannot use the one-time `paymentMode=1`
  exception and must not be reported as refund-complete. Retain refund
  reconciliation only when matching durable local `request-refund` provenance
  exists. The type-207 action is not applicable.
- `job_asp_reject_expire`: the ASP did not agree to the refund or start
  arbitration before its response timeout. Treat automatic refund as pending;
  do not execute a client-side claim, report completion, emit a terminal
  marker, or clean up the session. The retained local `request-refund` record
  plus a later fresh Buyer-owned subscription Failed(9) and matching original
  payment confirms the outcome even if no follow-up event or Tx Hash arrives.

These `job_*` payloads may use `jobStatus=expired` for different timeout
causes. Always re-read task plus subscription detail; never select an operation
from the event name, prose, or status string alone. `job_auto_refunded` is the
documented post-`RefundSettled(ExpireRefund)` notification for type 207 and may
omit a Tx Hash. It can describe the automatic-refund branch, but subscription
proof still comes from matching durable local `request-refund` provenance with
the core job/Buyer/formal-job-type/exact-positive-amount/token-address binding
plus fresh Buyer/status facts. Optional comparison-field absence is not a gate.

Final-event handling reuses the same task-plus-subscription composition as
`refund-prepare` and performs a short bounded re-read to absorb event/detail
projection races. It combines the backend-defined semantic event with fresh
lifecycle state, durable local provenance, and optional transaction metadata.
The semantic event is optional branch context and is never sufficient proof.
If provenance or fresh state is incomplete, it emits no refund-terminal marker
and performs no refund cleanup.
Reconcile through the read-only
`refund-prepare` command and follow only the actions it actually returns; do
not synthesize a watch, status, or write action from event prose.

ASP agree, ASP timeout, and User-won arbitration may converge on status 9, but
confirmation does not require `payload.settlement.cause` to be populated. Do
not label a more specific branch than a matching legacy event establishes when
that field is `null`, and never use that label as settlement proof. Likewise, the
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
