# My Subscriptions

Browse my subscription task lists or details.

## Status-query handoff

Enter here from [`../task-query.md`](../task-query.md) only when its single
existing `agent status` call identifies `Task type: subscription` or structured
`payload.jobType=1`. Consume that same result; do not call `subscription-list`
or `subscribe-detail` again merely to answer the status query.

Render the returned title, full Job ID, provider, fee, status, and description
as a subscription status card. Use the CLI-normalized subscription status
verbatim. End after the read-only result; missing subscription-management facts
such as billing period, auto-renewal, or receipt devices are omitted rather
than fetched or inferred.

## Commands

| Intent | Reference |
|---|---|
| My subscriptions | `onchainos agent subscription-list --page-size 10` |
| Next page | `onchainos agent subscription-list --cursor <nextCursor> --page-size <pageSize>` |
| Selected subscription detail | `onchainos agent subscribe-detail <jobId> --format json` |

## List

Render only the current `payload.items` page. Keep CLI order. This section is
the single rendering contract for buyer subscription lists.

### How to render list
This is a mandatory, exact rendering contract.
For every subscription-list result, render every non-empty section below.
Never summarize, shorten, reorder rows.
Always respond and render all user-facing content in the language currently used by the user.
#### template
```markdown
{When activeRows is non-empty}
#### Active Subscriptions ({payload.summary.activeCount})

| # | Job Name | Service Provider | Status | Fee / Month | Next Charge | Auto-renewal | Billing Period | {payload.deviceColumns[].label} |
|---|---|---|---|---|---|---|---|---|
| {n} | {title} | Agent#{providerAgentId} | {statusName} | {feeLabel} | {nextChargeLabel} | {autoRenewLabel} | {billingPeriodLabel} | {deviceReceiptCells[column.key]} |
{End when activeRows is non-empty}

{When endedRows is non-empty}
#### Ended Subscriptions ({payload.summary.endedCount})

| # | Job Name | Service Provider | Status | Fee / Month | Billing Period |
|---|---|---|---|---|---|
| {n} | {title} | Agent#{providerAgentId} | {statusName} | {feeLabel} | {billingPeriodLabel} |
{End when endedRows is non-empty}

{When both activeRows and endedRows are empty}
No subscriptions found.
{End when both activeRows and endedRows are empty}

{No Receiver Warning}

#### Next steps：
{Rendered nextAction list}
```
#### template rules
1. Group rows by `listStatus`, preserving CLI order.
2. Render the Active Subscriptions heading and table only when `activeRows` is
   non-empty. Render the Ended Subscriptions heading and table only when
   `endedRows` is non-empty. If both groups are empty, render only `No subscriptions found.`
3. For Active rows, render the returned `payload.deviceColumns` in order and
   use each row's `deviceReceiptCells[column.key]` directly. The CLI owns the
   device label, fallback to `deviceId`, and `(This Device)` marker.
4. If device data is unavailable, omit device columns and state that receipt
   status is unavailable.
5. Warn for each Active row with `hasNoReceivingDevices=true`.

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
