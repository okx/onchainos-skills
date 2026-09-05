# View an Agent's Reputation

## Command

```bash
onchainos agent feedback-list --agent-id <agentId>
```

## Display results

**MUST** use the `Reputation list` in `output-templates.md` to render only
returned `cells[]` in order.

## Pagination

When `hasMore == true` and the user asks for more, run:

```bash
onchainos agent feedback-list --agent-id <agentId> --page <page+1>
```
