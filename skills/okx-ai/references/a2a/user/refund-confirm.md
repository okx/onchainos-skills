# Buyer Refund Confirmation

Use this leaf only for a fresh Refund V2 confirmation flow.

## Delivered-task rejection

After the user rejects a delivered result, run:

```text
onchainos agent refund-prepare <jobId>
```

Render the complete [Confirm Refund Request](#confirm-refund-request) Template
6.1 from `payload.display`. The preceding `B` or rejection enters this
confirmation. Start refund submission when the user replies `Submit refund
request` or an unambiguous localized equivalent.

Analyze the reply for both the submission intent and a refund reason.

- When the reply contains clear submission intent and a non-blank reason,
  preserve the reason verbatim and continue immediately.
- When the reply contains clear submission intent without a reason, ask only
  for the refund reason and keep the Job ID, latest Refund V2 context, and that
  explicit submission intent active. Treat the next non-blank User-authored
  reply as the verbatim reason and continue immediately.
- When the reply provides a reason while the confirmation is waiting for
  submission intent, preserve it as a draft reason, rerun `refund-prepare` with
  that reason, re-render Template 6.1, and continue waiting for `Submit refund
  request`.

After both submission intent and the reason are present, rerun:

```text
onchainos agent refund-prepare <jobId> --reason <verbatimReason>
```

Continue only when the fresh result has `payload.schemaVersion=2`,
`phase=refund_confirmation`, `decision=ready`,
`reason=refund_request_confirmation_required`, and exactly one
`nextAction[id=submit_refund_request]`. Execute that action immediately through
[`refund-execute.md`](refund-execute.md).

## Other refund confirmations

For `zero_amount_close_confirmation_required` or
`direct_refund_confirmation_required`, render the fresh CLI values with
[Output Templates](#output-templates) and execute only the action selected from
that result.

Bind each write to an explicit action selected from the latest preparation
result. Render the returned recovery guidance for a blocked, stale, or malformed
result.

## Output Templates

The templates below are English sources. Reply in the language of the current
conversation while preserving Job IDs, Agent IDs, amounts, token symbols,
timestamps, and user-authored reasons exactly.

### Confirm Refund Request

Use `payload.display` from the latest `refund-prepare` result.

```markdown
### Confirm Refund Request

- Service Name: {serviceName}
- Job ID: {jobId}
- Service Provider: {serviceProviderName} (Agent ID: {agentId})
- Task Type: {taskType}
- Current Period: {currentPeriod}
- Refund Amount: {refundAmount}
- Reason for Refund: {reasonForRefund}

If everything is correct, reply “Submit refund request” and include your refund reason. To make changes, describe what you want to update.
```

Display rules:

1. Show the full Job ID.
2. Show `Current Period` only for a subscription.
3. Show `Reason for Refund` only when the CLI returns a non-empty value.
4. Preserve the original reason verbatim.
5. Use the CLI-provided service-name fallback, task type, amount, and formatted timestamps directly.
