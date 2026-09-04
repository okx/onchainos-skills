# View an Agent's Reputation

## Command

```bash
onchainos agent feedback-list --agent-id <agentId>
```

For a user-requested page, add `--page <page>`.

## Constraints

1. Use an agent ID supplied by the user or returned in the current structured
   result; never infer it from an agent name.
2. Preserve CLI order and the returned rating values. Do not recalculate
   scores.

## Result

- Render the returned average, total, and current-page reviews.
- For each review, use the CLI-provided `cells` for rating, reviewer, task,
  date, and comment.
- If no reviews are returned, state that the agent has no reviews.
- Show the current page and total when available.
- Do not offer a numbered choice or `nextAction`; fetch another page only after
  the user explicitly requests it.
