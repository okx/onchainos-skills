# User Refund V2

## Scope

Use this reference for a User Agent request to return funds, reject a paid
deliverable, check refund progress, or inspect a refund-related arbitration
result.

Cancellation and refund are different:

- cancelling trial conversion or formal auto-renew changes future service;
- closing a zero-price task changes task state but moves no funds;
- refunding returns the original payment;
- a User cannot open arbitration directly from this flow. A submitted refund
  request first waits for the ASP to agree or dispute.

The CLI is authoritative for Buyer ownership, task type, state, payment, and
available actions. In every successful Refund V2 command response, the `data`
object must contain exactly `phase`, `decision`, `reason`, `nextAction`, and
`payload`, with `payload.schemaVersion=2`. Missing, malformed, or unknown
contract fields block the flow; never fall back to a legacy command.

## Standard flow

### Entry and target resolution

Resolve exactly one Buyer-owned `jobId` from the current message or a fresh
User task list. If no candidate or more than one candidate remains, ask the User
to provide or select one. Do not run preparation without a resolved `jobId`.

Run the read-only command:

```text
onchainos agent refund-prepare <jobId> [--reason <user-authored-text>]
```

If preparation returns `login` or `register_user_agent`, complete the owning
flow and rerun `refund-prepare` with the same returned `params.jobId`. Do not
replace it with a creation `sid` or a remembered task identifier.

Route the structured result by `nextAction[].id`. Treat labels and legacy
`action` prose as display data, never as commands. Do not invent an action that
the current result did not return.

### Deliverable-review rejection

For an active post-delivery review card, `B` together with a non-blank
User-authored reason is the User's final confirmation to submit the full refund
request on-chain. This changes only how the reply is executed; keep the existing
acceptance-review card copy unchanged.

Handle this reply in the current user conversation:

1. Preserve the rejection reason verbatim.
2. Run `refund-prepare <jobId> --reason "<verbatim reason>"` for a fresh state
   and ownership check.
3. Continue only for `payload.schemaVersion=2`, `phase=refund_confirmation`,
   `decision=ready`, `reason=refund_request_confirmation_required`, and the
   returned `nextAction.id=submit_refund_request`.
4. Copy that action's `params.jobId`, `params.operation`,
   `params.refundContextId`, and `params.reason` unchanged into
   `refund-execute ... --confirm` and execute immediately.
5. Render the execution result and continue only through its returned actions.

This review-card path uses the B reply as the explicit confirmation. The current
user conversation owns reason extraction, fresh preparation, execution, and
result rendering end to end. A blocked, changed, or malformed preparation result
is rendered as the authoritative outcome.

For `reason=refund_request_broadcast_submitted`, give one concise localized
confirmation: the rejection request was submitted with the User's verbatim
reason, and refund or arbitration progress will update in this task. Describe it
as submitted rather than settled. End with a practical query hint: the User can
ask the assistant to check the task result, or run
`onchainos agent status <jobId> --agent-id <buyerAgentId>` using the active
review-card identifiers.

### Submitted one-time or Active formal subscription

When the result is `refund_reason_required` or `refund_reason_too_long`, ask
only for a reason and end the turn. The reason must be authored by the User,
non-blank, no longer than `payload.input.reasonMaxChars`, and preserved
verbatim. Never draft, paraphrase, translate, or improve it.

Rerun preparation with that exact reason. In the standard flow, a reason
validates the request but does not confirm a write. For
`refund_request_confirmation_required`, render the returned task details,
verbatim reason, rules, and actions, then wait for an explicit selection of
`submit_refund_request`. The deliverable-review path above uses its active
B + reason reply as that explicit selection and continues immediately.

### Execute the offered action

After explicit confirmation, including the active deliverable-review B + reason
selection, copy the latest write action's values unchanged:

```text
onchainos agent refund-execute <jobId> \
  --operation <operation> \
  --refund-context-id <refundContextId> \
  [--reason "<verbatim User reason>"] \
  --confirm
```

`jobId`, `operation`, and `refundContextId` must come from the same latest
preparation result. `request-refund` must also carry the exact prepared User
reason; every other operation omits it. Never combine fields across results or
reuse an old context.

Every write action requires explicit confirmation. In the deliverable-review
path, the active card's B + reason reply supplies that confirmation; a reason
received outside that active card remains input only.

Execution re-reads authoritative state. Route only its returned actions. A
broadcast-submitted result is pending, not proof of settlement.

## Action allowlist

Only the following state and payment combinations may offer a write:

