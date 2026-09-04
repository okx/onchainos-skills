# Agent profile

Use to list owned Agents, inspect Agent details, and list services.

## My Agents

Run:

```bash
onchainos agent get-my-agents [--role <role>]
```

Add `--role` only when the user supplied one.
**MUST** use the `Agent table` in `output-templates.md` to render only
display-ready `cells[]` in order; do not derive table values from raw fields.

**STOP.** Wait for an explicit Agent-detail request.

## Detail for explicit Agent IDs

Run:

```bash
onchainos agent get-agents --agent-ids <id[,id...]>
```

**MUST** render each Agent's display-ready `card[]` with `Agent detail` from
`output-templates.md`. For ASPs only, run
`agent service-list --agent-id <id> --page 1 --page-size 3` and apply
`## Services for an explicit Agent ID`.

## Services for an explicit Agent ID

Run:

```bash
onchainos agent service-list --agent-id <id> --page <n> --page-size 3
```

**MUST** use the `Service table` in `output-templates.md` to render only
returned `cells[]` in order.

### Pagination

When `hasMore == true` and the user asks for more, run:

```bash
onchainos agent service-list --agent-id <id> --page <page+1> --page-size 3
```
