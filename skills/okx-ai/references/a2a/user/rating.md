# Active Subscription Rating

Use this flow when a Buyer rates or reviews an Active subscription. Viewing an
Agent's existing reviews or reputation remains an Identity query.

## Select the subscription

```bash
onchainos agent my-tasks --task-type subscription --status-type 1 --page 1
```

- A `jobId` in the current request must match a returned row exactly. Continue
  pagination only while `hasNext=true`.
- Confirm a `jobId` carried only by earlier conversation context before using it.
- Without a confirmed `jobId`, show the localized equivalent of `I found the
  following unreviewed orders. Please select the order you want to review.`,
  then render this compact table and wait. Preserve pagination, continuing only
  while `hasNext=true`:

  | # | Task | Provider | Status | Job ID |
  |---|---|---|---|---|
  | 1 | `<title>` | `Agent#<providerAgentId>` | `<localizedStatusLabel>` | `<jobId>` |

Populate every table cell from its returned row. Render and localize the CLI
`statusLabel`; do not display raw `status`, `statusName`, or `statusCode`.
Use only the selected row's `jobId`, `buyerAgentId`, and `providerAgentId`. Stop
if the row or either Agent ID is missing. Do not substitute detail, status,
device, or task-session data. Do not add fee, renewal, device, or billing fields
to the selection table.

## Check for an existing rating

```bash
onchainos agent task-feedback \
  --agent-id BUYER_AGENT_ID_ARG \
  --task-id JOB_ID_ARG
```

Bind both arguments to the selected row. A non-empty `data[]` means the Buyer
already rated this subscription: show that result and stop. Continue only when
`data[]` is empty.

## Collect the rating

Require both values from the User:

- `score`: `0.00`–`5.00`, with at most two decimal places.
- `description`: concrete, non-blank review text.

Keep valid values already supplied and ask once for all missing or invalid
values. A sentiment-only request is not review text. Never draft, infer,
translate, or rewrite the description.

The User's complete score-and-description reply authorizes one submission; do
not request another confirmation.

## Submit

```bash
onchainos agent feedback-submit \
  --agent-id PROVIDER_AGENT_ID_ARG \
  --creator-id BUYER_AGENT_ID_ARG \
  --score SCORE_ARG \
  --task-id JOB_ID_ARG \
  --description REVIEW_ARG
```

Bind Agent and task IDs to the selected row. Pass the User's score and review
verbatim, with each dynamic value as one literal argv value. Submit once; never
retry an unknown result automatically.

## Result

Only `ok=true` with a non-empty `data.txHash` proves success. Render a localized
success result in this order:

```text
Review submitted.

- Task ID: <jobId>
- Score: <score> / 5
- Review: <description>
- Transaction hash: <txHash>
```

Use the selected and submitted values verbatim. On command failure or a missing
transaction hash, report the error and do not claim that the rating succeeded.