| Target and fresh state | Required facts | Action ID | Operation |
|---|---|---|---|
| Trial subscription, Active | `trialType=1`, `autoRenew=1` | `cancel_trial_conversion` | `cancel-trial-conversion` |
| One-time, Created | exact original amount is zero | `close_zero_price` | `close-zero` |
| One-time, Created | positive original amount, `paymentMode=1` | `execute_direct_refund` | `direct-refund` |
| One-time, Submitted | positive original amount, `paymentMode=1`, valid User reason | `submit_refund_request` | `request-refund` |
| Formal subscription, Active | positive current-period payment, complete period boundary, valid User reason | `submit_refund_request` | `request-refund` |

All four write actions require explicit confirmation, even when only one write
is displayed. In the standard flow, the initial word "refund", a supplied
reason, or a previous confirmation is not confirmation of the current prepared
action. In the deliverable-review path, the active card's B + reason reply is
the explicit selection for the freshly prepared `submit_refund_request` action.

No other state/type combination may produce a Refund V2 write. In particular,
Accepted one-time tasks and Created formal subscriptions are read-only contract
gaps. Expired tasks are terminal and never offer a Buyer claim/finalize write.

## Result handling

Use the returned `reason`, payload, and actions together:

| Result family | Handling |
|---|---|
| `refund_target_required` | Resolve exactly one Buyer-owned `jobId`, then prepare. |
| `refund_reason_required`, `refund_reason_too_long` | Collect only a verbatim User reason, then prepare again. |
| `trial_subscription_not_refundable` | Explain that no charge is being returned. Offer conversion cancellation only when returned. |
| `zero_amount_close_confirmation_required`, `direct_refund_confirmation_required`, `refund_request_confirmation_required` | Render the prepared details and wait for explicit selection of the returned write. |
| `*_broadcast_submitted` | State that the operation is pending. Follow only returned read/watch actions; never retry the write. |
| `provider_response_pending` | The ASP has not agreed or disputed. Permit only returned status/watch actions. |
| `arbitration_in_progress` | No refund is decided. Permit only returned arbitration/read actions. |
| `refund_confirmed` | Render a full original-token refund as terminal using the finality matrix below. |
| `expired_without_refundable_payment` | Render a terminal lifecycle with `settlement.state=not_required`; do not claim funds moved. |
| `refund_operation_pending_reconciliation`, `refund_outcome_unknown` | Keep the operation pending/read-only and never repeat it automatically. |
| `refund_context_stale`, `refund_write_rejected`, `refund_prebroadcast_failed` | Discard the old context and use only a returned fresh-prepare action. |
| `refund_execution_confirmation_required` | Render the current prepared write and wait for explicit confirmation. |
| `refund_operation_not_available` | Use only the freshly recomputed action set. |
| `trial_conversion_already_cancelled`, `trial_conversion_state_unknown`, `zero_amount_subscription_not_refundable` | Explain the current fact and keep the flow read-only. |
| `zero_amount_task_closed`, `trial_subscription_closed_without_refund`, `refund_not_approved_or_task_completed` | Render the returned terminal no-refund outcome; offer no new write. |
| `*_contract_required`, `*_not_verified`, `*_details_incomplete`, `refund_not_available_for_status` | Explain the proven gap and keep the flow read-only. Never substitute another write. |
| `refund_wallet_preflight_failed`, `refund_reconciliation_guard_unavailable` | Stop before mutation; do not bypass the guard. |

Unknown reasons or action IDs block. `refund_task_details_incomplete` blocks a
pre-write card because Provider/Service identity is incomplete. Missing display
labels in an otherwise proven terminal result do not undo finality; render them
as unavailable.

<a id="progress-arbitration-and-finality"></a>

## Finality (compact matrix)

Always use a fresh authoritative read. Conversation history and caller events
cannot establish settlement.

| Fresh authoritative facts | Result |
|---|---|
| Paid non-trial one-time task or formal subscription at Expired(8), with matching Buyer, task kind, and exact positive original payment | `refund_confirmed`; the backend automatic refund has arrived. No Failed(9), Tx Hash, local provenance, or Buyer write is required. |
| Trial subscription or exact-zero task at Expired(8) | `expired_without_refundable_payment`; terminal with `settlement.state=not_required`. |
| One-time task at Closed(7), with positive original amount and `paymentMode=1` | `refund_confirmed`; the close returned escrow. |
| One-time task at Failed(9), with matching Buyer and exact positive original payment | `refund_confirmed`; this one-time lifecycle state is reserved for successful refund transitions. |
| Formal subscription at Failed(9), with matching durable local `request-refund` provenance and fresh Buyer/type/payment facts | `refund_confirmed`. Core provenance binds the same job, Buyer, formal `jobType=1`, exact positive original amount, and token address. |
| Subscription at bare or event-only Failed(9), or any `sub_failed_notify` path without independently trustworthy cause | `refund_settlement_details_incomplete`; make no refund or charge-failure claim, terminal marker, or cleanup. |
| Subscription at Closed(7) | `task_closed_no_new_refund_action`; Closed alone is not subscription-refund proof. |

