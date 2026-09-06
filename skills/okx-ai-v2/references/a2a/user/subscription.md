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

### how to render list
This is a mandatory, exact rendering contract.
For every subscription-list result, render this structure in full.
Never summarize, shorten, reorder.
#### template
```markdown
#### Active Subscriptions ({payload.summary.activeCount})

| # | Job Name | Service Provider | Status | Fee / Month | Next Charge | Auto-renewal | Billing Period | {payload.deviceColumns[].label} |
|---|---|---|---|---|---|---|---|---|
| {n} | {title} | Agent#{providerAgentId} | {statusName} | {feeLabel} | {nextChargeLabel} | {autoRenewLabel} | {billingPeriodLabel} | {deviceReceiptCells[column.key]} |

#### Ended Subscriptions ({payload.summary.endedCount})

| # | Job Name | Service Provider | Status | Fee / Month | Billing Period |
|---|---|---|---|---|---|
| {n} | {title} | Agent#{providerAgentId} | {statusName} | {feeLabel} | {billingPeriodLabel} |

{No Receiver Warning}

{Rendered nextAction list}
```
#### template rules
1. Group rows by `listStatus`, preserving CLI order.
2. For Active rows, render the returned `payload.deviceColumns` in order and
   use each row's `deviceReceiptCells[column.key]` directly. The CLI owns the
   device label, fallback to `deviceId`, and `(This Device)` marker.
3. If device data is unavailable, omit device columns and state that receipt
   status is unavailable.
4. Warn for each Active row with `hasNoReceivingDevices=true`.

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
