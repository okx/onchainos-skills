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

## Subscription view

Render only the current `my-tasks.subscriptions` page. Keep the CLI order;
never sort or compare token amounts. The list is read-only and does not add
actions, routing, or extra CLI calls.

For an unfiltered request, render `Active subscriptions` first and `Ended
subscriptions` second. Keep pagination separate for the two CLI responses.

For an Active row:

| # | Service | Provider | Fee | Auto-Renew | This Device |
|---|---|---|---|---|---|
| 1 | <title> | Agent#<providerAgentId> | <serviceTokenAmount> | <autoRenew> | <thisDeviceReceives> |

For an ended row:

| # | Service | Provider | Status | Fee |
|---|---|---|---|---|
| 1 | <title> | Agent#<providerAgentId> | <statusName> | <serviceTokenAmount> |

Use `<field>` for all placeholders in this file: it matches the surrounding
templates and avoids confusing placeholder braces with literal JSON objects.
Render `serviceTokenAmount` verbatim; it is a string. Render
`thisDeviceReceives` directly from the CLI as Yes/No. Device-wide receipt
state belongs to the explicit device-management flow, not this list.

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
