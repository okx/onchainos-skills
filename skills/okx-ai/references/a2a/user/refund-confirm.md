# Buyer Refund Confirmation

Use this leaf only for a freshly prepared Refund V2 confirmation result.

## Standard confirmation

For `refund_request_confirmation_required`, render the Refund Request
Confirmation scene in [`refund-display.md`](refund-display.md). For
`zero_amount_close_confirmation_required` or
`direct_refund_confirmation_required`, render the Other Refund Confirmations
scene in that file. Wait for an explicit selection of the current write action.
The initial word “refund”, a supplied reason, or a prior confirmation never
confirms a new proposal.

## Active deliverable-review rejection

For an active post-delivery review card, `B` plus a non-blank User-authored
reason is final confirmation to submit a full refund request. Keep the existing
review-card copy unchanged and perform this binding in the current User
conversation:

1. Preserve the reason verbatim.
2. Run `refund-prepare <jobId> --reason "<verbatim reason>"` for fresh state.
3. Continue only if `schemaVersion=2`, `phase=refund_confirmation`,
   `decision=ready`, `reason=refund_request_confirmation_required`, and the
   returned action is `submit_refund_request`.
4. Copy that action's `jobId`, `operation`, `refundContextId`, and `reason`
   unchanged into [`refund-execute.md`](refund-execute.md) and execute
   immediately.

A blocked, changed, or malformed preparation result is authoritative and must
be rendered instead of executing.

Every other confirmation must be explicit and bound to the latest preparation
result. Never combine fields across results or reuse a stale context.
