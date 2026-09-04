# Duplicate Subscription Guide

Use this guide only when `reason=duplicate_subscription`.

## Input

Require non-empty `payload.jobId` and `payload.title`, numeric `payload.status`,
and boolean `payload.active`. Missing or invalid fields are a hard stop; never
guess the subscription from history or run another task list.

Map `payload.status` for display: `-1=INIT`, `1=ACTIVE`, `3=REJECTED`,
`4=DISPUTED`, `6=COMPLETED`, `7=CLOSED`, `8=EXPIRED`, and `9=FAILED`. Render any other value
as `UNKNOWN_<status>`. Use the mapped value as `<statusName>` below without
changing the payload.

## Routing

- `active=true`: render this pattern in the user's language, substituting the
  payload values. Use exactly:
  `A subscription task for this service already exists. Job ID: <jobId>. Task name: <title>. Status: <statusName>. Another subscription cannot be created. Restore listening?`
  Offer only the returned `nextAction` entries and wait for an explicit choice.
  - `restore_subscription`: retain `payload.jobId` as the explicit current
    subscription and enter `task-user-playbook.md` **Signal-receipt watch
    entry**. This is receipt-only restoration, so its first authorization gate
    omits `--review-existing`.
  - `stop`: end the current flow without creating or watching a subscription.
- `active=false`: render this pattern in the user's language, substituting the
  payload values. Use exactly:
  `A subscription task for this service already exists. Job ID: <jobId>. Task name: <title>. Status: <statusName>. Another subscription cannot be created.`
  Do not enter watch; only `stop` is valid.

Do not add a separate `userFacingPrompt` field and do not omit the task name or
status from the rendered message.
