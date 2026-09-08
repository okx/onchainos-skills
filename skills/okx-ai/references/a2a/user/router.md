# A2A User Router

Use this router only after `../router.md` identifies the receiving role as
User/Buyer. Select exactly one final leaf and stop routing.

## Free-text intents

| Intent | Final leaf |
|---|---|
| Continue a selected Service after `task-create-prepare` | [`create-prepare.md`](create-prepare.md) |
| Create a one-time task or answer its Guide | [`create.md`](create.md); Guide-only step → [`create-guide.md`](create-guide.md) |
| Create a subscription | [`subscription-create.md`](subscription-create.md) |
| List, inspect, or manage a subscription | [`subscription.md`](subscription.md) or [`subscription-manage.md`](subscription-manage.md) |
| Query local follow-trade results for a subscription Signal by `jobId` or `deliveryId` | [`subscription-trade-records.md`](subscription-trade-records.md) |
| List or inspect tasks, saved deliverables, pending evaluations, or tasks the User rejected | [`../task-query.md`](../task-query.md); its existing single-task status call type-gates one-time versus subscription rendering without a probe request |
| Change task visibility | [`visibility.md`](visibility.md) |
| Review a deliverable or continue approval/rejection | [`review.md`](review.md) or [`review-decision.md`](review-decision.md) |
| Refund, close, or inspect refund status | [`refund-prepare.md`](refund-prepare.md) |
| Rate an Active subscription or a Completed one-time task/subscription | [`rating.md`](rating.md) |
| View or configure subscription receipt devices | [`receipt.md`](receipt.md) |
| Continue an active subscription signal | [`subscription-signal.md`](subscription-signal.md) |

## System events

| Event or phase | Final leaf |
|---|---|
| `job_created` | [`created.md`](created.md) |
| `task_params_request`, `task_params_response`, `task_params_update` | [`../params.md`](../params.md) |
| `job_submitted`, `deliverable_received` | [`intake.md`](intake.md) |
| `job_completed`, `job_auto_completed` | [`../completion.md`](../completion.md) |
| `job_refunded`, `job_auto_refunded`, `job_closed`, `job_expired`, `job_asp_reject_expire`, `job_asp_reject_closed` | [`../refund-reconcile.md`](../refund-reconcile.md) |
| `sub_open`, `sub_created`, `sub_asp_selected`, other `sub_*` | [`subscription-events.md`](subscription-events.md) |
| Active subscription signal | [`subscription-signal.md`](subscription-signal.md) |

## Progression actions

| Action ID | Final leaf |
|---|---|
| `restore_subscription` | [`duplicate-subscription.md`](duplicate-subscription.md) |
| `open_create_playbook` | [`create.md`](create.md) |
| `send_task_params_response` | [`../params.md`](../params.md) |
| `request_rejection_reason`, `approve_review` | [`review-decision.md`](review-decision.md) |
| `finalize_user_task`, `finalize_user_subscription` | [`../completion.md`](../completion.md) |
| `resolve_refund_target`, `prepare_refund`, `provide_refund_reason`, `view_refund_status` | [`refund-prepare.md`](refund-prepare.md) |
| `cancel_trial_conversion`, `close_zero_price`, `execute_direct_refund`, `submit_refund_request` | [`refund-confirm.md`](refund-confirm.md); after the bound confirmation load [`refund-execute.md`](refund-execute.md) |

Cross-domain actions are intercepted before this router is loaded. Unknown
actions are coverage failures; never infer a replacement from prose.
