# A2MCP Funding

## Enter

Only after the user selects the latest `fund_a2mcp_token`, run once:

```text
onchainos agent a2mcp-probe funding \
  --prepared-id <action.params.preparedId> \
  --candidate-id <action.params.candidateId>
```

Keep the bound `resume_a2mcp_after_funding` action and follow the shared wallet
[`Insufficient-balance entry`](../../../okx-agentic-wallet/references/funding.md#insufficient-balance-entry)
for payload validation and Funding presentation. Its caller-owned continuation
rule returns here when the user reports completion.

## Resume

After the user explicitly reports completion, run the latest bound action once:

```text
onchainos agent a2mcp-probe resume-after-funding \
  --prepared-id <action.params.preparedId> \
  --candidate-id <action.params.candidateId> \
  --yes
```

Do not probe the Endpoint, run generic
`wallet funding-check` or `prepare-payment`, replace bound IDs, or infer
progression from balance fields.
