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
or fiat conversion. This section owns display semantics only. Refund evidence,
event handling, and terminal behavior are defined in
[`task-user-refund.md` Finality](task-user-refund.md#progress-arbitration-and-finality).

### Reason-to-display groups

| Reason group | Display contract |
|---|---|
| `refund_target_required` | Ask the User to select exactly one buyer-owned `jobId`. |
| `refund_reason_required`, `refund_reason_too_long` | Ask only for a non-blank User-authored reason within `payload.input.reasonMaxChars`. |
| `direct_refund_confirmation_required`, `refund_request_confirmation_required` | Render the confirmation contract below and the returned write action. |
| `zero_amount_close_confirmation_required`, `trial_subscription_not_refundable` | Explain the exact non-refund effect and render only the returned choices; any write still requires explicit selection. |
| `refund_execution_confirmation_required` | Render the complete currently prepared action and its exact payment or non-payment effect, then wait for explicit selection. |
| `zero_amount_close_broadcast_submitted`, `refund_broadcast_submitted`, `refund_request_broadcast_submitted`, `trial_conversion_cancel_broadcast_submitted` | Render the corresponding post-submit contract as pending. A close or trial conversion cancellation must not be described as a refund. |
| `provider_response_pending`, `arbitration_in_progress`, `refund_operation_pending_reconciliation`, `refund_outcome_unknown` | Show the returned pending state and read-only next actions. Do not imply settlement or suggest a retry. |
| `refund_confirmed` | Render the confirmed settlement contract below. |
| `expired_without_refundable_payment`, `trial_subscription_closed_without_refund`, `zero_amount_task_closed`, `refund_not_approved_or_task_completed`, `task_closed_no_new_refund_action` | Render the returned terminal or closed state and state whether funds moved. Never use refund-complete copy for a no-refund result. |
| `refund_settlement_details_incomplete` | Render the incomplete settlement contract below. |
| `accepted_task_refund_contract_required`, `direct_subscription_refund_contract_required`, `zero_amount_close_contract_required`, `subscription_period_contract_required`, `refund_task_details_incomplete`, `direct_refund_funding_not_verified`, `refund_payment_not_verified`, `zero_amount_subscription_not_refundable`, `refund_not_available_for_status`, `trial_conversion_already_cancelled`, `trial_conversion_state_unknown` | Explain the returned capability or fact gap and show only returned read actions. Do not reconstruct missing facts. |
| `refund_context_stale`, `refund_operation_not_available`, `refund_write_rejected`, `refund_prebroadcast_failed`, `refund_wallet_preflight_failed`, `refund_reconciliation_guard_unavailable` | Render the returned diagnostic and only its returned recovery action. |

### Confirmation and post-submit fields

The contract is semantic rather than literal copy. Localize labels and prose to
the User's language, while preserving IDs, exact decimal amounts, token symbols,
and the User-authored reason. Use these fields in this order:

| Order | Semantic field | Authoritative source |
|---:|---|---|
| 1 | `task_name` | `payload.job.jobName` |
| 2 | `job_id` | `payload.job.jobId` |
| 3 | `task_type` | `payload.job.jobType` |
| 4 | `service_provider` | `payload.job.providerName` plus `payload.job.providerAgentId`, formatted as `<providerName> (<providerAgentId>)` |
| 5 | `current_status` | `payload.job.statusName` |
| 6 | `payment_amount` | `payload.payment.originalAmount` plus `payload.payment.tokenSymbol` |

Use exactly fields 1-6 for both `direct_refund_confirmation_required` and its
immediate `refund_broadcast_submitted` result. For
`refund_request_confirmation_required` and its immediate
`refund_request_broadcast_submitted` result, render exactly fields 1-6, then:

7. `refund_reason` from `payload.request.userReason`, verbatim.
8. `refund_rules`, using the four semantic rules below in order.

If the Provider name is unavailable, render an unavailable label and retain the
authoritative Agent ID. The post-submit status is the latest returned lifecycle
status; do not relabel it as terminal. Do not add a separate Service, response
deadline, or receipt row. Keep `pkgId`, `orderId`, `orderType`, `bizUniqKey`,
internal phase/reason identifiers, and other receipt handles out of these
sections.

When `refund_request_confirmation_required` is the fresh preparation result for
an active deliverable-review B + reason reply, execute its returned
`submit_refund_request` immediately. Render the fields and rules from the
subsequent `refund_request_broadcast_submitted` result.

### Refund rules

| Order | Semantic rule | Meaning |
|---:|---|---|
| 1 | `provider_response` | The ASP may agree to the refund or open arbitration; if the returned rules say the response timeout leads to automatic refund, state that expectation. |
| 2 | `full_original_payment` | The refund uses the full original payment token, without partial refund or time-based proration. |
| 3 | `onchain_confirmation` | Settlement requires on-chain confirmation. Task detail shows the refund amount and confirmation time, and shows the transaction hash when available. |
| 4 | `progress_visibility` | After submission, the User can return to task detail to view progress. |

Render all four rules for the refund-request confirmation and immediate
post-submit result. Derive their statements from `payload.rules` and returned
read actions; do not promise stronger behavior than those fields support.

### Settlement displays

#### Pending

For a broadcast-submitted, provider-pending, arbitration-pending,
reconciliation, or unknown result, label the outcome as pending. Show a
candidate transaction hash only from `payload.settlement.broadcastReceipt.txHash`
and label it submitted or unconfirmed. A missing hash is still a valid pending
display. Render only returned read-only next actions.

#### Confirmed

Use refund-complete copy only for `reason=refund_confirmed`. Show the settled
amount and original token. Show Service, ASP, confirmation time, and
`payload.settlement.txHash` when available; otherwise render those optional
fields as unavailable without downgrading the confirmed result.

#### No refundable payment

For `reason=expired_without_refundable_payment` with
`payload.settlement.state=not_required`, state that the lifecycle is terminal
and that no refundable payment existed. Do not claim a refund or fund movement.

#### Incomplete or blocked

For `reason=refund_settlement_details_incomplete` with
`payload.settlement.state=details_incomplete`, show the returned gap and
read-only next actions. Do not use confirmed copy, infer a fund destination, or
promote pending transaction metadata. For every other blocked reason, explain
only returned facts and actions.

Write actions (`cancel_trial_conversion`, `close_zero_price`,
`execute_direct_refund`, `submit_refund_request`) always require explicit
selection, even when only one write is offered.

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
