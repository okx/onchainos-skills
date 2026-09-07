# Buyer Refund Preparation

Use this leaf to resolve a Buyer refund target, collect a reason when required,
and obtain a fresh read-only Refund V2 proposal. Cancellation and refund are
different: cancellation changes future renewal; refund returns an original
payment. Ask which outcome is intended when the request is ambiguous.

## Resolve exactly one target

Resolve one Buyer-owned `jobId` from the prompt or a fresh task list. If no
candidate or multiple candidates remain, ask the User to provide or select one.
Never prepare against a creation `sid` or an identifier remembered from another
task.

Run:

```text
onchainos agent refund-prepare JOB_ID_ARG [--reason REASON_ARG]
```

If the result requests `login` or `register_user_agent`, complete that flow and
rerun with the same returned `params.jobId`.

The CLI owns Buyer ownership, task kind, state, payment facts, and available
actions. A successful Refund V2 result must contain exactly `phase`, `decision`,
`reason`, `nextAction`, and `payload`, with `payload.schemaVersion=2`. Missing or
unknown contract fields block the flow.

Route only by `nextAction[].id`; labels and prose are display data. Read
[`refund-confirm.md`](refund-confirm.md) for confirmation cards,
[`refund-execute.md`](refund-execute.md) only after confirmation, and
[`../refund-reconcile.md`](../refund-reconcile.md) for pending or terminal results.

## Reason collection

For `refund_reason_required` or `refund_reason_too_long`, ask only for a reason
and end the turn. It must be User-authored, non-blank, at most
`payload.input.reasonMaxChars`, and preserved verbatim. Never draft, translate,
or improve it. Rerun preparation with that exact reason.

A reason validates input but does not normally confirm a write. The one
exception is an active post-delivery review card: `B` plus a non-blank reason is
the final confirmation for the freshly prepared `submit_refund_request`; follow
the binding procedure in [`refund-confirm.md`](refund-confirm.md).

## Read-only outcomes

- `trial_subscription_not_refundable`: explain that no charge is returned;
  offer conversion cancellation only if the CLI returns it.
- `provider_response_pending`: permit only returned status/watch actions.
- `arbitration_in_progress`: permit only returned arbitration/read actions.
- `refund_operation_not_available`: use only the freshly recomputed actions.
- Any `*_contract_required`, `*_not_verified`, `*_details_incomplete`, or
  `refund_not_available_for_status`: explain the proven gap and stay read-only.
- `refund_wallet_preflight_failed` or `refund_reconciliation_guard_unavailable`:
  stop before mutation; never bypass the guard.
- Unknown reasons or action IDs block the flow.

## Conditional references

- Read [`../../shared/refund-contract.md`](../../shared/refund-contract.md) whenever
  validating eligibility, finality, provenance, or safety.
- Route `view_arbitration` to [`../provider/arbitration-query.md`](../provider/arbitration-query.md).
- Route cancellation-only requests to
  [`subscription-manage.md`](subscription-manage.md).
- Route `watch_task` to [`../../runtime/watch.md`](../../runtime/watch.md) and preserve
  its scope.
