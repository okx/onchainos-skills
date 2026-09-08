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

Localize each list title and every table header. In Chinese, use `待处理退款申请`
with `序号`、`服务名称`、`Job ID`、`任务类型`、`申请退款金额`、`响应截止时间` for the
pending-request table, and `评审任务` with `序号`、`服务名称`、`任务 ID`、`状态`、
`评审开始时间`、`关键时间` for the Evaluation-record table.

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
5. In Chinese, use the document's recommendation verbatim: `可回复序号或 Job ID 查看详情，并选择“同意退款”或“发起评审”。逾期未处理将自动全额退款。`

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
2. Render and localize the CLI `statusLabel`; `evaluationStatus` is the stable
   machine key. In a Chinese conversation use `证据准备中`, `评审中`, `已裁决`,
   and `评审状态暂不可用` for `Evidence preparation`, `Evaluating`, `Decided`,
   and `Status unavailable`, respectively.
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
3. Localize `statusLabel`, `statusDescription`, and `verdictDescription` into
   the conversation language. In Chinese, use these exact concise status
   mappings:
   - `Evidence preparation` -> `证据准备中`
   - `Evaluating` -> `评审中`
   - `Decided` -> `已裁决`
   - `Status unavailable` -> `评审状态暂不可用`
   Use these status descriptions:
   - `Evidence is collected automatically. Please wait.` -> `证据自动收集中，请等候。`
   - `Evaluators are reviewing the submitted evidence.` -> `评审方正在审查双方提交的证据`
   - `The Evaluation has concluded.` -> `本次评审已完成`
   - `The Evaluation status is currently unavailable.` -> `当前返回信息不足，暂无法确定评审状态`
4. Render `Evaluation Result` from `verdictDescription` when it is available.
   In Chinese, use these exact result mappings:
   - `verdict=null` -> `尚未产生裁决`
   - `verdict=asp_won` -> `ASP 胜诉，任务款项已结算给 ASP`
   - `verdict=asp_lost_auto_refund` -> `用户胜诉，退款成功`
   - an unrecognized verdict -> `裁决结果暂无法识别`
   The CLI derives these descriptions from `taskStatus`, `arbitrationPhase`,
   and `verdict`; keep those raw fields as protocol keys.
5. Use the CLI-provided formatted time directly.
6. Render each available optional value and end after the detail block.
