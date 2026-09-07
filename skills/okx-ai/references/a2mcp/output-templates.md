# A2MCP Output Templates

Use only after `invoke.md` explicitly routes a `payment_confirmation` result
here. Use the latest structured CLI result only; never reuse the previously
loaded Identity template with the same basename.

## Service card

Require `payload.presentation.type=a2mcp_confirmation`. Render its
`columns[]` and `rows[]` in returned order as one Markdown table. Localize only
column/row labels and static sentinels such as `Free` or `Select a payment
option`; preserve every returned value. Render the Service Parameters value as
inline code. Do not replace the table with bullets, a sentence, or a paragraph.

The CLI presentation contract is equivalent to:

```markdown
| Field | Value |
|---|---|
| {localized presentation.rows[0].label} | {presentation.rows[0].value} |
| {localized presentation.rows[1].label} | {presentation.rows[1].value} |
| {localized presentation.rows[2].label} | {presentation.rows[2].value} |
| {localized presentation.rows[3].label} | {localized presentation.rows[3].value when static} |
| {localized presentation.rows[4].label} | `{presentation.rows[4].value}` |
```

- Require exactly these row keys in this order: `serviceProvider`,
  `serviceName`, `endpoint`, `fee`, `serviceParameters`.
- Parameter keys and types are dynamic. Never extract or invent an `Asset`,
  `Token`, HTTP method, or other business row.
- `Free` is localized as free and never displayed as `0` or `0 {token}`.
- If presentation is absent or malformed, do not improvise another layout;
  report that the installed CLI/Skill contract is incompatible and stop.

Immediately below the table render a localized `Recommend actions:` label.
Use only the latest non-blank `nextAction[].actionLabel`; never invent an
operation from `reason`, price, balance, or prose.

- For `free_confirmation_required`, combine the returned confirm and cancel
  labels into one numbered localized instruction asking whether to invoke the
  service.
- For `payment_confirmation_required`, combine the returned confirm and cancel
  labels into one numbered localized instruction asking whether to pay.
- For `token_selection_required`, ask the User to select a numbered candidate;
  cancellation remains available.
- For `insufficient_balance`, list the returned Funding, alternative-selection,
  and cancellation labels in their returned order. Never offer confirmation.

Wait after rendering. A displayed action is not authorization.

## Payment candidates

For every paid state, render returned candidates in order; one candidate still
gets one row. Before selection, show every candidate simultaneously. This
candidate table follows the Service card and precedes Recommend actions.

```markdown
| # | Token | Network | Fee | Available Balance | Status | Shortfall |
|---|---|---|---|---|---|---|
| 1 | {tokenSymbol} | {chainName or network} | {amountDisplay} | {availableDisplay} | {localized balanceStatus} | {shortfallDisplay or —} |
```

A selection binds only that candidate to `select_a2mcp_token`; it never
authorizes payment or permits an automatic token/network switch.

For `free_confirmation_required`, do not render payment candidates, network, or
balance. Only the returned confirmation action may invoke `confirm_a2mcp_free`.
For insufficient balance, Funding enters `funding.md`; only that file may
continue the bound payment after funding.
