# ASP Evaluation Queries

Use this leaf for pending refund requests, evaluation records, and evaluation
details.

## Pending refund requests

Pending evaluation, evaluable-task, and required-evaluation intents all mean
the current rejected-task set, rather than the filed-evaluation list. Query it
first:

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

Use tables only for multi-record list results. Render a selected single-record
detail as one `- Label: value` item per available field.

Translate each list title and every table header into the user's language.
The English templates below are the source wording and field order only.

### Pending Refund Requests

```markdown
You have {pendingCount} refund requests from buyers awaiting your decision:

| # | Service Name | Job ID | Task Type | Requested Refund | Response Deadline |
|---|---|---|---|---|---|
| {n} | {serviceName} | {jobId} | {taskType} | {requestedRefund} | {responseDeadline} |

Reply with the number or Job ID to view details, then select "Approve Refund" or "Request Review". A full refund will be issued automatically if no action is taken by the deadline.
```

Display rules:

1. Use only pending records returned by the CLI.
2. Number records sequentially and preserve the full Job ID.
3. Preserve CLI order after its response-deadline sort.
4. Use the CLI-provided service name, task type, amount, and formatted deadline.
   The deadline is `rejectDeadline` from this pending-list row, formatted with
   the same minute precision and UTC offset as other task times.
5. Translate the English recommendation in the template into the user's
   language without changing its actions or deadline consequence.

### Evaluation Records

```markdown
You have {evaluationCount} evaluation records:

| # | Service Name | Job ID | Status | Evaluation Started | Key Time |
|---|---|---|---|---|---|
| {n} | {serviceName} | {jobId} | {localizedStatusLabel} | {evaluationStarted} | {keyTime} |

Reply with a number or Job ID to view the evaluation details.
```

Display rules:

1. Number records sequentially and show the full Job ID.
2. Render and translate the CLI `statusLabel`; `evaluationStatus` is the
   stable machine key.
3. Use the CLI-provided evaluation-started and key-time values directly.
4. Render `Key Time` when at least one returned value is present.
5. Restrict selection to `nextAction[id=view_arbitration].params.allowedJobIds`.

### Evaluation Details

```markdown
### Evaluation Details

- Service Name: {serviceName}
- Job ID: {jobId}
- Requested Refund: {requestedRefund}
- Buyer’s Reason: {buyerReason}
- Evaluation Status: {localizedStatusLabel}
- Status Description: {localizedStatusDescription}
- Evaluation Result: {localizedVerdictDescription}
- Evaluation Started: {evaluationStarted}
```

Display rules:

1. Render only fresh `payload` fields returned by `arbitration-detail`.
2. Preserve the full Job ID and the buyer-authored reason.
3. Translate `statusLabel`, `statusDescription`, and `verdictDescription`
   into the conversation language.
4. Render `Evaluation Result` from `verdictDescription` when it is available.
   The CLI derives these descriptions from `taskStatus`, `arbitrationPhase`,
   and `verdict`; keep those raw fields as protocol keys.
5. Use the CLI-provided formatted time directly.
6. Render each available optional value and end after the detail block.
