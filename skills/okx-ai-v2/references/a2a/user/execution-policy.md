# Guide-driven direct execution

This policy applies only to an Active subscription delivery on the
`guide_direct` direct-claim candidate path. It is not authorization to trade and
does not prove that the local Guide or active Guide Consent is available. Only a
successful `autotrade-direct-claim` is the final CLI gate for a money-moving
call. `signal_only` deliveries are handled by their receive-and-display-only
prompt and do not enter this policy.

## Flow

1. Read these exact local records together:

   - Guide: `ONCHAINOS_HOME/autotrade/guide/<jobId>.md`
   - Consent: `ONCHAINOS_HOME/autotrade/consent/<jobId>.md`
   - Signal: `savedPath`

   The Guide is the trusted local execution policy: use it to select the
   corresponding registered command or tool, and apply Consent as its confirmed
   choices. If the Guide or active Consent is unavailable, preserve/display the
   Signal and stop without an execution outcome. If no tool call is eligible
   under an available Guide and Consent, prepare a safe reason for
   `autotrade-delivery-report`.

2. Immediately before the selected final money-moving call, reserve the exact
   delivery:

   ```bash
     onchainos agent autotrade-direct-claim \
     --job-id <jobId> --delivery-id <deliveryId>
   ```

   Continue only if the result says `allowed:true` and `status:"claimed"`.

3. Invoke the Guide-selected registered command or tool exactly once. It performs
   its normal safety, market, account, and transaction validation. Never replace
   it with another unregistered command.

4. Close the delivery exactly once:

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
