# ASP Evaluation Queries

Use this leaf for pending refund requests, evaluation records, and evaluation
details.

## Pending refund requests

Pending evaluation, evaluable-task, and required-evaluation intents all mean
the current rejected-task set:

```text
onchainos agent refund-list --role provider --scope requested --agent-id <aspAgentId> --page 1 --page-size 20
```

Render [Pending Refund Requests](#pending-refund-requests). A selected
sequence or Job ID runs the following fresh detail query, then loads
[`arbitration-decision.md`](arbitration-decision.md) to render its Buyer Refund
Request decision template:

```text
onchainos agent refund-detail <jobId> --role provider --agent-id <aspAgentId>
```

## Evaluation records

An in-progress, filed, or completed evaluation query uses:

```text
onchainos agent arbitration-list --agent-id <aspAgentId> [--page <n>] [--page-size <n>]
```

Render [Evaluation Records](#evaluation-records).

## Evaluation details

```text
onchainos agent arbitration-detail <jobId> --agent-id <aspAgentId>
```

Render [Evaluation Details](#evaluation-details).

## Output Templates

The templates below are English sources. Reply in the language of the current
conversation while preserving Job IDs, amounts, token symbols, timestamps, and
user-authored reasons exactly.

### Pending Refund Requests

```markdown
You have {pendingCount} refund requests from buyers awaiting your decision:

| # | Service Name | Job ID | Task Type | Requested Refund | Response Deadline |
|---|---|---|---|---|---|
| {n} | {serviceName} | {jobId} | {taskType} | {requestedRefund} | {responseDeadline} |

A full refund will be issued automatically if no action is taken by the deadline. Reply with a number or Job ID to view the request.
```

Display rules:

1. Use only pending records returned by the CLI.
2. Number records sequentially and preserve the full Job ID.
3. Preserve CLI order after its response-deadline sort.
4. Use the CLI-provided service name, task type, amount, and formatted deadline.
5. When `pendingCount>0`, the final sentence is the only Recommend action. Omit it for an empty list; selecting a request does not approve or dispute it.

### Evaluation Records

```markdown
You have {evaluationCount} evaluation records:

| # | Service Name | Job ID | Status | Evaluation Started | Key Time |
|---|---|---|---|---|---|
| {n} | {serviceName} | {jobId} | {status} | {evaluationStarted} | {keyTime} |

Reply with a number or Job ID to view the evaluation details.
```

Display rules:

1. Number records sequentially and show the full Job ID.
2. Use only the CLI-provided `Evidence preparation`, `Evaluating`, or `Decided` status.
3. Use the CLI-provided evaluation-started and key-time values directly.
4. Omit `Key Time` when every returned value is empty.
5. Restrict selection to `nextAction[id=view_arbitration].params.allowedJobIds`.

### Evaluation Details

```markdown
### Evaluation Details

| Service Name | Job ID | Requested Refund | Buyer’s Reason | Status | Evaluation Started |
|---|---|---|---|---|---|
| {serviceName} | {jobId} | {requestedRefund} | {buyerReason} | {status} | {evaluationStarted} |
```

Display rules:

1. Render only fresh `payload` fields returned by `arbitration-detail`.
2. Preserve the full Job ID and the buyer-authored reason.
3. Use only the CLI-provided evaluation status and formatted time.
4. Omit unavailable optional values instead of inferring them.
5. End after the table and keep transaction hashes internal.
