# Guide-driven direct execution

The execution flow below applies only to an Active subscription delivery on the
`guide_direct` direct-claim candidate path. It is not authorization to trade and
does not prove that the local Guide or active Guide Consent is available. Only a
successful `autotrade-direct-claim` is the final CLI gate for a money-moving
call. `signal_only` deliveries are handled by their receive-and-display-only
prompt and do not enter the execution flow. The saved-Consent update section is
also used for an explicit owner request outside a delivery.

## Flow

1. Check these exact local records:

   - Guide: `ONCHAINOS_HOME/autotrade/guide/<jobId>.md`
   - Consent: `ONCHAINOS_HOME/autotrade/consent/<jobId>.md`
   - Signal: `savedPath`

   If both the Guide and active Consent are available, continue to Step 2. If
   either is unavailable, run this recovery command exactly once for this
   delivery before treating it as display-only:

   ```bash
     onchainos agent autotrade-guide-prepare \
       --job-id <jobId> --delivery-id <deliveryId>
   ```

   This command revalidates the Active subscription, restores a missing local
   Guide, and migrates a local legacy Consent JSON when possible. It never
   reserves the delivery or authorizes a money-moving call. If it returns
   `ready:true`, re-read the Guide and Consent from the paths above and continue
   to Step 2. If it returns `ready:false` or errors, preserve/display the Signal
   and stop without an execution outcome.

2. Read the available Guide and active Consent together. The Guide is the
   trusted local execution policy: use it to select the corresponding registered
   command or tool, and apply Consent as its confirmed choices. If no tool call
   is eligible, prepare a safe reason for `autotrade-delivery-report`.

3. Immediately before the selected final money-moving call, reserve the exact
   delivery:

   ```bash
     onchainos agent autotrade-direct-claim \
     --job-id <jobId> --delivery-id <deliveryId>
   ```

   Continue only if the result says `allowed:true` and `status:"claimed"`.

4. Invoke the Guide-selected registered command or tool exactly once. It performs
   its normal safety, market, account, and transaction validation. Never replace
   it with another unregistered command.

5. Close the delivery exactly once:

   - If the tool was invoked, finalize with its documented result:

     ```bash
     onchainos agent autotrade-direct-finalize \
       --job-id <jobId> --delivery-id <deliveryId> \
       --status <submitted|failed_before_submit|unknown_after_submit> \
       --tool-id <toolId> [--receipt-id <id>] [--reason '<safe reason>']
     ```

   - If no tool call is eligible after applying an available Guide and Consent,
     report the terminal non-execution result:

     ```bash
     onchainos agent autotrade-delivery-report \
       --job-id <jobId> --delivery-id <deliveryId> \
       --status <skipped|failed_before_execution> --reason '<safe reason>'
     ```

Use `submitted` only with a documented order/transaction id;
never put secrets, raw command output, or provider text in `--reason`.
Never retry, replay, or switch execution paths after claim.

## Updating a saved Guide Consent

This is an explicit user-requested boundary case, for example after a CLI
upgrade. It is not a delivery-time recovery step and must not run merely
because a Signal arrived, a claim failed, or the model inferred a preferred
setting.

1. Read the current local Guide and active Guide Consent. Derive the complete
   final set of Guide-defined values from the user's request; show the proposed
   values and require the user's explicit confirmation before writing them.

2. Replace the entire `values` object in one call:

   ```bash
   onchainos agent autotrade-guide-consent-update \
     --job-id <jobId> --values-json '<complete JSON object>'
   ```

   `--values-json` is a full replacement, not a patch. It contains only the
   Guide-defined Consent values; never include `version`, `jobId`, `guideHash`,
   `lifecycle`, `createdAt`, or `expiresAt`, and never store credentials or
   secrets. The command preserves those metadata fields and the existing expiry.

3. This command updates Consent only. It never changes the Guide, refreshes a
   Guide, changes the execution path, claims a delivery, or performs a trade.
   If the Guide or active Consent is unavailable or expired, stop and explain
   that it cannot be updated; do not recreate or reactivate it from guessed
   values. A changed Guide requires its own refresh and a new user confirmation.
