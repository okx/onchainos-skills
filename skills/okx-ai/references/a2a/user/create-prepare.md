# One-Time Task Preparation

## Module Description

Use this leaf only after the User explicitly selects an A2A Service. Keep the
selected numeric `sid` internal and run:

```bash
onchainos agent task-create-prepare --sid <selected-sid>
```

Run this read exactly once for the selected `sid` in one turn. Once the
command has started, wait for that invocation and never start a duplicate
because output is delayed. Reuse its returned result for the rest of the turn.

The CLI already checks login, User identity, authoritative Service state,
subscription conflicts, effective price or trial, and payable balance. Do not
repeat those reads in the Skill. A successful response contains
`phase/decision/reason/nextAction/payload` under `data`; select exactly one row
from the Result matrix below without re-entering a router.

## Flow

### Result matrix

| Phase / reason | Required behavior |
|---|---|
| `login_validation / login_required` | Run only the returned `login` action; then rerun prepare with the same arguments. |
| `identity_validation / user_identity_required` | Register a User Agent; then rerun this prepare with the same `sid`. |
| `service_validation / unsupported_service_type` | Explain the returned reason and stop. |
| `subscription_validation / duplicate_subscription` | Read `duplicate-subscription.md` and use only the returned restore/stop actions. |
| `funding_required / insufficient_balance` | Follow [Insufficient balance](#insufficient-balance) below. |
| `creation / all_checks_passed` | Bind the returned Service payload and enter `create.md`. |

`decision=ready` means preparation succeeded; it is not authorization to create
or pay. Invalid Service data and dependency failures are command errors, not
new business cases. Never derive an action from `phase`, prose, or a previous
prepare result.

A2MCP services are rejected here. Prepare them through the explicit MCP branch
in [`../../a2mcp/handoff.md`](../../a2mcp/handoff.md).

Use `payload.serviceId` as the later creation UUID. `payload.sid` remains only
the search/preparation selector. A non-blank `serviceGuide` carries its matching
CLI-derived `serviceGuideHash`; retain both exactly.

## Troubleshooting

### Insufficient balance

#### Funding actions

- On `select_funding_token`, show the returned funding options and ask the User
  to choose USDC, USDG or USDT. Do not choose for the User or combine balances.
- On `fund_selected_token`, enter the
  [`okx-agentic-wallet` funding leaf](../../../../okx-agentic-wallet/references/funding.md)
  with the returned bundle. Follow the rendering rules below.

After a choice, a changed choice, or a report that funding is complete, run:

```bash
onchainos agent task-create-prepare \
  --sid <resumeContext.sid> \
  --funding-token-symbol <chosen-USDC-USDG-or-USDT> \
  [--payment-token-symbol <resumeContext.paymentTokenSymbol>]
```

- On a fresh `open_create_playbook`, return to the appropriate confirmation card
  with retained inputs. Ask for final task confirmation again.
- If still insufficient, follow the latest funding action. A query error stops
  recovery. Trial eligibility alone does not prove funding succeeded.
- Recovery never creates automatically and does not charge funds.

#### Recovery rules

- Retain the Service, task fields, attachments, Guide, Consent and auto-renew
  preference. Keep `serviceGuide` and `serviceGuideHash` unchanged.
- Keep `resumeContext.sid` and the chosen funding target when returning from the
  funding leaf. Preserve all prepare arguments through login recovery.
- With `resumeContext.paymentTokenSymbol`, retain its matching
  `resumeContext.serviceTokenAddress`. Pass the symbol as `--payment-token-symbol`
  and use the same `--funding-token-symbol`. Show and fund only this token.
- Without that constraint, omit the payment flag and allow changes to the funding
  target. Never turn a funding target into a payment constraint.
- To change an explicit payment token, restart creation confirmation with the new
  choice.

#### Rendering rules

- For `select_funding_token`, show `quoteAmount` in USD and each returned
  `fundingOptions` entry: token, balance, required amount and shortfall.
- Show balances and shortfalls in token units with USD equivalents. Use the fixed
  1 USD = 1 token unit rule for supported stablecoins; do not fetch exchange rates.
- For `fund_selected_token`, use the shared funding template. Also show the
  selected token contract and the shortfall's USD equivalent.
- Preserve precision. Show EVM addresses in lowercase; keep original values in
  machine fields.
- Explain that choosing a funding target alone does not fix the payment token.
