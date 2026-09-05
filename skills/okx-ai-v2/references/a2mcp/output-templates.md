# A2MCP Output Templates

Use these templates for the A2MCP invocation flow. Render structured CLI
fields as facts; never expose raw JSON, machine reason codes, action IDs, Skill
names, or internal routing narration.

## Payment-confirmation card

Show exactly one card after the request parameters are complete, including for
free services and insufficient balances.

```text
Service: {serviceName}
Endpoint: {endpoint}
Request: {method} {typedParams}
Amount: {selected amount and token, Free, or "Select a payment option"}
{ASP quote comparison only when amountMismatch=true}
{Network only for paid candidates}
{Token only for paid candidates}
{Balance only for paid candidates}
{Shortfall when selected token is insufficient}
```

Rules:

- `Free` is the exact amount display for a free path.
- When `amountSemantics=maximum`, label the amount as localized “Up to”
  (Chinese: “最多支付”); never present an authorization ceiling as a fixed
  charge.
- Preserve Endpoint and Request from the latest `payment_confirmation` payload.
- Show the quote-difference row only when `amountMismatch=true`.
- Insufficient balance disables payment confirmation but does not replace the
  card with a balance-only message.

When `payload.candidates[]` contains multiple entries, append:

```text
Payment options:
1. {tokenSymbol} · {network/chainName} · {amountDisplay} · {availableDisplay} available{ · Shortfall: shortfallDisplay when insufficient}
2. {...}
```

Use only the latest returned candidates and preserve their order. Ask the user
to choose one numbered option; add that option's `candidateId` to the latest
`select_a2mcp_token.params.preparedId` without displaying either ID. A
selection does not authorize payment.

## Paid confirmation

When the selected candidate is sufficient before Funding, present exactly two
choices:

1. Confirm the displayed payment and use the service.
2. Cancel.

Only the user's confirmation may trigger `prepare-payment --yes`, followed by
the payment protocol execution. Cancellation performs no write.
The sole exception is user-declared completed Funding: `funding.md` invokes
`resume-after-funding --yes`, whose CLI-owned flow refreshes the balance and
prepares payment only when the same candidate is now sufficient.

When the selected candidate is insufficient, keep confirmation disabled and
render only the returned funding/cancellation choices. If the user selects
Funding, read `funding.md`; this template does not own the Funding continuation.

## Free result

For `free_confirmation_required`, show the single confirmation card with
`Amount: Free`, using `serviceName`, `endpoint`, `method`, and `typedParams`
from that payload. Do not invent a token, network, balance, or payment
candidate. Present exactly two choices: confirm the free invocation or cancel.
Only confirmation may invoke `confirm_a2mcp_free`; render the result solely from
the resulting `endpoint_result/free_result` payload.
