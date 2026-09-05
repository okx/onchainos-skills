# A2MCP Funding Handoff

This file defines the boundary; the shared wallet Funding reference owns the
actual address, QR, and shortfall presentation.

## Entry rule

Do not enter Funding from the initial Probe or from the first balance check.
The initial check only marks returned payment candidates as sufficient or
insufficient on the single payment-confirmation card. Enter Funding only after
the user selects the funding action for a specific insufficient candidate.

Invoke:

```text
onchainos agent a2mcp-probe funding \
  --prepared-id <action.params.preparedId> \
  --candidate-id <action.params.candidateId>
```

The CLI calls `build_funding_bundle_for_address()`, consumes the original
prepared handle, and returns the standard Funding envelope plus a bound
`resume_a2mcp_after_funding` continuation action. Hand the presentation to
`../../../okx-agentic-wallet/references/funding.md` unchanged.

## Return rule

After the user reports funding completion, this A2MCP return rule overrides the
shared Funding reference's generic balance-check, re-preview, and continuation
prompt. Re-enter this flow immediately and run exactly one command using the
IDs bound in the returned `resume_a2mcp_after_funding` action:

```text
onchainos agent a2mcp-probe resume-after-funding \
  --prepared-id <action.params.preparedId> \
  --candidate-id <action.params.candidateId> \
  --yes
```

The CLI alone refreshes the balance, preserves and replaces prepared state,
checks the same candidate, and decides whether to create the payment intent. It
must not Probe the Endpoint. Route only its returned `nextAction`: sufficient
balance returns `execute_a2mcp_payment` with a bound `paymentId`; insufficient
balance returns Funding/cancel actions with replacement IDs; stale or invalid
state returns structured recovery. Do not inspect balance fields, replace IDs,
or invoke `prepare-payment` from this file.
