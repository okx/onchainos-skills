# Service CLI Reference

## Commands

### `onchainos service login`

#### Syntax

```bash
onchainos service login --service newsliquid
```

#### Parameters

| Flag | Meaning |
| --- | --- |
| `--service` | Required: newsliquid, News Liquid, 6551 or 6551Team |
| `--refresh` | Obtain a fresh login instead of reusing an unexpired credential |
| `--wallet` | Expected current wallet; required with the warning flag |
| `--confirm-wallet-warning` | Accept only the wallet warning already shown to the user |

#### Return Fields

The wrapper is `{ok:true,data:{service,result}}`. Login result contains `wallet`, `expiresAt` and `reused`, never the Token.

#### Examples

```bash
onchainos service login --service newsliquid --refresh
onchainos service login --service newsliquid --wallet "$NEWSLIQUID_WALLET" --confirm-wallet-warning
```

#### Error Handling

A wallet warning returns Confirming and its continuation. A refresh continuation retains `--refresh`. Other signing errors do not authorize overriding a warning.

### `onchainos service token`

#### Syntax

```bash
onchainos service token --service newsliquid --wallet "$NEWSLIQUID_WALLET"
```

#### Parameters

| Flag | Meaning |
| --- | --- |
| `--service` | Required service name or supported alias |
| `--wallet` | Optional expected current wallet; use to bind a script to its intended wallet |

#### Return Fields

`data.result` contains `wallet`, `token` and `expiresAt`. This command reads the current wallet's valid credential. It does not log in, sign, refresh or check plan rights.

#### Examples

Use the root Skill's Token variable for HTTP requests. Custom clients read `data.result.token` inside their own process.

#### Error Handling

| Error | Meaning |
| --- | --- |
| wallet_login_required | No selected OnchainOS wallet |
| newsliquid_login_required | No valid service credential; complete service login |
| newsliquid_wallet_changed | Current or stored wallet differs from the expected wallet; stop |
| newsliquid_credentials_corrupted | Stored credential cannot be read; use explicit login refresh to replace it |

### `onchainos payment validate`

#### Syntax

```bash
printf '%s' "$NEWSLIQUID_CHECK_INPUT" |
  onchainos payment validate --service newsliquid --stage prepare --from "$NEWSLIQUID_WALLET"
```

#### Parameters

| Input | Meaning |
| --- | --- |
| `--service` | `newsliquid` |
| `--stage` | `prepare`, `before-sign` or `after-sign` |
| `--from` | Required intended payment wallet |
| stdin `order` | Native response data from the original order read |
| stdin `kind` | Original `one_time`, `subscription` or `auto_renew` selection |
| stdin `challenge` | Complete decoded PAYMENT-REQUIRED JSON; preserve every field |
| stdin `allowance` | Native allowance-status data for period payments |
| stdin `planServiceEnd` | Current quota subscription.serviceEnd for automatic renewal |
| stdin `approved` | Complete unsigned prepare result or its data; required before/after signing |
| stdin `signed` | Native signer data; required after signing, supplied only inside the execution process |

#### Return Fields

- Prepare returns an unsigned approval snapshot: wallet, orderId, kind, quote, accepted, resource, expiresAt, autoRenewTerms and allowance.
- Before-sign compares fresh responses with that snapshot. It returns valid, orderId and the exact checked URL and unsigned payload/accepted entry/cap for the signer.
- After-sign returns only valid and orderId. It never returns the supplied proof or signature.
- All stages are stateless and perform no network, signing or cache writes.

#### Examples

Keep the prepare snapshot as `approved`. After consent, validate fresh responses with `before-sign`. Use its `payload` for `exact`, or `accepted`, `url` and `cap` for `period`.

For `exact`:

```bash
SIGNED=$(onchainos payment pay --payload "$PAYLOAD" --selected-index 0 --from "$NEWSLIQUID_WALLET")
```

For `period`:

```bash
SIGNED=$(onchainos payment subscription subscribe --accepts "$ACCEPTED" --from "$NEWSLIQUID_WALLET" --url "$NEWSLIQUID_PAY_URL" --no-cache --expected-cap "$CAP")
```

Never echo `$SIGNED`. Pass its data to `after-sign` validation, then submit the signer's payment header once to the checked URL in the same tool invocation. Stop on signing or validation failure. Read the original order and quota to verify fulfillment.

#### Error Handling

- Reject multiple candidates, wrong wallets/resources, missing fields, invalid integer ranges and expired/non-payable orders before signing.
- On payment_terms_changed, prepare and confirm the changed terms again.
- A before_signing allowance error includes the actual asset, required/available amounts and authoritative Permit2 contract. Use bounded approval consent, then prepare again.
- After any signature, validation failure, transport failure or unconfirmed submission permits original-order queries only.
- Keep actual wallet warnings; do not execute their raw signer continuation outside the captured handoff.

## Command Constraints

- Only login and Token retrieval are exposed. Business HTTP requests and WSS connections run outside this CLI.
- Login uses the fixed service host and the existing wallet signer and credential store. It creates no provider order, challenge or cancellation state file.
- Token output contains a credential. Capture it inside the execution process; do not return it as a standalone AI tool result or write it to logs or source code.
- Token retrieval never buys a plan or grants payment authority.
