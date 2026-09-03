# Task Output Templates

Use this reference when a task or subscription command returns the structured
progression fields `phase`, `decision`, `reason`, `nextAction`, and `payload`.
Render the result in the user's language. Treat `action` as legacy guidance,
not as a routing signal.

## Common shell

Use three sections:

```text
[Result]
<one-sentence conclusion>

[Details]
<only the fields needed for this phase>

[Next]
1. <action label>
2. <action label>

Reply with a number.
```

Rules:

- Render every returned `nextAction` item in order; number from 1.
- Mark the item with `recommend=true` as the recommended option in prose only
  when useful; do not change its number or meaning.
- Never invent an action not returned by the CLI.
- Numbers are valid only for the latest response.
- If there is one action, execute it when safe or show it without forcing a
  numbered choice when no user decision is required.
- Do not expose raw JSON, internal phase names, or provider instructions.

## `decision=blocked`

Use a concise status result and a recovery-oriented action list.

```text
[Result]
<operation> cannot continue: <reason message>.

[Details]
Phase: <localized phase>
<relevant payload fields>

[Next]
1. <recommended recovery>
2. <alternative>
3. <cancel or return>

Reply with a number.
```

For `task_create_prepare`, use these reason mappings:

| Reason | Result | Typical action label |
|---|---|---|
| `login_required` | Login is required. | Log in |
| `user_identity_required` | A User Agent is required. | Register User Agent |
| `unsupported_service_type` | This service type is not supported for task creation. | Stop |
| `duplicate_subscription` | An active subscription already exists. | Restore listening / Stop |
| `insufficient_balance` | The balance is insufficient. | Fund account |

Use the exact action IDs returned by `nextAction`; the labels above are display
guidance only.

## `decision=requires_user_input`

```text
[Result]
<operation> needs more information.

[Details]
Missing: <payload.requiredParams>

[Next]
1. Provide the missing information
2. Choose another service
3. Cancel

Reply with a number, or provide the requested values directly.
```

Only ask for fields present in the current payload. Collect all missing required
fields together.

## `decision=ready`

Use a confirmation card for task or subscription creation. The business
reference defines which payload fields are required and how prices, trial, and
auto-renewal are rendered.

```text
[Result]
<operation> is ready for confirmation.

[Details]
| Item | Value |
|---|---|
| Service | <service> |
| Provider | <provider> |
| Parameters | <confirmed parameters> |
| Price | <price> |

[Next]
1. Confirm and continue
2. Modify
3. Cancel

Reply with a number.
```

For `task_create_prepare`, `nextAction.id=open_create_playbook` means to open
the task-creation reference and continue its confirmation flow; it does not
mean that the subscription has already been created.

For `phase=service_routing` and `nextAction.id=invoke_a2mcp`, do not render the
generic task-creation confirmation card above. Open
`a2mcp-direct-invoke.md`. Preserve `payload.serviceSnapshot` verbatim; that
reference owns parameter collection, supported-token and balance display,
funding recovery, and the final mutually exclusive Confirm/Cancel card.

## `task_create_prepare` phase mapping

| Phase | Decision | Next action IDs |
|---|---|---|
| `login_validation` | `blocked` | `login` |
| `identity_validation` | `blocked` | `register_user_agent` |
| `service_validation` | `blocked` | `stop` |
| `service_routing` | `ready` | `invoke_a2mcp` |
| `subscription_validation` | `blocked` | `restore_subscription`, `stop` |
| `payment_validation` | `blocked` | `fund_account` |
| `creation` | `ready` | `open_create_playbook` |

## Subscription completion phase mapping

| Role | Phase | Decision | Reason | Next action IDs |
|---|---|---|---|---|
| User | `subscription_completion` | `ready` | `notification_required` | `finalize_user_subscription` |
| ASP | `subscription_completion` | `ready` | `notification_required` | `notify_and_cleanup_subscription` |

## Task completion phase mapping

| Role | Phase | Decision | Reason | Next action IDs |
|---|---|---|---|---|
| User | `task_completion` | `ready` | `notification_and_rating_required` | `finalize_user_task` |
| ASP | `task_completion` | `ready` | `notification_and_rating_required` | `finalize_asp_task` |

## Deliverable review phase mapping

| Decision | Reason | Next action IDs |
|---|---|---|
| `ready` | `completion_submitted` / `rejection_submitted` | `stop` |
| `blocked` | `legacy_a2mcp_flow_removed` / `completion_failed` / `rejection_failed` | `stop` |
| `requires_user_input` | `rejection_reason_required` | `request_rejection_reason` |
