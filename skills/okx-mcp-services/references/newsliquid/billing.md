# NewsLiquid Billing

## Module Description

Contents: [Flow](#flow), [Security](#security), [Purchase template](#purchase-options-template), [Confirmation](#confirmation-and-success-templates).

Buy a single month or quota package, inspect/recover its original order, or independently authorize and cancel future renewal. These flows reuse existing wallet and x402 signing. NewsLiquid purchases use new service orders; do not replace them with generic subscription access/change or marketplace tasks.

## Flow

### Collect Parameters

1. Reuse fresh account/catalog responses, or request `GET /billing/quota` and public `GET /billing/offers`. Show the purchase-options template below. The user may decline.
2. Show both monthly plans and one-time quota packages by default. Narrow only for an explicit product or category request.
3. Show each current product's name, price/currency and points or rights. Identify the plan required for the requested capability; quota alone cannot unlock plan rights. Account status lists only held categories, while the purchase catalog lists all options.
4. Select the actual product ID from its offer kind. A price-only reply (such as “2 USDT”) selects only a uniquely matching current offer; if multiple products match, clarify the product. It is not payment consent. Keep the current wallet, one stable Idempotency-Key and order ID in the Agent task context.
5. Preserve any interrupted query and its one-resumption limit in the current task context.

### Execute

#### Single month or quota package

1. After product selection, request `POST /billing/orders` with `kind=subscription` or `one_time`, the actual `productId` and the stable `Idempotency-Key` header. Retain the returned order ID, quote, expiry and wallet. Creation does not pay.
2. Request `POST /billing/orders/{orderId}/pay` without a body or payment header. Capture the complete encoded `PAYMENT-REQUIRED` header from its 402 response in the execution process.
3. Validate the native order/challenge through `payment validate` in prepare mode. Retain the complete unsigned approval snapshot. Use its checked terms and cap in the confirmation template; obtain consent before signing.
4. Re-read the same original order and check its status/expiry.
5. Keep the intended wallet. Refresh authoritative allowance for period payments and current plan expiry for renewal.
6. Validate the fresh native responses against the approved snapshot before signing. Stop on any difference; prepare and confirm the new terms again.
7. Capture the signer output in a variable using the root-linked CLI reference. Validate its proof and submit once in the same tool invocation. Never echo the variable.
8. Read the original order and quota. Report arrival only when `paymentStatus=paid`, `fulfillmentStatus=completed` and the rights are verified. Unknown signing/submission outcomes allow status queries only.
9. Resume an interrupted query once and show its actual result before asking about renewal.

#### Payment checks

The Rust validator owns order/challenge consistency, integer ranges, exact cap calculation and signed-proof comparison. It reuses the subscription signer's arithmetic. The Skill selects the product, shows checked values, obtains consent and stops on validation errors.

| Purchase | Required terms |
| --- | --- |
| Quota package | One `exact` candidate matching the quoted amount |
| Single month | One `period` candidate; one calendar month; initial charge of one period |
| Automatic renewal | One `period` candidate; 12 calendar months; zero initial charge; future start matching current plan expiry |

Multiple candidates stop before signing. Changed quote, wallet, cap or renewal terms require new confirmation. The signing boundary pins the wallet and the confirmed cap. This order-based path uses `--no-cache`, preserving any existing active host mapping.

#### Arrival and optional automatic renewal

1. After a new single-month plan arrives, ask once whether to enable renewal. If there is no interrupted query, ask in the arrival notice. Ordinary login, viewing or use does not repeat this question.
2. Decline/no answer leaves renewal off. If renewal is already active, show its actual state without asking to open another authorization.
3. If the user chooses renewal, create a separate order with `kind=auto_renew` and the current `subscription.planId`. The plan must have more than 20 minutes until expiry.
4. Fetch that original order's payment challenge and apply the payment checks above. Fill the independent-renewal template with quoted `startAt`, 12 one-month periods and zero initial charge; obtain separate consent.
5. Sign and submit once using the same period-payment steps. Use the success template only after the order is `authorized` and the account renewal is `active`.

#### Renewal cancellation

1. Request `GET /billing/quota` and `GET /billing/subscriptions`. Identify this wallet's actual `autoRenew.subId`. Show the plan, wallet and paid expiry; obtain separate cancellation consent.
2. Run `onchainos payment subscription cancel --sub-id "$NEWSLIQUID_SUB_ID" --token "$NEWSLIQUID_ASSET" --chain xlayer --from "$NEWSLIQUID_WALLET" --valid-for-seconds 300`. The existing payment module resolves the authoritative contract and signs CancelAuth.
3. Submit its `data.cancelAuth` as `{"cancelAuth":...}` through `POST /billing/subscriptions/{subId}/cancel`, bound to the same wallet. Capture the signer output in a variable and submit within the same tool invocation. Check the signed subId, action=0, initiator=0 and deadline before submitting. Never echo CancelAuth or run the signer separately.
4. Verify cancellation through service subscriptions and quota. Then report that future charging stopped and paid rights remain to expiry. Cancellation is not a refund or token-allowance revocation.

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

Stop on mismatched terms, wallet, endpoint or contracts. A transport request does not obtain consent or verify payment terms. Complete the checks above before using the existing signer.

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

Use the templates below.

- Keep their structure, label meanings, field order and closing consent wording.
- Fill actual values and translate only the prose.
- Omit recipient addresses, order IDs, raw quote/typed-data fields and technical contract labels from routine confirmation.
- Verify rendered terms against the order's `autoRenewTerms.text`; do not repeat the raw terms.
- Explain other material authorization conditions in plain language.
- Preserve real wallet warnings and requested diagnostics. Do not add duplicate confirmation.

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

- Keep order ID, recipient and authoritative contract checks in the payment checks above; omit them only from routine user-facing copy.
- Show this order's actual payment in the template, not a larger allowance cap.
- If existing commitments make the signed permit cap exceed this payment, explain the actual cap and reason before consent, even when token allowance is sufficient.
- A required token-allowance increase needs its actual amount/change and separate consent through the existing wallet flow.
- Do not require an increase when allowance is sufficient or suppress a wallet risk warning.

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

- [service-cli-reference.md](../service-cli-reference.md)
