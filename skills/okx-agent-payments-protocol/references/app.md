# APP/1 Payment Flows

## Module Description

Use this reference only when the challenge declares `protocol="app/1"`.
This page owns APP decoding, confirmation, wallet checks, and execution.
Mid-session operations load it from their saved APP context.

APP/1 reuses the MPP HTTP framing (`WWW-Authenticate: Payment`,
`Authorization: Payment`, and `Payment-Receipt`) and adds `protocol="app/1"`.
The buyer signs through the CLI. The CLI returns an `authorization_header` for
replayable operations and a signed authorization object for subscription
cancellation. The buyer never calls the Facilitator directly.

All APP signing is TEE-only. Local private-key signing is not supported. Stop if
the wallet session is unavailable and the user cannot log in.

Use this page by intent:

- `charge`: one-shot exact amount.
- `upto`: variable settlement up to a ceiling.
- `session`: channel lifecycle with `open`, `voucher`, `topup`, and `close`.
- `subscription`: recurring payments authorized by one Permit2 allowance.

Page order:

1. [Collect parameters](#collect-parameters).
2. Apply the [confirmation and stop rules](#security).
3. [Execute the matching intent](#execute).
4. [Handle the result](#handle-result).
5. Use [Troubleshooting](#troubleshooting) and the [CLI Reference](#cli-reference)
   when needed.

## Flow

### Collect Parameters

Keep the full `WWW-Authenticate: Payment ...` challenge unchanged for execution.
Base64url-decode its `request` locally and collect the common terms: operation,
chain, token, recipient, challenge expiry, and who pays gas.

For `charge` and `upto`, select the signature type exactly as the CLI does. If
`acceptedTypes` exists, prefer `authorization`, then `permit2`, then `hash`.
Otherwise use `authorization` when `feePayer=true` and `hash` when
`feePayer=false`. Pass the confirmed type with `--type` so execution cannot
select a different type. `upto` does not allow `hash`.

For subscription `subscribe` or `change`, read the new commitment from the
challenge. Before confirmation, run the read-only `allowance-status` command and
read its `reservedAmount`. Show the final cumulative Permit2 authorization as
`reservedAmount + newCommit`. Save that confirmed atomic value and pass it as
`--expected-cap`. The CLI stops before signing if the current value changed.
Never invent or manually override these values.

### Execute

Complete [Confirmation](#confirmation) before running any signing or broadcast
command below.

#### Charge

Use `charge` for a one-shot exact amount. Its signature type is:

- `authorization`: EIP-3009, gasless, and the default when `feePayer=true`.
- `permit2`: Permit2 SignatureTransfer with a witness.
- `hash`: the user broadcasts first; requires `feePayer=false`.

```bash
onchainos payment app charge \
  --challenge '<full WWW-Authenticate header value>' \
  [--type authorization|permit2|hash] \
  [--tx-hash '0x<64-char hex>'] \   # hash mode only
  [--from '<0xPayer>']
```

When `acceptedTypes` is present, select `authorization`, then `permit2`, then
`hash`. Without `acceptedTypes`, use `authorization` for `feePayer=true` and
`hash` for `feePayer=false`. Skill and CLI execution use the same algorithm.

The command returns `{ ok, data: { authorization_header, mode, wallet, ... } }`.
Save `authorization_header` for [result handling](#handle-result). In hash mode,
the user must broadcast `transferWithAuthorization` first. Offer to hand off to
`okx-agentic-wallet`, then pass its `0x` hash with `--tx-hash`.

#### Upto

Use `upto` to authorize a ceiling. The seller runs the service and settles the
actual amount through the Facilitator. The actual amount cannot exceed the
ceiling. The challenge decides whether `authorization` or `permit2` is used.
Hash mode is not allowed because the seller must relay a lower actual amount.

```bash
onchainos payment app upto \
  --challenge '<full WWW-Authenticate header value>' \
  [--type authorization|permit2] \
  [--from '<0xPayer>']
```

Save the returned `authorization_header` for [result handling](#handle-result).

#### Session

The lifecycle is `open` → `voucher` × N → `close`, with optional `topup` calls.
APP/1 opens are deposit-only and do not contain an initial voucher. The flow
reuses the existing channel contract and Voucher EIP-712 type.

Only `open` fetches a challenge. Save the exact open
`WWW-Authenticate: Payment ...` header and returned `channel_id`. Every later
`voucher`, `topup`, and `close` for that channel must use the same saved header.
The complete lifecycle shares the `offerId` from the open challenge's `opaque`.
`open` is the only step whose challenge comes from a fresh 402.

Do not re-request the resource to obtain a new challenge for `voucher`, `topup`,
or `close`. A new challenge has a new `offerId`; the Facilitator rejects it with
`offer_not_found`. If the open challenge is missing, ask the user for it or open
a new channel. Never fabricate or replace `opaque`.

The seller defines pricing in the decoded request:

- `request.amount` is the fixed deposit. The buyer's `authorization.value` must
  equal it. It is not an under-fillable cap.
- `request.methodDetails.meter` is optional and contains `{ unit, unitPrice }`.

Show the deposit and `meter.unitPrice` with its `unit`. The user chooses each
Voucher `cumulativeAmount`. Do not invent it. Supported usage patterns are:

- Prepay and replay: sign one Voucher up to the deposit, then replay the same
  `authorization_header` for each access.
- Per-use increments: sign a new Voucher with `cumulativeAmount` increased by
  `unitPrice`, or by `units × unitPrice`.

The seller is authoritative on price and accrued usage. It deducts `unitPrice`
per use and rejects a Voucher that does not cover accrued spend. Track these
local values:

- `deposit`: starts at the open deposit and increases after a successful topup.
- `current_cum`: starts at `0` and becomes the latest successful Voucher
  `cumulativeAmount`.

Before a Voucher command, enforce:

```text
current_cum < new_cumulative_amount <= deposit
```

If the amount does not increase, ask for a higher amount. If it exceeds the
deposit, run topup first and re-evaluate. The CLI repeats this check against its
persisted channel state before TEE signing. Missing or inconsistent state fails
closed.

Close does not authorize new spend. Set `final_cum = current_cum`. Do not raise
or lower it. The CLI verifies the same equality and deposit bound before signing.

```bash
# open — deposit authorization (EIP-3009 via TEE), or hash mode (broadcast first).
# The CLI always uses challenge.request.amount as the fixed deposit.
onchainos payment app session open --challenge '<hdr>' \
  [--from '<0xPayer>'] \
  [--tx-hash '0x..' --salt '0x..'] [--authorized-signer '0x..']

# voucher — sign an absolute cumulative amount (strictly increasing).
# = user-chosen prepay (up to deposit) OR units×unitPrice from meter.
onchainos payment app session voucher --challenge '<hdr>' \
  --channel-id '0x..' --cumulative-amount '<atomic>' --escrow '0x..' --chain-id <n> [--from '0x..']

# topup — add deposit (EIP-3009 via TEE, or --tx-hash hash mode).
onchainos payment app session topup --challenge '<hdr>' \
  --channel-id '0x..' --additional-deposit '<atomic>' --escrow '0x..' --chain-id <n> \
  [--currency '0x..'] [--tx-hash '0x..'] [--from '0x..']

# close — sign the final voucher as a close credential (action=close).
onchainos payment app session close --challenge '<hdr>' \
  --channel-id '0x..' --cumulative-amount '<atomic>' --escrow '0x..' --chain-id <n> [--from '0x..']

# After replay, resolve topup or close with the returned operation_id.
onchainos payment app session resolve --operation-id '<op_..>' --receipt '<Payment-Receipt value>'

# Use one of these only when replay has no authoritative receipt.
onchainos payment app session resolve --operation-id '<op_..>' --outcome network-error
onchainos payment app session resolve --operation-id '<op_..>' --outcome no-receipt
```

`open`, `voucher`, `topup`, and `close` return an `authorization_header`. Replay
it to the session endpoint. The seller dispatches by `payload.action`. For
`close`, the seller adds its payee `CloseAuthorization` before submitting
`POST /session/close`.

`topup` and `close` also return an `operation_id`. They keep the operation
pending without changing the confirmed channel state. After replay, run
`session resolve` with that ID. A `success` receipt commits the topup or removes
the closed channel. A `failed`, `pending`, or `timeout` receipt clears the
pending operation without changing confirmed state. Network errors and missing
receipts also clear it through `--outcome`. While an operation is pending, do
not sign another Voucher, topup, or close for the same channel. Repeating
`resolve` for the same `operation_id` returns the saved result without applying
the state change again.

#### Subscription

One Permit2 AllowanceTransfer signature authorizes many periods. The seller
drives per-period charges through `POST /subscription/charge`; the buyer does not
sign again for each period.

Pass `--url <resourceUrl>` and `--no-cache` to `subscribe` or `change`. After a
successful seller replay, run `subscription list` to reconcile the confirmed
host-to-`subId` mapping. Then run `access --url <same>` to resolve the `subId`
automatically. The CLI stages an inactive mapping for a new subscription. It
cannot be used by `access` until `subscription list` confirms it as active.

```bash
# Read the commitment already reserved by active subscriptions before confirmation.
onchainos payment app subscription allowance-status \
  --token '<challenge.currency>' --chain-id '<challenge.methodDetails.chainId>' \
  [--from '<0xPayer>']

# subscribe (create) — bind the confirmed cap and defer cache updates until replay succeeds.
onchainos payment app subscription subscribe --challenge '<hdr>' --url '<resourceUrl>' \
  --expected-cap '<confirmed atomic final cap>' --no-cache \
  [--from '<0xPayer>']

# change (up/downgrade) — challenge carries changeFrom.
# Same double-sign as subscribe. --sub-id overrides changeFrom.fromSubId.
# Reconcile the cache with `subscription list` after successful replay.
onchainos payment app subscription change --challenge '<hdr>' --url '<resourceUrl>' \
  --expected-cap '<confirmed atomic final cap>' --no-cache \
  [--sub-id '0x..'] [--from '<0xPayer>']

# access — prove an active subscription to a protected resource.
# Emits APP-Access. --url resolves the cached subId; --sub-id overrides it.
# This is not a 402 flow. Do not replay the subscribe credential.
onchainos payment app subscription access --url '<resourceUrl>' [--sub-id '0x<subId>'] [--from '<0xPayer>'] [--chain xlayer]

# cancel — sign a payer cancelAuthorization. Use --token instead of
# --subscription-contract when the contract must be resolved from allowance-status.
onchainos payment app subscription cancel \
  --sub-id '0x..' --offer-id '<offerId>' \
  --subscription-contract '0x..' --chain-id <n> [--from '0x..']

# cancel a not-yet-effective downgrade. The same --token fallback is supported.
onchainos payment app subscription cancel-pending \
  --sub-id '0x..' --new-sub-id '0x..' --offer-id '<offerId>' \
  --subscription-contract '0x..' --chain-id <n> [--from '0x..']

# After seller replay, resolve with the returned operation_id and Payment-Receipt.
onchainos payment app subscription resolve \
  --operation-id '<op_..>' --receipt '<Payment-Receipt value>'

# Use one of these only when replay has no authoritative receipt.
onchainos payment app subscription resolve --operation-id '<op_..>' --outcome network-error
onchainos payment app subscription resolve --operation-id '<op_..>' --outcome no-receipt
```

`subscribe` and `change` return an `authorization_header` for result handling.
After a successful seller replay, run `subscription list` to reconcile the
confirmed subscription into the cache before using `access --url`. Do not remove
`--no-cache` from the signing commands because the seller has not accepted the
subscription when the CLI produces the header. The returned
`reconcile_for_host` identifies the host that `subscription list` must
reconcile. For a new subscription, that host has an inactive staged mapping.
`cancel` and `cancel-pending` return a signed authorization object
(`cancelAuthorization` or `authorization`) plus an `operation_id`. Hand the
authorization to the seller, which submits
`POST /subscription/cancel[-pending-change]`. Do not replay cancellation objects
as an Authorization header. Keep the `operation_id` for result handling.

After a successful replay, run `subscription list`, then call
`subscription access --url <resourceUrl>` to use the protected service. The
command resolves the confirmed `subId` from the reconciled cache and returns
`APP-Access`. Send that header to the protected
resource. The seller recovers the signer and verifies that the subscription is
active. Replaying the subscribe header or sending a bare GET returns a fresh
`402` and re-enters settlement. Per-period charges are seller-driven, so the
buyer never re-signs to keep access. If the cache is missing, run
`subscription list` to reconcile it or pass `--sub-id` explicitly.

For `cancel-pending`, use the subscription's top-level `subId` with
`pendingChange.newSubId`.

The subscribe command reads the spender, Permit2 contract, and on-chain uint48
nonce from Facilitator allowance status. Do not pass them manually.

Before signing, show the user the final cumulative Permit2 authorization:

```text
final cumulative authorization = reservedAmount + newCommit
```

Show `reservedAmount`, `newCommit`, and the final cumulative authorization in
both readable and atomic units using `_shared/amount-display.md`.

The signed PermitSingle `amount` is cumulative. The challenge's
`request.allowance` covers only this subscription's commitment:
`maxPeriods × amountPerPeriod`. Permit2 stores one allowance per
`(owner, token, spender)`, and all subscriptions share the same subscription
contract. The buyer therefore signs:

```text
amount = reservedAmount + newCommit
newCommit = amountPerPeriod × (maxPeriods − initialChargePeriods) + initialChargeAmount
```

`reservedAmount` is the total commitment of existing active subscriptions from
allowance status. The signed amount can exceed the challenge allowance or wallet
balance because it includes future commitments, not an instant charge. For
example, with N active subscriptions of `0.06`, the signed amount is about
`(N + 1) × 0.06`. The contract still enforces each subscription's
`amountPerPeriod` and `maxPeriods`, so the cumulative allowance is not a blank
cheque. Do not halt or flag this expected behavior as over-authorization. Cancel
unused subscriptions to reduce `reservedAmount`. The buyer also computes
`expiration = reservedExpiration.max(newSubEnd)`, which may differ from the
challenge estimate.

### Handle Result

For subscription `cancel` and `cancel-pending`, handle the seller result in this
order:

1. Hand the signed authorization object to the seller.
2. Read the seller's `Payment-Receipt` after replay.
3. Run `subscription resolve --operation-id ... --receipt ...` with the values
   returned by the signing command and the seller.
4. Treat the operation as committed only when the receipt has `status=success`
   and matches `protocol`, `intent`, `offerId`, and `payer`. Resolve also requires
   a unique transaction `reference`.
5. A successful `cancel` marks the APP cache entry `canceled`. A successful
   `cancel-pending` keeps the current subscription active and records only that
   the pending cancellation operation committed.
6. A `failed`, `pending`, or `timeout` receipt resolves without changing the
   confirmed cache state. Use `--outcome network-error` or `--outcome no-receipt`
   only when no authoritative receipt exists. These outcomes also leave the
   cache unchanged.
7. Repeating resolve for the same `operation_id` returns the saved result without
   applying the state change again.

For `charge`, `upto`, Session operations, `subscribe`, and `change`, send:

```text
Authorization: <authorization_header>
```

Send it to the original URL. The value already includes the `Payment ` prefix.
Do not add another prefix because `Payment Payment ...` is rejected.

Expect `HTTP 200` with `Payment-Receipt`, then decode it:

```bash
onchainos payment decode-receipt --header '<Payment-Receipt value>'
```

Read these fields:

- `status`: `success`, `pending`, `failed`, or `timeout`.
- `transaction`: APP/1 `reference`, which is the on-chain transaction hash.
- `amount`: APP/1 `settledAmount`.
- `offerId`: the business key.

After a successful subscription `subscribe` or `change`, run
`onchainos payment app subscription list` to reconcile the local cache. Do not
use the staged subscription for access before the seller confirms it. The CLI
keeps it inactive until the authoritative list reports it as active.

For non-Session operations, `timeout` is not a failure because settlement is
asynchronous. Do not re-sign. Query the offer status later. For Session topup
and close, follow the `session resolve` rules above. If replay returns a fresh
HTTP 402 because the challenge is stale, fetch the new challenge and restart
the flow.

## Security

### Confirmation

Ask for exactly one confirmation before every fund-affecting APP signing or
broadcast operation:

1. Keep and decode the challenge without calling an APP signing command.
2. Show one confirmation card with the common and intent-specific terms below.
3. Stop and wait for `yes` or `no`.
4. After `yes`, run `onchainos wallet status`.
5. Run the matching APP command. Do not ask a second confirmation.

Show these common fields: operation, chain, token, recipient, challenge expiry,
and who pays gas. Add:

| Intent or operation | Fields to confirm |
| --- | --- |
| `charge` | Exact amount; selected signature type; Permit2 spender when selected |
| `upto` | Maximum authorization amount; selected signature type; Permit2 spender and Facilitator when selected |
| Session `open` | Fixed deposit; escrow contract; meter unit and unit price; authorized signer when used |
| Session `voucher` | `channelId`; new absolute `cumulativeAmount` |
| Session `topup` | `channelId`; `additionalDeposit`; escrow |
| Session `close` | `channelId`; final `cumulativeAmount`; escrow |
| Subscription create or change | Amount per period; period mode or length; maximum periods; initial charge; maximum new commitment; existing `reservedAmount`; final cumulative Permit2 authorization; plan; Facilitator; terms deadline; `changeFrom` when present |
| Subscription cancel or cancel-pending | Affected `subId` values; subscription contract |

Subscription `access` and `list` are read-only service or status operations. They
do not require payment confirmation.

### Risk and Stop Conditions

- Do not run wallet status or any signing or broadcast command before the user
  confirms. Stop if the user declines.
- Stop if the wallet session is unavailable. APP has no local-key fallback.
- Stop when the challenge is expired. Fetch a fresh challenge before signing.
- For Session mid-lifecycle operations, stop if the saved open challenge is
  missing. Never substitute a fresh `offerId` or fabricate `opaque`.
- For Session Voucher and Close, stop when the persisted state is missing or
  inconsistent. Enforce the cumulative bounds before signing.
- For subscription, use allowance status as the authority for nonce, reserved
  amount, spender, and expiration.

## Troubleshooting

Read seller errors using `../SKILL.md` → "Reading seller errors". Use its priority
order and display `❌ Seller rejected: <reason> (code <code>, HTTP <status>)`.
APP/1 errors such as `unsupported_protocol`, `signature_invalid`, `expired`,
`nonce_used`, `duplicate_offer`, and `insufficient_balance` use the same format.

| Symptom | Cause | Fix |
| --- | --- | --- |
| Challenge rejected: protocol not `app/1` | An APP command received an MPP challenge | Use `payment charge` or `payment session` for MPP, which has no `protocol=app/1` |
| `--type hash` rejected: requires feePayer=false | Hash mode needs a no-fee-payer challenge | Use `authorization` or `permit2`, or obtain a `feePayer=false` challenge |
| `upto` + `--tx-hash` rejected | `upto` has no hash mode | Use `authorization` or `permit2` |
| Challenge expired | `expires` is in the past | Re-send the original request to fetch a fresh 402 |
| `chain not found` | Unsupported `chainId` | Run `onchainos wallet chains` |

## CLI Reference

Use `onchainos payment app <charge | upto | session <sub> | subscription <sub>>`
to sign an APP/1 credential.

| Command | Key parameters | Emits |
| --- | --- | --- |
| `app charge` | `--challenge` (required), `--type`, `--tx-hash`, `--from` | `authorization_header` |
| `app upto` | `--challenge` (required), `--type`, `--from` | `authorization_header` |
| `app session open` | `--challenge` (required), `--tx-hash` + `--salt`, `--authorized-signer`, `--from` | `authorization_header` + `channel_id` |
| `app session voucher` | `--challenge`, `--channel-id`, `--cumulative-amount`, `--escrow`, `--chain-id` (required) | `authorization_header` |
| `app session topup` | `--challenge`, `--channel-id`, `--additional-deposit`, `--escrow`, `--chain-id` (required), `--currency`, `--tx-hash` | `authorization_header` + `operation_id` |
| `app session close` | `--challenge`, `--channel-id`, `--cumulative-amount`, `--escrow`, `--chain-id` (required) | `authorization_header` + `operation_id` |
| `app session resolve` | `--operation-id` and either `--receipt` or `--outcome` | Resolution result; commits only a `success` receipt |
| `app subscription subscribe` | `--challenge`, `--expected-cap` (required), `--url`, `--no-cache`, `--from` | `authorization_header`; reconcile cache after successful replay |
| `app subscription change` | `--challenge`, `--expected-cap` (required), `--url`, `--sub-id`, `--no-cache`, `--from` | `authorization_header`; reconcile cache after successful replay |
| `app subscription access` | `--url` or `--sub-id`, `--from`, `--chain` | `APP-Access`; resolves `subId` from cache |
| `app subscription cancel` | `--sub-id`, `--offer-id`, `--chain-id` (required); `--subscription-contract` or `--token` | `cancelAuthorization` + `operation_id` |
| `app subscription cancel-pending` | `--sub-id`, `--new-sub-id`, `--offer-id`, `--chain-id` (required); `--subscription-contract` or `--token` | `authorization` + `operation_id` |
| `app subscription resolve` | `--operation-id` and either `--receipt` or `--outcome` | Resolution result; commits only a matching `success` receipt |
| `app subscription list` | `--from`, `--limit`, `--offset` | `subscriptions[]` |
| `app subscription allowance-status` | `--token`, `--chain-id` (required), `--from` | Authoritative read-only allowance and contract state |

Use `--from` and `--base-url` only on commands that show those options in help.
`app session resolve` and `app subscription resolve` update local state and
accept neither option.
