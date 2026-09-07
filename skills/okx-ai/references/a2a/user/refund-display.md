# Refund Presentation

Read this file only when rendering a refund result. Use the English labels and
action copy in the selected display template. Preserve IDs, exact decimal amounts,
token symbols, and the User-authored reason.

## Refund request confirmation

scene: Refund request confirmation

display template:

```markdown
### Refund Request

| # | Service name | Job ID | Service Provider | Task Type | Current Period | Refund Amount | Reason for Refund |
|---|---|---|---|---|---|---|---|
| 1 | {payload.job.serviceName} | {payload.job.jobId} | {payload.display.serviceProviderLabel} | {payload.display.taskTypeLabel} | {payload.display.currentPeriodLabel} | {payload.display.refundAmountLabel} | {payload.request.userReason} |

To submit the request, reply “Submit refund request”. To handle it later, reply “Not now”.
```

display rules:

1. Use this scene only for `refund_request_confirmation_required` with the current `submit_refund_request` action.
2. Preserve the Service name, full Job ID, Service Provider ID, token symbol, and User-authored reason exactly.
3. Use `taskTypeLabel`, `serviceProviderLabel`, `currentPeriodLabel`, and `refundAmountLabel` directly. Do not calculate or infer them.
4. Show Current Period only for a Subscription. For a One-time task, omit the Current Period column and cell.
5. The Current Period and all displayed times use minute precision with an explicit UTC offset.
6. `Submit refund request` selects only the returned `submit_refund_request` action. `Not now` ends the turn without a write.

## Other refund confirmations

For `zero_amount_close_confirmation_required` and
`direct_refund_confirmation_required`, render these fields in order:

| Order | Field | Source |
|---:|---|---|
| 1 | `task_name` | `payload.job.jobName` |
| 2 | `job_id` | `payload.job.jobId` |
| 3 | `task_type` | `payload.job.jobType` |
| 4 | `service_provider` | `payload.job.providerName` and `payload.job.providerAgentId`, as `<name> (<id>)` |
| 5 | `current_status` | `payload.job.statusName` |
| 6 | `payment_amount` | `payload.payment.originalAmount` and `payload.payment.tokenSymbol` |

Use only fields 1–6 for `direct_refund_confirmation_required` and its immediate
`refund_broadcast_submitted` result. If the Provider name is unavailable, show
a localized unavailable value and keep the authoritative Agent ID.

For every fresh Expired(8) result, render fields 1–6 first. A paid non-trial
task then uses the confirmed settlement display; a trial or zero-price task
uses the no-refundable-payment display.

Do not add receipt, internal phase/reason, package, order, or context rows to
these other confirmation cards.

## Settlement

- Pending: show only returned read actions. A submitted Tx Hash is unconfirmed
  metadata, not settlement proof.
- Confirmed: show the settled amount and original token. Show confirmation time
  and `payload.settlement.txHash` when available; a missing Tx Hash does not
  weaken the result.
- No refundable payment: state that no funds moved.
- Incomplete: state only which returned facts are unavailable and keep the flow
  read-only.
