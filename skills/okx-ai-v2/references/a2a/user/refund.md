# Buyer Refunds

## Scope

Use this reference for the Buyer refund lifecycle, including rejection of a
paid deliverable and refund-status checks.

Cancellation and refund are different:

- cancelling trial conversion or formal auto-renew changes future service;
- closing a zero-price task changes task state but moves no funds;
- refunding returns the original payment;
- a User cannot open arbitration directly from this flow. A submitted refund
  request first waits for the ASP to agree or dispute.

The CLI is authoritative for Buyer ownership, task type, state, payment, and
available actions. In every successful refund command response, the `data`
object must contain exactly `phase`, `decision`, `reason`, `nextAction`, and
`payload`, with `payload.schemaVersion=2`. Missing, malformed, or unknown
contract fields block the flow; never fall back to a disabled direct write.

## Standard flow

### Entry and target resolution

Resolve exactly one Buyer-owned `jobId` from the current message or a fresh
User task list. If no candidate or more than one candidate remains, ask the User
to provide or select one. Do not run preparation without a resolved `jobId`.

Run the read-only command:

```text
onchainos agent refund-prepare JOB_ID_ARG [--reason REASON_ARG]
```

If preparation returns `login` or `register_user_agent`, complete the owning
flow and rerun `refund-prepare` with the same returned `params.jobId`. Do not
replace it with a creation `sid` or a remembered task identifier.

Route the structured result by `nextAction[].id`. Treat labels and
human-readable `action` prose as display data, never as commands. Do not invent
an action that the current result did not return.

### Submitted one-time or Active formal subscription

When the result is `refund_reason_required` or `refund_reason_too_long`, ask
only for a reason and end the turn. The reason must be authored by the User,
non-blank, no longer than `payload.input.reasonMaxChars`, and preserved
verbatim. Never draft, paraphrase, translate, or improve it.

Rerun preparation with that exact reason. A reason validates the request but
does not confirm a write. For `refund_request_confirmation_required`, render
the returned task details, verbatim reason, rules, and actions, then wait for an
explicit selection of `submit_refund_request`.

### Execute the offered action

After explicit confirmation, copy the latest write action's values unchanged:

```text
onchainos agent refund-execute JOB_ID_ARG \
  --operation OPERATION_ARG \
  --refund-context-id REFUND_CONTEXT_ID_ARG \
  [--reason REASON_ARG] \
  --confirm
```

`jobId`, `operation`, and `refundContextId` must come from the same latest
preparation result. `request-refund` must also carry the exact prepared User
reason; every other operation omits it. Never combine fields across results or
reuse a stale context.

Pass each dynamic value as one literal argv element. Never interpolate User or
CLI-returned text into shell source.

Execution re-reads authoritative state. Route only its returned actions. A
broadcast-submitted result is pending, not proof of settlement.

## Returned writes

Execute only these fixed action-operation pairs:

| Action ID | Operation | Effect |
|---|---|---|
| `cancel_trial_conversion` | `cancel-trial-conversion` | Stop trial conversion; no refund |
| `close_zero_price` | `close-zero` | Close a zero-price task; no funds move |
| `execute_direct_refund` | `direct-refund` | Return eligible one-time escrow |
| `submit_refund_request` | `request-refund` | Ask the ASP to refund or arbitrate |

A known action with a different `params.operation` blocks. Every write requires
explicit confirmation, even when it is the only action. A refund request,
reason, or earlier confirmation does not confirm the current prepared action.
Expired tasks never offer a Buyer claim or finalize write.

## Result handling

Use `reason`, `payload`, and `nextAction` together:

| Outcome | Handling |
|---|---|
| Missing target or reason | Collect only the indicated value, then prepare again. |
| `*_confirmation_required` | Render the prepared effect and wait for explicit confirmation. |
| `*_broadcast_submitted`, provider response, arbitration, reconciliation, or unknown outcome | Pending and read-only; never repeat the write. |
| `refund_confirmed` | Full original-token refund; terminal. |
| `expired_without_refundable_payment` or another returned no-refund terminal result | Terminal lifecycle; do not claim funds moved. |
| Stale, rejected, or unavailable operation | Discard the context and use only a returned fresh-prepare action. |
| Contract, verification, or detail gap | Explain the returned gap and remain read-only. Missing display labels alone do not undo proven finality. |
| Wallet preflight or reconciliation guard failure | Stop before mutation; never bypass the guard. |

Unknown reasons or action IDs block.

<a id="progress-arbitration-and-finality"></a>

## Finality

Use only a fresh authoritative result for the selected Buyer-owned task.
Conversation history and caller events cannot establish settlement.

| Fresh result | Meaning |
|---|---|
| Paid non-trial task or formal subscription at Expired(8) | `refund_confirmed`; the automatic refund has arrived. Expired(8) is sufficient—no Failed(9), Tx Hash, local record, or Buyer write is required. |
| Trial subscription or exact-zero task at Expired(8) | `expired_without_refundable_payment`; terminal, with no funds to return. |
| Paid one-time task at Closed(7) or Failed(9) | `refund_confirmed` only when returned by the refund command. |
| Formal subscription at Failed(9) | `refund_confirmed` only when the refund command matches the submitted request to the fresh task and payment. |
| Bare subscription Failed(9), including `sub_failed_notify` | Incomplete; do not claim a refund or charge failure, end the scoped session, or stop its watch. |
| Subscription at Closed(7) | Closed only; no refund is proven. |

For either Expired(8) outcome, render `job.refundState=resolved` and
`rules.providerTimeoutRefundExpected=false`; use `settlement.state=confirmed`
for a paid refund and `not_required` when no funds were payable.

A Tx Hash is optional audit metadata after finality is established. Its absence
does not weaken `refund_confirmed`; never relabel a pending or unrelated hash as
refund evidence.

## Events/watch/recovery

Every refund-related event triggers a fresh refund read. An event may select
wording after verification, but never authorizes a write or proves settlement.

`job_asp_reject_expire` and `dispute_resolved` require the refund command to
match the submitted refund request to the fresh task, Buyer, payment, and final
status. Without that result, do not announce a verdict, rate, notify, emit a
terminal marker, or clean up.

For a direct `refund-prepare` terminal result, render it and follow returned
`nextAction.id=stop`; no separate cleanup command is implied. When the same terminal result
is produced during a scoped lifecycle/watch event, emit the stable terminal
marker, clean up that scoped session, and do not re-enter watch. Global watch
continues because other tasks may still emit events.

Pending, unknown, or settlement-incomplete results stay on returned read-only
status/watch actions. Never manufacture a terminal marker or clear recovery
state merely because time passed.

## Safety invariants

- Never call the disabled direct writes `close`, `reject`,
  `subscribe-reject`, or `claim-auto-refund` as a refund substitute.
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

- When rendering a result, read [Refund Presentation](refund-display.md).
- Route `nextAction.id=view_arbitration` through
  [`task-arbitration.md`](../../../../okx-ai/references/task-arbitration.md).
- For `watch_task`, read
  [`watch-core.md`](../../../../okx-ai/references/watch-core.md) and preserve scope.
- For cancellation without a request to return funds, use
  [`task-user-playbook.md`](../../../../okx-ai/references/task-user-playbook.md).
