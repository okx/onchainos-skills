# Duplicate Subscription Guide

Use this guide only when `reason=duplicate_subscription`.

## Input

Require non-empty `payload.jobId`, `payload.title`, `payload.statusLabel`, and
`payload.statusDescription`, plus boolean `payload.active`. Missing or invalid
fields are a hard stop; never guess the subscription from history or run another
task list. The CLI maps the server state to the readable fields; translate them
into the conversation language and do not display a raw numeric `payload.status`.

## Routing

- `active=true`: render this pattern in the user's language, substituting the
  payload values. Use exactly:
  `A subscription task for this service already exists. Job ID: <jobId>. Task name: <title>. Status: <localizedStatusLabel>. Status description: <localizedStatusDescription>. Another subscription cannot be created. Restore listening?`
  Offer only the returned `nextAction` entries and wait for an explicit choice.
  - `restore_subscription`: retain `payload.jobId` as the explicit current
    subscription and enter `subscription-manage.md` **Signal-receipt watch
    entry**. This is receipt-only restoration, so its first authorization gate
    omits `--review-existing`.
  - `stop`: end the current flow without creating or watching a subscription.
- `active=false`: render this pattern in the user's language, substituting the
  payload values. Use exactly:
  `A subscription task for this service already exists. Job ID: <jobId>. Task name: <title>. Status: <localizedStatusLabel>. Status description: <localizedStatusDescription>. Another subscription cannot be created.`
  Do not enter watch; only `stop` is valid.

Do not add a separate `userFacingPrompt` field and do not omit the task name or
status from the rendered message.
