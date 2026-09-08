# Subscription Management

Use this leaf after `subscription.md` identifies one exact subscription. Device
receipt changes are owned by `receipt.md`; refunds are owned by
`refund-prepare.md`.

## Post-creation handoff

After `create-subscribe` succeeds, state that messages go to all logged-in
devices and can be changed later. Then ask, without waiting before watch:

- Replay missed deliverables (default): background receipt continues and
  deliverables appear when the User returns.
- Discard offline deliverables: save the preference; the subscription remains
  active.

If the returned `offlineReplaySupported=false`, include the exact returned fix
commands and explain that the preference takes effect after upgrade. Never run
an independent capabilities probe.

Immediately process the returned `[Watch]` block and enter the exact scoped
runtime watch. Absence of the block ends the turn. The preference question must
not delay the initial watch or `sub_open` event.

## Management actions

| Intent | Command/boundary |
|---|---|
| Enable auto-renew | `onchainos agent start-autorenew <jobId>`; use the CLI confirmation/signing flow. |
| Cancel trial conversion or formal auto-renew | `onchainos agent subscribe-cancel <jobId>`; cancellation does not end the live trial/current period and is not a refund. |
| Active subscription cost | `onchainos agent subscribe-cost` |
| Replay offline deliverables | Fresh-read; when changed, `subscribe-offline-update --job-id <jobId> --flag 0`, then reread. |
| Discard offline deliverables | Fresh-read; when changed, use flag 1, then reread and report support state. |
| Pause Guide-driven automatic execution | `autotrade-consent-set --job-id <jobId> --mode pause`; receipt and subscription stay active. |
| Receive/resume/listen for signals | Follow the scoped receipt flow below. |

Do not write when the fresh value already matches. A preference-write failure
does not roll back creation and does not authorize a retry.

## Scoped receipt flow

1. Resolve exactly one Active buyer subscription from an explicit Job ID/title
   or fresh current list/detail. Historical recency is not enough.
2. Run `subscribe-detail <jobId> --format json`.
3. Require Active status. Ensure this device receives using `receipt.md`; never
   collapse `deviceList:null` and `[]`, and never drop other device IDs.
4. Immediately before watch, fresh detail must show
   `thisDeviceReceives=true`.
5. Enter `../../runtime/watch.md` with sticky `--job-id <jobId>`. Never substitute
   a global watch or claim that starting watch proves a new signal exists.

Restoring receipt does not recreate or modify Guide Consent. A missing Guide
or Consent keeps signals visible but disables local automatic execution.

## Device and execution safety

- `deviceList:null` means default-all; `[]` means explicitly none.
- Any explicit receiver update is built from a fresh complete read and
  overwrites the entire list.
- Confirm before a removal that would leave no receiving device.
- Pausing automatic execution never cancels the subscription or receipt.
- Creation-time device selection is unsupported; configure it only after
  successful creation.
