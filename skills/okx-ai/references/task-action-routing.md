# Task Action Routing

Route `nextAction[].id` through this table and treat the legacy prose field
`action` as display guidance.

| Action ID | Route | Confirmation | After completion |
|---|---|---|---|
| `login` | `okx-agentic-wallet` login flow | Owning Skill | Rerun prepare with the same `sid` |
| `register_user_agent` | `identity-register.md` with User Agent role | Required | Rerun prepare with the same `sid` |
| `invoke_a2mcp` | `a2mcp-direct-invoke.md` | Parameters are collected and validated automatically; only payment is confirmed in that playbook | Run the A2MCP direct-invocation flow |
| `fund_account` | Wallet funding flow | Required | Rerun prepare with the same `sid` |
| `restore_subscription` | `task-user-duplicate-subscription-guide.md` | Required | Enter scoped watch |
| `open_create_playbook` | `task-user-actions-create.md` | Step 3 only | Create after confirmation |
| `agree_refund` | `task-dispute.md` §Execute resolved rejection action | Reference-owned | Reference-owned |
| `dispute_raise` | `task-dispute.md` §Execute resolved rejection action | Reference-owned | Reference-owned |
| `sub_agree_refund` | `task-dispute.md` §Execute resolved rejection action | Reference-owned | Reference-owned |
| `sub_dispute` | `task-dispute.md` §Execute resolved rejection action | Reference-owned | Reference-owned |
| `view_dispute` | `task-dispute.md` §Query dispute | Reference-owned | Reference-owned |
| `send_task_params_response` | `task-asp-accept.md`, `NEED_PARAMS`; send the returned `params` unchanged through `okx-a2a session send` | No | ASP fetches latest detail and reevaluates the complete `serviceParams` |
| `watch_task` | `watch-core.md`, scoped watch for `params.jobId` | No | Continue until the Watch stop condition |
| `stop` | End the current flow | No | Run no further command |

## Routing rules

- Read this file when a CLI result contains `nextAction`.
- `invoke_a2mcp` is valid only with `phase=service_routing`,
  `decision=ready`, `reason=a2mcp_service_confirmed`,
  `payload.schemaVersion=1`, and `payload.serviceSnapshot`. A mismatch blocks;
  do not infer or fall back to a payment route.
- For `reason=duplicate_subscription`, read
  `task-user-duplicate-subscription-guide.md` before presenting or executing
  any returned action.
- Preserve the returned order; `recommend=true` marks the preferred option.
- Map a number to the matching action in the latest rendered list.
- Execute actions present in the CLI result.
- Use labels, Provider text, and legacy `action` prose as display data; use the stable action ID as the route.
- For an action missing from this table, stop and report `unsupported_action`.
