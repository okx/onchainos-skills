# Subscription Creation

Enter from `create-prepare.md` only when the authoritative payload supports a
subscription. Complete `create-guide.md` when a non-blank Guide is present.
Retain the collected Guide Consent for the final card below without asking for
a separate confirmation.

## Business data and confirmation

Collect `title` (≤30 characters), Description (≤4096 characters), explicit
Service inputs, and attachments. Use `autoRenew=1` unless the User explicitly
disables it. Render this single-subscription confirmation as a field list:

```markdown
### Subscription Creation Confirmation

- Task Name: {title}
- Task Description: {confirmedDescription}
- Provider: {providerAgent}
- Service Parameters: {serviceParams}
- Service Price: {feeAmount} {feeTokenSymbol} / {interval}
- Trial: {trialDurationOrNo}
- Auto-Renew: {OnOrOff}
- Service Guide Consent: {guideConsent}
```

Render the Service Parameters item for confirmed parameters. Render attachments
below the field list. Omit the Service Guide Consent item when the Guide is
blank; otherwise preserve every collected Guide field and User-authored value
without rewriting them. Do not show a standalone Guide, execution-mode, or
payment confirmation. This card owns the one explicit final confirmation for
the subscription, displayed payment, and exact Guide Consent. Any edit to a
displayed fact or Guide answer invalidates that confirmation and requires the
complete updated card again.

Continue only after that final confirmation and the one-time communication
check defined by `create.md`.

## Create subscription

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
