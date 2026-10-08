# A2MCP Output Templates

Presentation only; follow `invoke.md` for result routing and actions. Use the
immutable marketplace `serviceSnapshot`, the typed parameters supplied to
`--params-base64`, and the latest structured invocation result.

## Service card

Render exactly these five rows in order before the result, payment candidates,
or funding prompt. Localize labels and static values such as `Free`; preserve
service names, identifiers, addresses, and parameter values.

| Field | Value |
|---|---|
| Service Provider | {provider} |
| Service Name | {serviceSnapshot.serviceName or —} |
| Endpoint | {serviceSnapshot.endpoint} |
| Fee | {fee} |
| Service Parameters | `{typed parameter JSON}` |

- `provider`: `Agent ID {serviceSnapshot.asp.aspAgentId}`, or `—` when absent
  or blank. Use `—` for a missing or blank Service Name too.
- `fee`: `Free` only for `data.needsConfirm=false` with `data.result`;
  otherwise use the selected candidate's `amountHuman` and `tokenSymbol`.
  For `scheme=upto`, prefix `Up to` to show the authorization cap.
- Render parameter JSON as inline code; escape Markdown as needed without
  changing the displayed values. Keep balance and payment details below the
  card rather than adding rows.

## Payment candidates

For paid results, show the selected entry from `data.candidates` first, then
`data.alternatives` in returned order, clearly marked as alternatives:

| Token | Network | Fee | Available Balance | Status | Shortfall |
|---|---|---|---|---|---|
| {tokenSymbol} | {chainName or network} | {amountHuman} | {availableAmount or Unavailable} | {localized balanceStatus} | {shortfall or —} |

Apply the same `Up to` prefix to candidates with `scheme=upto`. Show the
selected candidate's `scheme` and `data.recipient` below the table. Do not
invent missing balances or addresses. Omit this section for free results.
