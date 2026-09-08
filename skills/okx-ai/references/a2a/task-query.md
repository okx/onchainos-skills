# Task and Deliverable Queries

This leaf performs read-only queries and returns fresh task data.

## One task

Require an explicit Job ID. If none is identified, run `active-tasks`, show
numbered candidates with title, role, status, and counterparty, then wait for a
selection.

```text
onchainos agent status <jobId> --agent-id <currentAgentId>
```

Use this existing status call as the task-type gate; never add a probe request.
The normal result includes `Task type: one_time|subscription|unknown`, derived
from the authoritative `jobType` in the same task-detail response.

- `one_time`: continue below and render the one-time task card.
- `subscription`: stop the one-time branch before rendering its card and enter
  [`user/subscription.md`](user/subscription.md) §Status-query handoff with the
  same status result. Do not call `subscription-list` or `subscribe-detail` for
  this handoff.
- `unknown`: stop and report that the task type could not be established. Never
  assume an untyped task is one-time.

When `agent status` returns a structured arbitration detail instead of the
normal text summary, use its authoritative `payload.jobType` (`0` one-time,
`1` subscription) as the same gate. Missing or unsupported values fail closed.

For a one-time task, render this exact card from fresh returned facts:

scene: One-time task details

display template:

```markdown
### One-time Job Details

| Field | Value |
|---|---|
| Job Name | {title} |
| Job ID | {jobId} |
| Service Provider | Agent ID {providerAgentId} |
| Fee | {Fee} |
| Status | {status} |
| Job Description | {description} |
```

display rules:

1. Display the complete `jobId`; never shorten it.
2. Render Fee as `{tokenAmount} {tokenSymbol}`. Render `Free` when the exact
   amount is zero.
3. Use only the status name rendered by the CLI.
4. Preserve the returned Job Description without rewriting it.

A task status is not a substitute for Refund V2 settlement provenance.

## Buyer refund tasks

- `available`: one-time Submitted tasks and Active subscription periods.
- `requested`: one-time Rejected tasks and Rejected subscription periods.

```text
onchainos agent refund-list --role buyer --scope available --agent-id <userAgentId> --page 1 --page-size 20
onchainos agent refund-list --role buyer --scope requested --agent-id <userAgentId> --page 1 --page-size 20
```

The CLI applies the one-time and subscription status filters and returns one
display-ready `items` array. Render [Available Refund Tasks](#available-refund-tasks)
for `available` and [Pending Refund Requests](#pending-refund-requests) for
`requested`. Keep the two modes separate.

For an `available` selection, run `refund-prepare <jobId>` and render
[Confirm Refund Request](user/refund-confirm.md#confirm-refund-request). For a
`requested` selection, run:

```text
onchainos agent refund-detail <jobId> --role buyer --agent-id <userAgentId>
```

Render [Refund Request Details](#refund-request-details) and end after the
table.

For an explicit one-time list, run:

```bash
onchainos agent my-tasks --task-type one-time --status-type <0|1|2> \
  --page <page> --page-size <pageSize>
```

scene: One-time task list

display template:

```markdown
### One-time Jobs

| # | Job Name | Job ID | Service Provider | Fee | Status |
|---|---|---|---|---|---|
| {n} | {title} | {jobId} | Agent ID {providerAgentId} | {Fee} | {statusName} |
```

display rules:

1. Render only the current `oneTimeTasks.list` page in CLI order and number it
   from 1.
2. Display every `jobId` in full.
3. Render Fee as `{tokenAmount} {tokenSymbol}`. Render `Free` when the exact
   amount is zero.
4. Use only the CLI-normalized `statusName`.
5. Preserve the returned pagination; do not merge pages.

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

### Available Refund Tasks

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
4. Use this template only for `scope=available`.

### Pending Refund Requests

```markdown
You have {pendingCount} pending refund requests:

| # | Service name | Job ID | Task Type | Refund Amount | Result Deadline |
|---|---|---|---|---|---|
| {n} | {serviceName} | {jobId} | {taskType} | {refundAmount} | {resultDeadline} |

Reply with the number or Job ID to view details.
```

Display rules:

1. Use this template only for `scope=requested`.
2. Number records sequentially in CLI order and preserve every full Job ID.
3. Use only the CLI-provided Service name, Task Type, Refund Amount, and Result Deadline.
4. `No refund required` is the authoritative zero-amount label.
5. When `pendingCount>0`, the final sentence is the only Recommend action. Omit it for an empty list.

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
3. The CLI must return Service Name, Service Provider, Requested Refund, Reason for Refund, and Result Deadline. A missing value blocks the card; do not infer it.
4. End after the table. This scene has no Recommend action.
5. Keep transaction hashes internal.
