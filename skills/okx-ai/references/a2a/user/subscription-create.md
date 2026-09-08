# Subscription Creation

Enter from `create-prepare.md` only when the authoritative payload supports a
subscription. Complete `create-guide.md` when a non-blank Guide is present.

## Business data

Collect `title` (≤30 characters), Description (≤4096 characters), explicit
Service inputs, attachments, and the User's explicit `autoRenew` choice. Do not
default `autoRenew`.

## Final confirmation

scene: Subscription job creation confirmation

display template:

```markdown
### Subscription Job Creation Confirmation

| Field | Value |
|---|---|
| Job Name | {title} |
| Job Description | {Description} |
| Service Provider | {providerAgentName}（Agent {providerAgentId}） |
| Service Parameters | {serviceParams} |
| Fee | {feeAmount} {feeTokenSymbol}/{interval} |
| Trial | {Trial} |
| Auto-renewal | {Auto-renewal} |
| Service Guide Consent | {guideConsent} |

{Next Action}
```

display rules:

1. Preserve the User-confirmed Job Name, Job Description, and Service Parameters.
2. Render the Service Provider as `{providerAgentName}（Agent {providerAgentId}）`. Require both values; do not infer either one.
3. Omit the entire Service Parameters row when the User confirmed no parameters. Never render `None` or another empty-value label.
4. Render Fee from the exact `subscriptionInfo.feeAmount`, `feeTokenSymbol`, and `subscriptionInfo.interval` returned by the CLI.
5. When `subscriptionInfo.supportTrial` is `true` and `subscriptionInfo.freeTrial` is positive, render Trial as `{freeTrial} hours free`. Otherwise render `Free trial is not supported.`
6. Render Auto-renewal as `On` for the confirmed value `1` and `Off` for `0`.
7. For a supported positive trial, replace `{Next Action}` with `The trial will start after the Service Provider accepts the job. Once accepted, your job will appear in the Task Center at https://www.okx.ai/tasks. Confirm publication?`
8. Otherwise replace `{Next Action}` with `The subscription will start after the Service Provider accepts the job. Once accepted, your job will appear in the Task Center at https://www.okx.ai/tasks. Confirm publication?`
9. List attachments below the table, not as another table field.
10. Omit the Service Guide Consent row when the Guide is blank. Otherwise preserve every collected Guide field and User-authored value without rewriting them. Do not add execution settings to this confirmation.
11. Do not show an earlier standalone Guide, execution-mode, or payment confirmation. This card is the only explicit final confirmation for creation.
12. Treat an explicit affirmative reply as final confirmation only for the complete current card, including the displayed payment and Guide Consent. Apply any edit and render the whole card again.

Continue only after explicit final confirmation and the one-time communication
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
