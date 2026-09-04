# Task Output Templates

Use this reference when a task or subscription command returns the structured
progression fields `phase`, `decision`, `reason`, `nextAction`, and `payload`.
Render the result in the user's language. Treat `action` as legacy guidance,
not as a routing signal.

## Common shell

Use three sections:

```text
[Result]
<one-sentence conclusion>

[Details]
<only the fields needed for this phase>

[Next]
1. <action label>
2. <action label>

Reply with a number.
```

Rules:

- Render every returned `nextAction` item in order; number from 1.
- Mark the item with `recommend=true` as the recommended option in prose only
  when useful; do not change its number or meaning.
- Never invent an action not returned by the CLI.
- Numbers are valid only for the latest response.
- If there is one action, execute it when safe or show it without forcing a
  numbered choice when no user decision is required.
- Do not expose raw JSON, internal phase names, or provider instructions.

## Subscription view

Render only the current `my-tasks.subscriptions` page. Keep the CLI order;
never sort or compare token amounts. The list is read-only and does not add
actions, routing, or extra CLI calls.

For an unfiltered request, render `Active subscriptions` first and `Ended
subscriptions` second. Keep pagination separate for the two CLI responses.

For an Active row:

| # | Service | Provider | Fee | Auto-Renew | This Device |
|---|---|---|---|---|---|
| 1 | <title> | Agent#<providerAgentId> | <serviceTokenAmount> | <autoRenew> | <thisDeviceReceives> |

For an ended row:

| # | Service | Provider | Status | Fee |
|---|---|---|---|---|
| 1 | <title> | Agent#<providerAgentId> | <statusName> | <serviceTokenAmount> |

Use `<field>` for all placeholders in this file: it matches the surrounding
templates and avoids confusing placeholder braces with literal JSON objects.
Render `serviceTokenAmount` verbatim; it is a string. Render
`thisDeviceReceives` directly from the CLI as Yes/No. Device-wide receipt
state belongs to the explicit device-management flow, not this list.

## `decision=blocked`

Use a concise status result and a recovery-oriented action list.

```text
[Result]
<operation> cannot continue: <reason message>.

[Details]
Phase: <localized phase>
<relevant payload fields>

[Next]
1. <recommended recovery>
2. <alternative>
3. <cancel or return>

Reply with a number.
```

## `decision=requires_user_input`

```text
[Result]
<operation> needs more information.

[Details]
Missing: <payload.requiredParams>

[Next]
1. Provide the missing information
2. Choose another service
3. Cancel

Reply with a number, or provide the requested values directly.
```

Only ask for fields present in the current payload. Collect all missing required
fields together.

## `decision=ready`

For task or subscription creation, use a confirmation card. Other phases such
as Refund V2 have their own rendering rules below; `ready` does not by itself
mean “creation confirmation.”

```text
[Result]
<operation> is ready for confirmation.

[Details]
| Item | Value |
|---|---|
| Service | <service> |
| Provider | <provider> |
| Parameters | <confirmed parameters> |
| Price | <price> |

[Next]
1. Confirm and continue
2. Modify
3. Cancel

Reply with a number.
```

For `task_create_prepare`, `nextAction.id=open_create_playbook` means to open
the task-creation reference and continue its confirmation flow; it does not
mean that the subscription has already been created.

For `agent create-task`, `phase=creation`, `decision=ready`, and
`reason=broadcast_submitted` mean the create-and-fund UserOperation was
submitted but is not yet final. Render `payload.jobId`, `payload.broadcast.txHash`
when present, and the locally saved attachment count. Then execute the returned
`nextAction.id=watch_task`; do not offer `set-payment-mode`, ASP apply, or Buyer
accept.

For `agent create-subscribe`, the same progression state means the subscription
UserOperation was submitted but is not yet final. Require
`payload.type=204`, `payload.bizType=204`, and use `payload.jobId` as the sole
subscription identifier. Render the broadcast transaction hash when present,
attachment count, and `guideStatus` / `consentStatus` / `executionProfileSaved`.
Then execute
the returned `nextAction.id=watch_task`. Do not establish the A2A session in
this creation step; the `sub_open` event owns that transition.

For `phase=service_routing` and `nextAction.id=invoke_a2mcp`, do not render the
generic task-creation confirmation card above. Continue through the owning
A2MCP route selected by `SKILL.md`. Preserve `payload.serviceSnapshot` verbatim;
that route owns parameter collection, supported-token and balance display,
funding recovery, and the final mutually exclusive Confirm/Cancel card.

