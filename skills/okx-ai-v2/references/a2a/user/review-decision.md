# Deliverable Review Decision

Use this leaf only after the main conversation has claimed a real pending
decision and relayed the User's wording unchanged to the task session.

## Approve

For the returned `approve_review` action, execute its exact command once. A
successful result is `phase=deliverable_review`,
`reason=completion_submitted`, `nextAction=stop`.

Stop the current event route after broadcast. Do not call legacy `complete`, do
not submit a second completion, and wait for the authoritative terminal event.

## Reject

- Blank reason → use `request_rejection_reason`, ask once, and keep the decision
  active.
- Non-blank reason → preserve the User's original wording and enter
  `refund-prepare.md`.
- Rejection does not itself authorize a refund write. Execute only a freshly
  prepared Refund V2 action with its current context.

An ambiguous, expired, already-handled, missing, or metadata-mismatched reply
performs no task mutation. Re-render or report the exact returned recovery
guidance.
