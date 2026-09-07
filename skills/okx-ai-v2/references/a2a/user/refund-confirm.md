# Buyer Refund Confirmation

Use this leaf only for a fresh Refund V2 confirmation flow.

## Delivered-task rejection

After the user rejects a delivered result, run:

```text
onchainos agent refund-prepare <jobId>
```

Render [Confirm Refund Request](#confirm-refund-request) and wait for
`Submit refund request` or an unambiguous localized equivalent.

Analyze the reply for both the submission intent and a refund reason.

- When the reply contains clear submission intent and a non-blank reason,
  preserve the reason verbatim and continue immediately.
- When the reply contains clear submission intent without a reason, ask only
  for the refund reason and keep the Job ID and latest Refund V2 context active.
- During that reason follow-up, treat the next non-blank user-authored reply as
  the reason and preserve it verbatim.

After obtaining the reason, rerun:

```text
onchainos agent refund-prepare <jobId> --reason <verbatimReason>
```

Continue only when the fresh result has `payload.schemaVersion=2`,
`phase=refund_confirmation`, `decision=ready`,
`reason=refund_request_confirmation_required`, and exactly one
`nextAction[id=submit_refund_request]`. Execute that action immediately through
[`refund-execute.md`](refund-execute.md). The collected reason is the final
input and immediately authorizes the returned action.

## Other refund confirmations

For `zero_amount_close_confirmation_required` or
`direct_refund_confirmation_required`, render the fresh CLI values with
[Output Templates](#output-templates) and execute only the action selected from
that result.

A blocked, stale, or malformed result remains read-only. Keep every action
bound to one latest preparation result.

## Output Templates

The templates below are English sources. Reply in the language of the current
conversation while preserving Job IDs, Agent IDs, amounts, token symbols,
timestamps, and user-authored reasons exactly.

### Confirm Refund Request

Use `payload.display` from the latest `refund-prepare` result.

```markdown
### Confirm Refund Request

| Service Name | Job ID | Service Provider | Task Type | Current Period | Refund Amount | Reason for Refund |
|---|---|---|---|---|---|---|
| {serviceName} | {jobId} | {serviceProviderName} (Agent ID: {agentId}) | {taskType} | {currentPeriod} | {refundAmount} | {reasonForRefund} |

If everything is correct, reply “Submit refund request” and include your refund reason. To make changes, describe what you want to update.
```

Display rules:

1. Show the full Job ID.
2. Show `Current Period` only for a subscription.
3. Show `Reason for Refund` only when the CLI returns a non-empty value.
4. Preserve the original reason verbatim.
5. Use the CLI-provided service-name fallback, task type, amount, and formatted timestamps directly.