## Refund V2

Read `task-user-refund.md` before rendering any `phase` beginning with
`refund_`. Require `payload.schemaVersion=2`. Display exact decimal strings and
the original token; never calculate a partial amount, unused-time adjustment,
or fiat conversion.

| Reason | Result | Required details |
|---|---|---|
| `refund_target_required` | A buyer-owned task must be selected first. | Ask for or list exactly one current `jobId` |
| `trial_subscription_not_refundable` | This free trial has no paid amount to refund. | Service, trial end when returned, and that cancellation affects conversion only |
| `trial_conversion_already_cancelled` / `trial_conversion_state_unknown` | No new trial-conversion cancellation is currently safe. | Current auto-renew fact and read actions only |
| `zero_amount_close_confirmation_required` | This zero-price task can be closed; no funds will move. | Service, exact zero amount/token, current status |
| `direct_refund_confirmation_required` | A direct full refund is ready for confirmation. | The ordered `refund_task_details` presentation contract below |
| `refund_reason_required` / `refund_reason_too_long` | A valid User-authored refund reason is required. | Only `payload.input.requiredParams` and `reasonMaxChars` |
| `refund_request_confirmation_required` | The full refund request is ready for confirmation. | The ordered `refund_task_details` and `refund_rules` presentation contracts below, including the verbatim User reason |
| `zero_amount_close_broadcast_submitted` | The zero-price close was broadcast; no refund occurred. | Required receipt identifiers, Transaction hash when present, and pending state |
| `refund_broadcast_submitted` | The direct-refund transaction was broadcast and is pending final reconciliation. | The ordered `refund_task_details` presentation contract below; keep broadcast receipt handles internal |
| `refund_request_broadcast_submitted` | The refund request was broadcast and is awaiting reconciliation/ASP response. | The ordered `refund_task_details` and `refund_rules` presentation contracts below; keep broadcast receipt handles internal |
| `trial_conversion_cancel_broadcast_submitted` | Trial-to-paid cancellation was broadcast; no refund occurred. | Required receipt identifiers, Transaction hash when present, and pending state |
| `provider_response_pending` | The refund request is still awaiting the ASP. | Deadline and current notification states |
| `arbitration_in_progress` | The refund is under arbitration; no refund has been decided. | Arbitration phase/round/deadlines only when returned |
| `refund_confirmed` | The full original-token refund is confirmed by fresh authoritative paid non-trial Expired(8); by an ordinary-polling authoritative one-time lifecycle state; by `job_asp_reject_expire` plus matching durable `request-refund` provenance and fresh Failed(9) owner/type/payment facts; or by the existing proven User-requested subscription path. The Expired(8) path needs no Failed(9), Tx Hash, or local request provenance. | Settled amount/token; Service, ASP, and Tx Hash when available. For Expired(8), require `job.refundState=resolved`, `settlement.state=confirmed`, and `rules.providerTimeoutRefundExpected=false`. A direct read follows `stop`; a scoped lifecycle/watch event emits the terminal marker and cleans up |
| `expired_without_refundable_payment` | Fresh authoritative Expired(8) is terminal, but the task was a trial or had an exact zero original amount, so no refundable payment existed. | `job.refundState=resolved`, `settlement.state=not_required`, and `rules.providerTimeoutRefundExpected=false`. A direct read follows `stop`; a scoped lifecycle/watch event emits the terminal marker and cleans up without claiming a refund or fund movement |
| `refund_settlement_details_incomplete` | A possible refund terminal is present, but the required durable request provenance or a core job/Buyer/type/payment/fresh-status fact is missing or conflicting; completion cannot be claimed. | Exact core gap and only returned read actions; optional Provider/Service, period, token-symbol, `paymentMode`, and Tx Hash absence alone never select this reason |
| `refund_operation_pending_reconciliation` | This device already started this write and authoritative state has not proved it advanced. | Saved pending state/Tx Hash when available; read actions only and no repeat write, even if revision/period formatting drifted |
| `refund_not_approved_or_task_completed` | No confirmed refund can be reported. | Returned status and `settlement.state=not_refunded`; do not infer arbitration cause |
| `trial_subscription_closed_without_refund` / `zero_amount_task_closed` | The task/cancellation flow is closed with no new refund action. | Exact task state and whether funds moved |
| `task_closed_no_new_refund_action` | The subscription is Closed, so refund settlement is not established; do not claim refund completion. | Exact task state; retain read-only refund reconciliation only when matching durable local `request-refund` intent exists |
| `accepted_task_refund_contract_required` / `direct_subscription_refund_contract_required` / `zero_amount_close_contract_required` / `subscription_period_contract_required` | This state/type has no proven unambiguous Refund V2 write contract. | Task type/status and read-only actions only; Expired(8) instead uses terminal `refund_confirmed` for paid non-trial tasks or `expired_without_refundable_payment` for trial/zero-amount tasks |
| `refund_task_details_incomplete` | The ASP or Service identity needed for a pre-write confirmation card is incomplete. | Exact returned identifiers only; do not invent names. Terminal refund finality does not depend on these display labels |
| `direct_refund_funding_not_verified` / `refund_payment_not_verified` | The client cannot prove funded escrow for this one-time task. | Payment mode and read-only actions only |
| `zero_amount_subscription_not_refundable` | The formal subscription has no paid amount to refund. | Exact zero amount and read-only actions only |
| `refund_context_stale` | Task facts changed; the prior refund confirmation is no longer valid. | Fresh status; offer only `prepare_refund` |
| `refund_operation_not_available` | The requested operation is no longer allowed. | Render only the freshly returned action set |
| `refund_execution_confirmation_required` | This exact Refund V2 write still needs confirmation. | Render the complete prepared action; never treat the reason as permission |
| `refund_outcome_unknown` | The write outcome is not yet known. | Known operation/transaction facts; never suggest rebroadcast unless a later preparation explicitly offers it |
| `refund_write_rejected` | The backend returned a definitive business rejection; no unknown write remains. | Diagnostic and a fresh `prepare_refund` action |
| `refund_prebroadcast_failed` | Validation, local signing, or simulation failed before any broadcast request. | Diagnostic and a fresh `prepare_refund` action; never use this reason for a malformed/unknown broadcast response |
| `refund_wallet_preflight_failed` / `refund_reconciliation_guard_unavailable` | Execution stopped before mutation. | Diagnostic and `stop`; do not bypass the guard |
| `refund_not_available_for_status` | A refund is not available in the current state. | Current type/status and only the returned alternatives |

