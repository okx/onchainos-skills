# ASP Refund-or-Evaluation Decision

Use this leaf for `job_rejected`, `sub_user_reject`, a pending refund request,
or an explicit request to evaluate one rejected task.

## Refund request detail and decision

Use the fresh structured event decision or `refund-detail` result.
Render [Buyer Refund Request](#buyer-refund-request).

## Resolve the decision

For an event-created card, preserve its Job ID, decision ID, deadline, and
choice binding in `pending-decisions-v2`.

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

To refund the buyer, reply “Approve refund”. To request platform evaluation, reply “Request evaluation” and include your evaluation reason.
```

Display rules:

1. Show `Current Period` only for a subscription.
2. Preserve the full Job ID and the buyer-authored reason.
3. Use CLI-provided display values directly.
4. An explicit request to evaluate a selected task uses this same decision view.
