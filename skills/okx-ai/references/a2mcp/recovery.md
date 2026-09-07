# A2MCP Recovery

Route recovery from the latest structured result; never reconstruct state from
chat prose.

| Condition | Recovery |
|---|---|
| `invalid_a2mcp_routing` | Stop; require a fresh service selection and routing result |
| `a2mcp_prepared_expired_or_missing` or `a2mcp_candidate_invalid_or_missing` | Stop and require a fresh service routing result with a trusted `serviceSnapshot`; never reuse an old ID or reconstruct the routing payload |
| `a2mcp_free_result_expired_or_missing` | Stop and require a fresh service routing result with a trusted `serviceSnapshot`; never infer the result or routing payload from chat history |
| Endpoint or payment execution fails | Present the readable returned failure and stop; do not switch methods, tokens, networks, or payment routes automatically |

The CLI owns classification of stale, missing, and invalid invocation state.
Do not parse error text or judge whether an ID is valid. A recovery result has
`phase=invocation_recovery`, `decision=blocked`, and only `cancel_a2mcp`; present
its readable message and stop. A new Probe and confirmation card require a new
trusted service routing result. Normal input collection, cancellation, and
Funding state transitions remain in `invoke.md` and `funding.md`.