For `direct_refund_confirmation_required` and
`refund_request_confirmation_required`, the CLI must return an ASP Agent ID and
a Service name or ID before showing a write card. Service identity remains a
pre-write validation fact; it is not an additional row in the presentation
contracts below. For terminal
`refund_confirmed`, missing ASP/Service labels affect display completeness only:
render `unavailable` and preserve the confirmed result. Never invent a display
name or identifier. If the CLI returns `refund_task_details_incomplete` or
`refund_settlement_details_incomplete`, render the gap and do not reconstruct
facts from conversation history.

### Refund task-detail presentation contract

This section defines semantic structure and field sources, not literal UI
copy. Localize every section heading, row label, task-type value, status value,
and rule sentence into the User's language at render time. Do not hard-code
localized prose in Rust, command output, or this reference. Preserve IDs,
exact decimal amounts, token symbols, and the User-authored reason unchanged.

For an ASP-not-yet-accepted direct-refund flow, render a localized heading for
the same `refund_task_details` section for both
`direct_refund_confirmation_required` and the immediate
`refund_broadcast_submitted` result. Use exactly these rows and this order:

| Semantic row | Authoritative source |
|---|---|
| `task_name` | `payload.job.jobName` |
| `job_id` | `payload.job.jobId` |
| `task_type` | `payload.job.jobType` |
| `service_provider` | `payload.job.providerName` plus `payload.job.providerAgentId`, formatted exactly as `<providerName> (<providerAgentId>)`; do not add a label inside the parentheses |
| `current_status` | `payload.job.statusName` |
| `payment_amount` | `payload.payment.originalAmount` plus `payload.payment.tokenSymbol` |

If the Provider name is unavailable, render a localized unavailable label while
still showing the authoritative Agent ID. The status in an immediate broadcast
result is the latest authoritative lifecycle status read before the write; do
not relabel it as a terminal post-broadcast status.

After the User supplies a refund reason, render the following for both
`refund_request_confirmation_required` and the immediate
`refund_request_broadcast_submitted` result:

1. The same ordered `refund_task_details` section above.
2. One additional `refund_reason` row sourced from
   `payload.request.userReason` and rendered verbatim.
