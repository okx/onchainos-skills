# Deliverable Review Decision

Use this leaf only after the main conversation has claimed a real pending
decision and relayed the User's wording unchanged to the task session.

## Approve

For the returned `approve_review` action, execute its exact command once. A
successful result is `phase=deliverable_review`,
`reason=completion_submitted`, `nextAction=stop`.

Stop the current event route after broadcast and wait for the authoritative
terminal event. The returned `approve_review` action is the completion action
for this route.

## Reject

For any unambiguous rejection, call the `reject_review` next action. A submitted
zero-price one-time task requires a non-blank User-authored rejection reason. If
it is missing, execute the returned `request_rejection_reason` action and wait;
no reject endpoint is called. Preserve the supplied reason verbatim, then call
`reject_review` again so the existing `/pre-reject` + `/reject` lifecycle runs.
The backend transitions that case directly to Failed(9), with no refund request.
For every other task, preserve the User-authored wording verbatim, enter
[`refund-prepare.md`](refund-prepare.md), and render the complete fresh Template
6.1 Refund V2 confirmation. Continue with the intent-and-reason response matrix
in [`refund-confirm.md`](refund-confirm.md).

For an ambiguous, expired, already-handled, missing, or metadata-mismatched
reply, re-render or report the exact returned recovery guidance.
