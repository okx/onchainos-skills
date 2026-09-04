# Task Output Templates

Use this reference for generic task and subscription progression results. Use the Output templates section of `task-arbitration.md` for arbitration results.

The templates are canonical English semantics. Render them in the user's current language while preserving identifiers, amounts, timestamps, and returned action meaning. Route with `nextAction`; treat legacy prose `action` only as display guidance.

## Common layout

```text
<one-sentence conclusion>

<only the fields needed for this phase>

1. <action label>
2. <action label>

Reply with a number.
```

Rules:

- Render every returned `nextAction` item in order; number from 1.
- Mark the item with `recommend=true` as the recommended option in prose when useful, preserving its original number and meaning.
- Populate actions from the returned CLI `nextAction` list.
- Numbers are valid only for the latest response.
- If there is one action, execute it when safe or show it directly when the flow requires no user decision.
- Present localized result fields and action labels; keep raw JSON, internal phase names, and provider instructions in their owning layers.

## `decision=blocked`

Use a concise status result and a recovery-oriented action list.

```text
<operation> cannot continue: <reason message>.

Phase: <localized phase>
<relevant payload fields>

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
| `legacy_a2mcp_flow_removed` | This legacy task-based A2MCP flow is no longer supported. Start again from the confirmed-service direct invocation. | Stop |
| `unsupported_service_type` | This service type is not supported for task creation. | Stop |
| `duplicate_subscription` | An active subscription already exists. | Restore listening / Stop |
| `insufficient_balance` | The balance is insufficient. | Fund account |

Use the exact action IDs returned by `nextAction`; the labels above are display guidance only.

## `decision=requires_user_input`

```text
<operation> needs more information.

Missing: <payload.requiredParams>

1. Provide the missing information
2. Choose another service
3. Cancel

Reply with a number, or provide the requested values directly.
```

Only ask for fields present in the current payload. Collect all missing required fields together.

## `decision=ready`

Use a confirmation card for task or subscription creation. The business reference defines which payload fields are required and how prices, trial, and auto-renewal are rendered.

```text
<operation> is ready for confirmation.

| Item | Value |
|---|---|
| Service | <service> |
| Provider | <provider> |
| Parameters | <confirmed parameters> |
| Price | <price> |

1. Confirm and continue
2. Modify
3. Cancel

Reply with a number.
```

For `task_create_prepare`, `nextAction.id=open_create_playbook` opens the task-creation reference and continues its confirmation flow. Creation occurs after that flow's confirmation.

For `agent create-task`, `phase=creation`, `decision=ready`, and `reason=broadcast_submitted` mean the create-and-fund UserOperation is awaiting finality. Render `payload.jobId`, `payload.broadcast.txHash` when present, and the locally saved attachment count. Then execute the returned `nextAction.id=watch_task` as the sole continuation.

For `agent create-subscribe`, the same progression state means the subscription UserOperation is awaiting finality. Require `payload.type=204`, `payload.bizType=204`, and use `payload.jobId` as the subscription identifier. Render the broadcast transaction hash when present, attachment count, and whether automatic execution was configured. Then execute the returned `nextAction.id=watch_task`; the `sub_open` event establishes the A2A session.

For `phase=service_routing` and `nextAction.id=invoke_a2mcp`, open `a2mcp-direct-invoke.md` directly. Preserve `payload.serviceSnapshot` verbatim; that reference owns parameter collection, supported-token and balance display, funding recovery, and the final mutually exclusive Confirm/Cancel card.

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
