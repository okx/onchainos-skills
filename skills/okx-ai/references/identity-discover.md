# Discover Agents and Services

Use this reference for owned-Agent lists, Agent details, and an Agent's Service
list. All operations here are read-only.

Command syntax and response fields live in `identity-cli-reference.md`.
Service display rules live in `identity-service-contract.md`.

## Route

| Intent | Command / route |
|---|---|
| My Agents | `agent get-my-agents` |
| Detail for explicit Agent IDs | `agent get-agents --agent-ids <ids>` |
| Services for an explicit Agent ID | `agent service-list --agent-id <id>` |

Ownership words such as “my” select `get-my-agents`. Explicit IDs in a detail
request select `get-agents`.

## My Agents

Run `agent get-my-agents`, adding `--role` only when the user supplied one.
Render returned account groups and display-ready `cells[]` in order. Do not
recompute role, status, approval, rating, wallet ownership, or totals.

For an empty group, show that it has no Agents. Offer detail only as a short
follow-up suggestion; do not automatically query every Agent.

## Agent detail

Run:

```bash
onchainos agent get-agents --agent-ids <id[,id...]>
```

Render each returned `card[]` in order. Multiple Agents are separated clearly.
Do not invent identity fields or inline reviews.

For each returned ASP, run at most one
`agent service-list --agent-id <id>` and render its Services. User Agents and
Evaluators do not trigger a Service query. Load `identity-reviews.md` only when
the user asks for reviews.

## Service list

Run:

```bash
onchainos agent service-list --agent-id <id>
```

Render the returned display-ready rows in order. Preserve `A2A` / `A2MCP`,
Agent IDs, prices, endpoints, and normalized labels exactly. Omit a display
column only when every row marks it unavailable. Do not display `serviceGuide`
or internal Service UUIDs.
