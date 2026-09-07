# Buyer Refund Preparation

Use this leaf to resolve one Buyer-owned refund target and obtain a fresh,
read-only Refund V2 result. Cancellation changes future renewal; a refund
returns the applicable original payment.

Resolve exactly one Job ID from the request or a fresh refund-task list. Run:

```text
onchainos agent refund-prepare <jobId> [--reason <verbatimReason>]
```

The CLI owns eligibility, ownership, task type, service-name fallback, payment
facts, billing period, deadlines, and available actions. Route only by the
returned `nextAction[].id`.

- Use [`refund-confirm.md`](refund-confirm.md) for confirmation and reason collection.
- Use [`refund-execute.md`](refund-execute.md) only after the required intent and reason are complete.
- Use [`../refund-reconcile.md`](../refund-reconcile.md) for pending or terminal results.

For `refund_reason_required` or `refund_reason_too_long`, follow the active
confirmation flow. Preserve the reason verbatim and enforce the CLI-provided
maximum length. Unknown reasons or action IDs keep the flow read-only.
