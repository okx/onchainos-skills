# Subscription Creation

Enter from `create-prepare.md` only when the authoritative payload supports a
subscription. Complete `create-guide.md` when a non-blank Guide is present.

## Business data and confirmation

Collect `title` (≤30 characters), Description (≤4096 characters), explicit
Service inputs, and attachments. Use `autoRenew=1` unless the User explicitly
disables it. Render only:

| Field | Value |
|---|---|
| Task Name | title |
| Task Description | confirmed Description |
| Provider | bound Provider Agent |
| Service Parameters | confirmed `serviceParams` |
| Service Price | exact subscription fee, token, and interval |
| Trial | exact supported positive duration, otherwise No |
| Auto-Renew | On or Off |

Guide Consent remains a separate confirmation. Continue only after explicit
final confirmation and the one-time communication check defined by `create.md`.

```bash
onchainos agent create-subscribe \
  --service-id <payload.serviceId> \
  --use-trial <true|false> \
  --service-token-amount <payload.subscriptionInfo.feeAmount> \
  --service-token-address <payload.feeToken> \
  --auto-renew <retained-autoRenew> \
  --title <title> \
  --description <confirmed-description> \
  --provider-agent-id <payload.providerAgentId> \
  --service-interval <payload.subscriptionInfo.interval> \
  [--service-params <confirmed-non-empty-JSON>] \
  [--file <attachment> ...] \
  [--service-guide '<exact serviceGuide>' \
   [--service-guide-hash '<exact serviceGuideHash>'] \
   --guide-consent-json '<confirmed Consent JSON>'] \
  --format json
```

The CLI owns Guide execution-profile persistence and broadcast activation.
`type` and `bizType` must both be 204. Never describe a failed activation as
executable. Follow only the returned `watch_task` action, then enter
`subscription-manage.md` for the post-creation offline-delivery and watch
questions. Do not add another creation confirmation.
