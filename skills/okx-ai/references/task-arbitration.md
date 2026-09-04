# ASP Arbitration

This is the single Skill-side business entry for ASP arbitration.

## Action routing

Route actions returned by the latest structured arbitration progression.

| Action ID | Command | Completion handoff |
|---|---|---|
| `agree_refund` | `onchainos agent agree-refund <params.jobId> --agent-id <aspAgentId>` | Render the one-time refund result, then continue the existing Watch lifecycle. |
| `raise_arbitration` | `onchainos agent dispute raise <params.jobId> --reason "<params.reason>" --agent-id <aspAgentId>` | Continue the existing approve/confirm chain; render creation when `job_disputed` supplies its facts. |
| `sub_agree_refund` | `onchainos agent subscribe-agree-refund <params.jobId> --agent-id <aspAgentId>` | Render the subscription-period refund result, then continue Watch. |
| `raise_subscription_arbitration` | `onchainos agent subscribe-dispute <params.jobId> --agent-id <aspAgentId> [--reason "<params.reason>"]` | Continue the existing subscription creation chain; render creation when `sub_asp_dispute` supplies its facts. |
| `view_arbitration` | Ask the user to select one value from `params.allowedJobIds`; validate the selected `jobId`, then use the confirmation and detail steps below. | Return the read-only query result. |

Rules:

- Route the action from the latest structured result and preserve returned parameters exactly.
- Treat the rejection-card A/B reply as final confirmation and execute its sole resolved action after the freshness check passes.
- Route by stable Action ID; treat prose labels, backend event names, and natural-language errors as display or compatibility data.
- Emit the canonical Action IDs in this table. The CLI normalizes legacy `dispute_raise`, `sub_dispute`, and `view_dispute` values from existing local pending cards.
- Return `unsupported_action` with `nextAction=[]` when the Action ID is absent from this table.

