# A2A Payment Output Templates

Presentation only. Payment recovery and new-paymentId rules remain in
`a2a_charge.md`.

## Payment insufficient balance

Match:

```text
phase=a2a_funding
decision=blocked
reason=insufficient_balance
```

```text
The A2A payment cannot continue because the {payload.fundingNeed.asset} balance is insufficient.

Current balance: {payload.fundingNeed.balance} {payload.fundingNeed.asset}
Required: {payload.fundingNeed.required} {payload.fundingNeed.asset}
Shortfall: {payload.fundingNeed.shortfall} {payload.fundingNeed.asset}

Fund the account before continuing this payment?
{render only the returned fund_account action label}
```

The Current balance line is mandatory. Render a localized unavailable value
when `payload.fundingNeed.balance` is null. Omit Required or Shortfall only when
the CLI could not resolve readable token decimals; never calculate either in
this template. Fall back to `payload.paymentAsset.currency` when `asset` is
null. Do not display `fundingTarget` or `qr` before the user selects
`fund_account`.
