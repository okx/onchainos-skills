# ASP Refund-or-Evaluation Decision

Use this leaf for `job_rejected`, `sub_user_reject`, a pending refund request,
or an explicit request to evaluate one rejected task.

## Refund request detail and decision

For a System entry, consume the fresh progression result already produced by
the A2A router. For a selected pending request, use the fresh `refund-detail`
result. Render [Buyer Refund Request](#buyer-refund-request).

For a one-time task, the CLI reads Buyer’s Reason from
`message.rejectReason` on `job_rejected`, or `detail.rejectReason` from the
fresh `refund-detail` response. It preserves the value verbatim and normalizes
it to `payload.buyerReason`; the Skill must use that normalized value and must
not extract or reconstruct a reason from free text.

Continue only when every template field is present. If a System result is
blocked for missing fields, run the same `refund-detail` query used by a
selected request. If fresh detail is still incomplete, show the missing fields
and stop; do not infer or reconstruct them.

## Resolve the decision

For an event-created card, preserve its Job ID, decision ID, deadline, and
choice binding in `pending-decisions-v2`.

Pass the CLI-encoded card and label unchanged:

```text
onchainos agent pending-decisions-v2 request-prompt \
  --job-id <payload.jobId> --role asp --agent-id <aspAgentId> \
  --source-event <job_rejected|sub_user_reject> \
  --decision-id <payload.decisionId> \
  --choices-json '<choices built mechanically from nextAction>' \
  --user-content-b64 <payload.userContentB64> \
  --list-label-b64 <payload.listLabelB64> \
  --refund-display-b64 <payload.refundDisplayB64> \
  --expires-at <payload.responseDeadlineTimestamp>
```

Never decode, edit, or replace the encoded values. Preserve subscription
`decisionBindingKey` and `decisionBindingValue`. A missing encoded value blocks
delivery; never fall back to raw shell interpolation.

- `Approve refund` resolves to `agree_refund` or `sub_agree_refund` and runs the returned command in the current conversation.
- Analyze a `Request evaluation` reply for both the intent and an evaluation reason. When both are present, preserve the reason verbatim, resolve the bound action, and run it in the current conversation.
- When the reply contains the `Request evaluation` intent without a reason, ask only for the evaluation reason. Treat the next non-blank reply as the reason, preserve it verbatim, resolve the bound action, and run it in the current conversation.

For a card opened directly from a selected pending request, keep the selected
Job ID and fresh `nextAction` values in the current conversation:

- `Approve refund` runs the matching refund action returned by `refund-detail`.
- Analyze a `Request evaluation` reply for both the intent and an evaluation reason. When both are present, preserve the reason verbatim, add it to the matching evaluation action, and run it immediately.
- When the reply contains the `Request evaluation` intent without a reason, ask only for the evaluation reason. Treat the next non-blank reply as the reason, preserve it verbatim, add it to the matching evaluation action, and run it immediately.

The returned action is the final authorization. Present one concise localized
result after the command completes.

## Output Templates

The template below is an English source. Reply in the language of the current
conversation while preserving the full Job ID, amounts, token symbols,
timestamps, and user-authored reasons exactly.

### Buyer Refund Request

```markdown
### Buyer Refund Request

| Service Name | Job ID | Task Type | Current Period | Requested Refund | Buyer’s Reason | Response Deadline |
|---|---|---|---|---|---|---|
| {serviceName} | {jobId} | {taskType} | {currentPeriod} | {requestedRefund} | {buyerReason} | {responseDeadline} |

Please respond by the deadline. Otherwise, a full refund will be issued automatically.

To refund the buyer, reply “Approve refund.” To dispute the request, reply “Request evaluation” and provide your reason.
```

Display rules:

1. Show `Current Period` only for a subscription.
2. Preserve the full Job ID and the buyer-authored reason exactly.
3. Use only CLI-provided Service Name, Task Type, Current Period, Requested Refund, Buyer’s Reason, and Response Deadline values.
4. A missing field blocks the card. Do not infer, calculate, or reconstruct it.
5. `Approve refund` selects only the returned refund action. `Request evaluation <reason>` selects only the returned evaluation action and preserves the complete reason.
