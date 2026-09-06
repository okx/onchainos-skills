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

## Lifecycle handoff

- A one-time B decision submits `dispute raise`. Its task session later handles
  `dispute_approved` by running `dispute confirm` exactly once with the original
  reason when available.
- `job_disputed` begins [`evidence-upload.md`](evidence-upload.md) only after a
  fresh `disputed` status check.
- A subscription B decision submits `subscribe-dispute`; `sub_asp_dispute`
  supplies dispute creation facts.
- Continue later events through [`../../runtime/watch.md`](../../runtime/watch.md).

Protocol values such as `disputed`, `job_disputed`, `sub_asp_dispute`,
`dispute_approved`, `dispute_resolved`, and `disputeRoundStatus` remain exact at
the backend boundary.