3. A localized heading and an unordered bullet list for the `refund_rules`
   section containing exactly these four semantic rules, in order (do not
   number these rules):
   - the ASP may agree to the refund or open arbitration, and the User is
     expected to receive an automatic refund if the ASP does not respond before
     timeout;
   - the refund returns the full original payment token, with neither partial
     refund nor time-based proration;
   - the refund requires on-chain confirmation, after which task detail exposes
     the refund amount, refund time, and the Tx Hash field;
   - after submission, the User can return to task detail at any time to view
     progress.

Derive those statements from `payload.rules` and the returned read actions;
never infer a stronger rule than the payload supports. A Tx Hash field may be
rendered unavailable when the unchanged backend does not return transaction
metadata. Do not place `pkgId`, `orderId`, `orderType`, `bizUniqKey`, internal
phase/reason identifiers, or an unconfirmed broadcast hash inside either
presentation section. Render only the returned `nextAction` choices after the
sections.

Write actions (`cancel_trial_conversion`, `close_zero_price`,
`execute_direct_refund`, `submit_refund_request`) always require an explicit
selection, even when they are the only non-`stop` action. Do not apply the
common one-action auto-execution rule to them.

`payload.settlement.state=broadcast_submitted` is pending. Render completion
only for `reason=refund_confirmed`. For a one-time task, fresh backend
chain-projected Failed(9), or fresh paid-escrow Closed(7) with
positive amount and `paymentMode=1`, confirms the refund when User/payment facts
match. A Tx Hash and same-device receipt are optional audit details, not
completion prerequisites. `job_asp_reject_expire` additionally requires
durable `request-refund` provenance plus fresh Failed(9) owner/type/payment
facts before event-driven terminal output for either task kind. Bare
subscription Failed(9) remains ambiguous because it also represents terminal
charge failure. Subscription confirmation requires durable local Refund V2
`request-refund` provenance for a User-requested/provider-decision-timeout path,
bound to the same job, Buyer, formal `jobType=1` subscription, exact positive
original amount, and token address plus fresh Buyer-owned Failed(9).
Fresh paid non-trial Expired(8) is a separate direct finality path for both task
kinds; it needs no Failed(9) or local provenance. Provider/Service, period,
token-symbol, and `paymentMode` fields veto only when both recorded and fresh
values exist and conflict; missing values reduce detail/display only. Result
events (`sub_asp_agree`, `sub_reject_refund_notify`, `job_asp_reject_expire`,
`job_refunded`, `job_auto_refunded`, or `dispute_resolved`) may describe the branch but cannot
create proof. Event-only Failed(9) remains incomplete. `sub_failed_notify` does
not prove a refund. Subscription Closed(7) does not qualify. Never
reuse a refund-complete heading for `refund_not_approved_or_task_completed`.
For `dispute_resolved`, render neither the User-won/refund status-9 branch nor
the ASP-won/no-refund status-6 branch unless durable local `request-refund`
provenance and fresh composed job type, Buyer ownership, and exact terminal
status all match. Event JSON alone must not trigger verdict, rating,
notification, or cleanup output.

For `refund_confirmed`, render `settlement.txHash` as optional transaction
metadata when available. Otherwise render `Tx Hash: unavailable`; keep the
confirmed heading and do not relabel the result as incomplete. There is no
required `refundTxHash` or `settlementTxHash` backend field.
`settlement.confirmationSource=backend_onchain_lifecycle` records
the authoritative business-finality source and is not transaction provenance.
`settlement.provenance`, when present, is optional same-device wallet-order
audit context. For pending writes, render any candidate hash only from
`settlement.broadcastReceipt.txHash` and label it as submitted/unconfirmed;
normalized `settlement.txHash` remains null.

For any Refund V2 `*_broadcast_submitted` result, a missing broadcast `txHash`
is an allowed pending receipt shape when the required `pkgId`, `orderId`,
`orderType`, and `bizUniqKey` are present. It is not evidence of failure or
permission to retry. Evidence validity comes from operation-scoped receipt
context rather than a prescribed hash field name. A persisted direct-refund
receipt binds the pending client operation; fresh backend chain-projected
one-time terminal state determines its business outcome even when no hash is
returned. A durable `request-refund` journal entry proves only that exact refund
intent/submission by itself; when retained across restart and bound to the same
job, Buyer, formal job type, exact positive original amount, and token address,
it combines with fresh Buyer-owned subscription Failed(9) to disambiguate the
terminal from unrelated charge failure. Optional comparison fields do not need
to be present, but a two-sided mismatch vetoes. Every other
broadcast receipt proves only its own client operation.
Continue only through the returned read-only reconciliation actions.