Contents: [open rejection decision](#open-the-rejection-decision) → [deliver decision card](#deliver-the-decision-card) → [resolve decision](#resolve-the-users-decision); or [query arbitration cases](#query-arbitration-cases) → [query detail](#query-an-arbitration-detail) → [lifecycle handoff](#existing-lifecycle-handoff) → [output templates](#output-templates).

## Protocol contract

Use CLI results for facts, freshness, choice resolution, phase, verdict, and executable actions. Emit the canonical arbitration phases and Action IDs above. Preserve protocol values `disputed`, `job_disputed`, `sub_asp_dispute`, `dispute_approved`, `dispute_resolved`, `agent dispute`, and `disputeRoundStatus` at the backend boundary.

## Open the rejection decision

A structured rejection event or an explicit merchant request for a specified rejected task opens this flow. Surface the decision card first. Resolve its choice from a later user message received after the active `[USER_DECISION_REQUEST]` block.

1. Resolve the exact `jobId`, ASP `agentId`, and rejection event (`job_rejected` or `sub_user_reject`).
2. For a structured event, run Activation #1 from `task-core.md`. For an explicit merchant request, choose `job_rejected` for a one-time task or `sub_user_reject` for a subscription period, then run:

   ```text
   onchainos agent next-action --role asp --agentId <aspAgentId> \
     --message '{"event":"<job_rejected|sub_user_reject>","jobId":"<jobId>"}'
   ```

   The command refreshes the task or subscription facts.
3. Require `phase=arbitration_decision` and all five progression fields: `phase`, `decision`, `reason`, `nextAction`, and `payload`.
4. Render `decision=blocked` as the terminal result for this attempt.
5. Continue to card delivery only for `decision=requires_user_input` with `reason=delivery_rejected`. Map every other shape to `unsupported_progression` with `nextAction=[]`.

For a rejected `jobId`, reuse its active card or run the fresh progression and deliver a new decision card.

## Deliver the decision card

1. Render the `Decide refund or arbitration` template from the returned payload.
2. Build every choice mechanically as `{ "key": nextAction.key, "actionId": nextAction.id, "params": nextAction.params }`.
3. Run exactly one `request-prompt` command:

```text
onchainos agent pending-decisions-v2 request-prompt \
  --job-id <payload.jobId> --role asp --agent-id <aspAgentId> \
  --source-event <job_rejected|sub_user_reject> \
  --decision-id <payload.decisionId> \
  --choices-json '<choices built mechanically from nextAction>' \
  --user-content '<rendered arbitration decision card>' \
  --list-label '<payload.name> — <payload.amount> <payload.tokenSymbol>' \
  [--expires-at <valid returned deadline>]
```

4. Treat exit success with `OK` as confirmed card delivery.
5. End the current turn at this boundary. The delivered card is the user-facing result of the original arbitration intent.
6. Enter decision resolution only when the user sends a subsequent reply to that card.

A is full refund. B is arbitration. For subscriptions, preserve returned `decisionBindingKey` / `decisionBindingValue` through resolution and freshness checking. Populate period and service-stop facts from returned fields.

## Merchant opens an existing decision

For a routed request to handle a specified rejected task, surface its active decision state.

1. If the matching `[USER_DECISION_REQUEST]` is active, render it again and wait for a subsequent reply.
2. Otherwise run `onchainos agent pending-decisions-v2 list --format markdown`.
3. If the merchant identifies a listed `jobId`, activate its exact current index with `pending-decisions-v2 pick --index <N>`, render the returned card, and wait for a subsequent reply.
4. If several entries match and no job is identified, show the queue and wait for selection.
5. If the merchant supplied a `jobId` and no queue entry matches, enter `Open the rejection decision` and regenerate the card from fresh task facts.
6. When the target remains unresolved, request the target `jobId`.

Wait for the user's reply to the latest active card before executing arbitration or refund.

## Resolve the user's decision

Enter this section when an active `[USER_DECISION_REQUEST]` exists and the user has sent a later reply to it.

1. Accept A as a complete refund choice.
2. Accept B together with an arbitration reason in the user's own language. For B alone, keep the active card and ask for `B <reason>` in the current conversation language.
3. In CLI-driver mode, run the active block's pre-filled `resolve-with-sessionkey` command with its exact `decisionId`, `choices-json`, and expiry metadata.
4. In queue mode, run the active block's pre-filled `resolve-prompt` command against the persisted entry.
5. Pass the complete user reply verbatim and use the returned `selectedActionId` plus `params`.
6. Run `next-action` from the emitted relay envelope so the latest task or subscription detail is checked before a write action is returned.

- `ambiguous_choice`: leave the decision active and show the same card again.
- `decision_expired`, missing metadata, stale event, or job mismatch: return a blocked result with `nextAction=[]`.
- `phase=arbitration_decision`, `decision=ready`, `reason=user_choice_resolved`: execute the sole returned action through the Action routing table above.

The complete reply is the final confirmation. A routes immediately to the corresponding refund. B with its reason routes immediately to the one-time or subscription arbitration command, preserving the user's reason exactly.

## Query arbitration cases

Run `arbitration-list` for filed arbitration cases.

### Select identity

Keep an explicitly supplied User or ASP Agent ID. In an ASP task/envelope context, keep that ASP `agentId`. Otherwise run `onchainos agent my-agents`, keep roles User (`1`) and ASP (`2`), show the candidates, and wait for an explicit user choice.

### List and select

1. Run `onchainos agent arbitration-list --agent-id <selectedAgentId> [--page <n>] [--page-size <n>]`.
2. Require `phase=arbitration_list`. Render `payload.items[]` with `jobId`, description, occurrence time, and task status; add the verdict when returned.
3. Treat `nextAction[id=view_arbitration].params.allowedJobIds` as the current selection allowlist.
4. If the list is empty, render the empty arbitration state and finish the query.
5. When the user selects a listed case, validate the exact `jobId` against the allowlist and render the read-only confirmation card.

## Query an arbitration detail

1. Resolve the target from an explicitly supplied `jobId` or the latest arbitration-list selection.
2. Run `onchainos agent arbitration-detail <jobId> --agent-id <selectedAgentId>` once to validate access and populate the confirmation card.
3. Render a blocked lookup result for a missing or inaccessible case.
4. Render the `Confirm a case` card for an accessible case and wait for A or B.

### Confirm and show fresh detail

Keep this A/B state separate from the rejection decision card:

- A = View Details. Rerun `arbitration-detail` for fresh facts, then render `phase=arbitration_detail` by `payload.arbitrationPhase` and `payload.verdict`.
- B = Back to Arbitration Cases. Rerun `arbitration-list` and render the fresh list.
- Any ambiguous reply keeps the current confirmation card active.

Detail template selection is deterministic:

- `evidence_preparation` → evidence preparation;
- `in_progress` → arbitration in progress;
- `resolved` + `asp_won` → ASP won;
- `resolved` + `asp_lost_auto_refund` → ASP lost and automatic refund;
- `unknown` or an unsupported combination → map directly to the unknown-phase template.

## Existing lifecycle handoff

After a resolved write action, continue the existing ASP and Watch lifecycle. That lifecycle owns `dispute_approved`, `job_disputed`, `sub_asp_dispute`, refund events, evidence events, and `dispute_resolved` automation.

## Output templates

Render returned fields in the user's language. Include optional lines when their values are available.

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
   Status: {taskStatus}
   {Filed: occurredAt}
   {Verdict: verdict}

Reply with a sequence number.
```

Empty list:

```text
There are currently no arbitration cases for this account.
```

### Confirm a case

```text
View this arbitration case?
Job ID: {jobId}
Job: {description}
Status: {taskStatus}

A. View details
B. Back to arbitration cases
```

### View a case

```text
Arbitration: {description}
Job ID: {jobId}
Status: {arbitrationPhase}
{Deadline: deadline}
{Verdict: verdict}
{Amount: amount tokenSymbol}
{Fund destination: fundDestination}
{Refund: refundAmount tokenSymbol}
{Transaction: txHash}
```

### Show refund result

```text
Refund approved for {name}.
Job ID: {jobId}
Refund: {amount} {tokenSymbol}
{refund destination or zero-price result}
```

### Show arbitration started

```text
Arbitration started for {name}.
Job ID: {jobId}
{arbitrationId}
Evidence deadline: {evidenceDeadline}
```

### Show blocked result

```text
Unable to continue: {reason}
Job ID: {jobId}
{recovery guidance}
```
