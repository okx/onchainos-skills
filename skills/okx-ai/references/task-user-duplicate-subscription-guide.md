# Duplicate Subscription Guide

Use this guide only when `reason=duplicate_subscription`.

## Input

Require `payload.jobId` as a non-empty string and `payload.active` as a
boolean. Missing or invalid fields are a hard stop; never guess the
subscription from history or run another task list.

## Routing

- `active=true`: tell the user that this Service is already subscribed and ask
  whether to restore listening. Offer only the returned `nextAction` entries
  and wait for an explicit choice.
  - `restore_subscription`: retain `payload.jobId` as the explicit current
    subscription and enter `task-user-playbook.md` **Signal-receipt watch
    entry**. This is receipt-only restoration, so its first authorization gate
    omits `--review-existing`.
  - `stop`: end the current flow without creating or watching a subscription.
- `active=false`: tell the user that this Service already has a subscription
  and cannot be subscribed to again. Do not enter watch; only `stop` is valid.
