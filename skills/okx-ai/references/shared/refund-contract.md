# Refund Contract

This shared helper defines Refund eligibility, settlement proof, and safety.
It never routes an intent and never authorizes a command by itself.

## Action allowlist

Only fresh authoritative combinations below may offer a write:

| Target and state | Required facts | Action ID | Operation |
|---|---|---|---|
| Trial subscription, Active | `trialType=1`, `autoRenew=1` | `cancel_trial_conversion` | `cancel-trial-conversion` (legacy operation ID; revokes the trial) |
| One-time, Created | original amount is zero | `close_zero_price` | `close-zero` |
| One-time, Created | positive amount, `paymentMode=1` | `execute_direct_refund` | `direct-refund` |
| One-time, Submitted | positive amount, `paymentMode=1`, valid reason | `submit_refund_request` | `request-refund` |
| Formal subscription, Active | positive current-period payment, complete period boundary, valid reason | `submit_refund_request` | `request-refund` |

All writes require explicit confirmation. Accepted one-time tasks and Created
formal subscriptions are read-only contract gaps. Expired tasks never offer a
Buyer claim/finalize write.

## Finality matrix

Always use a fresh read. History or caller events cannot prove settlement.

| Fresh authoritative facts | Outcome |
|---|---|
| Paid non-trial one-time or formal subscription at Expired(8), matching Buyer, kind, and exact positive original payment | `refund_confirmed`; backend automatic refund arrived. |
| Trial subscription or exact-zero task at Expired(8) | `expired_without_refundable_payment`; settlement not required. |
| One-time at Closed(7), positive original amount, `paymentMode=1` | `refund_confirmed`; close returned escrow. |
| One-time at Failed(9), matching Buyer and exact-zero original payment | `zero_amount_task_failed`; terminal failure with `settlement.state=not_required`. |
| One-time at Failed(9), matching Buyer and exact positive original payment | `refund_confirmed`. |
| Formal subscription at Failed(9), matching fresh Buyer/type and exact positive original payment | `refund_confirmed`; render `Refund completed`. |
| Subscription at Closed(7) | `task_closed_no_new_refund_action`; Closed alone proves no refund. |

Both Expired(8) outcomes require `job.refundState=resolved` and
`rules.providerTimeoutRefundExpected=false`. Paid uses
`settlement.state=confirmed`; no-funds uses `not_required`.

For a positive-payment task, `job_asp_reject_expire` requires matching durable
`request-refund` provenance plus fresh Failed(9), owner, type, exact amount,
and token address. An exact-zero one-time task at buyer-owned Failed(9) is
instead terminal with no refund settlement and requires no refund provenance.
`dispute_resolved` requires that provenance plus fresh kind, ownership, and
terminal status: status 9 is User-winning refund; status 6 is ASP-winning
no-refund. An event alone is never proof.

A Tx Hash is optional audit metadata after proof passes. Missing optional
display facts do not undo finality; when recorded and fresh values both exist,
a mismatch vetoes it.

## Safety invariants

- Never substitute legacy writes `close`, `reject`, `subscribe-reject`, or
  `claim-auto-refund`. `subscribe-cancel` is cancellation-only.
- Use exact authoritative decimal amounts, never service pricing, floats, fiat
  estimates, or conversation memory.
- Return the full eligible original token amount only. Never convert, prorate,
  or calculate a partial refund.
- Preserve exact action fields and exact User reason. Never invent endpoints,
  events, results, hashes, or success.
- Broadcast, event, vote, or pending hash is not finality.
- Preparation is read-only; never automatically repeat execution.
