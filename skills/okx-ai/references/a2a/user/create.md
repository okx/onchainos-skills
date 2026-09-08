# One-Time Task Creation

Enter only from `create-prepare.md` with the latest bound Service payload.
`decision=ready` does not authorize creation. Subscription Services route to
`subscription-create.md` instead.

## Collect inputs

If `payload.serviceGuide` is non-blank, complete `create-guide.md` first and
retain its collected Guide Consent for the final card below. Do not request a
separate Guide confirmation. Then parse only `payload.serviceDescription` for
explicit required and optional inputs. Ignore promotional text and never
invent scope.

Fill values from the User's request or direct answers. Ask for all missing
required values together; re-ask only missing or invalid values. Produce:

- `title`: concise, at most 30 characters;
- `Description`: requested outcome plus confirmed inputs, 20–2000 characters;
- `serviceParams`: only confirmed inputs required by the Service description;
- attachments: each explicit local file, retained for repeated `--file` flags.

If supplementary files are implied but absent, ask once whether to add them now
or after creation. Do not show a second standalone parameter confirmation.

## Final confirmation

scene: One-time job creation confirmation

display template:

```markdown
### One-time Job Creation Confirmation

| Field | Value |
|---|---|
| Job Name | {title} |
| Job Description | {Description} |
| Service Provider | Agent {providerAgentId}({providerAgentName}) |
| Fee | {feeAmount} {feeTokenSymbol} |
| Service Parameters | {serviceParams} |
| Service Guide Consent | {guideConsent} |

To create this job and authorize the displayed payment and Service Guide
Consent, reply “Confirm”. To cancel, reply “Cancel”.
```

display rules:

1. Preserve the collected Job Name, Job Description, Service Parameters, and
   complete Guide Consent.
2. Render the Service Provider as `Agent {providerAgentId}({providerAgentName})`. Omit only the parenthesized name when it is absent.
3. Render a zero Fee as `Free`; otherwise render the exact amount and token symbol.
4. Omit the Service Parameters row when no parameters were collected. Include
   the Service Guide Consent row only when `serviceGuide` is non-blank, and
   render every collected Guide field and User-authored value without
   rewriting them.
5. List attachments below the table, not as another table field.
6. Do not show an earlier standalone Guide, parameter, or payment confirmation.
   This card is the only explicit final confirmation for creation.
7. `Confirm` explicitly authorizes the task creation, displayed payment, and
   exact Guide Consent for only the current complete card. Any edit to a task,
   payment, attachment, Service Parameter, or Guide Consent fact invalidates
   that confirmation; apply the edit and render the whole card again.

## Communication check

After confirmation, run once:

```bash
onchainos agent communication-check
```

`data.ok=true` without a note continues silently. Otherwise show the returned
hint/note and ask the User to choose Repair communication or Continue creation.
Repair uses `../../shared/chat-comm-init.md`. Reuse the confirmed task parameters;
do not repeat this check or the confirmation afterward.

## Create once

Use `payload.serviceId` for `--service-id`; `sid` is never the Service UUID.

```bash
onchainos agent create-task \
  --title <title> \
  --description <confirmed-description> \
  --provider-agent-id <payload.providerAgentId> \
  --payment-token-symbol <payload.feeTokenSymbol> \
  --payment-token-amount <payload.feeAmount> \
  --service-id <payload.serviceId> \
  --service-params '<confirmed JSON or {}>' \
  --service-token-address <payload.feeToken> \
  --service-token-amount <payload.feeAmount> \
  [--file <attachment> ...] \
  [--service-guide '<exact serviceGuide>' \
   [--service-guide-hash '<exact serviceGuideHash>'] \
   --guide-consent-json '<confirmed Consent JSON>']
```

Include the complete Guide bundle only for a non-blank Guide. Pass confirmed
Service context unchanged; do not repeat price, balance, or provider checks.
On `broadcast_submitted/watch_task`, enter the returned scoped watch. Creation
is final only after `job_created`. For an uncertain mutation result, query fresh
task state before considering any retry.
