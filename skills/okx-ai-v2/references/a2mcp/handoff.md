# A2MCP Exclusion Handoff

Use this leaf only when `task-create-prepare` returns
`phase=service_routing`, `decision=ready`,
`reason=a2mcp_service_confirmed`, `payload.schemaVersion=1`, an immutable
`payload.serviceSnapshot`, and `nextAction.id=invoke_a2mcp`.

Hand the exact snapshot and action parameters to [`router.md`](router.md).
Create no A2A task, subscription, session, or watch. Any missing or mismatched
field blocks the handoff rather than falling back to A2A creation.