Both Expired(8) outcomes require `job.refundState=resolved` and
`rules.providerTimeoutRefundExpected=false`; the paid branch uses
`settlement.state=confirmed`, while the no-funds branch uses `not_required`.

For caller event `job_asp_reject_expire` on either task kind, require matching
durable `request-refund` provenance plus fresh Failed(9) owner, type, exact
positive original amount, and token-address facts before terminal output. The
event alone is never proof.

For `dispute_resolved`, require matching durable `request-refund` provenance
plus fresh job type, Buyer ownership, and exact terminal status before stating
a verdict: status 9 is the User-winning refund branch; status 6 is the
ASP-winning no-refund branch. Without that proof, do not announce a verdict,
rate, notify, emit a terminal marker, or clean up.

A Tx Hash is optional audit metadata after a finality row passes. Its absence
does not downgrade `refund_confirmed`; an unrelated or pending transaction hash
must never be relabelled as refund evidence.

For provenance-based paths, missing optional Provider/Service, period,
token-symbol, or `paymentMode` values reduce display detail only. When both the
recorded and fresh values exist, a mismatch vetoes finality.

## Events/watch/recovery

Lifecycle events such as `job_closed`, `job_refunded`, `job_auto_refunded`,
`job_expired`, `submit_expired`, `job_asp_accept_expire`,
`job_asp_reject_expire`, `sub_asp_agree`, `sub_reject_refund_notify`, and
`dispute_resolved` are signals to re-read fresh state. They may select branch
wording after the proof gate passes, but never authorize a write or create
settlement proof.

For a direct `refund-prepare` terminal result, render it and follow its returned
`stop`; no separate cleanup command is implied. When the same terminal result
is produced during a scoped lifecycle/watch event, emit the stable terminal
marker, clean up that scoped session, and do not re-enter watch. Global watch
continues because other tasks may still emit events.

Keep valid `request-refund` provenance across Rejected(3), Disputed(4), process
restart, polling, and terminal recovery. It proves the classified request path,
not settlement by itself; combine it only with the fresh facts required by the
finality matrix. A core mismatch or definitive rejection disqualifies it.

Pending, unknown, or settlement-incomplete results stay on returned read-only
status/watch actions. Never manufacture a terminal marker or clear recovery
state merely because time passed.

## Safety invariants

- Never call the disabled legacy User writes `close`, `reject`,
  `subscribe-reject`, or `claim-auto-refund` as a Refund V2 substitute.
  `subscribe-cancel` is cancellation-only.
- Amounts are exact decimal strings from authoritative payment fields. Never
  use service-list pricing, floats, fiat estimates, or conversation memory.
- Refunds return the full eligible original token amount. Never convert the
  asset, calculate a partial refund, or prorate unused time. A formal
  subscription covers only its current charged period.
- Preserve the exact action fields and the User's exact reason. Never invent an
  endpoint, event, result, transaction hash, or successful outcome.
- A broadcast receipt, event name, arbitration vote, or pending hash is not
  refund finality. Apply only the matrix above.
- Repeated preparation is read-only. After broadcast, pending reconciliation,
  an unknown outcome, or stale context, never automatically repeat execution.

## Conditional references

- Route `nextAction.id=view_arbitration` through
  [`../../okx-ai-v2/references/a2a/provider/arbitration.md`](../../okx-ai-v2/references/a2a/provider/arbitration.md). Route every other returned
  action through [`task-action-routing.md`](task-action-routing.md).
- When rendering a Refund V2 result, read the Refund V2 section of
  [`task-output-templates.md`](task-output-templates.md).
- Read the refund section of
  [`task-cli-reference.md`](task-cli-reference.md) only when raw command flags
  or payload schema details are needed; it is not part of the default flow.
- For `watch_task`, read [`watch-core.md`](watch-core.md) and preserve scope.
- For cancellation without a request to return funds, use
  [`task-user-playbook.md`](task-user-playbook.md).