`uopData.executeResult` is the backend preflight result. Render its error when
it is explicitly boolean `false` and do not broadcast. Preserve the legacy
contract for null, missing, and non-boolean values: continue through normal
signing/broadcast validation, but never describe any `executeResult` value as
transaction success, finality, or refund completion.

The unchanged backend contract defines `job_closed`, `job_refunded`, and
`job_auto_refunded` as transaction-result notifications that may omit Tx Hash.
Re-read detail before rendering: a matching one-time positive-amount
paid-escrow Closed(7) or Failed(9) can confirm independently without a hash.
For subscriptions, result events `sub_asp_agree`,
`sub_reject_refund_notify`, `job_asp_reject_expire`, `job_refunded`,
`job_auto_refunded`, and `dispute_resolved` may select branch wording only after
fresh ownership and composed facts are verified; they cannot create refund
proof. User-requested paths, including `job_asp_reject_expire`, require durable
local `request-refund` provenance bound to job, Buyer, formal `jobType=1`, exact
positive original amount, and token address plus fresh Buyer-owned Failed(9).
An acceptance/delivery timeout instead confirms directly from fresh
Buyer-owned paid non-trial Expired(8), task kind, and exact original payment.
Event-only and bare subscription Failed(9) remain insufficient.
`dispute_resolved` additionally requires matching durable local
`request-refund` provenance and fresh composed job type/Buyer/status for **both**
status 9 (User wins) and status 6 (ASP wins); otherwise render no verdict and
perform no rating, notification, or cleanup side effects.

`sub_failed_notify` names a possible charge/conversion failure but never proves
that cause or a refund. The current caller-supplied/replayable event has no
trustworthy cause provenance, so both with and without durable refund intent
must render settlement-incomplete/read-only output: no fund-direction claim,
terminal copy, terminal marker, or cleanup. Only independently trustworthy
cause provenance returned by the CLI may select terminal charge-failure copy.

Render `job_asp_accept_expire` (ASP acceptance timeout) and `job_expired` /
legacy `submit_expired` (ASP delivery timeout) only after a fresh authoritative
read. For a paid non-trial task, fresh Buyer-owned Expired(8), task kind, and
exact positive original payment produce `refund_confirmed`: state that the
automatic refund has arrived. No Failed(9), Tx Hash, or request provenance is
needed. Never offer `claim-auto-refund` or another Buyer write. For a trial or
zero-amount task, render terminal `expired_without_refundable_payment` with
`settlement.state=not_required` without claiming fund movement. A direct
`refund-prepare` read follows its returned `stop` action and does not imply a
separate session-cleanup command. A scoped lifecycle/watch event emits the
terminal marker, cleans up that scoped session, and stops re-entry. If a
trial's future configured price is shown, label it as a configured post-trial
amount, never as a paid or escrowed amount.

Render `job_asp_reject_expire` (ASP refund-decision timeout) as a Failed(9)
backend automatic-refund terminal only after durable `request-refund`
provenance plus fresh matching Buyer ownership, task type, exact positive
original amount, and token address pass the proof gate. Tx Hash is optional.
The caller event alone cannot produce terminal copy, a marker, or cleanup.
The ASP-side checkout does not possess that Buyer-local durable provenance.
For a subscription, its replayable `job_asp_reject_expire` input plus fresh
provider-owned Failed(9) therefore renders a neutral refund-result-unverified
notice; it must not claim that funds were returned. This restriction does not
change the User-side terminal result after the full proof gate passes.
Render `job_asp_reject_closed` as Closed without claiming refund settlement.

The current client exposes only ASP notification state as `not_requested` or
`unknown`. Do not upgrade it to sent or send an extra peer message to
compensate.

## `task_create_prepare` phase mapping

| Phase | Decision | Next action IDs |
|---|---|---|
| `login_validation` | `blocked` | `login` |
| `identity_validation` | `blocked` | `register_user_agent` |
| `service_validation` | `blocked` | `stop` |
| `service_routing` | `ready` | `invoke_a2mcp` |
| `subscription_validation` | `blocked` | `restore_subscription`, `stop` |
| `funding_required` | `blocked` | none; enter shared Funding directly |
| `creation` | `ready` | `open_create_playbook` |
