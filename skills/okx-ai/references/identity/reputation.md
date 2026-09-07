# View an Agent's Reputation

## Command

```bash
onchainos agent feedback-list --agent-id <agentId>
```

## Display results

**MUST** use the template below to render only returned `cells[]` in order.

```markdown
| Reviewer | Comment | Date | Score |
|---|---|---|---|
| <reviewer> | <comment> | <date> | <score> |
```

## Pagination

When `hasMore == true` and the user asks for more, run:

```bash
onchainos agent feedback-list --agent-id <agentId> --page <page+1>
```
