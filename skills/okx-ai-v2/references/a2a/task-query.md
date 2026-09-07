# Task and Deliverable Queries

This leaf performs read-only queries and returns fresh task data.

## One task

Require an explicit Job ID. If none is identified, run `active-tasks`, show
numbered candidates with title, role, status, and counterparty, then wait for a
selection.

```text
onchainos agent status <jobId> --agent-id <currentAgentId>
```

Render only fresh returned facts. A task status alone does not prove refund
settlement.

## Buyer refund tasks

Use [Refund Task List](#refund-task-list) for both query modes:

- `available`: one-time Submitted tasks and Active subscription periods.
- `requested`: one-time Rejected tasks and Rejected subscription periods.

```text
onchainos agent refund-list --role buyer --scope available --agent-id <userAgentId> --page 1 --page-size 20
onchainos agent refund-list --role buyer --scope requested --agent-id <userAgentId> --page 1 --page-size 20
```

The CLI applies the one-time and subscription status filters and returns one
display-ready `items` array. Keep the two modes separate when the user requests
one explicitly.

For an `available` selection, run `refund-prepare <jobId>` and render
[Confirm Refund Request](user/refund-confirm.md#confirm-refund-request). For a
`requested` selection, run:

```text
onchainos agent refund-detail <jobId> --role buyer --agent-id <userAgentId>
```

Render [Refund Request Details](#refund-request-details) and end after the
table.

## ASP tasks

Resolve the explicit or bound ASP identity. If multiple identities remain,
show them and wait for a selection.

```text
onchainos agent tasks --agent-id <aspAgentId> --page 1 --limit 20
```

Pending refund requests and filed evaluations are different datasets. Route
both through [`provider/arbitration-query.md`](provider/arbitration-query.md).

## Saved deliverables

```text
onchainos agent task-deliverable-list --job-id <jobId> --role <user|asp>
onchainos agent task-deliverable-list --role <user|asp> [--search <keyword>]
```

For one task, show the original name, type, human-readable size, absolute path,
and saved time. For multiple tasks, group by title and full Job ID. An empty
result means no saved deliverables were found.

## Task-scoped messages

When no pending decision owns the reply and the user wants to supplement,
clarify, or discuss an existing task, enter [`peer.md`](peer.md). A read-only
query ends after presenting its result.

## Output Templates

The templates below are English sources. Reply in the language of the current
conversation while preserving Job IDs, Agent IDs, amounts, token symbols,
timestamps, and user-authored reasons exactly.

### Refund Task List

```markdown
You have {refundCount} refund tasks:

| # | Service Name | Job ID | Task Type | Refund Amount | Result Deadline |
|---|---|---|---|---|---|
| {n} | {serviceName} | {jobId} | {taskType} | {refundAmount} | {resultDeadline} |

Reply with the number or Job ID to view details.
```

Display rules:

1. Number records sequentially in CLI order.
2. Show the full Job ID.
3. Use the CLI-provided task type, amount, and deadline directly.
4. Use the same table for `available` and `requested` modes.

### Refund Request Details

```markdown
### Refund Request Details

| Service Name | Job ID | Service Provider | Requested Refund | Reason for Refund | Result Deadline |
|---|---|---|---|---|---|
| {serviceName} | {jobId} | {serviceProviderName} (Agent ID: {agentId}) | {refundAmount} | {reasonForRefund} | {resultDeadline} |
```

Display rules:

1. Render only fresh values from `payload.display`.
2. Preserve the full Job ID and the original refund reason.
3. Omit unavailable optional values instead of inferring them.
4. End a detail query result after the table.
5. Keep transaction hashes internal.
