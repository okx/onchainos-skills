# Task Output Templates

Use this reference when a task or subscription command returns the structured
progression fields `phase`, `decision`, `reason`, `nextAction`, and `payload`.
Render the result in the user's language. Treat `action` as legacy guidance,
not as a routing signal.

## Task creation insufficient balance

Match this business-owned template before the generic blocked shell:

```text
phase=payment_validation
decision=blocked
reason=insufficient_balance
payload.fundingNeed is present
```

```text
The task cannot be created because the {payload.fundingNeed.asset} balance is insufficient.

Current balance: {payload.fundingNeed.balance} {payload.fundingNeed.asset}
Required: {payload.fundingNeed.required} {payload.fundingNeed.asset}
Shortfall: {payload.fundingNeed.shortfall} {payload.fundingNeed.asset}

Fund the account before continuing task creation?
{render only the returned fund_account action label}
```

All balance fields come directly from `fundingNeed`. Do not display
`fundingTarget` or `qr` before the user selects `fund_account`; after selection,
route to the shared Funding Reference and its common address + QR template.

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
| `a2mcp_service` | This is an A2MCP service, not an A2A task. | Route to payment protocol |
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

When `payload.paymentBalance` is present, show its `available` total together
with `currency` before the fresh creation confirmation. Do not label the
difference from an earlier warning as “received” because the CLI does not
return a verified deposit delta.

## `task_create_prepare` phase mapping

| Phase | Decision | Next action IDs |
|---|---|---|
| `login_validation` | `blocked` | `login` |
| `identity_validation` | `blocked` | `register_user_agent` |
| `service_validation` | `blocked` | `route_payment_protocol`, `stop` |
| `subscription_validation` | `blocked` | `restore_subscription`, `stop` |
| `payment_validation` | `blocked` | `fund_account` |
| `creation` | `ready` | `open_create_playbook` |
