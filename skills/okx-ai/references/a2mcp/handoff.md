# A2MCP Exclusion Handoff

Use only for the latest trusted result with `phase=service_routing`,
`decision=ready`, `reason=a2mcp_service_confirmed`,
`nextAction.id=invoke_a2mcp`, `payload.schemaVersion=1`, and an object
`payload.serviceSnapshot`.

Preserve `data.payload` byte-for-byte as the base routing object. Follow
[`invoke.md`](invoke.md) to transport that object and the initial `{}` dynamic
parameter object through the Base64 CLI flags. Let `a2mcp-probe` validate the
Service ID, type, and endpoint. Never reconstruct or re-query the Service, or
enter an A2A task, subscription, session, or watch flow.
