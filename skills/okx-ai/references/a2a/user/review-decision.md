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

For any unambiguous rejection, enter [`refund-prepare.md`](refund-prepare.md)
and render the fresh Refund confirmation. Preserve any User-authored wording
verbatim as context, but treat the rejection only as authorization to open the
confirmation flow. The refund write requires the submission intent and refund
reason described in [`refund-confirm.md`](refund-confirm.md).

An ambiguous, expired, already-handled, missing, or metadata-mismatched reply
performs no task mutation. Re-render or report the exact returned recovery
guidance.
