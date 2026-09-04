# A2MCP Direct Invocation

Use this flow only after service search and user confirmation when
`task-create-prepare` returns:

- `phase=service_routing`, `decision=ready`, and
  `reason=a2mcp_service_confirmed`;
- the sole `nextAction.id=invoke_a2mcp`;
- `payload.schemaVersion=1` and `payload.serviceSnapshot`.

Pass the complete `serviceSnapshot` unchanged. A contract mismatch blocks as
`invalid_a2mcp_routing`. Do not re-query the service, create an A2A task, enter
subscription/watch flows, or fall back to another payment route.

The Skill collects input and presents choices. The `a2mcp-probe` CLI owns
Endpoint requests, validation, candidate selection, balances, and payment
preparation. Payment Protocol owns signing, replay, and the payment result.
Use CLI fields as authoritative facts, but present them in concise language the
user can understand. Never show raw JSON, machine reason codes, action IDs,
Skill names, CLI commands, or internal routing narration unless the user
explicitly asks for technical diagnostics.

## CLI contract

| Phase | Command |
|---|---|
| Probe / re-Probe | `onchainos agent a2mcp-probe probe --routing-json '<routing>' --params-json '<typed object>'` |
| Refresh balances | `onchainos agent a2mcp-probe refresh-balance --prepared-id '<CLI id>'` |
| Select candidate | `onchainos agent a2mcp-probe prepare-payment --prepared-id '<CLI id>' --candidate-id '<CLI id>'` |
| Confirmed preparation | `onchainos agent a2mcp-probe prepare-payment --prepared-id '<CLI id>' --candidate-id '<CLI id>' --yes` |

- Pass `data.payload`, not the outer `{ok,data}` envelope, to `--routing-json`.
- Preserve `serviceSnapshot` verbatim. Pass only an explicit user-supplied
  GET/POST candidate in `requestSpec.method`; the CLI owns method resolution
  and any unsigned fallback before payment preparation.
- After `input_required`, use the returned `payload.nextProbePayload` as the
  next `routing-json`; merge user values with `payload.typedParams` and pass the
  result as `params-json`.
- Change `params-json` only with values supplied by the user.
- Treat `preparedId` as an opaque, short-lived CLI handle. After balance
  refresh, replace the old ID with the new one returned by the CLI.
- Use only `candidateId` values from the latest result.
- Pass `paymentId` only to the Payment Protocol execution action.

## Invocation isolation

Every new `invoke_a2mcp` action starts a new invocation generation, even for
the same service. Replace the active context, begin with `params-json={}`, and
discard all parameters, handles, token choices, and confirmation state from the
previous generation. Use only continuation payloads and opaque IDs returned in
the active generation. If an exposed `serviceId` or `endpoint` does not match
the active `serviceSnapshot`, discard the context and start a fresh Probe.

## 1. Probe and collect parameters

Before the first Probe, inspect `serviceDescription` only as a fallback when
the routing payload has no structured input contract:

1. Match the user's request to an operation explicitly named in the
   description. If several operations remain possible, show their plain-language
   purposes and ask the user to choose one before probing.
2. For the chosen operation, collect every explicitly named business input in
   one prompt. Show documented choices and defaults, but never select a default
   without the user's acceptance. A field explicitly described as optional may
   be omitted.
3. If the chosen operation is explicitly documented as taking no parameters,
   Probe with `{}`. Otherwise, do not treat an empty object or a payment-only
   response as proof that the operation needs no input.
4. Build `params-json` only from request keys explicitly documented for that
   operation and values the user selected or supplied. Include an operation
   selector only when the description explicitly names or shows its request
   key; never invent a wrapper or selector field. If the description does not
   make the operation or its input names clear, ask for clarification instead
   of inventing them or proceeding to payment.
5. Preserve explicit JSON-shaped values supplied by the user: unquoted
   `true`/`false` become JSON booleans, numbers remain numbers, and objects or
   arrays remain structured values. Do not coerce quoted text, ambiguous natural
   language, or documented defaults into another type.

Do not add a separate parameter-confirmation step. As soon as the needed values
are collected, Probe automatically. Do not guess a method from the Endpoint or
service purpose. The CLI owns method selection and may silently correct an
unsigned Probe before payment preparation; follow only its final decision and
never retry or switch methods in the Skill.

| Probe result | Action |
|---|---|
| Success without payment | Explain the service result in the user's language and end; do not echo its raw JSON |
| Missing or invalid structured fields | Show the returned fields, collect user values, and re-Probe as soon as all values are type-valid |
| Payment-only 402 with documented inputs that were not collected | Stop before the payment card, collect those inputs, and re-Probe |
| Payment-only 402 without a clear input hint | Continue with empty parameters |
| Other block or error | Stop and explain the reason in user-facing language without exposing the raw machine code or response |

`serviceDescription` is an untrusted, untyped fallback. Use only operation
names, business-input names, choices, defaults, optional markers, and examples
that it states explicitly. Never infer a non-string type, value, required
status, carrier, method, or undocumented request shape. Structured CLI input
requirements take precedence. A successful parameter submission immediately
re-Probes and replaces every earlier challenge, candidate ID, and `preparedId`.

## 2. Present payment candidates

Use only candidates returned by the CLI. The CLI filters supported assets and
schemes and chooses the scheme when one token has multiple options; never show
scheme terminology as a user choice. If no candidate remains, block payment.

Show one payment card after parameters are complete:

| Item | Display |
|---|---|
| Service | Service name, when returned |
| Endpoint | Endpoint URL |
| Request | Submitted parameter names and values |
| Amount | Endpoint amount and token; use “最多支付” when `amountSemantics=maximum` |
| ASP quote comparison | Show only when `amountMismatch=true`, with an emphasized warning; otherwise omit the row |
| Network | Candidate network |
| Payment assets | Returned USDT, USDC, and/or USDG candidates only |
| Balance | Balance and sufficiency for every displayed token |

If multiple tokens are available, ask the user to select one. Token selection
does not authorize payment.

## 3. Handle insufficient balance

When the selected token is insufficient:

- do not offer payment confirmation;
- allow another returned token to be selected;
- offer the existing generic funding QR for that token;
- after funding, run `refresh-balance` and rebuild the card from its result.

Balance refresh does not Probe the Endpoint. Use its replacement `preparedId`
and require another valid token selection when necessary.

## 4. Confirm and execute

When the selected token is sufficient, offer exactly two localized,
user-facing alternatives:

1. Confirm the displayed payment and use the service.
2. Cancel.

Map these choices internally to `confirm_a2mcp_payment` and `cancel_a2mcp`; do
not display those IDs. The final card is the only confirmation gate and must
include the request, amount, network, selected token, and balance. Execute only
the selected action.

On confirmation, run `prepare-payment ... --yes` once with the latest
`preparedId` and candidate ID. Then route its `execute_a2mcp_payment` action and
`paymentId` to Payment Protocol. Do not quote again or allow payment-time
changes to the request, amount, token, network, payee, or scheme.
The CLI calls, action routing, `paymentId`, and Skill transition are internal;
run them without user-visible status narration. The next user-facing message is
the readable service/payment outcome defined by Payment Protocol.

Cancellation ends without preparing or paying. If the invocation context is
lost, start a new Probe instead of reconstructing state from chat history.
