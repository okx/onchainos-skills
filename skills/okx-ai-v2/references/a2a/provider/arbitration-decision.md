# ASP Refund-or-Arbitration Decision

Use this leaf for `job_rejected`, `sub_user_reject`, or an active
refund-or-arbitration card. An explicit instruction to arbitrate a specified
task is already authorization and routes directly to
[`dispute.md`](dispute.md); do not create another decision card. Rejected
candidates and filed cases are different sets; use
[`arbitration-query.md`](arbitration-query.md) for queries.

## Open a decision

For a structured event, resolve the complete envelope through
[`../router.md`](../router.md):

```text
onchainos agent next-action --role auto --agentId <envelope.agentId> \
  --message '<complete envelope.message as one JSON string>'
```

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
  [--refund-display-b64 <payload.refundDisplayB64>] \
  [--expires-at <returned deadline>]
```

Preserve subscription `decisionBindingKey` and `decisionBindingValue`. Pass a
non-null `refundDisplayB64` unchanged; never construct or edit it. Its absence
does not change the individual decision card, but the normalized refund list
will remain blocked until the affected rejection event is refreshed. After
delivery, end the turn.

## Pending refund decisions

```text
onchainos agent pending-decisions-v2 list --scope refund --format json
```

scene: Pending refund decision list

display template:

```markdown
You have {pendingCount} refund requests awaiting a decision:

| # | Service name | Job ID | Task Type | Refund Amount | Response Deadline |
|---|---|---|---|---|---|
| {n} | {serviceName} | {jobId} | {taskType} | {refundAmount} | {responseDeadlineLabel} |

Reply with the number or Job ID to view and process a request.
```

display rules:

1. Render every returned item in CLI order and number the current list from 1.
2. Preserve each full Job ID. A reply containing the exact Job ID selects it with `pending-decisions-v2 pick --job-id <jobId>`; a number selects it with `--index <n>`.
3. Use only the CLI-provided Service name, Task Type, Refund Amount, and Response Deadline. Do not parse display copy or infer missing fields.
4. `No refund required` is the authoritative zero-amount label.
5. The CLI returns only current ASP refund decisions and sorts them by Response Deadline ascending.
6. When `pendingCount>0`, the final sentence is the only recommendation for this list. Omit it for an empty list. Selection only opens the request; it does not approve a refund or file for evaluation.

## Resume and resolve

Re-render a matching active `[USER_DECISION_REQUEST]`. If absent, list with
`pending-decisions-v2 list --scope refund --format json` and open a selection
with `pending-decisions-v2 pick --index <N>` or
`pending-decisions-v2 pick --job-id <fullJobId>`. Regenerate only when the
specified job has no durable entry.

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
