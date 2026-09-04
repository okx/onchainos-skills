# Task Output Templates

Use this reference when a task or subscription command returns the structured
progression fields `phase`, `decision`, `reason`, `nextAction`, and `payload`.
The templates in this file are canonical English semantic templates. Render them
in the user's current language, preserving every identifier, amount, timestamp,
action meaning, and A/B mapping. Use `nextAction` for routing and treat `action`
as legacy guidance.

## Dispute decision

Match `phase=dispute_decision` and `decision=requires_user_input`.

Use this standard layout:

```text
[Action Required] The user requested a full refund for <payload.name><for subscription: for the current period>.
<For subscription: The subscription has ended. Stop service.>
Respond by the deadline.
Job ID: <payload.jobId>
<For subscription when both values exist: Current period: <payload.extraFields.subStartTime>-<payload.extraFields.subEndTime>>
Refund amount: <payload.amount> <payload.tokenSymbol>
<When rejectWindowEndsAt or expireTime is valid: Deadline: <localized time>>
Choose:
A. Approve Refund
B. File a Dispute
```

One-time example:

```text
[Action Required] The User Agent rejected Data Analysis Report.
Job ID: 0x1234
Refund amount: 10 USDT
Deadline: 2026-09-04 18:00
Choose:
A. Approve Refund
B. File a Dispute
```

Display rules:

- For subscriptions, render `subStartTime` / `subEndTime` and service-stop wording.
- Render each optional line when its source value is available.
- Render the fields shown in the template using available source values.
- Render the A/B labels from the returned `nextAction` order.

### Rejection decision result

For a successful subscription refund result, use:

```text
[Refund Completed] You approved the refund request for the current period of <jobName>. The refund of <amount> <tokenSymbol> will be returned automatically to the user's wallet.
Job ID: <jobId>
No further action is required.
```

For a successful one-time refund result with price greater than zero, use:

```text
[Refund Completed] You approved the refund request for <jobName>. The refund of <amount> <tokenSymbol> will be returned to the User Agent wallet.
Job ID: <jobId>
No further action is required.
```

For a successful zero-price one-time refund result, replace the first paragraph with: `[Refund Process Completed] You approved the refund request for <jobName>. No payment was made for this job, so no refund is required.`

For a confirmed `job_disputed` or `sub_asp_dispute` creation result, use:

```text
[Dispute Filed] Submit evidence for <jobName> by <evidenceDeadline>.
<When available: Dispute ID: <disputeId>>
Job ID: <jobId>
Refund amount: <amount> <tokenSymbol>
Include the job requirements, delivery records, messages, on-chain proof, and a response to each refund claim. Missing the deadline will approve the refund by default.
```

Populate the dispute ID and evidence deadline from CLI/event values when available.

## Dispute query

### Dispute list

Match `phase=dispute_list`.

Use this compact list:

```text
[Current ASP Dispute Cases]
- Job ID: <jobId>
  - <description> - <occurredAt or —> - <taskStatus>< - <ruling, when returned>>
- Job ID: <jobId>
  - <description> - <occurredAt or —> - <taskStatus>
```

Display rules:

- Use `payload.items[].jobId` as the visible identifier.
- Use `payload.items[].taskStatus` as the displayed status.
- Keep each list item to Job ID, description, occurrence time, task status, and an available normalized ruling.
- Localize the title with the same meaning as `Current ASP Dispute Cases`.
- When `payload.items` is empty, output `[No Dispute Cases] There are currently no dispute cases associated with your account.`
- Show the normalized ruling for `verdict=asp_won` or `verdict=asp_lost_auto_refund`.

### Dispute query confirmation

Use this card when `task-dispute.md` requests confirmation for one validated `jobId`:

```text
Do you want to view this dispute case?
Job ID: <jobId>
Job: <description>
Filed at: <occurredAt or —>
Current status: <taskStatus>
Choose:
A. View Details
B. Back to Dispute Cases
```

For `reason=dispute_not_found` or an inaccessible dispute result, use:

```text
[Case Lookup Required] Verify that Job ID <jobId> identifies a dispute case available to this account.
Then try again, or reply "Check Disputes" to view the dispute list.
```

### Dispute query result

Match `phase=dispute_detail`. Select by `payload.disputePhase` and, for a resolved dispute, `payload.verdict`.

#### Evidence preparation

Match `payload.disputePhase=evidence_preparation`.

```text
[Arbitration Status] The arbitration regarding <description> is in the evidence preparation stage.
<When available: Arbitration ID: <arbitrationId>>
Job ID: <jobId>
Evidence deadline: <prepareEndTime>
Current status: Preparing Evidence
Evidence must be submitted before the deadline; otherwise the refund will be approved by default.
```

#### In progress

Match `payload.disputePhase=in_progress`.

```text
[Arbitration Status] The arbitration regarding <description> is in progress.
<When available: Arbitration ID: <arbitrationId>>
Job ID: <jobId>
<When available: Estimated completion time: <roundEndTime>>
Current status: Under Arbitration
<When available: Evidence status: <evidenceStatus>>
<When available: <supplementInstruction>>
```

#### ASP won

Match `payload.disputePhase=resolved` and `payload.verdict=asp_won`.

