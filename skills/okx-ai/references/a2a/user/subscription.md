# My Subscriptions

Browse my subscription task lists or details.

## Commands

| Intent | Reference |
|---|---|
| My subscriptions | `onchainos agent subscription-list --page-size 10` |
| Next page | `onchainos agent subscription-list --cursor <nextCursor> --page-size <pageSize>` |
| Selected subscription detail | `onchainos agent subscribe-detail <jobId> --format json` |

## Status-query handoff

Use this entry only when `../task-query.md` has type-gated its fresh lifecycle
or status result as `subscription`. Render only the current user-facing status
facts already present in that result, in the conversation language, then end.
Do not run `subscription-list`, `subscribe-detail`, or a one-time timeline for
this handoff.

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
| {n} | {title} | Agent#{providerAgentId} | {localizedStatusLabel} | {feeLabel} | {nextChargeLabel} | {autoRenewLabel} | {billingPeriodLabel} | {deviceReceiptCells[column.key]} |
{End when activeRows is non-empty}

{When endedRows is non-empty}
#### Ended Subscriptions ({payload.summary.endedCount})

| # | Job Name | Service Provider | Status | Fee / Month | Billing Period |
|---|---|---|---|---|---|
| {n} | {title} | Agent#{providerAgentId} | {localizedStatusLabel} | {feeLabel} | {billingPeriodLabel} |
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
6. Translate each row's CLI-provided `statusLabel` into the user's language.

### Constraints

- Use `nextCursor` unchanged to continue the list.
- The query and rendered recommendations are read-only. Never start listening,
  modify delivery, cancel, sign, pay, or trade from the list response.

## Detail

Render current fields only as a single-record field list:

```markdown
### Subscription Details

- Job Name: {title}
- Job ID: {jobId}
- Status: {localizedStatusLabel}
- Status Description: {localizedStatusDescription}
- Buyer: Agent#{buyerAgentId}
- Service Provider: Agent#{providerAgentId}
- Fee: {serviceTokenAmount}
- Current Period: {currentPeriod}
- Auto-Renewal: {autoRenewLabel}
- Trial Window: {trialWindow}
- Offline Receipt: {offlineReceiveFlag}
- Receiving Devices: {deviceList}
- This Device Receives: {thisDeviceReceives}
```

Render available optional items. Preserve `deviceList`: `null` means all
logged-in devices by default, `[]` means none, and a non-empty array is an
explicit allowlist.

Translate `statusLabel` and `statusDescription` into the conversation language.
For status code `9`, use `Refund completed` and its matching success
description before translating both.

### Constraints

- Never infer a `jobId` from a title or prior context.
- Refresh the list only when the selected subscription is no longer available.
