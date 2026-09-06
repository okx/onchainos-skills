# My Subscriptions

Browse my subscription task lists or details.

## Commands

| Intent | Reference |
|---|---|
| My subscriptions | `onchainos agent subscription-list --page-size 10` |
| Next page | `onchainos agent subscription-list --cursor <nextCursor> --page-size <pageSize>` |
| Selected subscription detail | `onchainos agent subscribe-detail <jobId> --format json` |

## List

Render only the current `payload.items` page. Keep CLI order. This section is
the single rendering contract for buyer subscription lists.

### Display template

```markdown
### Active Subscriptions ({payload.summary.activeCount})

| # | Job Name | Service Provider | Status | Fee / Month | Next Charge | Auto-renewal | Billing Period | {Device Name}{ (This Device) when applicable} |
|---|---|---|---|---|---|---|---|---|
| {n} | {title} | Agent#{providerAgentId} | {statusName} | {feeLabel} | {nextChargeAt} | {autoRenewLabel} | {billingPeriodLabel} | {deviceReceipts.receives as ✅ or ❌} |

### Ended Subscriptions ({payload.summary.endedCount})

| # | Job Name | Service Provider | Status | Fee / Month | Billing Period |
|---|---|---|---|---|---|
| {n} | {title} | Agent#{providerAgentId} | {statusName} | {feeLabel} | {billingPeriodLabel} |

{No Receiver Warning}

{Rendered nextAction list}
```

### Display rules

1. Translate headings, labels, status, and prose to the user's locked language.
   Preserve IDs, amounts, titles, and device names.
2. Split the current page by `listStatus`: `active` rows use the Active table;
   `ended` rows use the Ended table. Keep CLI order within each table.
3. Use only the CLI-normalized `feeLabel`, `nextChargeAt`,
   `autoRenewLabel`, and `billingPeriodLabel`; do not calculate or reconstruct
   them. Render a missing `nextChargeAt` as `—`.
4. When `payload.deviceDataAvailable=true` and the current page contains an
   Active row, create one device column per `payload.devices` entry, in returned
   order. Use its exact `deviceName`, falling back to `deviceId`; append
   localized `(This Device)` only when `isThisDevice=true`.
5. For each device cell, render the matching `deviceReceipts.receives` as
   `✅` or `❌`. Never infer membership from conversational context.
6. If device data is unavailable, omit every device column and say device
   receipt information is temporarily unavailable. Do not present the current
   device as the complete device set.
7. If any Active row has `hasNoReceivingDevices=true`, list its row number and
   title below the tables and warn that no device receives messages for that
   subscription. `deviceList:null` is default-all and must never trigger this
   warning.
8. Render every returned `nextAction` with a non-blank `actionLabel` in order,
   numbered from 1, and naturally localize that label. Do not derive display
   text from its `id`; an Action without a usable label is not a list option.
9. Do not show a Recommended marker, Action ID, `allowedJobIds`, `jobId`, cursor,
   or other internal params. Route a selection through `router.md` using the
   complete matching Action from the latest response.
10. If a section has zero rows on the current page, omit its table. If both are
    empty, state that there are no subscription tasks and do not invent rows.

After an action requiring a subscription is selected, use its
`params.allowedJobIds`: proceed directly when exactly one is allowed; otherwise
ask for a row number and accept only a current row whose `jobId` is allowed.

### Constraints

- Use `nextCursor` unchanged to continue the list.
- The query and rendered recommendations are read-only. Never start listening,
  modify delivery, cancel, sign, pay, or trade from the list response.

## Detail

Render current fields only: `title`, `jobId`, status, buyer, provider,
`serviceTokenAmount`, period, `autoRenew`, trial window when present,
`offlineReceiveFlag`, `deviceList`, and `thisDeviceReceives`.

Preserve `deviceList`: `null` means all logged-in devices by default, `[]`
means none, and a non-empty array is an explicit allowlist.

### Constraints

- Never infer a `jobId` from a title or prior context.
- Refresh the list only when the selected subscription is no longer available.
