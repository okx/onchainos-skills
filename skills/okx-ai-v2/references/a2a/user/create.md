# One-Time Task Creation

Enter only from `create-prepare.md` with the latest bound Service payload.
`decision=ready` does not authorize creation. Subscription Services route to
`subscription-create.md` instead.

## Collect inputs

If `payload.serviceGuide` is non-blank, complete and independently confirm
`create-guide.md` first. Then parse only `payload.serviceDescription` for
explicit required and optional inputs. Ignore promotional text and never invent
scope.

Fill values from the User's request or direct answers. Ask for all missing
required values together; re-ask only missing or invalid values. Produce:

- `title`: concise, at most 30 characters;
- `Description`: requested outcome plus confirmed inputs, 20–2000 characters;
- `serviceParams`: only confirmed inputs required by the Service description;
- attachments: each explicit local file, retained for repeated `--file` flags.

If supplementary files are implied but absent, ask once whether to add them now
or after creation. Do not show a second standalone parameter confirmation.

## Final confirmation

Render exactly these business fields in the User's language:

| Field | Value |
|---|---|
| Task Name | `title` |
| Task Description | confirmed `Description` |
| Provider | `Agent <providerAgentId>(<providerAgentName>)`; omit absent name |
| Service Parameters | confirmed `serviceParams` |
| Service Price | exact `feeAmount feeTokenSymbol`; zero means localized Free |

List attachments below the table, not as another table row. Guide Consent was
confirmed separately and is not merged into this form. Apply edits and show the
whole confirmation again. Continue only after an explicit final confirmation.

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
