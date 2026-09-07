# ASP Dispute Actions

Use this leaf to execute one action returned by the latest arbitration
progression. Preserve all parameters exactly.

| Action ID | Command |
|---|---|
| `agree_refund` | `onchainos agent agree-refund <params.jobId> --agent-id <aspAgentId>` |
| `raise_arbitration` | `onchainos agent dispute raise <params.jobId> --reason "<params.reason>" --agent-id <aspAgentId>` |
| `sub_agree_refund` | `onchainos agent subscribe-agree-refund <params.jobId> --agent-id <aspAgentId>` |
| `raise_subscription_arbitration` | `onchainos agent subscribe-dispute <params.jobId> --reason "<params.reason>" --agent-id <aspAgentId>` |

The CLI may normalize legacy `dispute_raise` and `sub_dispute` from persisted
cards. Any unregistered action is `unsupported_action` with no executable
next action. Give one concise localized result with relevant returned fields
and a query hint; never infer submission or settlement.

## Start arbitration directly

Treat an explicit instruction to arbitrate a specified task as authorization
to start arbitration. A supplied reason completes that instruction; do not
insert a decision card.

1. Resolve the exact Job ID and ASP Agent ID.
2. Find the task in the fresh rejected-candidate sources in
   [`arbitration-query.md`](arbitration-query.md). The source identifies a
   one-time task or subscription period and confirms current eligibility.
3. Preserve a reason supplied in the same request verbatim. If missing, ask
   only for the reason and treat the next plain reply as that reason.
4. Run exactly one matching command:

   ```text
   onchainos agent dispute raise <jobId> --reason "<reason>" --agent-id <aspAgentId>
   onchainos agent subscribe-dispute <jobId> --reason "<reason>" --agent-id <aspAgentId>
   ```

5. Present the submission result and the arbitration-detail query method.

## Reason handoff

Both arbitration commands send one local task-session message before their
on-chain broadcast. Preserve the exact reason and all returned fields.

One-time task:

```text
[ARBITRATION_REASON_CONTEXT]
{"version":1,"intent":"arbitration_reason_context","jobId":"<jobId>","providerAgentId":"<aspAgentId>","reason":"<exact reason>","reasonB64":"<URL-safe base64>","confirmArgs":[...]}
```

Subscription:

```text
[ARBITRATION_REASON_CONTEXT]
{"version":1,"intent":"arbitration_reason_context","taskType":"subscription","jobId":"<jobId>","providerAgentId":"<aspAgentId>","reason":"<exact reason>","reasonB64":"<URL-safe base64>","resumeEvent":"sub_asp_dispute"}
```

When this message arrives, match `jobId` and `providerAgentId` to the current
task conversation and retain `reason` and `reasonB64` unchanged. For a
one-time task, also retain `confirmArgs` until the matching `dispute_approved`.
For a subscription, require `taskType=subscription` and
`resumeEvent=sub_asp_dispute` before its evidence flow.

For `dispute_approved`, read the latest matching context and run once:

```text
onchainos agent dispute confirm <jobId> \
  --reason-b64 <reasonB64> --agent-id <aspAgentId>
```

The CLI decodes `reasonB64` to the exact original reason before building the
dispute broadcast. A missing matching context returns
`arbitration_reason_context_missing`; stop the event turn without fabricating
an empty reason. For `sub_asp_dispute`, continue to
[`evidence-upload.md`](evidence-upload.md).

## Lifecycle handoff

- A one-time B decision submits `dispute raise`. Its task session later handles
  `dispute_approved` by running `dispute confirm` exactly once with the original
  reason from the matching context.
- `job_disputed` begins [`evidence-upload.md`](evidence-upload.md) only after a
  fresh `disputed` status check.
- A subscription B decision submits `subscribe-dispute`. The command relays the
  reason before its combined approve-and-create broadcast; `sub_asp_dispute`
  then supplies dispute creation facts and starts evidence upload.
- Continue later events through [`../../runtime/watch.md`](../../runtime/watch.md).

Protocol values such as `disputed`, `job_disputed`, `sub_asp_dispute`,
`dispute_approved`, `dispute_resolved`, and `disputeRoundStatus` remain exact at
the backend boundary.
