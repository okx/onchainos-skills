# ASP Arbitration Query

Use this leaf for rejected candidates, filed cases, and case details. Keep
candidate and filed-case result sets separate.

## Rejected candidates

“可仲裁/待仲裁/哪些可以仲裁” means rejected candidates:

```text
onchainos agent tasks --status rejected --agent-id <aspAgentId> --page 1 --limit 20
onchainos agent my-subscriptions --role provider --status rejected
```

Render CLI order, type, Job ID, exact amount/token, rejection time, and
pagination. Ask for a sequence or Job ID. An empty set means there is currently
no rejected task or period eligible for this decision.

## Filed cases

“仲裁列表/已发起仲裁/仲裁案件” means filed cases. Preserve an explicitly
provided or envelope-bound ASP Agent ID. Otherwise use `my-agents`, retain role
ASP (`2`), and select only a sole match or ask the User to choose.

```text
onchainos agent arbitration-list --agent-id <aspAgentId> [--page <n>] [--page-size <n>]
```

Render `payload.items[]` in order. Selection is restricted to
`nextAction[id=view_arbitration].params.allowedJobIds`; the CLI may normalize
legacy `view_dispute` from a persisted card.

For an explicit or allowed Job ID:

```text
onchainos agent arbitration-detail <jobId> --agent-id <aspAgentId>
```

Map fresh detail only: `evidence_preparation` means evidence preparation,
`in_progress` means arbitration in progress, resolved `asp_won` means ASP won,
and resolved `asp_lost_auto_refund` means ASP lost with automatic refund.
Unknown values remain unknown. Include exact Job ID, deadline, verdict, amount,
fund destination, refund amount, and Tx Hash only when returned.
