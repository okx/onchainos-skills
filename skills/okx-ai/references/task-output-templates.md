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
this creation step; the `sub_created` event owns that transition.

For `phase=service_routing` and `nextAction.id=invoke_a2mcp`, do not render the
generic task-creation confirmation card above. Open
`a2mcp-direct-invoke.md`. Preserve `payload.serviceSnapshot` verbatim; that
reference owns parameter collection, supported-token and balance display,
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
| `direct_refund_confirmation_required` | A direct full refund is ready for confirmation. | Service, ASP, scope, original amount/token |
| `expired_subscription_refund_cause_ambiguous` | Subscription status 8 does not prove whether type-207 buyer finalization or backend auto-refund applies. | Fresh task/subscription status, original amount/token, and read-only actions only; no write action |
| `refund_reason_required` / `refund_reason_too_long` | A valid User-authored refund reason is required. | Only `payload.input.requiredParams` and `reasonMaxChars` |
| `refund_request_confirmation_required` | The full refund request is ready for confirmation. | Reason, Service, ASP, amount/token, ASP deadline, full/no-partial/no-proration rules |
| `zero_amount_close_broadcast_submitted` | The zero-price close was broadcast; no refund occurred. | Required receipt identifiers, Transaction hash when present, and pending state |
| `refund_broadcast_submitted` | The direct-refund transaction was broadcast and is pending final reconciliation. | Required receipt identifiers, Transaction hash when present, amount/token, pending state |
| `refund_request_broadcast_submitted` | The refund request was broadcast and is awaiting reconciliation/ASP response. | Required receipt identifiers, Transaction hash when present, deadline, ASP notification state |
| `trial_conversion_cancel_broadcast_submitted` | Trial-to-paid cancellation was broadcast; no refund occurred. | Required receipt identifiers, Transaction hash when present, and pending state |
| `provider_response_pending` | The refund request is still awaiting the ASP. | Deadline and current notification states |
| `arbitration_in_progress` | The refund is under arbitration; no refund has been decided. | Arbitration phase/round/deadlines only when returned |
| `refund_confirmed` | The full original-token refund is confirmed by authoritative backend chain-projected state or a server-verifiable successful refund result. | Settled amount/token; Service, ASP, and Tx Hash when available. Render unavailable display fields explicitly without downgrading the result |
| `refund_settlement_details_incomplete` | A possible refund terminal is present, but required lifecycle cause/state, User ownership, or payment facts are incomplete; completion cannot be claimed. | Exact missing facts and only returned read actions; do not use this reason for missing ASP/Service labels or Tx Hash alone |
| `refund_operation_pending_reconciliation` | This device already started this write and authoritative state has not proved it advanced. | Saved pending state/Tx Hash when available; read actions only and no repeat write, even if revision/period formatting drifted |
| `refund_not_approved_or_task_completed` | No confirmed refund can be reported. | Returned status and `settlement.state=not_refunded`; do not infer arbitration cause |
| `trial_subscription_closed_without_refund` / `zero_amount_task_closed` | The task/cancellation flow is closed with no new refund action. | Exact task state and whether funds moved |
| `task_closed_no_new_refund_action` | The subscription is Closed, but authoritative refund cause is unavailable; do not claim refund completion. | Exact task state and read-only status/watch actions; Tx Hash availability does not resolve the cause |
| `accepted_task_refund_contract_required` / `direct_subscription_refund_contract_required` / `accept_expired_refund_contract_ambiguous` / `zero_amount_close_contract_required` / `subscription_period_contract_required` | This state/type has no proven unambiguous Refund V2 write contract. | Task type/status and read-only actions only; one-time status 8 must not reuse the subscription-only finalize-expired contract |
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
a Service name or ID before showing a write card. For terminal
`refund_confirmed`, missing ASP/Service labels affect display completeness only:
render `unavailable` and preserve the confirmed result. Never invent a display
name or identifier. If the CLI returns `refund_task_details_incomplete` or
`refund_settlement_details_incomplete`, render the gap and do not reconstruct
facts from conversation history.

Write actions (`cancel_trial_conversion`, `close_zero_price`,
`execute_direct_refund`, `submit_refund_request`) always require an explicit
selection, even when they are the only non-`stop` action. Do not apply the
common one-action auto-execution rule to them.

`payload.settlement.state=broadcast_submitted` is pending. Render completion
only for `reason=refund_confirmed`. For a one-time task, fresh backend
chain-projected Failed(9), or fresh paid-escrow Closed(7) with
positive amount and `paymentMode=1`, confirms the refund when User/payment facts
match. A Tx Hash and same-device receipt are optional audit details, not
completion prerequisites. Subscription Failed(9) remains ambiguous because it
also represents terminal charge failure; the current caller-supplied event name
cannot disambiguate it. Require an authoritative backend cause/query,
authenticated server event record, or typed source. Subscription
Closed(7) does not qualify. Never reuse a refund-complete heading for
`refund_not_approved_or_task_completed`.

For `refund_confirmed`, render `settlement.txHash` as the refund transaction
hash only when it is authoritatively available. Otherwise render `Tx Hash:
unavailable`; keep the confirmed heading and do not relabel the result as
incomplete. `settlement.confirmationSource=backend_onchain_lifecycle` records
the authoritative business-finality source and is not transaction provenance.
`settlement.provenance`, when present, is optional same-device wallet-order
audit context. For pending writes, render any candidate hash only from
`settlement.broadcastReceipt.txHash` and label it as submitted/unconfirmed;
normalized `settlement.txHash` remains null.

For any Refund V2 `*_broadcast_submitted` result, a missing broadcast `txHash`
is an allowed pending receipt shape when the required `pkgId`, `orderId`,
`orderType`, and `bizUniqKey` are present. It is not evidence of failure or
permission to retry. Evidence validity comes from operation-scoped, typed
provenance rather than a prescribed field name. A persisted direct-refund
receipt binds the pending client operation; fresh backend chain-projected
one-time terminal state determines its business outcome even when no hash is
returned. Every other broadcast receipt proves only its own client operation.
Continue only through the returned read-only reconciliation actions.

`uopData.executeResult` is the backend preflight result. Render its error when
false and do not broadcast. Null, missing, or non-boolean values also fail
closed; never silently treat them as success. Never describe
`executeResult=true` as transaction success, finality, or refund completion.

The backend defines `job_closed` and `job_auto_refunded` as successful,
confirmed transaction-result notifications that may omit Tx Hash. The current
event input is caller-supplied, so the name alone is not authoritative. Re-read
detail before rendering: a matching one-time positive-amount paid-escrow
Closed(7) or Failed(9) can confirm independently without a hash; subscription
Failed(9) remains ambiguous even when the caller supplies
`job_auto_refunded`.

For the other subscription events, render `job_asp_accept_expire` as
acceptance-expiry pending authoritative cause support,
`job_asp_reject_closed` as Closed without claiming refund settlement, and
`job_asp_reject_expire` as automatic settlement pending. Event labels,
`jobStatus=expired`, and human-readable event text never authorize a write or a
refund-complete heading. Keep the session and terminal marker open until an
authoritative refund cause/query or server-verifiable result and matching fresh
lifecycle facts pass the normal Refund V2 gate.

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
