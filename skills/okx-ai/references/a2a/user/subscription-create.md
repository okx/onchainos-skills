# Subscription Creation

Enter from `create-prepare.md` only when the authoritative payload supports a
subscription. Complete `create-guide.md` when a non-blank Guide is present.

## Execution mode

Collect the User's choice of `signal_only` or `guide_direct` without asking for
a separate confirmation. `guide_direct` requires the exact Guide and collected
Guide Consent; an absent Guide permits only `signal_only`. Do not default the
mode or store it in `serviceParams` or Guide Consent. The final card below owns
the one confirmation for Guide Consent, subscription, and payment together.
Do not expose the internal execution-mode value in that card.

## Business data and confirmation

Collect `title` (≤30 characters), Description (≤4096 characters), explicit
Service inputs, and attachments. Use `autoRenew=1` unless the User explicitly
disables it. Render only:

| Field | Value |
|---|---|
| Task Name | title |
| Task Description | confirmed Description |
| Provider | {providerAgentName}（Agent {providerAgentId}） |
| Service Parameters | confirmed `serviceParams` |
| Service Price | exact subscription fee, token, and interval |
| Trial | exact supported positive duration, otherwise No |
| Auto-Renew | On or Off |
| Service Guide Consent | complete collected Guide Consent when the Guide is non-blank |

Render the Provider with both the bound Provider name and Agent ID, exactly as
`{providerAgentName}（Agent {providerAgentId}）`; never render only one of them.
Omit the Service Parameters row when no parameters were collected. Omit the
Service Guide Consent row when the Guide is blank. Do not show a standalone
Guide, execution-mode, or payment confirmation. Continue only after one
explicit final confirmation of the complete card and the one-time communication
check defined by `create.md`. Any edit to a displayed fact invalidates that
confirmation and requires the complete updated card again.

## Persist mode and create

```bash
onchainos agent subscription-execution-config-set \
  --service-id <payload.serviceId> \
  --execution-mode <guide_direct|signal_only>
```

Changing an existing mode requires another confirmation and `--replace`.
Failure blocks creation.

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