```text
[Dispute Won] The dispute for <description> was resolved in the ASP's favor.
Job ID: <jobId>
<Subscription: The system will automatically collect the current-period income of <amount> <tokenSymbol>.>
<One-time, amount > 0: The system has automatically collected job income of <amount> <tokenSymbol>.>
<One-time, amount = 0: The job is complete.>
<When available: Transaction hash: <txHash>>
```

#### ASP lost with automatic refund

Match `payload.disputePhase=resolved` and `payload.verdict=asp_lost_auto_refund`.

```text
[Dispute Lost] The dispute for <description> was resolved in the User Agent's favor.
Job ID: <jobId>
<Amount > 0: A full refund of <amount> <tokenSymbol> will be returned automatically to the User Agent wallet.>
<Amount = 0: No payment was made, so no refund is required.>
<When available: Transaction hash: <txHash>>
```

#### Unknown phase

Match `payload.disputePhase=unknown`. Show the returned Job ID, description, and task status, then state that additional phase detail is currently unavailable.

## Common layout

Use this layout:

```text
<one-sentence conclusion>

<only the fields needed for this phase>

1. <action label>
2. <action label>

Reply with a number.
```

Rules:

- Render every returned `nextAction` item in order; number from 1.
- Mark the item with `recommend=true` as the recommended option in prose when
  useful, preserving its original number and meaning.
- Populate actions from the CLI `nextAction` list.
- Numbers are valid only for the latest response.
- If there is one action, execute it when safe or show it directly when the
  flow requires no user decision.
- Render the user-facing conclusion, relevant fields, and action labels.

## `decision=blocked`

Use a concise status result and a recovery-oriented action list.

```text
<operation> cannot continue: <reason message>.

Phase: <localized phase>
<relevant payload fields>

1. <recommended recovery>
2. <alternative>
3. <cancel or return>

Reply with a number.
```

For `task_create_prepare`, use these reason mappings:

| Reason | Result | Typical action label |
|---|---|---|
| `login_required` | Login is required. | Log in |
| `user_identity_required` | A User Agent is required. | Register User Agent |
| `legacy_a2mcp_flow_removed` | This legacy task-based A2MCP flow is no longer supported. Start again from the confirmed-service direct invocation. | Stop |
| `unsupported_service_type` | This service type is not supported for task creation. | Stop |
| `duplicate_subscription` | An active subscription already exists. | Restore listening / Stop |
| `insufficient_balance` | The balance is insufficient. | Fund account |

Use the exact action IDs returned by `nextAction`; the labels above are display
guidance only.

## `decision=requires_user_input`

```text
<operation> needs more information.

Missing: <payload.requiredParams>

1. Provide the missing information
2. Choose another service
3. Cancel

Reply with a number, or provide the requested values directly.
```

Only ask for fields present in the current payload. Collect all missing required
fields together.

## `decision=ready`

Use a confirmation card for task or subscription creation. The business
reference defines which payload fields are required and how prices, trial, and
auto-renewal are rendered.

```text
<operation> is ready for confirmation.

| Item | Value |
|---|---|
| Service | <service> |
| Provider | <provider> |
| Parameters | <confirmed parameters> |
| Price | <price> |

1. Confirm and continue
2. Modify
3. Cancel

Reply with a number.
```

For `task_create_prepare`, `nextAction.id=open_create_playbook` means to open
the task-creation reference and continue its confirmation flow; it does not
mean that the subscription has already been created.

For `agent create-task`, `phase=creation`, `decision=ready`, and
`reason=broadcast_submitted` mean the create-and-fund UserOperation was
submitted but is not yet final. Render `payload.jobId`, `payload.broadcast.txHash`
when present, and the locally saved attachment count. Then execute the returned
`nextAction.id=watch_task`; do not offer `set-payment-mode`, ASP apply, or Buyer
accept.

For `agent create-subscribe`, the same progression state means the subscription
UserOperation was submitted but is not yet final. Require
`payload.type=204`, `payload.bizType=204`, and use `payload.jobId` as the sole
subscription identifier. Render the broadcast transaction hash when present,
attachment count, and whether automatic execution was configured. Then execute
the returned `nextAction.id=watch_task`. Do not establish the A2A session in
this creation step; the `sub_open` event owns that transition.

For `phase=service_routing` and `nextAction.id=invoke_a2mcp`, do not render the
generic task-creation confirmation card above. Open
`a2mcp-direct-invoke.md`. Preserve `payload.serviceSnapshot` verbatim; that
reference owns parameter collection, supported-token and balance display,
funding recovery, and the final mutually exclusive Confirm/Cancel card.

## `task_create_prepare` phase mapping

| Phase | Decision | Next action IDs |
|---|---|---|
| `login_validation` | `blocked` | `login` |
| `identity_validation` | `blocked` | `register_user_agent` |
| `service_validation` | `blocked` | `stop` |
| `service_routing` | `ready` | `invoke_a2mcp` |
| `subscription_validation` | `blocked` | `restore_subscription`, `stop` |
| `payment_validation` | `blocked` | `fund_account` |
| `creation` | `ready` | `open_create_playbook` |
