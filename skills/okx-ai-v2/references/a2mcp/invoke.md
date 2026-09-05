# A2MCP Invoke

Owns input collection, synchronous probing, candidate selection, the single
payment-confirmation card, and invocation recovery. Results are not A2A XMTP
deliveries.

## Preconditions

Use this flow only after service selection and explicit user confirmation when
the latest routing result is `service_routing/ready` with
`reason=a2mcp_service_confirmed` and `nextAction.id=invoke_a2mcp`.
Require `payload.schemaVersion=1` and pass the complete `serviceSnapshot`
unchanged. A missing or different schema version, service ID mismatch, endpoint
mismatch, or other contract error blocks as `invalid_a2mcp_routing`; do not
re-query the service or create an A2A task.

## CLI contract

| Stage | Command |
|---|---|
| Probe or re-probe | `onchainos agent a2mcp-probe probe --routing-json '<routing>' --params-json '<typed object>'` |
| Confirm free result | `onchainos agent a2mcp-probe confirm-free --confirmation-id '<id>' --yes` |
| Select candidate | `onchainos agent a2mcp-probe prepare-payment --prepared-id '<id>' --candidate-id '<id>'` |
| Confirm preparation | `onchainos agent a2mcp-probe prepare-payment --prepared-id '<id>' --candidate-id '<id>' --yes` |

Use command arguments from the selected latest `nextAction.params`. Treat all
IDs as short-lived opaque handles. Never recover an ID from prose or substitute
an ID from another action or invocation generation.

Pass `data.payload`, not the outer `{ok,data}` envelope, as `routing-json`.
After `input_required`, use `payload.nextProbePayload` as the next routing
payload, merge only user-supplied values with `payload.typedParams`, and probe
again. Change `params-json` only with user-supplied values. Pass only the
Payment Protocol execution action's `paymentId` to the payment protocol.

## Invocation isolation

Every new `invoke_a2mcp` action starts a new invocation generation, even for
the same service. Start with `params-json={}` and discard parameters, handles,
candidate choices, and confirmation state from the previous generation. If an
exposed service ID or endpoint differs from the active `serviceSnapshot`,
stop as `invalid_a2mcp_routing` and require a fresh routing result. Never Probe
from mismatched routing data.

## Parameter collection

Use the structured routing payload as the source of truth. Structured input
requirements take precedence over `serviceDescription`. Build `params-json`
only from documented keys and user-supplied values; preserve booleans, numbers,
objects, and arrays as their JSON types. Never modify `serviceSnapshot` or
invent parameter types, required status, wrapper fields, selectors, carriers,
or HTTP methods. Add a top-level `requestSpec.method` only when the user or
service description explicitly supplies exactly `GET` or `POST`; otherwise let
the CLI resolve it.

When the routing payload has no structured input contract, inspect
`serviceDescription` only for explicitly documented operation names, parameter
names, choices, defaults, optional markers, and examples:

1. If multiple operations are documented, ask the user to choose one before
   probing.
2. Collect every explicitly named required business input in one prompt. Show
   documented choices and defaults, but do not select a default without user
   acceptance.
3. Probe with `{}` only when the operation explicitly takes no parameters, or
   when a payment-only 402 has no clear input hint.
4. Do not treat an empty object or a payment-only response as proof that an
   operation needs no input.
5. Do not add a parameter-confirmation card. Probe automatically once the
   required values are valid; the payment-confirmation card remains the only
   confirmation card.

## Workflow

1. Collect only documented business parameters. Preserve user-supplied JSON
   types and do not invent defaults, wrappers, methods, or selectors.
2. Probe automatically once all required values are valid. The CLI owns
   endpoint requests, method resolution, validation, candidate filtering,
   balance lookup, and payment preparation.
3. Route by `decision`, then `reason`, `nextAction`, and `payload`; never infer
   progression from prose.
4. After parameters are complete, show exactly one payment-confirmation card.
   This includes free results and insufficient balances. Follow
   `output-templates.md`. A free Probe returns only card facts and an opaque
   `confirmationId`; it does not expose the endpoint result yet.
5. If multiple candidates are returned, let the user select only among those
   candidates. Selection does not authorize payment.
6. If the selected candidate is insufficient, keep the same card and wait for
   the user to choose funding or cancellation. For `fund_a2mcp_token`, read
   `funding.md` and follow it end to end. Never show a QR from the initial Probe.
7. If the selected candidate is sufficient, offer confirm/cancel. On confirm,
    run `prepare-payment --yes` once with the selected action's bound params and
    route `execute_a2mcp_payment.params.paymentId` to the payment protocol. On
    cancel, end without preparing or paying.
8. For `free_confirmation_required`, offer confirm/cancel. On confirm, run
   `confirm-free --yes` once with the latest `confirmationId`; only that
   command returns `endpoint_result/free_result`. On cancel, end without
   releasing the stored result.

## Probe result routing

| Result | Handling |
|---|---|
| Success without payment | Show the single confirmation card from `free_confirmation_required`; release and present the result only after `confirm_a2mcp_free` |
| Missing or invalid structured fields | Show only the returned fields, collect valid user values, and re-Probe |
| Payment-only 402 with documented uncollected inputs | Stop before the card, collect those inputs, and re-Probe |
| Payment-only 402 without a clear input hint | Continue with empty parameters |
| No supported payment candidate | Block payment and explain the returned reason |
| Other block or error | Stop and follow `recovery.md` without exposing raw machine codes |

## Invariants

- Outside the Funding continuation owned by `funding.md`, the confirmation card
  is the only payment confirmation gate for an invocation.
- Endpoint and Request remain visible on an insufficient-balance card.
- A free result displays `Free` and does not invent payment metadata.
- A free result body is not present in the confirmation payload and is released
  only once by the CLI after explicit confirmation.
- For `fund_a2mcp_token`, follow `funding.md` end to end.
- A new invocation generation discards previous parameters, candidates,
  handles, token choices, and confirmation state.
- Payment-time changes to request, amount, token, network, payee, or scheme
  require a new invocation generation.
