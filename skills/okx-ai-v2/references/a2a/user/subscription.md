# My Subscriptions

Browse my subscription task lists or details.

## Commands

| Intent | Reference |
|---|---|
| My subscriptions | `onchainos agent subscription-list --page-size 10` |
| Next page | `onchainos agent subscription-list --cursor <nextCursor> --page-size <pageSize>` |
| Selected subscription detail | `onchainos agent subscribe-detail <jobId> --format json` |

## List

Render only the current `payload.items` page. Keep CLI order and render
`listStatus`, `title`, `providerAgentId`, and `serviceTokenAmount` verbatim.
For Active rows, also render `autoRenew` and `thisDeviceReceives` as Yes/No.

### Constraints

- Subscription tasks are listed Active first, then Ended.
- Use `nextCursor` unchanged to continue the list.

## Detail

Render current fields only: `title`, `jobId`, status, buyer, provider,
`serviceTokenAmount`, period, `autoRenew`, trial window when present,
`offlineReceiveFlag`, `deviceList`, and `thisDeviceReceives`.

Preserve `deviceList`: `null` means all logged-in devices by default, `[]`
means none, and a non-empty array is an explicit allowlist.

### Constraints

- Never infer a `jobId` from a title or prior context.
- Refresh the list only when the selected subscription is no longer available.
