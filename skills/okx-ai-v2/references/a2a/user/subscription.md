# My Subscriptions

Read-only subscription list and detail. Device delivery, execution policy,
renewal, cancellation, and refunds are outside this flow.

## Commands

| Intent | Command |
|---|---|
| My subscriptions | Read Active, then Ended. |
| Active subscriptions | `onchainos agent my-tasks --task-type subscription --status-type 1 --page 1` |
| Ended subscriptions | `onchainos agent my-tasks --task-type subscription --status-type 2 --page 1` |
| Selected subscription detail | `onchainos agent subscribe-detail <jobId> --format json` |

For an unfiltered request, keep Active and Ended pagination separate. If both
sections have a next page, ask which section to continue. If `my-tasks` returns
`errorCode=user_identity_required`, render its returned
`register_user_identity` next step and stop. Do not fall back to another role
or cached rows.

## List

Render only the current `my-tasks.subscriptions` page. Keep CLI order and do
not make extra queries. Render `serviceTokenAmount` verbatim; it is a string.
Retain the returned `jobId` for selection only; never infer it from a title,
index, or conversation history.

| Status | Fields |
|---|---|
| Active | `title`, `providerAgentId`, `serviceTokenAmount`, `autoRenew`, `thisDeviceReceives` |
| Ended | `title`, `providerAgentId`, `statusName`, `serviceTokenAmount` |

Render `thisDeviceReceives` as Yes/No. An empty section has no actions.

For an unfiltered request, render `Active subscriptions` before `Ended
subscriptions`. Preserve each section's `page`, `pageSize`, and `hasNext`.
Only a selected row may open detail; the list itself does not start receipt,
watch, or execution flows.

## Detail

Call `subscribe-detail` only with a `jobId` from the selected current list row.
If selection or detail lookup fails, refresh the list and ask the user to
select a row.

Render current fields only: `title`, `jobId`, status, buyer, provider,
`serviceTokenAmount`, period, `autoRenew`, trial window when present,
`offlineReceiveFlag`, `deviceList`, and `thisDeviceReceives`.

Preserve `deviceList`: `null` means all logged-in devices by default, `[]`
means none, and a non-empty array is an explicit allowlist. A detail read never
authorizes a write. Non-Active subscriptions expose no delivery, signal, or
execution action.

## Boundaries

- Delivery or offline handling: read `receipt.md` only for a new explicit request.
- Execution policy: read `execution-policy.md` only for a new explicit request.
- On read failure, do not present cached data as current.

## Wallet login handoff

Wallet login owns this entry. Its successful poll may provide
`data.postLoginSubscriptions.activeSubscriptionCount`.

- If absent or zero, render nothing about OKX.AI subscriptions and issue no
  follow-up subscription or device query.
- If positive, render one localized hint: `You have <count> active subscription
  task(s). Say “view my subscriptions” to inspect them.`
- This hint does not enter the free-text router, select a subscription, start
  receipt/watch, or change a device or execution policy.
