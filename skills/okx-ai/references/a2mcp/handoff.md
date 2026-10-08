# A2MCP Exclusion Handoff

After the user selects an `A2MCP` Service, prepare its snapshot exactly once:

```bash
onchainos agent task-create-prepare --sid <selected-sid> --service-type A2MCP --asp-agent-id <selected-asp-agent-id>
```

Use the selected Service's `sid` and `asp.aspAgentId`. This explicit branch
requires wallet login, not an A2A User Agent identity. On `login_required`,
follow the wallet login flow and rerun the same preparation. On any other
error, explain it and stop; never fall back to A2A preparation.

If already entering with a trusted routing result, skip preparation. Accept
only the latest result with `phase=service_routing`,
`decision=ready`, `reason=a2mcp_service_confirmed`,
`nextAction.id=invoke_a2mcp`, `payload.schemaVersion=1`, and an object
`payload.serviceSnapshot`.

Preserve `data.payload.serviceSnapshot` byte-for-byte. Follow
[`invoke.md`](invoke.md) to collect typed parameters from its `inputSchema`,
then Base64-encode that exact Service object for `--service-base64` and the
typed parameter object for `--params-base64`. Let `agent a2mcp invoke` validate
the Service type, request type, method, endpoint, and MCP tool name. Never
reconstruct or re-query the Service, or enter an A2A task, subscription,
session, or watch flow.
