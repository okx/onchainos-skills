# ASP Refund-or-Arbitration Decision

Use this leaf for `job_rejected`, `sub_user_reject`, or an active
refund-or-arbitration card. An explicit instruction to arbitrate a specified
task is already authorization and routes directly to
[`dispute.md`](dispute.md); do not create another decision card. Rejected
candidates and filed cases are different sets; use
[`arbitration-query.md`](arbitration-query.md) for queries.

## Open a decision

For a System entry, consume the progression result already produced by the A2A
entry router. Do not call `next-action` again for the same envelope.

Only an explicit free-text request to respond to a specified rejection without
a fresh progression result makes one role-bound call:

```text
onchainos agent next-action --role asp --agentId <aspAgentId> \
  --message '{"event":"<job_rejected|sub_user_reject>","jobId":"<jobId>"}'
```

Call it exactly once. An explicit instruction to start arbitration remains the
direct `dispute.md` entry and does not open this decision flow.

Render the returned task name, exact amount/token, period/deadline when present,
and these choices:

```text
A. Approve full refund
B. Start arbitration — reply with B followed by your reason
```

Build each choice mechanically from the same result:

```json
{"key":nextAction.key,"actionId":nextAction.id,"params":nextAction.params}
```

Request one durable decision card:

```text
onchainos agent pending-decisions-v2 request-prompt \
  --job-id <payload.jobId> --role asp --agent-id <aspAgentId> \
  --source-event <job_rejected|sub_user_reject> \
  --decision-id <payload.decisionId> \
  --choices-json '<choices built from nextAction>' \
  --user-content '<rendered card>' \
  --list-label '<payload.name> — <payload.amount> <payload.tokenSymbol>' \
  [--expires-at <returned deadline>]
```

Preserve subscription `decisionBindingKey` and `decisionBindingValue`. After
delivery, end the turn.

## Resume and resolve

Re-render a matching active `[USER_DECISION_REQUEST]`. If absent, list with
`pending-decisions-v2 list --format markdown` and activate a selection with
`pending-decisions-v2 pick --index <N>`. Regenerate only when the specified job
has no durable entry.

Final replies are `A` or `B <reason>`. Preserve the complete B reason verbatim;
an empty reason keeps the card active.

1. Run the card's pre-filled `resolve-with-sessionkey` command in CLI-driver
   mode, or `resolve-prompt` in queue mode, with the full reply.
2. Execute returned `validate_arbitration_choice` through `next-action` using
   its exact role, agentId, and complete message.
3. Route the resulting action through [`dispute.md`](dispute.md).

`ambiguous_choice` re-renders the card. `arbitration_reason_required` asks only
for `B <reason>`. Metadata missing, expiry, unsupported action, stale event, or
job mismatch is rendered with returned recovery guidance. A valid reply is the
final confirmation for exactly that bound action.
