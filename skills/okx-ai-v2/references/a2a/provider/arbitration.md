# A2A Arbitration

Own rejected-work decisions, arbitration case queries, and lifecycle handoffs.

## Intent routing

| Intent | Flow |
|---|---|
| View work currently eligible for arbitration | [Rejected candidates](#rejected-candidates) |
| Respond to a specified rejection or the active refund-or-arbitration card | [Rejection decision](#rejection-decision) |
| View filed arbitration cases | [Filed cases](#filed-cases) |
| Inspect a case or its ruling progress | [Case detail](#case-detail) |

Requests for work eligible for arbitration mean rejected-task candidates.
Requests for arbitration cases mean already-filed cases. Keep these result sets
separate.

- `可仲裁`, `待仲裁`, `哪些可以仲裁`, and `可以仲裁的任务` route to rejected
  candidates. These tasks reached `rejected` after the deliverable was rejected.
- `仲裁列表`, `已发起仲裁`, and `仲裁案件` route to filed cases.

Contents: [action routing](#action-routing); [rejected candidates](#rejected-candidates);
[rejection decision](#rejection-decision);
[filed cases](#filed-cases); [case detail](#case-detail);
[lifecycle handoff](#lifecycle-handoff); [output templates](#output-templates).

## Action routing

Execute the action returned by the latest arbitration command with its returned
parameters.

| Action ID | Command | Result presentation |
|---|---|---|
| `agree_refund` | `onchainos agent agree-refund <params.jobId> --agent-id <aspAgentId>` | State the refund submission result and task-result query method. |
| `raise_arbitration` | `onchainos agent dispute raise <params.jobId> --reason "<params.reason>" --agent-id <aspAgentId>` | State the arbitration submission result and arbitration-detail query method. |
| `sub_agree_refund` | `onchainos agent subscribe-agree-refund <params.jobId> --agent-id <aspAgentId>` | State the refund submission result and relevant returned fields. |
| `raise_subscription_arbitration` | `onchainos agent subscribe-dispute <params.jobId> --reason "<params.reason>" --agent-id <aspAgentId>` | State the arbitration submission result and arbitration-detail query method. |
| `view_arbitration` | Validate a selected `jobId` against `params.allowedJobIds`, then run the case-detail flow. | [View a case](#view-a-case) |

For ordinary action results, give one concise localized update containing the
outcome, relevant returned fields, and the next available query.

## Rejected candidates

Run:

```text
onchainos agent tasks --status rejected --agent-id <aspAgentId> --page 1 --limit 20
```

Render [View rejected candidates](#view-rejected-candidates) with the returned
task fields and pagination. A filed-case query enters [Filed cases](#filed-cases)
instead.

## Rejection decision

### Open

A `job_rejected` or `sub_user_reject` event opens this flow. An inbound
structured event uses its top-level `agentId` and complete current `message`
object:

```text
onchainos agent next-action --role auto --agentId <envelope.agentId> \
  --message '<complete envelope.message as one JSON string>'
```

An explicit request for a specified rejected task uses a compact fresh
progression call:

```text
onchainos agent next-action --role asp --agentId <aspAgentId> \
  --message '{"event":"<job_rejected|sub_user_reject>","jobId":"<jobId>"}'
```

Render [Decide refund or arbitration](#decide-refund-or-arbitration) with the
returned `payload`, then present the decision card.

### Present the decision card

Build each choice from the returned `nextAction`:

```json
{"key":nextAction.key,"actionId":nextAction.id,"params":nextAction.params}
```

Run one card request:

```text
onchainos agent pending-decisions-v2 request-prompt \
  --job-id <payload.jobId> --role asp --agent-id <aspAgentId> \
  --source-event <job_rejected|sub_user_reject> \
  --decision-id <payload.decisionId> \
  --choices-json '<choices from nextAction>' \
  --user-content '<rendered decision card>' \
  --list-label '<payload.name> — <payload.amount> <payload.tokenSymbol>' \
  [--expires-at <returned deadline>]
```

After delivery, end the turn and wait for the next user reply. Preserve
subscription `decisionBindingKey` and `decisionBindingValue` through
resolution.

### Reopen an existing decision

1. Re-render the active `[USER_DECISION_REQUEST]` for the specified job.
2. When no matching card is active, run
   `onchainos agent pending-decisions-v2 list --format markdown`.
3. Activate a selected entry with
   `onchainos agent pending-decisions-v2 pick --index <N>`.
4. When the specified job has no queue entry, regenerate its card through
   [Open](#open).
5. Wait for the reply to the latest active card.

### Resolve A or B

Accept these final decisions:

- `A`: approve the full refund.
- `B <reason>`: open arbitration with the user's reason preserved verbatim.

For B without a reason, keep the card active and request `B <reason>` in the
current conversation language.

1. Run the active card's pre-filled `resolve-with-sessionkey` command in
   CLI-driver mode, or its pre-filled `resolve-prompt` command in queue mode.
2. Pass the complete reply verbatim.
3. Run `next-action` from the returned `validate_arbitration_choice` action
   with its exact `role`, `agentId`, and complete
   `message`.
4. Execute the returned action through [Action routing](#action-routing) and
   present its result using the action table.

For `ambiguous_choice`, render [Decide refund or arbitration](#decide-refund-or-arbitration)
again. For `arbitration_reason_required`, keep the active decision and request
`B <reason>` in the current conversation language. For an unavailable
decision, state the returned reason and recovery guidance.

The complete A or B reply is the final confirmation. A executes the matching
full-refund action. B with its reason executes the matching arbitration action
in the current conversation.

## Filed cases

Keep an explicitly supplied ASP Agent ID. In an active provider task envelope,
keep its bound `agentId`. Otherwise run `onchainos agent my-agents` and retain
ASP (`2`) identities. Use the sole match, or present the ASP identities for
selection when several are available.

Run:

```text
onchainos agent arbitration-list --agent-id <aspAgentId> [--page <n>] [--page-size <n>]
```

Render [View arbitration cases](#view-arbitration-cases) with
`payload.items[]` in CLI order. Use
`nextAction[id=view_arbitration].params.allowedJobIds` for case selection. Use
the empty-list variant when `payload.items[]` is empty.

## Case detail

Resolve the case from an explicit `jobId` or the latest allowed list selection.
Run once:

```text
onchainos agent arbitration-detail <jobId> --agent-id <aspAgentId>
```

For a missing or inaccessible case, state the returned reason and recovery
guidance. For an accessible case, render [View a case](#view-a-case) directly
with the fresh detail fields. Localize status with:

| `payload.arbitrationPhase` | `payload.verdict` | Display |
|---|---|---|
| `evidence_preparation` | any | Evidence preparation |
| `in_progress` | any | Arbitration in progress |
| `resolved` | `asp_won` | ASP won |
| `resolved` | `asp_lost_auto_refund` | ASP lost; automatic refund |
| other | other | Unknown phase |

## Lifecycle handoff

- A one-time B decision submits `dispute raise` in the current conversation.
  The task sub-session receives `dispute_approved` and runs `dispute confirm`
  once with the original reason when available.
- `job_disputed` starts the independent evidence flow after a fresh
  `disputed` status check. That flow resolves the buyer, reads task chat
  history, attaches saved deliverables when available, uploads evidence, and
  waits for `dispute_resolved`.
- A subscription B decision runs `subscribe-dispute`; `sub_asp_dispute`
  supplies the dispute-creation facts.
- Refund, evidence, and ruling events continue through their scoped lifecycle
  handlers and `../../runtime/watch.md`.

## Output Templates

Render returned fields in the user's language. Include optional lines when
their values are available. Fill placeholders from the current command result
and keep IDs and amount strings exact.

### View rejected candidates

```text
Tasks currently available for refund or arbitration:
{sequence}. {name or description}
   Job ID: {jobId}
   {Refund: amount tokenSymbol}
   {Rejected: rejectedAt}

Reply with a sequence number or Job ID.
```

Empty list:

```text
There are currently no rejected tasks available for arbitration.
```

### Decide refund or arbitration

```text
The user requested a refund for {name}.
Job ID: {jobId}
Refund: {amount} {tokenSymbol}
{Period: extraFields.subStartTime–extraFields.subEndTime}
{Deadline: extraFields.rejectWindowEndsAt or extraFields.expireTime}

A. Approve full refund
B. Start arbitration — reply with B followed by your reason
```

### View arbitration cases

```text
Arbitration cases:
{sequence}. {description}
   Job ID: {jobId}
   Status: {localized task status}
   {Filed: occurredAt}
   {Verdict: localized verdict}

Reply with a sequence number.
```

Empty list:

```text
There are currently no arbitration cases for this account.
```

### View a case

```text
Arbitration: {description}
Job ID: {jobId}
Status: {localized status mapped from arbitrationPhase and verdict}
{Deadline: deadline}
{Verdict: localized verdict}
{Amount: amount tokenSymbol}
{Fund destination: fundDestination}
{Refund: refundAmount tokenSymbol}
{Transaction: txHash}
```
