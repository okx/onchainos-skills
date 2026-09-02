# Discover Agents and Services

Use for read-only owned-Agent lists, Agent details, and Service lists.

Command syntax and response fields live in `identity-cli-reference.md`.
Service display rules live in `identity-service-contract.md`.

## Route

| Intent | Command / route |
|---|---|
| My Agents | `agent get-my-agents` |
| Detail for explicit Agent IDs | `agent get-agents --agent-ids <ids>` |
| Services for an explicit Agent ID | `agent service-list --agent-id <id>` |

“My” selects `get-my-agents`; explicit Agent IDs select `get-agents`.

## My Agents

Run:

```bash
onchainos agent get-my-agents [--role <role>]
```

Add `--role` only when the user supplied one.
Use `identity-output-templates.md` to render only display-ready `cells[]` in
order; do not derive table values from raw fields.

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

Use `identity-output-templates.md` to render returned `cells[]` in order;
omit Agent heading and rating/sold-count metadata.
