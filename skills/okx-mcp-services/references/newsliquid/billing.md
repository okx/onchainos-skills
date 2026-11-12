# NewsLiquid Billing

## Module Description

Contents: [Flow](#flow), [Security](#security), [Purchase template](#purchase-options-template), [Confirmation](#confirmation-and-success-templates).

Buy a single month or quota package, inspect/recover its original order, or independently authorize and cancel future renewal. These flows reuse existing wallet and x402 signing. NewsLiquid purchases use new service orders; do not replace them with generic subscription access/change or marketplace tasks.

## Flow

### Collect Parameters

1. Use `quotaCheck` and `purchaseOptions` supplied by the query Flow, or read `billing-quota` and public `billing-offers` when absent. Show the purchase-options template below. The user may decline.
2. Show both monthly plans and one-time quota packages by default. Narrow only for an explicit product or category request.
3. Show each current product's name, price/currency and points or rights. Identify the plan required for the requested capability; quota alone cannot unlock plan rights. Account status lists only held categories, while the purchase catalog lists all options.
4. Select the actual product ID from its offer kind. A price-only reply (such as “2 USDT”) selects only a uniquely matching current offer; if multiple products match, clarify the product. It is not payment consent. Keep the current wallet, one operation key and order ID in the Agent task context.
5. Preserve any interrupted query and its one-resumption limit in the current task context.

### Execute

#### Single month or quota package

1. After product selection, call create-order with kind=subscription or one_time, the actual productId and stable operationKey. Creation does not pay.
2. Call prepare-payment with the order ID via `--id` and its selected `kind` in params. The CLI reads the order from 6551. Check the returned terms against its quote and the confirmation table below.
3. Render the single-payment template below with the current product, quote and preparation result. Obtain consent through the existing wallet interaction.
4. After consent, call pay with the order ID, `--confirmed` and the complete unsigned preparation result as `params.approval`. Keep this object in task context, without writing a file. The CLI re-reads the order/challenge and rejects changed terms before signing.
5. Read the original order and quota. Report arrival only when `paymentStatus=paid`, `fulfillmentStatus=completed` and the corresponding rights are verified.
6. Resume an interrupted query once and show its actual result before asking about renewal.

#### Arrival and optional automatic renewal

1. After a new single-month plan arrives, ask once whether to enable renewal. If there is no interrupted query, ask in the arrival notice. Ordinary login, viewing or use does not repeat this question.
2. Decline/no answer leaves renewal off. If renewal is already active, show its actual state without asking to open another authorization.
3. If the user chooses renewal, create a separate `auto_renew` order for the current `subscription.planId`. The plan must have more than 20 minutes until expiry.
4. Prepare that order with kind=auto_renew and read current allowance through the existing `payment subscription allowance-status` command. Use the actual preview fields documented in the CLI Reference.
5. Check the renewal terms in the confirmation table, fill the independent-renewal template and obtain separate consent.
6. Pay that renewal order with its approved preparation object and `--confirmed`. Use the success template only after its order is `authorized` and the account renewal is `active`.

#### Renewal cancellation

1. Read quota/subscriptions and identify this wallet's actual `autoRenew.subId`. Show the target plan, wallet and paid expiry; obtain separate cancellation consent.
2. Call cancel-renewal with that subId via `--id` and `--confirmed` for that target.
3. Verify actual cancellation through service subscriptions and quota. Then report that future charging stopped and paid rights remain to expiry. Cancellation is not a refund or token-allowance revocation.

#### Confirmation checks

Use the latest trusted `prepare-payment` result. The CLI validates the order, contracts and signed payload internally.

| Purchase | Kind | Terms to confirm |
| --- | --- | --- |
| Quota package | `one_time` | Quoted one-time amount; no automatic renewal |
| Single month | `subscription` | One month; `maxPeriods=1` |
| Future renewal | `auto_renew` | 12 one-month periods; quoted `startAt`; zero initial charge; actual allowance cap |

#### Plan changes

| Purchase | Verified behavior to show |
| --- | --- |
| Same-tier manual repurchase | A month is added after paid service expiry; current-period quota does not reset |
| Upgrade | Arrival activates the new tier at its full monthly price; unused old quota moves to one-time quota |
| Downgrade | The new tier queues after the current plan; show its actual effective time |

### Handle Result

Use authoritative order and account states. For failure, pending or unknown results, use the root-linked troubleshooting reference. It owns allowance failures, wallet warnings, order reconciliation, cancellation failures and renewal conflicts.

## Security

### Confirmation

Use the confirmation templates below with actual values. Payment, future renewal and cancellation require their own consent. Respect the existing wallet interaction; a business 402 grants no payment authority.

### Risk and Stop Conditions

Stop on mismatched terms, wallet, endpoint or contracts. The CLI `--confirmed` flag records consent already obtained; it does not obtain consent.

## Output Rules

- Show prices and points as exact decimal strings; use quoted `amount` and `decimals` for on-chain values.
- Use the actual authorization cap. If it includes other reserved commitments, explain that difference from this plan's renewal cost in plain language; avoid duplicate cap fields in the normal template.
- Label actual arrival, authorization and cancellation states; pending remains confirmation in progress.

### Purchase options template

Render the reason, account and catalog supplied by the query Flow. Fill rows from the current response; apply an explicit product/category choice, and label products that cannot unlock a required plan right.

```text
{Available quota is exhausted / This request exceeds available quota / Explain the plan required for this feature}
Current wallet: {address} | Available quota: {actual totalRemaining, or unknown if unverified}
To continue using 6551, choose:
Monthly plans (one month only; no automatic renewal by default)
- {each plan's name, actual monthly price/currency, quota and rights}
One-time quota packages
- {each package's name, actual price/currency and points}
Purchase on {actual network}. Rights are bound to the wallet above.
Which option would you like? You may also decline. No payment will be made before confirmation.
```

### Confirmation and success templates

Use the PRD case 2 and case 3 templates below:

- Keep the structure, labels, field order and closing consent wording. Fill actual values and translate prose.
- Omit recipient addresses, order IDs, raw quote/typed-data fields and technical contract labels from routine confirmation.
- Use `autoRenewTerms.text` to verify the rendered terms rather than showing a second raw block. Explain additional material authorization conditions in plain language.
- Preserve actual wallet risk warnings and requested diagnostics. Do not add duplicate confirmation.

Single month or one-time quota package:

- Resolve missing authoritative fields before confirmation. Do not invent values or shorten the template.
- Do not append internal token aliases to the amount.

```text
Please review the payment details:
- Product: {product name}
- Payment wallet: {address}
- Payment network: {network}
- Authorized payment amount: {this order's amount and currency}
- Quote expires: {time and timezone}
After purchase, rights will be bound to the EVM wallet above.
This pays only the current order. It does not enable automatic renewal or authorize future automatic charges.
Do you confirm authorization and payment?
```

- Keep order, recipient and authoritative contract checks inside the CLI. Show this order's actual payment in the template.
- If the signed permit cap exceeds this payment because of existing commitments, explain the cap and reason before consent, even when allowance is sufficient.
- A required allowance increase needs separate consent through the existing wallet flow. Show its actual amount/change. Do not request an increase when allowance is sufficient or suppress a wallet warning.

Independent automatic-renewal authorization:

```text
Please review the automatic renewal and payment authorization:
- Plan: {name}
- Payment wallet: {address}
- Payment network: {network}
- Renewal amount: {amount and currency} per month
- Renewal periods: 12 (1 month each)
- First charge: {current plan's actual expiry time and timezone}
- Payment authorization cap: {actual maximum authorized amount and currency}
{If an increase is needed: Enabling 12-period renewal requires raising the payment authorization cap from {old amount} to {actual maximum amount}. Otherwise: The current authorization cap is sufficient; no increase is needed.}
No charge is made now. Monthly charges begin when the current plan expires, for up to 12 periods. You may cancel automatic renewal at any time.
To confirm authorization, reply: Enable automatic renewal for {name} and authorize {actual maximum amount and currency}.
```

Render one of the two allowance-change sentences, with actual old/new values. Do not claim an increase when none is needed. If the overall cap includes existing commitments, explain that inclusion and this plan's 12-period cost in plain language; do not imply the cap is the immediate charge. Cancellation preserves paid rights, and five consecutive charge failures disable renewal; explain these conditions when relevant without repeating the whole terms payload.

After verified authorization and an active account renewal:

```text
Automatic renewal for {plan} is authorized and verified as active. Your current plan remains valid; no immediate charge was made.
- Monthly amount: {actual amount and currency}
- First renewal / Next renewal: {actual next charge time and timezone}
- Authorization limit: up to 12 future periods, totaling no more than {this plan's 12-period renewal cost and currency}
- Cancel at any time by asking to cancel automatic renewal for {plan}.
```

Use First renewal for a new authorization and Next renewal for a later status query. If the overall allowance also covers other commitments, describe that distinction separately in plain language. This success template describes authorization at activation; for later status queries, report actual subsequent charges rather than repeating "no immediate charge was made".

## Troubleshooting

Use the centralized troubleshooting reference linked from the root Skill.

## CLI Reference

- [client-cli-reference.md](client-cli-reference.md)
