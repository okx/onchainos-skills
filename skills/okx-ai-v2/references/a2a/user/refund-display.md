# Refund Presentation

Read this file only when rendering a refund result. Localize headings, labels,
status names, and rule prose. Preserve IDs, exact decimal amounts, token
symbols, and the User-authored reason.

## Task details

Under a localized `refund_task_details` heading, render exactly these fields in
order:

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

For `refund_request_confirmation_required` and its immediate
`refund_request_broadcast_submitted` result, append `refund_reason` from
`payload.request.userReason` verbatim, then a localized `refund_rules` heading
with these rules in order:

1. `provider_response`: the ASP may agree or open arbitration; a response
   timeout produces an automatic refund only when `payload.rules` says so.
2. `full_original_payment`: return the full original payment token; no partial
   refund or time-based proration.
3. `onchain_confirmation`: settlement requires chain confirmation; task detail
   shows the amount and confirmation time, plus Tx Hash when available.
4. `progress_visibility`: the User may return to task detail to check progress.

Do not add Service, deadline, receipt, internal phase/reason, package, order, or
context rows to these cards.

## Settlement

- Pending: show only returned read actions. A submitted Tx Hash is unconfirmed
  metadata, not settlement proof.
- Confirmed: show the settled amount and original token. Show confirmation time
  and `payload.settlement.txHash` when available; a missing Tx Hash does not
  weaken the result.
- No refundable payment: state that no funds moved.
- Incomplete: state only which returned facts are unavailable and keep the flow
  read-only.
