# A2A User Router

| Intent | Read |
|---|---|
| Request a refund, check refund eligibility/status, or interpret a refund result | `../../../../okx-ai/references/task-user-refund.md` |
| Create, view, or manage a one-time job | `job.md` |
| My subscriptions, Active/Ended list, or selected detail | `subscription.md` |
| Device delivery, offline delivery, or signal receipt | `receipt.md` |
| Pause, resume, or change execution policy | `execution-policy.md` |

Select one file and stop routing. `receipt.md` reads `../../runtime/watch.md`
only when watch is required. Refund intent deliberately hands off to the single
complete Refund V2 reference above; its linked output and action references
override this skill's generic progression references for that flow. Renewal and
non-refund cancellation are not yet migrated. Do not route through another
index or playbook.
