# Swap Output Templates

Presentation only. Swap business flow and action routing remain in `swap.md`
and the shared Action Routing reference.

## Swap insufficient balance

Match:

```text
phase=swap_funding
decision=blocked
reason=insufficient_balance
```

```text
The swap cannot be quoted because the {payload.fundingNeed.asset} balance is insufficient.

Current balance: {payload.fundingNeed.balance} {payload.fundingNeed.asset}
Required: {payload.fundingNeed.required} {payload.fundingNeed.asset}
Shortfall: {payload.fundingNeed.shortfall} {payload.fundingNeed.asset}

Fund the account before continuing this swap?
{render only the returned fund_account action label}
```

The Current balance line is mandatory. Render a localized unavailable value
when `payload.fundingNeed.balance` is null. Omit Shortfall only when it is null;
never calculate it in this template. Do not display `fundingTarget` or `qr`
before the user selects `fund_account`.
