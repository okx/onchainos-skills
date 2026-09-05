# A2A User Router

| Intent | Reference |
|---|---|
| Request a refund, reject a paid deliverable, check refund eligibility/status, or interpret a refund result | `../../../../okx-ai/references/task-user-refund.md` |
| Create, view, or manage a one-time job | `job.md` |
| Change a task's visibility | `visibility.md` |
| View my subscription tasks | `subscription.md` |
| Manage which devices receive messages for a subscription task | `receipt.md` |
| Pause, resume, or change execution policy | `execution-policy.md` |

Select one file and stop routing. `receipt.md` controls device delivery only;
it does not start watch. Refund intent deliberately hands off to the single
complete Refund V2 reference above; its linked output and action references
override this skill's generic progression references for that flow. Renewal and
non-refund cancellation are not yet migrated. Do not route through another
index or playbook.
