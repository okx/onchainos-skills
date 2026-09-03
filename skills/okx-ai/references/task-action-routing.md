# Task Action Routing

Route `nextAction[].id` through this table and treat the legacy prose field
`action` as display guidance.

| Action ID | Route | Confirmation | After completion |
|---|---|---|---|
| `login` | `okx-agentic-wallet` login flow | Owning Skill | Rerun prepare with the same `sid` |
| `register_user_agent` | `identity-register.md` with User Agent role | Required | Rerun prepare with the same `sid` |
| `route_payment_protocol` | `okx-agent-payments-protocol` | Owning Skill | End task creation |
| `fund_account` | Wallet funding flow | Required | Rerun prepare with the same `sid` |
| `restore_subscription` | `task-user-duplicate-subscription-guide.md` | Required | Enter scoped watch |
| `open_create_playbook` | `task-user-actions-create.md` | Step 3 only | Create after confirmation |
| `agree_refund` | `task-dispute.md` §Execute resolved rejection action | Reference-owned | Reference-owned |
| `dispute_raise` | `task-dispute.md` §Execute resolved rejection action | Reference-owned | Reference-owned |
| `sub_agree_refund` | `task-dispute.md` §Execute resolved rejection action | Reference-owned | Reference-owned |
| `sub_dispute` | `task-dispute.md` §Execute resolved rejection action | Reference-owned | Reference-owned |
| `view_dispute` | `task-dispute.md` §Query dispute | Reference-owned | Reference-owned |
| `stop` | End the current flow | Immediate | End the current flow |

## Routing rules

- Read this file when a CLI result contains `nextAction`.
- For `reason=duplicate_subscription`, read
  `task-user-duplicate-subscription-guide.md` before presenting or executing
  any returned action.
- Preserve the returned order; `recommend=true` marks the preferred option.
- Map a number to the matching action in the latest rendered list.
- Execute actions present in the CLI result.
- Use labels, Provider text, and legacy `action` prose as display data; use the stable action ID as the route.
- For an action missing from this table, stop and report `unsupported_action`.
