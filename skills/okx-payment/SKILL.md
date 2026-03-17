---
name: okx-payment
description: "This skill should be used when the user encounters an HTTP 402 Payment Required response, wants to pay for a payment-gated API or resource, or mentions 'x402', 'pay for access', '402 payment', 'payment-gated URL', or 'sign x402 payment'. Requires an active login session (JWT). Signs an EIP-3009 authorization via TEE and returns the payment proof (signature + authorization) that the caller can attach as a payment header to access the resource. Do NOT use for swap or token transfers — use okx-dex-swap instead. Do NOT use for general programming questions."
license: Apache-2.0
metadata:
  author: okx
  version: "1.0.2"
  homepage: "https://web3.okx.com"
---

# OKX Wallet x402 CLI

Sign an [x402](https://x402.org) payment authorization via TEE and return the payment proof for accessing payment-gated resources.

## Pre-flight Checks

Every time before running any `onchainos` command, always follow these steps in order. Do not echo routine command output to the user; only provide a brief status update when installing, updating, or handling a failure.

1. **Confirm installed**: Run `which onchainos`. If not found, install it:
   ```bash
   curl -sSL https://raw.githubusercontent.com/okx/onchainos-skills/main/install.sh | sh
   ```
   If the install script fails, ask the user to install manually following the instructions at: https://github.com/okx/onchainos-skills

2. If any `onchainos` command fails with an unexpected error during this
   session, try reinstalling before giving up:
   ```bash
   curl -sSL https://raw.githubusercontent.com/okx/onchainos-skills/main/install.sh | sh
   ```

3. **Check login status**: Run `onchainos wallet status`. If the user is not logged in, run `onchainos wallet login` to complete login before proceeding.

## Skill Routing

- For querying authenticated wallet balance → use `okx-wallet-balance`
- For querying public wallet balance → use `okx-wallet-portfolio`
- For token swaps / trades → use `okx-dex-swap`
- For transaction broadcasting → use `okx-onchain-gateway`

## Background: x402 Protocol

x402 is an HTTP payment protocol. When a server returns `HTTP 402 Payment Required`, it includes a base64-encoded JSON payload describing what payment is required. The full flow is:

1. Send request → receive `HTTP 402` with base64-encoded payment payload
2. Decode the payload, extract payment parameters from `accepts[0]`
3. Sign via TEE → `onchainos payment x402-pay` → obtain `{ signature, authorization }`
4. Assemble payment header and replay the original request

This skill owns **steps 2–4** end to end.

## Quickstart

```bash
# Sign an x402 payment for a Base USDC-gated resource
onchainos payment x402-pay \
  --network eip155:8453 \
  --amount 1000000 \
  --pay-to 0xRecipientAddress \
  --asset 0x833589fcd6edb6e08f4c7c32d4f71b54bda02913 \
  --max-timeout-seconds 300
```

## Command Index

| # | Command | Description |
|---|---|---|
| 1 | `onchainos payment x402-pay` | Sign an x402 payment and return the payment proof |

## Operation Flow

### Step 1: Send the Original Request

Make the HTTP request the user asked for. If the response status is **not 402**, return the result directly — no payment needed.

### Step 2: Decode the 402 Payload

If the response is `HTTP 402`, the body is a base64-encoded JSON string. Decode it:

```
rawBody  = response.body          // base64 string
decoded  = JSON.parse(atob(rawBody))
option   = decoded.accepts[0]
```

Extract these fields from `option`:

| x402 field | CLI param | Notes |
|---|---|---|
| `option.network` | `--network` | CAIP-2 format, e.g. `eip155:8453` |
| `option.amount` or `option.maxAmountRequired` | `--amount` | prefer `amount`; fall back to `maxAmountRequired` |
| `option.payTo` | `--pay-to` | |
| `option.asset` | `--asset` | token contract address |
| `option.maxTimeoutSeconds` | `--max-timeout-seconds` | optional, default 300 |

### Step 3: Sign

Run `onchainos payment x402-pay` with the extracted parameters. Returns `{ signature, authorization }`.

### Step 4: Assemble Header and Replay

**Determine header name** from `decoded.x402Version`:
- `x402Version >= 2` → `PAYMENT-SIGNATURE`
- `x402Version < 2` (or absent) → `X-PAYMENT`

**Build header value**:
```
paymentPayload = { ...decoded, payload: { signature, authorization } }
headerValue    = btoa(JSON.stringify(paymentPayload))
```

**Replay** the original request with the header attached:
```
GET/POST <original-url>
<header-name>: <headerValue>
```

Return the final response body to the user.

### Step 5: Suggest Next Steps

After a successful payment and response, suggest:

| Just completed | Suggest |
|---|---|
| Successful replay | 1. Check balance impact → `okx-wallet-balance` 2. Make another request to the same resource |
| 402 on replay (expired) | Retry from Step 3 with a fresh signature |

Present conversationally, e.g.: "Done! The resource returned the following result. Would you like to check your updated balance?" — never expose skill names or internal field names to the user.

## CLI Command Reference

### 1. onchainos payment x402-pay

Sign an x402 payment and return the EIP-3009 payment proof.

```bash
onchainos payment x402-pay \
  --network <network> \
  --amount <amount> \
  --pay-to <address> \
  --asset <address> \
  [--from <address>] \
  [--max-timeout-seconds <seconds>]
```

| Param | Required | Default | Description |
|---|---|---|---|
| `--network` | Yes | - | CAIP-2 network identifier (e.g., `eip155:8453` for Base, `eip155:1` for Ethereum) |
| `--amount` | Yes | - | Payment amount in minimal units (e.g., `1000000` = 1 USDC with 6 decimals) |
| `--pay-to` | Yes | - | Recipient address (from x402 `payTo` field) |
| `--asset` | Yes | - | Token contract address (from x402 `asset` field) |
| `--from` | No | selected account | Payer address; if omitted, uses the currently selected account |
| `--max-timeout-seconds` | No | `300` | Authorization validity window in seconds |

**Return fields**:

| Field | Type | Description |
|---|---|---|
| `signature` | String | TEE-generated EIP-3009 signature (hex) |
| `authorization` | Object | EIP-3009 `transferWithAuthorization` message including `domainHash` |
| `authorization.chainIndex` | String | OKX chain index |
| `authorization.from` | String | Payer wallet address |
| `authorization.to` | String | Recipient address |
| `authorization.value` | String | Payment amount in minimal units |
| `authorization.validAfter` | String | Authorization valid-after timestamp (always `"0"`) |
| `authorization.validBefore` | String | Authorization valid-before timestamp (Unix seconds) |
| `authorization.nonce` | String | Random nonce (hex, 32 bytes) |
| `authorization.verifyingContract` | String | Token contract address |
| `authorization.domainHash` | String | EIP-712 domain separator hash |
| `authorization.sessionCert` | String | Session certificate (base64) |
| `authorization.sessionSignature` | String | Ed25519 session signature (base64) |

## Input / Output Examples

**User says:** "Fetch https://api.example.com/data — it requires x402 payment"

**Step 1** — original request returns 402:
```
HTTP 402
Body: "eyJ4NDAyVmVyc2lvbiI6MiwiYWNjZXB0cyI6W3s..."  ← base64
```

Decoded payload:
```json
{
  "x402Version": 2,
  "accepts": [{
    "network": "eip155:8453",
    "amount": "1000000",
    "payTo": "0xAbC...",
    "asset": "0x833589fcd6edb6e08f4c7c32d4f71b54bda02913",
    "maxTimeoutSeconds": 300
  }]
}
```

**Step 2–3** — sign:
```bash
onchainos payment x402-pay \
  --network eip155:8453 \
  --amount 1000000 \
  --pay-to 0xAbC... \
  --asset 0x833589fcd6edb6e08f4c7c32d4f71b54bda02913 \
  --max-timeout-seconds 300
# → { "signature": "0x...", "authorization": { ... } }
```

**Step 4** — assemble header and replay:
```
paymentPayload = { ...decoded, payload: { signature, authorization } }
headerValue    = btoa(JSON.stringify(paymentPayload))

GET https://api.example.com/data
PAYMENT-SIGNATURE: <headerValue>

→ HTTP 200  { "result": "..." }
```

## Edge Cases

- **Not logged in**: Run `onchainos wallet login`, then retry
- **Unsupported network**: Only EVM chains with CAIP-2 `eip155:<chainId>` format are supported
- **No wallet for chain**: The logged-in account must have an address on the requested chain; if not, inform the user
- **Amount in wrong units**: `--amount` must be in minimal units — remind user to convert (e.g., 1 USDC = `1000000` for 6 decimals)
- **Expired authorization**: If the server rejects the payment as expired, retry with a fresh signature
- **Network error**: Retry once, then prompt user to try again later

## Amount Display Rules

- `--amount` is always in minimal units (e.g., `1000000` for 1 USDC)
- When displaying to the user, convert to UI units: divide by `10^decimal`
- Show token symbol alongside (e.g., `1.00 USDC`)

## Global Notes

- This skill requires an **authenticated JWT session** — no OKX API key needed
- Signing is performed inside a TEE; the private key never leaves the secure enclave
- This skill only signs — it does **not** broadcast or deduct balance directly; payment settles when the recipient redeems the authorization on-chain
- `--network` must be CAIP-2 format: `eip155:<chainId>` (e.g., `eip155:1`, `eip155:8453`, `eip155:196`)
- The returned `authorization` object must be included alongside `signature` when building the payment header
