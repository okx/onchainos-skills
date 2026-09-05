# A2A User Router

| Intent | Read |
|---|---|
| Buyer refunds and paid-deliverable rejection | [Buyer refunds](refund.md) |
| Create, view, or manage a one-time job | `job.md` |
| My subscriptions, Active/Ended list, or selected detail | `subscription.md` |
| Device delivery, offline delivery, or signal receipt | `receipt.md` |
| Pause, resume, or change execution policy | `execution-policy.md` |

Select one file and stop routing. `refund.md` owns refund commands,
confirmation, presentation, and finality. `receipt.md` reads
`../../runtime/watch.md` only when watch is required. Renewal and non-refund
cancellation remain in their lifecycle reference.
