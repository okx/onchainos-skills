# ASP Dispute Interaction

This reference owns three user-interactive boundaries: ASP handling of `job_rejected` / `sub_user_reject`, reopening one of those pending decisions from a free-form merchant request, and dispute list/progress queries. Existing raise, confirm, refund, evidence-upload, watch, and verdict flows remain authoritative after an action is selected.

## Rejection event

Run Activation #1 from `task-core.md`. For these events, `next-action` returns the progression contract directly.

- `decision=blocked`: return the blocked result and end the turn. For `decision_metadata_missing`, replace the legacy card after a fresh rejection event or successful fact query.
- `decision=requires_user_input`, `reason=delivery_rejected`: render the unified template from `task-output-templates.md`, then push exactly one pending decision using returned facts and actions.
- Any other shape: return a blocked result and end the turn. Accept executable actions from structured progression.

```text
onchainos agent pending-decisions-v2 request-prompt \
  --job-id <jobId> --role asp --agent-id <aspAgentId> \
  --source-event <job_rejected|sub_user_reject> \
  --decision-id <payload.decisionId> \
  --choices-json '<A/B choices built from nextAction>' \
  --user-content '<rendered unified card>' \
  --list-label '<name> — <amount> <tokenSymbol>' \
  [--expires-at <valid event deadline>]
```

Render the card exactly with the `task-output-templates.md` standard. Its stable action mapping is A. Agree to a full refund and B. File a dispute. Add subscription period/service-stop copy for `taskType=subscription` and add a deadline when a valid deadline field exists. Use this unified visual structure for both task types.

Build `--choices-json` mechanically from each returned action as `{ "key": nextAction.key, "actionId": nextAction.id, "params": nextAction.params }`. Preserve values exactly.

## Merchant asks to handle a decision

Recognize `handle the refund request`, `handle the buyer rejection`, `handle this task decision`, `handle <jobId>`, `view pending refunds`, and semantic equivalents in any language as rejection-decision intents when the caller is ASP. They take precedence over generic task-list and task-status intents.

1. If the matching `[USER_DECISION_REQUEST]` is already active, render that card again using the standard decision template.
2. Otherwise run `onchainos agent pending-decisions-v2 list --format markdown` and use its current number-to-job mapping.
3. If the user identified a listed `jobId`, activate that exact entry with `pending-decisions-v2 pick --index <N>` and render the returned full card. If several entries match the intent but no task is identified, show the queue and wait for a selection.
4. If no matching `job_rejected` / `sub_user_reject` card exists, report that there is no active rejection decision to handle. Use the active pending-decision queue as the decision-card source.

Opening a decision is read-only. Begin resolution after the user replies A/B.

## User reply

First bind A/B to the latest active card. Keep separate state bindings for rejection decision cards and dispute-query confirmation cards because their A/B meanings differ.

For a rejection decision card, use the active decision block's resolver with the user's verbatim reply. The CLI alone maps A/B to `selectedActionId` and `params`.

- `ambiguous_choice`: keep the decision active, show the same card again, and remain in the read-only state.
- `decision_expired`: close it and stop.
- `decision=ready`, `reason=user_choice_resolved`: route the sole action through `task-action-routing.md`.

The A/B reply is final confirmation and routes immediately. A means full refund. B means dispute and may include an optional reason; otherwise preserve the existing downstream default.

## Execute resolved rejection action

Execute the sole `nextAction` returned with `decision=ready` and `reason=user_choice_resolved`:

| Action ID | Command | Completion handoff |
|---|---|---|
| `agree_refund` | `onchainos agent agree-refund <params.jobId> --agent-id <aspAgentId>` | Render the positive-price or zero-price one-time refund result from returned facts, then continue Watch. |
| `dispute_raise` | `onchainos agent dispute raise <params.jobId> --reason "<params.reason or existing default>" --agent-id <aspAgentId>` | Continue the existing approve/confirm flow and render the creation result when `job_disputed` provides its facts. |
| `sub_agree_refund` | `onchainos agent subscribe-agree-refund <params.jobId> --agent-id <aspAgentId>` | Render the subscription refund result from returned facts, then continue Watch. |
| `sub_dispute` | `onchainos agent subscribe-dispute <params.jobId> --agent-id <aspAgentId> [--reason "<params.reason>"]` | Continue the existing creation flow and render the creation result when `sub_asp_dispute` provides its facts. |

After dispute creation, continue the existing automated evidence behavior and Watch lifecycle.

## Query dispute

For ASP intents `dispute list`, `query disputes`, `current disputes`, `my disputes`, `dispute progress`, and semantic equivalents in any language:

1. With no jobId, run `onchainos agent tasks --status disputed --agent-id <aspAgentId>` and render its `phase=dispute_list` result.
2. Use `payload.items[].jobId` and `nextAction.params.allowedJobIds` from that result as the selectable case set.
3. When the merchant selects an item, confirm that its `jobId` belongs to the current selectable case set and render the query-confirmation card for that `jobId`.
4. When the merchant directly provides a jobId, run `onchainos agent status <jobId> --agent-id <aspAgentId>`. Use a valid `phase=dispute_detail` result to populate the query-confirmation card, and render the `dispute_not_found` result for a missing, non-dispute, or inaccessible case.
5. Use the latest query-confirmation card as a read-only navigation step with A = `View Details` and B = `Back to Dispute Cases`. Keep this card state separate from the refund/dispute decision card.
6. On A, run `onchainos agent status <jobId> --agent-id <aspAgentId>` again and select the detail template from the fresh `payload.disputePhase` and `payload.verdict`.
7. On B, rerun `onchainos agent tasks --status disputed --agent-id <aspAgentId>` and render the fresh list. An invalid or ambiguous reply keeps the current confirmation card active and rerenders it.

Template selection is deterministic:

- `disputePhase=evidence_preparation` → evidence-preparation template.
- `disputePhase=in_progress` → in-progress template.
- `disputePhase=resolved, verdict=asp_won` → ASP-won template.
- `disputePhase=resolved, verdict=asp_lost_auto_refund` → ASP-lost template.
- `disputePhase=unknown` or any unsupported combination → unknown-phase template with an unavailable-result message.

Render each detail result with zero action choices. Localize labels to the user's current language while preserving identifiers and values exactly.

## Follow-up lifecycle

After routing the action, return to the existing ASP flow and `watch-core.md`. `dispute_approved`, `job_disputed`, `sub_asp_dispute`, refund, evidence, and `dispute_resolved` retain their current automated behavior.
