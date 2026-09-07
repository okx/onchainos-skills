# My Subscriptions

Browse my subscription task lists or details.

## Commands

| Intent | Reference |
|---|---|
| My subscriptions | `onchainos agent subscription-list --page-size 10` |
| Next page | `onchainos agent subscription-list --cursor <nextCursor> --page-size <pageSize>` |
| Selected subscription detail | `onchainos agent subscribe-detail <jobId> --format json` |

## List

scene: Active subscription list

display template:

```markdown
#### Active Subscriptions ({payload.summary.activeCount})

| # | Job Name | Service Provider ID | Status | Subscription Fee | Next Charge | Auto-renewal | Billing Period | {payload.deviceColumns[].label} |
|---|---|---|---|---|---|---|---|---|
| {n} | {title} | {providerAgentId} | {statusName} | {feeLabel} | {nextChargeLabel} | {autoRenewLabel} | {billingPeriodLabel} | {deviceReceiptCells[column.key]} |

#### Ended Subscriptions ({payload.summary.endedCount})

| # | Job Name | Service Provider ID | Status | Subscription Fee | Billing Period |
|---|---|---|---|---|---|
| {n} | {title} | {providerAgentId} | {statusName} | {feeLabel} | {billingPeriodLabel} |

{No Receiver Warning}

You can ask me to view subscription details, adjust receiving devices, or cancel a subscription.
```

display rules:

1. Render the list only when `payload.displayReady=true`. Otherwise state the exact `payload.displayMissingFeeJobIds` and do not infer a Fee.
2. Group rows by `listStatus`, preserving CLI order.
3. Number the current page from 1. Use the full returned Service Provider ID.
4. Use `feeLabel`, `nextChargeLabel`, `autoRenewLabel`, and `billingPeriodLabel` directly. Do not recalculate them.
5. For Active rows, render `payload.deviceColumns` in order and use `deviceReceiptCells[column.key]` directly.
6. Omit device columns when there are no Active rows. If device data is unavailable, omit them and state that receipt status is unavailable.
7. Warn for every Active row with `hasNoReceivingDevices=true` that it has no receiving device and will not receive subscription messages.
8. Use `nextCursor` unchanged to continue the list.
9. Recommendations are read-only until the User selects an exact Job ID and action. Never modify delivery or cancel from the list response alone.

## Detail

scene: Subscription details

display template:

```markdown
### Subscription Details

| Job Name | Job ID | Status | User | Service Provider | Free Trial | Fee | Auto-renewal | Billing Period | Offline Message Handling | Receive on This Device |
|---|---|---|---|---|---|---|---|---|---|---|
| {title} | {jobId} | {statusName} | {buyerAgentId} | {serviceProviderLabel} | {freeTrialLabel} | {feeLabel} | {autoRenewLabel} | {billingPeriodLabel} | {offlineMessageHandlingLabel} | {receiveOnThisDeviceLabel} |

Subscription messages are delivered to the background process first.

{Next Action}
```

display rules:

1. Render only when `displayReady=true`. If false, state the exact `displayMissingFields` and do not infer them.
2. Preserve the full Job ID, User Agent ID, and Service Provider ID. Use `serviceProviderLabel` directly.
3. Use all CLI-provided labels directly. Do not infer trial eligibility, calculate the first charge time, resolve token symbols, or derive device state in the Skill.
4. Preserve `deviceList`: `null` means all logged-in devices by default, `[]` means none, and a non-empty array is an explicit allowlist.
5. If this conversation is not currently listening for the selected subscription, render: `To receive subscription messages in this conversation, ask me to start listening.`
6. If the selected subscription is already being listened to in this conversation, render: `To view the latest signals, ask me to show the latest subscription messages.`
7. If `copyTrade=1`, append: `To view copy-trade status, ask me to show the current copy-trade status.`
8. Never infer a Job ID from a title or prior context. Refresh the list only when the selected subscription is unavailable.
