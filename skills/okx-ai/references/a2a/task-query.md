# Task and Deliverable Queries

This leaf performs read-only queries and returns fresh task data.

## One task

Require an explicit Job ID. If none is identified, run `active-tasks`, show
numbered candidates with title, role, status, and counterparty, then wait for a
selection.

```text
onchainos agent status <jobId> --agent-id <currentAgentId>
```

For a one-time task, render this exact card from fresh returned facts:

scene: One-time task details

display template:

```markdown
### One-time Job Details

- Job Name: {title}
- Job ID: {jobId}
- Service Provider: Agent ID {providerAgentId}
- Fee: {Fee}
- Status: {localizedStatusLabel}
- Job Description: {description}
```

display rules:

1. Display the complete `jobId`.
2. Render Fee as `{tokenAmount} {tokenSymbol}`. Render `Free` when the exact
   amount is zero.
3. Render the localized CLI `statusLabel`. The raw `statusName` remains a
   protocol compatibility key and must not be shown. In a Chinese conversation,
   render `Awaiting ASP acceptance` as `ASP 待接单`; for a one-time task with
   raw status `failed` / code `9`, render `Refund completed` as `退款成功`.
4. Preserve the returned Job Description without rewriting it.

Use Refund V2 settlement provenance to confirm the refund result.

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
detail block.

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
| {n} | {title} | {jobId} | Agent ID {providerAgentId} | {Fee} | {localizedStatusLabel} |
```

display rules:

1. Render only the current `oneTimeTasks.list` page in CLI order and number it
   from 1.
2. Display every `jobId` in full.
3. Render Fee as `{tokenAmount} {tokenSymbol}`. Render `Free` when the exact
   amount is zero.
4. Localize the CLI-normalized `statusLabel`; retain `statusName` only as a raw
   compatibility key. In a Chinese conversation use these concise values:
   `Awaiting ASP acceptance` -> `ASP 待接单`, `In progress` -> `执行中`,
   `Awaiting buyer review` -> `等待用户验收`, `Awaiting refund decision` ->
   `等待退款处理`, `Evaluation in progress` -> `评审中`, `Stopped by platform`
   -> `平台已终止`, `Completed` -> `已完成`, `Closed` -> `已关闭`, `Expired`
   -> `已过期`, and `Refund completed` -> `退款成功`.
5. Preserve each returned page and its pagination.

## ASP tasks

Resolve the explicit or bound ASP identity. If multiple identities remain,
show them and wait for a selection.

```text
onchainos agent tasks --agent-id <aspAgentId> --page 1 --limit 20
```

Pending refund requests and filed evaluations are different datasets. Route
both through [`provider/arbitration-query.md`](provider/arbitration-query.md).

For the refund status or result of a known provided Job ID, run:

```text
onchainos agent refund-detail <jobId> --role provider --agent-id <aspAgentId>
```

The same command returns the pending decision for `Rejected(3)`, the current
Evaluation result for `Disputed(4)`, and a read-only refund result for terminal
states. Render [Refund Request Details](#refund-request-details) from its
`payload.display` fields.

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

Use tables only for multi-record list results. Render every single-record
detail or confirmation as one `- Label: value` item per available field.

### Refund Task List

```markdown
You have {refundCount} refund tasks:

| # | Service Name | Job ID | Task Type | Refund Amount | Response Deadline |
|---|---|---|---|---|---|
| {n} | {serviceName} | {jobId} | {taskType} | {refundAmount} | {responseDeadline} |

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

- Service Name: {serviceName}
- Job ID: {jobId}
- Service Provider: {serviceProviderName} (Agent ID: {agentId})
- Requested Refund: {refundAmount}
- Reason for Refund: {reasonForRefund}
- Response Deadline: {responseDeadline}
- Refund Result: {localizedStatusLabel}
- Result Description: {localizedStatusDescription}
- Evaluation Result: {localizedEvaluationResultDescription}
- Evaluation Reason: {localizedEvaluationReason}
```

Display rules:

1. Render only fresh values from `payload.display`.
2. Preserve the full Job ID and the original refund reason.
3. Render each available optional value from `payload.display`.
4. Localize `payload.display.statusLabel` and
   `payload.display.statusDescription` into the conversation language. In a
   Chinese conversation, use these refund result labels:
   - `Awaiting ASP decision` -> `等待 ASP 处理`
   - `Refund under evaluation` -> `退款评审中`
   - `Refund completed` -> `退款成功`
   - `Refund not issued` -> `未退款`
   - `Closed without refund` -> `已关闭，未退款`
   - `No refund required` -> `无需退款`
   - `Refund result unavailable` -> `退款结果暂不可用`
5. Render `Evaluation Result` and `Evaluation Reason` only when the CLI
   returns them. For a Chinese conversation, use:
   - `ASP won; refund not issued` -> `ASP 胜诉，未退款`
   - `The Evaluation concluded in favor of the ASP. The task funds were released to the ASP and no refund was issued.` -> `本次仲裁已结束，任务款项已结算给 ASP，因此未退款。`
   - `The Evaluation service did not return a specific evaluator rationale.` -> `评审服务未返回具体的评审员裁决理由。`
   Never treat the original `Reason for Refund` as an evaluation reason.
6. End a detail query result after the detail block.
7. Render the fields defined by this template.
