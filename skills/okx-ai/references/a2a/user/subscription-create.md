# Subscription Creation

Enter from `create-prepare.md` only when the authoritative payload supports a
subscription. Complete `create-guide.md` when a non-blank Guide is present.

## Execution mode

Separately confirm `signal_only` or `guide_direct`, then end the turn.
`guide_direct` requires the exact Guide and confirmed Guide Consent; an absent
Guide permits only `signal_only`. Do not default the mode or store it in
`serviceParams` or Guide Consent.

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
```

Render the Service Parameters item for confirmed parameters. Render attachments
below the field list.

Guide Consent remains a separate confirmation. Continue only after explicit
final confirmation and the one-time communication check defined by `create.md`.

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
