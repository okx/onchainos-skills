# ASP Arbitration

Use this provider-side entry for rejected-work decisions, arbitration filing,
filed-case queries, and lifecycle handoffs.

## Intent routing

| Intent | Flow |
|---|---|
| View work currently eligible for arbitration | [Rejected candidates](#rejected-candidates) |
| Start arbitration for a specified rejected task | [Start arbitration directly](#start-arbitration-directly) |
| Respond to a rejection event or the active refund-or-arbitration card | [Open the rejection decision](#open-the-rejection-decision) |
| Receive `[ARBITRATION_REASON_CONTEXT]` | [Reason handoff](#reason-handoff) |
| View filed arbitration cases | [Query arbitration cases](#query-arbitration-cases) |
| Inspect a case or its ruling progress | [Query an arbitration detail](#query-an-arbitration-detail) |

Requests for work eligible for arbitration mean rejected-task candidates.
Requests for arbitration cases mean already-filed cases. Keep these result sets
separate.

- `可仲裁`, `待仲裁`, `哪些可以仲裁`, and `可以仲裁的任务` route to rejected
  candidates. These tasks reached `rejected` after the deliverable was rejected.
- `仲裁列表`, `已发起仲裁`, and `仲裁案件` route to filed cases.

Contents: [action routing](#action-routing); [protocol contract](#protocol-contract);
[rejected candidates](#rejected-candidates);
[start arbitration directly](#start-arbitration-directly);
[open rejection decision](#open-the-rejection-decision);
[deliver decision card](#deliver-the-decision-card);
[resolve decision](#resolve-a-or-b); [query cases](#query-arbitration-cases);
[query detail](#query-an-arbitration-detail);
[reason handoff](#reason-handoff); [lifecycle handoff](#lifecycle-handoff);
[output templates](#output-templates).

## Action routing

Execute the action returned by the latest structured arbitration progression
with its returned parameters.

| Action ID | Command | Result presentation |
|---|---|---|
| `agree_refund` | `onchainos agent agree-refund <params.jobId> --agent-id <aspAgentId>` | State the refund submission result and task-result query method. |
| `raise_arbitration` | `onchainos agent dispute raise <params.jobId> --reason "<params.reason>" --agent-id <aspAgentId>` | State the arbitration submission result and arbitration-detail query method. |
| `sub_agree_refund` | `onchainos agent subscribe-agree-refund <params.jobId> --agent-id <aspAgentId>` | State the refund submission result and relevant returned fields. |
| `raise_subscription_arbitration` | `onchainos agent subscribe-dispute <params.jobId> --reason "<params.reason>" --agent-id <aspAgentId>` | State the arbitration submission result and arbitration-detail query method. |
| `view_arbitration` | Validate a selected `jobId` against `params.allowedJobIds`, then run the detail flow. | [View a case](#view-a-case) |

Route by the stable Action ID and preserve every returned parameter exactly.
The CLI normalizes legacy `dispute_raise`, `sub_dispute`, and `view_dispute`
values from persisted cards. An unregistered Action ID returns
`unsupported_action` with `nextAction=[]`.

For ordinary action results, give one concise localized update containing the
outcome, relevant returned fields, and the next available query.

## Protocol contract

Use CLI results for current facts, freshness, choice resolution, phase,
verdict, and executable actions. Preserve `disputed`, `job_disputed`,
`sub_asp_dispute`, `dispute_approved`, `dispute_resolved`, `agent dispute`, and
`disputeRoundStatus` at the backend boundary.

## Rejected candidates

Select the candidate source from the request:

```text
# Rejected one-time tasks
onchainos agent tasks --status rejected --agent-id <aspAgentId> --page 1 --limit 20

# Rejected subscription periods
onchainos agent my-subscriptions --role provider --status rejected
```

Render [View rejected candidates](#view-rejected-candidates) with the returned
task or period fields and available pagination. A filed-case query enters
[Query arbitration cases](#query-arbitration-cases).

## Start arbitration directly

Treat an explicit instruction to arbitrate a specified task as authorization
to start arbitration. The arbitration reason completes that instruction.

1. Resolve the exact `jobId` and ASP Agent ID.
2. Find the exact task in the fresh rejected-candidate sources above. Its source
   identifies a one-time task or subscription period and confirms current
   eligibility.
3. Preserve a reason supplied in the same request verbatim. When the request
   lacks a reason, ask only for the arbitration reason and treat the next plain
   reply as that reason.
4. For a one-time task, run:

   ```text
   onchainos agent dispute raise <jobId> --reason "<reason>" --agent-id <aspAgentId>
   ```

5. For a subscription period, run:

   ```text
   onchainos agent subscribe-dispute <jobId> --reason "<reason>" --agent-id <aspAgentId>
   ```

6. Present the submission result and the arbitration-detail query method.

Decision cards apply to event-driven rejection decisions and active cards.

## Open the rejection decision

A `job_rejected` or `sub_user_reject` event opens this flow. For a structured
event, enter through `../core.md` and use the top-level `agentId` plus the
complete current `message` object:

```text
onchainos agent next-action --role auto --agentId <envelope.agentId> \
  --message '<complete envelope.message as one JSON string>'
```

Render [Decide refund or arbitration](#decide-refund-or-arbitration) from the
returned payload, then deliver the decision card.

## Deliver the decision card

Build each choice mechanically from the returned `nextAction`:

```json
{"key":nextAction.key,"actionId":nextAction.id,"params":nextAction.params}
```

Run one card request:

```text
onchainos agent pending-decisions-v2 request-prompt \
  --job-id <payload.jobId> --role asp --agent-id <aspAgentId> \
  --source-event <job_rejected|sub_user_reject> \
  --decision-id <payload.decisionId> \
  --choices-json '<choices built from nextAction>' \
  --user-content '<rendered decision card>' \
  --list-label '<payload.name> — <payload.amount> <payload.tokenSymbol>' \
  [--expires-at <returned deadline>]
```

After successful card delivery, end the turn and wait for the next reply.
Preserve subscription `decisionBindingKey` and `decisionBindingValue` through
resolution.

## Open an existing decision

1. Re-render the active `[USER_DECISION_REQUEST]` for the specified job.
2. When a matching card is absent, run
   `onchainos agent pending-decisions-v2 list --format markdown`.
3. Activate a selected entry with
   `onchainos agent pending-decisions-v2 pick --index <N>`.
4. When the specified job has no queue entry, regenerate its card through
   [Open the rejection decision](#open-the-rejection-decision).
5. Wait for the reply to the latest active card.

## Resolve A or B

Accept these final decisions:

- `A`: approve the full refund.
- `B <reason>`: open arbitration with the supplied reason preserved verbatim.

For B with an empty reason, keep the card active and request `B <reason>` in
the current conversation language.

1. Run the active card's pre-filled `resolve-with-sessionkey` command in
   CLI-driver mode, or its pre-filled `resolve-prompt` command in queue mode.
2. Pass the complete reply verbatim.
3. Run `next-action` from the returned `validate_arbitration_choice` action
   with its exact `role`, `agentId`, and complete `message`.
4. Execute the returned action through [Action routing](#action-routing) in the
   current conversation and present its concise result.

For `ambiguous_choice`, render
[Decide refund or arbitration](#decide-refund-or-arbitration) again. For
`arbitration_reason_required`, keep the active decision and request
`B <reason>`. For `decision_metadata_missing`, `decision_expired`,
`unsupported_action`, `stale_event`, or a job mismatch, state the returned
reason and recovery guidance.

The complete A or B reply is the final confirmation. A executes the matching
full-refund action. B with its reason executes the matching arbitration action
in the current conversation.

## Reason handoff

For a one-time arbitration, `dispute raise` sends one local task-session
message before broadcasting the approval transaction:

```text
[ARBITRATION_REASON_CONTEXT]
{"version":1,"intent":"arbitration_reason_context","jobId":"<jobId>","providerAgentId":"<aspAgentId>","reason":"<exact reason>","reasonB64":"<URL-safe base64>","confirmArgs":[...]}
```

When this message arrives:

1. Match `jobId` and `providerAgentId` to the current task conversation.
2. Keep `reason` and `reasonB64` exactly in the conversation context.
3. End the turn and continue when the matching `dispute_approved` event
   arrives.

For that `dispute_approved` event, read the latest matching context and run its
confirmation once:

```text
onchainos agent dispute confirm <jobId> \
  --reason-b64 <reasonB64> --agent-id <aspAgentId>
```

The CLI decodes `reasonB64` back to the exact original reason before building
the dispute broadcast. A missing matching context returns
`arbitration_reason_context_missing` and ends the event turn.

## Query arbitration cases

Keep an explicitly supplied ASP Agent ID. In an active provider task envelope,
keep its bound `agentId`. Otherwise run `onchainos agent my-agents`, retain ASP
(`2`) identities, and use the sole match or present matching identities for
selection.

Run:

```text
onchainos agent arbitration-list --agent-id <aspAgentId> [--page <n>] [--page-size <n>]
```

Render [View arbitration cases](#view-arbitration-cases) with
`payload.items[]` in CLI order. Use
`nextAction[id=view_arbitration].params.allowedJobIds` for case selection. Use
the empty-list variant when `payload.items[]` is empty.

## Query an arbitration detail

Resolve the case from an explicit `jobId` or the latest allowed list selection.
Run once:

```text
onchainos agent arbitration-detail <jobId> --agent-id <aspAgentId>
```

For a missing or inaccessible case, state the returned reason and recovery
guidance. For an accessible case, render [View a case](#view-a-case) directly
with fresh detail fields. Localize status with:

| `payload.arbitrationPhase` | `payload.verdict` | Display |
|---|---|---|
| `evidence_preparation` | any | Evidence preparation |
| `in_progress` | any | Arbitration in progress |
| `resolved` | `asp_won` | ASP won |
| `resolved` | `asp_lost_auto_refund` | ASP lost; automatic refund |
| other | other | Unknown phase |

## Lifecycle handoff

- A one-time B decision submits `dispute raise` in the current conversation.
  The command sends the exact reason to the task sub-session, then broadcasts
  approval. The task sub-session receives `dispute_approved` and runs
  `dispute confirm` once with that reason.
- `job_disputed` starts the independent evidence flow after a fresh
  `disputed` status check. That flow resolves the buyer, reads task chat
  history, attaches saved deliverables when available, uploads evidence, and
  waits for `dispute_resolved`.
- A subscription B decision runs `subscribe-dispute`; `sub_asp_dispute`
  supplies the dispute-creation facts.
- Refund, evidence, and ruling events continue through their scoped lifecycle
  handlers and `../../runtime/watch.md`.

## Output templates

Use fixed templates for decision cards and structured list/detail views.
Render returned fields in the conversation language, include optional lines
when values are available, and keep IDs and amount strings exact.

### View rejected candidates

```text
Tasks or subscription periods currently available for refund or arbitration:
{sequence}. {name or description}
   Job ID: {jobId}
   {Type: one-time task or subscription period}
   {Refund: amount tokenSymbol}
   {Rejected: rejectedAt}

Reply with a sequence number or Job ID.
```

Empty list:

```text
There are currently no rejected tasks or subscription periods available for arbitration.
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
