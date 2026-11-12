# NewsLiquid Operation Reference

## Commands

### `onchainos service call --service newsliquid`

Contents: [parameters](#parameters), [results](#return-fields), [examples](#examples), [errors](#error-handling).

#### Syntax

Use the dedicated operations below. The HTTP reference lists each catalog endpoint's exact CLI operation and business fields.

- Set `--operation` to the documented operation name, not the HTTP path.
- Put catalog request body or query fields in `--params`. Pass path IDs separately in `--id`.
- The CLI supplies authentication and protocol headers internally.

```bash
onchainos service call --service newsliquid --operation login
onchainos service call --service newsliquid --operation status
onchainos service call --service newsliquid --operation news-search --params '{"coins":["BTC"],"limit":3}'
onchainos service call --service newsliquid --operation twitter-user-tweets --params '{"username":"VitalikButerin","maxResults":20}'
onchainos service call --service newsliquid --operation billing-orders-id --id "$NEWSLIQUID_ORDER_ID"
onchainos service call --service newsliquid --operation create-order --params "$NEWSLIQUID_CREATE_PARAMS"
onchainos service call --service newsliquid --operation prepare-payment --id "$NEWSLIQUID_ORDER_ID" --params '{"kind":"subscription"}'
onchainos service call --service newsliquid --operation pay --id "$NEWSLIQUID_ORDER_ID" --params "$NEWSLIQUID_PAY_PARAMS" --confirmed
onchainos service call --service newsliquid --operation cancel-renewal --id "$NEWSLIQUID_SUB_ID" --confirmed
onchainos service call --service newsliquid --operation twitter-watch-add --params '{"username":"VitalikButerin","newTweetBol":true,"newTweetReplyBol":false,"newTweetQuoteBol":false,"newRetweetBol":false}' --confirmed
onchainos service call --service newsliquid --operation billing-orders-id-reconcile --id "$NEWSLIQUID_ORDER_ID"
```

#### Parameters

| Operation | Parameters |
| --- | --- |
| login | Optional `confirmWalletWarning` boolean; a warning continuation also requires its original `wallet` |
| status | Empty params; no ID; returns local credential metadata |
| token | Empty params; no ID; reads the current wallet's valid Token for a user-managed client; no login or signing |
| create-order | `kind`: subscription/one_time/auto_renew; current integer `productId`; stable `operationKey` (1–128 ASCII letters/digits/underscore/hyphen) |
| prepare-payment | Order ID via `--id`; params `kind` from the chosen subscription/one_time/auto_renew purchase |
| pay | Order ID via `--id`; params `approval` containing the complete trusted preparation result; requires `--confirmed` |
| cancel-renewal | Current-wallet renewal subId via `--id`; optional `confirmWalletWarning` boolean and original `wallet` for warning continuation; requires `--confirmed` |
| Catalog operations | Exact operation name and business fields from HTTP reference; body/query fields via `--params`, path IDs via `--id`; monitor add/delete require `--confirmed` |
| billing-orders-id-reconcile | Original order ID; no body or signature; empty params |

Set NEWSLIQUID_CREATE_PARAMS to a JSON object using the selected offer's actual productId and one stable operationKey. Set NEWSLIQUID_PAY_PARAMS to a JSON object whose `approval` is the complete trusted unsigned preparation result. The Agent retains these values in task context; no provider state file or Token parameter is used.

#### Return Fields

The common wrapper is `{ok:true,data:{service,result}}`. Inspect the provider `result`; HTTP results retain `ok/httpStatus/data/retryAfter/paymentRequired/requestContext` and exact 402 `failureKind`. Outer success does not prove content or fulfillment.

- Login result: wallet, expiresAt and reused; no Token.
- Status result: wallet, authenticated, expiresAt; no account rights read. Use billing-quota for rights.
- Token result: wallet, token and expiresAt. Only this operation returns the raw service Token; ordinary results remain redacted.
- Preparation: unsigned quote, accepted challenge, autoRenewTerms, allowance preview, expiry and requiresConfirmation. Signed Permit fields appear later; CLI verifies them before submission.
- Allowance preview: permitAllowanceCapAmount, newCommitmentAmount and reservedAmount in minimal units. Only permit2_allowance_insufficient plus stage=before_signing proves a retryable pre-sign failure; message text does not.
- Query results: quotaState, quotaCheck and purchaseOptions when needed. GET follow-ups are bounded to five seconds and preserve the original result on failure.
- Signed cadence/identity/cap/window mismatches stop submission; unknown signatures/submissions return a query-only diagnosis. The CLI does not persist cross-call attempt guards.

#### Examples

```bash
onchainos service call --service newsliquid --operation login --params "$NEWSLIQUID_WARNING_PARAMS"
onchainos service call --service newsliquid --operation cancel-renewal --id "$NEWSLIQUID_SUB_ID" --params "$NEWSLIQUID_WARNING_PARAMS" --confirmed
onchainos payment subscription allowance-status --token "$NEWSLIQUID_PAYMENT_ASSET" --chain xlayer --from "$NEWSLIQUID_WALLET"
```

NEWSLIQUID_WARNING_PARAMS contains `confirmWalletWarning=true` and the actual `wallet` from the returned warning. Use it only after that warning has been accepted.

For user-managed clients only, capture the Token inside the client process. Before each connection, verify that the returned wallet matches the client's intended plan wallet. Stop on a mismatch.

```python
import json
import subprocess

credential = json.loads(subprocess.check_output(
    ["onchainos", "service", "call", "--service", "newsliquid", "--operation", "token"],
    text=True,
))["data"]["result"]
# Use credential["token"] for the WSS handshake; do not print or persist it.
```

#### Error Handling

| Error | Meaning |
| --- | --- |
| monthly_cadence_required | Reject non-calendar-month terms; never rewrite them |
| renewal_start_or_initial_charge_mismatch | Renewal requires a valid future integer `startAt` matching the quote and zero initial charge; stop before signing |
| credentialInvalidationFailed=true in an HTTP result | The original 401 response is preserved, but credential cleanup failed |
| signed_subscription_terms_mismatch / signed_permit_window_too_short | Signed terms or window do not cover the approved order; no proof submitted |
| order_not_payable | Order is not explicitly unpaid; query its status without a fresh signature |
| payment_terms_changed | Re-read and show changed terms; obtain new consent |
| newsliquid_operation_outcome_unknown | Stop; error data identifies the target/stage and retry=query-only |
| permit2_allowance_insufficient with before_signing | Prepare/continue the original order only after approved bounded allowance |
| approval_verification_unsupported | Internal operation cannot continue that verification scene |
| order_expired | Original quote expired; inspect before another purchase |
| wallet_warning_requires_original_wallet | Warning continuation is missing its original wallet |
| newsliquid_login_required from token | No valid current-wallet service credential; complete CLI login before the client retries |

## Command Constraints

- NewsLiquid uses a fixed HTTPS destination, existing credential storage and selected-wallet binding. No automatic charged-query/pay retry.
- `token` reads local credentials only. It does not verify plan rights, refresh login or grant payment authority. Capture its JSON only in the user's client; keep it out of AI tool results, logs and source code.
- The CLI stores no provider order, login challenge, cancellation history or lock file. Order/subscription state is read from 6551. The Agent owns sequential attempts and query-only recovery; the CLI cannot remember attempts across processes.
- Single-month uses one period; renewal uses twelve calendar-month periods, a valid future integer `startAt` matching the quote, and zero initial charge. Signatures remain in existing wallet/payment modules.
- No standalone compatibility/import/state-write commands are exposed. See the root-linked common CLI reference for generic CLI flags.
