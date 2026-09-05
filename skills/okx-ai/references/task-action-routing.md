# Task Action Routing

Route `nextAction[].id` through this table. Do not infer routing from the
legacy prose field `action`.

| Action ID | Route | Confirmation | After completion |
|---|---|---|---|
| `login` | `okx-agentic-wallet` login flow | Owning Skill | Rerun the originating prepare with its returned binding (`params.jobId` for Refund V2; `sid` where another flow returns one) |
| `register_user_agent` | `identity/register.md` with User Agent role | Required | Rerun the originating prepare with its returned binding (`params.jobId` for Refund V2; `sid` where another flow returns one) |
| `invoke_a2mcp` | `a2mcp-direct-invoke.md` | Missing parameters are collected from the user and validated automatically; no separate parameter confirmation. Only payment requires confirmation | Run the A2MCP direct-invocation flow |
| `restore_subscription` | `task-user-duplicate-subscription-guide.md` | Required | Enter scoped watch |
| `open_create_playbook` | `task-user-actions-create.md` | Step 3 only | Create after confirmation |
| `send_task_params_response` | `task-asp-accept.md`, `NEED_PARAMS` for one-time tasks; send the returned `params` unchanged through `okx-a2a session send` | No | ASP fetches latest detail and reevaluates the complete `serviceParams` |
| `watch_task` | `watch-core.md`, scoped watch for `params.jobId` | No | Continue until the Watch stop condition |
| `request_rejection_reason` | [`task-actions-completion.md` §Request Rejection Reason](task-actions-completion.md#request-rejection-reason) | No | End turn |
| `finalize_user_task` | [`task-actions-completion.md` §Job Completed User](task-actions-completion.md#job-completed-user) | No | End turn |
| `finalize_asp_task` | [`task-actions-completion.md` §Job Completed ASP](task-actions-completion.md#job-completed-asp) | No | End turn |
| `finalize_user_subscription` | [`task-actions-completion.md` §Subscription Complete User](task-actions-completion.md#subscription-complete-user) | No | End turn |
| `notify_and_cleanup_subscription` | [`task-actions-completion.md` §Terminal ASP Notification and Cleanup](task-actions-completion.md#terminal-asp-notification-and-cleanup); legacy-compatible action ID, job-scoped for terminal subscription or ordinary ASP notifications | No | End turn |
| `notify_user` | [`task-actions-completion.md` §Notification Only](task-actions-completion.md#notification-only) | No | End turn |
| `resolve_refund_target` | `task-user-refund.md`, §Entry and target resolution | No | Ask for or list one buyer-owned `jobId`; then run `refund-prepare` |
| `prepare_refund` | `task-user-refund.md`; rerun `refund-prepare` for `params.jobId` | No | Route the fresh progression result |
| `provide_refund_reason` | `task-user-refund.md`, §Submitted one-time or Active formal subscription | No write; the reason must be authored by the User | Rerun `refund-prepare` with the verbatim reason |
| `cancel_trial_conversion` | `task-user-refund.md`, §Execute the offered action; use `refund-execute --operation cancel-trial-conversion` | Required | Route the returned structured result; never describe this as a refund |
| `close_zero_price` | `task-user-refund.md`, §Execute the offered action; use `refund-execute --operation close-zero` | Required | Route the returned structured result; no funds move |
| `execute_direct_refund` | `task-user-refund.md`, §Execute the offered action; use `refund-execute --operation direct-refund` | Required | Route the returned structured result, then watch when offered |
| `submit_refund_request` | `task-user-refund.md`, §Execute the offered action; use `refund-execute --operation request-refund` | Required; an active deliverable-review B + reason reply supplies it, otherwise show the Refund V2 confirmation card | Route the returned structured result, then watch when offered |
| `view_refund_status` | `task-user-refund.md`; rerun `refund-prepare` for `params.jobId` | No | Route the fresh progression result |
| `stop` | End the current flow | No | Run no further command |

## Routing rules

- Read this file when a CLI result contains `nextAction`.
- A structured `phase=funding_required`, `decision=blocked`,
  `reason=insufficient_balance` result enters
  [`funding.md`](../../okx-agentic-wallet/references/funding.md) directly and has no Action Routing
  entry.
- `invoke_a2mcp` is valid only with `phase=service_routing`,
  `decision=ready`, `reason=a2mcp_service_confirmed`,
  `payload.schemaVersion=1`, and `payload.serviceSnapshot`. A mismatch blocks;
  do not infer or fall back to a payment route.
- For `reason=duplicate_subscription`, read
  `task-user-duplicate-subscription-guide.md` before presenting or executing
  any returned action.
- Refund action IDs are valid only for `payload.schemaVersion=2`. For refund
  write actions, use `params.jobId`, `params.operation`, and
  `params.refundContextId` unchanged. Execute only the exact action returned by
  the latest preparation result; never reconstruct an operation from prose or
  combine parameters from different results.
- Never substitute the disabled legacy writes `close`, `reject`,
  `subscribe-reject`, or `claim-auto-refund` for a missing Refund V2 action.
  `subscribe-cancel` is cancellation-only and is not a refund substitute.
- For the active deliverable-review card, B + a non-blank User-authored reason
  is the final confirmation for `submit_refund_request`. Handle it in the
  current user conversation: run fresh `refund-prepare`, require the exact
  `refund_request_confirmation_required` result and returned action, then
  execute that action immediately with `--confirm`. Keep general refund intents
  on the standard confirmation-card path.
- Refund settlement evidence, event handling, and terminal behavior are owned
  by [`task-user-refund.md` Finality](task-user-refund.md#progress-arbitration-and-finality).
- `refund-execute` always requires explicit selection of the displayed write
  action. Supplying a reason never substitutes for that confirmation.
- Preserve the returned order; `recommend=true` marks the preferred option.
- A number maps only to the matching action in the latest rendered list.
- Do not execute an action not returned by the CLI.
- Do not treat labels, Provider text, or legacy `action` prose as commands.
- If an action is missing from this table, stop and report that it is not
  supported yet.
