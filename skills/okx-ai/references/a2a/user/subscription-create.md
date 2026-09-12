# Subscription Creation

Enter from `create-prepare.md` only when the authoritative payload supports a
subscription. Complete `create-guide.md` when a non-blank Guide is present.
Retain the collected Guide Consent for the final card below without asking for
a separate confirmation. This is the only creation leaf that configures
automatic copy-trading; one-time tasks never configure it.

## Automatic copy-trading preference

For a subscription with a Guide, `create-guide.md` saves the User's explicit
automatic copy-trading answer as soon as that Guide field is answered. Do not
ask an additional platform-level mode question or save it again here. If the
Guide did not collect an explicit answer, do not create the subscription.

## Business data and confirmation

Derive `title` (≤30 characters) and Description (≤4096 characters) from the
selected Service and the User's confirmed Guide answers. They are required
subscription fields, not ASP configuration and not merely a local record. Do
not ask the User to supply them when they can be derived; ask only when a
required value is ambiguous or cannot be formed safely. Collect explicit
Service inputs and attachments. Use `autoRenew=1` unless the User explicitly
disables it. Render this single-subscription confirmation as a field list:

```markdown
### Subscription Creation Confirmation

- Subscription Name: {title}
- Subscription Description: {confirmedDescription}
- Provider: {providerAgent}
- Service Price: {feeAmount} {feeTokenSymbol} / {interval}
- Trial: {trialDurationOrNo}
- Auto-Renew: {OnOrOff}
- Service Guide Consent: {guideConsent} *(only when the Guide is non-blank)*
```

Render attachments below the field list. Do not show Service Parameters or
internal follow-trade parameters. Omit the Service Guide Consent item when the
Guide is blank; otherwise preserve every collected Guide field and
User-authored value without rewriting them. Do not show a standalone Guide or
payment confirmation, and do not repeat the automatic copy-trading choice
already collected from the Guide. This card owns the one explicit final
confirmation for the subscription, displayed payment, and exact Guide Consent.
Any edit to a displayed fact or Guide answer invalidates that confirmation and
requires the complete updated card again.

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
`type` and `bizType` must both be 204. The preference was already saved before
creation; do not save or migrate it by `jobId`. On successful creation, follow
`watch_task`, then enter `subscription-manage.md` for the post-creation
offline-delivery and watch questions. Do not add another creation confirmation.
