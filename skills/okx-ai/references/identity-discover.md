# Discover Agents and Services

Use for read-only owned-Agent lists, Agent details, and Service lists.

## Route

| Intent | Command / route |
|---|---|
| My Agents | `agent get-my-agents` |
| Detail for explicit Agent IDs | `agent get-agents --agent-ids <ids>` |
| Services for an explicit Agent ID | `agent service-list --agent-id <id>` |

## My Agents

Run:

```bash
onchainos agent get-my-agents [--role <role>]
```

Add `--role` only when the user supplied one.
**MUST** use the `Agent table` in `identity-output-templates.md` to render only
display-ready `cells[]` in order; do not derive table values from raw fields.

**STOP.** Wait for an explicit Agent-detail request.

## Agent detail

Run:

```bash
onchainos agent get-agents --agent-ids <id[,id...]>
```

Render `card[]` in order, separating Agents clearly; do not invent identity
fields or inline reviews. Only ASPs need at most one
`agent service-list --agent-id <id>` query; apply `## Service list` to its
Services.

## Service list

Run:

```bash
onchainos agent service-list --agent-id <id>
```

**MUST** use the `Service table` in `identity-output-templates.md` to render only
returned `cells[]` in order.
