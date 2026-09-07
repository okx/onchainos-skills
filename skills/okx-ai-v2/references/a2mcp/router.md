# A2MCP Router

| Intent | Reference |
|---|---|
| Invoke a confirmed A2MCP service, collect parameters, or handle synchronous results | `invoke.md` |
| Recover an invalid, stale, or expired invocation context | `recovery.md` |
| Structured `execute_a2mcp_payment` action with its `paymentId` | `okx-agent-payments-protocol` |

## Action routing

Route only from the latest CLI `nextAction` and use that action's `params` as
its command arguments. Do not infer an action or opaque ID from prose.

| Action ID | Route |
|---|---|
| `invoke_a2mcp` | Read `handoff.md` once; on successful validation it continues directly to `invoke.md` with a fresh invocation generation |
| `provide_a2mcp_params` | Continue `invoke.md` with the returned `nextProbePayload` |
| `select_a2mcp_token` | Continue `invoke.md`; add only the candidate selected by the user to the action's bound `preparedId` |
| `fund_a2mcp_token` | Follow `funding.md` end to end with its bound `preparedId` and `candidateId` |
| `resume_a2mcp_after_funding` | Continue `funding.md` with its one-time bound `preparedId` and `candidateId` |
| `confirm_a2mcp_free` | Continue `invoke.md` with its bound `confirmationId` |
| `confirm_a2mcp_payment` | Continue `invoke.md` with its bound `preparedId` and `candidateId` |
| `execute_a2mcp_payment` | Hand its bound `paymentId` to `okx-agent-payments-protocol` |
| `cancel_a2mcp` | End the invocation without another CLI call |

Invocation results are synchronous and do not enter A2A assignment, XMTP,
subscription, or watch flows.

An HTTP 402 response remains inside `invoke.md` for input resolution,
candidate filtering, balance checks, and payment preparation. Do not hand off
the raw 402 response to the Payment Protocol.
