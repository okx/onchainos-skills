# A2MCP Router

For a confirmed free-text invocation, read `invoke.md`. For every active
invocation, route only from the latest CLI `nextAction`; never infer an action
or opaque ID from prose.

An active `endpoint_result/free_result` with an empty `nextAction` returns to
`invoke.md` for result rendering, then ends the invocation.

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

Read `recovery.md` only for `phase=invocation_recovery` or when `invoke.md`
routes an error there. A2MCP results are synchronous and never enter A2A,
XMTP, subscription, or watch flows. Keep raw HTTP 402 responses in `invoke.md`;
only `execute_a2mcp_payment.params.paymentId` enters the Payment Protocol.
