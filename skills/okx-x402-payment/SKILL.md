---
name: okx-x402-payment
description: "This skill should be used when the user encounters an HTTP 402 Payment Required response, wants to pay for a payment-gated API or resource, or mentions 'x402', 'pay for access', '402 payment', 'payment-gated URL', or 'sign x402 payment'. Primary path signs via TEE with a wallet session (JWT) — recommended. Fallback path allows local EIP-3009 signing with the user's own private key only when the user explicitly opts in (key stays local but is not TEE-protected). Returns the payment proof (signature + authorization) that the caller can attach as a payment header to access the resource. Do NOT use for swap or token transfers — use okx-dex-swap instead. Do NOT use for wallet balance or portfolio queries — use okx-agentic-wallet or okx-wallet-portfolio. Do NOT use for security scanning — use okx-security. Do NOT use for transaction broadcasting — use okx-onchain-gateway. Do NOT use for general programming questions."
license: MIT
metadata:
  author: okx
  version: "2.2.8"
  homepage: "https://web3.okx.com"
---

# Onchain OS x402 Payment

Sign an [x402](https://x402.org) payment authorization and return the payment proof for accessing payment-gated resources. Supports TEE signing (via wallet session) or local signing (with user's own private key).

## Pre-flight Checks

> Read `../okx-agentic-wallet/_shared/preflight.md`. If that file does not exist, read `_shared/preflight.md` instead.

## Skill Routing

- For querying authenticated wallet balance / send tokens / tx history → use `okx-agentic-wallet`
- For querying public wallet balance (by address) → use `okx-wallet-portfolio`
- For token swaps / trades / buy / sell → use `okx-dex-swap`
- For token search / metadata / rankings / holder info / cluster analysis → use `okx-dex-token`
- For token prices / K-line charts / wallet PnL / address tracker activities → use `okx-dex-market`
- For smart money / whale / KOL signals / leaderboard → use `okx-dex-signal`
- For meme / pump.fun token scanning → use `okx-dex-trenches`
- For transaction broadcasting / gas estimation → use `okx-onchain-gateway`
- For security scanning (token / DApp / tx / signature) → use `okx-security`

## Chain Name Support

`--network` uses CAIP-2 format: `eip155:<realChainIndex>`. All EVM chains returned by `onchainos wallet chains` are supported. The `realChainIndex` field in the chain list corresponds to the `<chainId>` portion of the CAIP-2 identifier.

Common examples:

| Chain        | Network Identifier |
|--------------|--------------------|
| Ethereum     | `eip155:1`         |
| X Layer      | `eip155:196`       |
| Base         | `eip155:8453`      |
| Arbitrum One | `eip155:42161`     |
| Linea        | `eip155:59144`     |

For the full list of supported EVM chains and their `realChainIndex`, run:
```bash
onchainos wallet chains
```

> Non-EVM chains (e.g., Solana, Tron, Ton, Sui) are **not** supported by x402 payment — only `eip155:*` identifiers are accepted.

## Background: x402 Protocol

x402 is an HTTP payment protocol with two versions:

|                                | v2                                                                | v1                                          |
|--------------------------------|-------------------------------------------------------------------|---------------------------------------------|
| **402 payload location**       | `PAYMENT-REQUIRED` **header** (base64-encoded JSON); body is `{}` | Response **body** (direct JSON, not base64) |
| **Client header name**         | `PAYMENT-SIGNATURE`                                               | `X-PAYMENT`                                 |
| **Client payload structure**   | `{ x402Version, resource, accepted, payload }`                    | `{ x402Version, scheme, network, payload }` |
| **Amount field in accepts**    | `amount`                                                          | `maxAmountRequired`                         |
| **Settlement response header** | `PAYMENT-RESPONSE`                                                | `X-PAYMENT-RESPONSE`                        |

The full flow is:

1. Send request → receive `HTTP 402`
2. Decode the payload (from `PAYMENT-REQUIRED` header for v2; from response body for v1), pass the `accepts` array to the CLI
3. Sign via TEE → `onchainos payment x402-pay --accepts '<accepts array>'` → CLI selects best scheme, returns payment proof
4. Assemble payment header and replay the original request

This skill owns **steps 2–4** end to end.

## Quickstart

```bash
onchainos payment x402-pay \
  --accepts '[{"scheme":"aggr_deferred","network":"eip155:196","amount":"1000000","payTo":"0xRecipientAddress","asset":"0x4ae46a509f6b1d9056937ba4500cb143933d2dc8","maxTimeoutSeconds":300}]'
```

## Command Index

| # | Command                           | Description                                                          |
|---|-----------------------------------|----------------------------------------------------------------------|
| 1 | `onchainos payment x402-pay`      | Sign an x402 payment via TEE (wallet session) and return the proof   |
| 2 | `onchainos payment eip3009-sign`  | Sign an EIP-3009 authorization locally with a hex private key        |

## Operation Flow

### Step 1: Send the Original Request

Make the HTTP request the user asked for. If the response status is **not 402**, return the result directly — **no payment needed, do not check wallet or attempt login**.

> **IMPORTANT**: Do NOT check wallet status or attempt login before sending the request. Only proceed to payment steps if the response is HTTP 402.

### Step 2: Decode the 402 Payload

If the response is `HTTP 402`, extract the payment requirements. The location differs by version:

**v2** — payload is in the `PAYMENT-REQUIRED` response **header** (base64-encoded JSON):

```
headerValue = response.headers['PAYMENT-REQUIRED']
decoded     = JSON.parse(atob(headerValue))
```

**v1** — payload is in the response **body** (direct JSON, not base64):

```
decoded = JSON.parse(response.body)
```

In both cases, extract the `accepts` array:

```
accepts = decoded.accepts        // keep the full array for the CLI
option  = decoded.accepts[0]     // for display purposes
```

Save `decoded.accepts` as a JSON string — it will be passed directly to the CLI via `--accepts`. The CLI handles scheme selection internally (prefers `"exact"` > `"aggr_deferred"` > first entry).

Save `decoded` for later — you will need `decoded.x402Version` and `decoded.resource` (v2) when assembling the payment header in Step 5.

**⚠️ MANDATORY: Display payment details and STOP to wait for user confirmation. Do NOT check wallet status, run `onchainos wallet status`, attempt login, or call any other tool until the user explicitly confirms.**

Present the following information to the user:

> This resource requires x402 payment:
> - **Network**: `<chain name>` (`<option.network>`)
> - **Token**: `<token symbol>` (`<option.asset>`)
> - **Amount**: `<human-readable amount>` (from `option.amount` for v2, or `option.maxAmountRequired` for v1; convert from minimal units using token decimals)
> - **Pay to**: `<option.payTo>`
>
> Proceed with payment? (yes / no)

Then STOP and wait for the user's response. Do not proceed in the same turn.

- **User confirms** → proceed to Step 3.
- **User declines** → stop immediately, no payment is made, no wallet check.

### Step 3: Check Wallet Status (only after user explicitly confirms payment)

Now that payment is required, check if the user has a wallet session:

```bash
onchainos wallet status
```

- **Logged in** → proceed to Step 4 (Sign via TEE).
- **Not logged in** → **STOP and ask the user** which signing method to use. Do NOT check for private keys, read files, or call any other tool until the user responds:

> "You are not logged in. How would you like to sign the payment?"
> 1. **Wallet login** — log in to the wallet, then sign via TEE (recommended)
> 2. **Local private key** — sign locally with your own private key (no login needed)

Then STOP and wait for the user's response. Do not proceed in the same turn.

#### Option 1: Wallet Login

Run `onchainos wallet login` (AK login, no email) or `onchainos wallet login <email>` (OTP login), then proceed to Step 4.

#### Option 2: Local Private Key

Only after the user chooses this option, read `~/.onchainos/.env` to check if `EVM_PRIVATE_KEY` is already configured:

- **Key found** → inform the user and proceed to the **Local Signing Fallback** below. The payer address will be derived from the private key automatically by the CLI.

- **Key not found** → inform the user:

  > "No private key configured. Please save it to `~/.onchainos/.env`: add a line `EVM_PRIVATE_KEY=0x<your_hex_key>`, then let me know."

  Wait for user action before proceeding.

### Step 4: Sign

Pass the raw `accepts` array to the CLI. The CLI automatically selects the best scheme (prefers `"exact"` over `"aggr_deferred"`):

```bash
onchainos payment x402-pay \
  --accepts '<JSON.stringify(decoded.accepts)>'
```

The CLI returns different fields depending on the scheme it selected:

- **`aggr_deferred` scheme** (preferred): Only Session Key (Ed25519) signing, skips EOA signing. Returns `{ signature, authorization, sessionCert }`. The `signature` is the base64-encoded session-key signature.
- **`exact` scheme**: Full EIP-3009 signing via TEE. Returns `{ signature, authorization }`. The `signature` is the secp256k1 EIP-3009 signature.

Check the response: if `sessionCert` is present, the CLI selected aggr_deferred scheme.

**If signing fails** (e.g., session expired, not logged in, AK re-login failed):

- Do NOT simply cancel or give up.
- Ask the user: "Signing failed because there is no active wallet session. Would you like to log in now, or sign locally with your own private key?"
  - **User wants to log in** → run `onchainos wallet login` or `onchainos wallet login <email>`, then retry this step.
    - **User wants local signing** → switch to the **Local Signing Fallback** (see below).
    - **User wants to cancel** → only then cancel the request.

### Step 5: Assemble Header and Replay

The PaymentPayload structure and header name differ between v1 and v2.

#### v2 (`x402Version >= 2`)

Header name: `PAYMENT-SIGNATURE`

The `accepted` field is a **single object** (the entry the CLI selected from `accepts`), not the whole array. If the CLI returned `sessionCert` (aggr_deferred scheme), **merge** it into the existing `accepted.extra` (preserve original fields like `name`, `version`):

```
// Pick the accepted entry (CLI selected aggr_deferred if available)
accepted = decoded.accepts.find(a => a.scheme === selectedScheme)

// aggr_deferred: merge sessionCert into existing extra
if (sessionCert) {
  accepted.extra = { ...accepted.extra, sessionCert }
}

paymentPayload = {
  x402Version: decoded.x402Version,
  resource:    decoded.resource,          // from the 402 payload
  accepted:    accepted,                  // single object, NOT the array
  payload:     { signature, authorization }
}
headerValue = btoa(JSON.stringify(paymentPayload))
```

#### v1 (`x402Version < 2` or absent)

Header name: `X-PAYMENT`

v1 has no `accepted` or `resource` — instead, `scheme` and `network` are top-level:

```
paymentPayload = {
  x402Version: 1,
  scheme:      selectedScheme,            // "exact" or "aggr_deferred"
  network:     option.network,
  payload:     { signature, authorization }
}
headerValue = btoa(JSON.stringify(paymentPayload))
```

#### Replay

Attach the header and replay the original request:

```
GET/POST <original-url>
<header-name>: <headerValue>
```

Return the final response body to the user.

### Step 6: Suggest Next Steps

After a successful payment and response, suggest:

| Just completed          | Suggest                                                                                     |
|-------------------------|---------------------------------------------------------------------------------------------|
| Successful replay       | 1. Check balance impact → `okx-agentic-wallet` 2. Make another request to the same resource |
| 402 on replay (expired) | Retry from Step 4 with a fresh signature                                                    |

Present conversationally, e.g.: "Done! The resource returned the following result. Would you like to check your updated balance?" — never expose skill names or internal field names to the user.

## Cross-Skill Workflows

### Workflow A: Pay for a 402-Gated API Resource (most common)

> User: "Fetch https://api.example.com/data — it requires x402 payment"

```
1. Send GET https://api.example.com/data                              → HTTP 402
       ↓ decode (v2: PAYMENT-REQUIRED header; v1: response body)
2. okx-x402-payment   onchainos payment x402-pay \
                        --accepts '<accepts array JSON>'         → { signature, authorization[, sessionCert] }
       ↓ CLI selects best scheme; assemble payment header per version
3. Replay with payment header (v2: PAYMENT-SIGNATURE, v1: X-PAYMENT)  → HTTP 200
```

**Data handoff**:

- `decoded.accepts` → `--accepts`
- CLI output `sessionCert` → `accepted.extra.sessionCert` (present only when aggr_deferred scheme was selected)

### Workflow B: Pay then Check Balance

> User: "Access this paid API, then show me how much I spent"

```
1. okx-x402-payment   (Workflow A above)                              → payment proof + successful response
2. okx-agentic-wallet  onchainos wallet balance --chain 196            → current balance after payment
```

### Workflow C: Security Check before Payment

> User: "Is this x402 payment safe? The asset is 0x4ae46a..."

```
1. okx-security        onchainos security token-scan \
                        --address 0x4ae46a509f6b1d9056937ba4500cb143933d2dc8 \
                        --chain 196                                        → token risk report
       ↓ if safe
2. okx-x402-payment   (Workflow A above)                              → sign and pay
```

## CLI Command Reference

### 1. onchainos payment x402-pay

Sign an x402 payment and return the payment proof. Pass the raw `accepts` array from the 402 payload — the CLI selects the best scheme automatically (prefers `"exact"` > `"aggr_deferred"` > first entry).

```bash
onchainos payment x402-pay \
  --accepts '<accepts array JSON>' \
  [--from <address>]
```

| Param       | Required | Default          | Description                                                        |
|-------------|----------|------------------|--------------------------------------------------------------------|
| `--accepts` | Yes      | -                | JSON `accepts` array from the 402 payload; CLI selects best scheme |
| `--from`    | No       | selected account | Payer address; if omitted, uses the currently selected account     |

**Return fields (exact scheme)**:

| Field                       | Type   | Description                                                                              |
|-----------------------------|--------|------------------------------------------------------------------------------------------|
| `signature`                 | String | EIP-3009 secp256k1 signature (65 bytes, r+s+v, hex) returned by TEE backend              |
| `authorization`             | Object | Standard x402 EIP-3009 `transferWithAuthorization` parameters                            |
| `authorization.from`        | String | Payer wallet address                                                                     |
| `authorization.to`          | String | Recipient address (= `payTo`)                                                            |
| `authorization.value`       | String | Payment amount in minimal units (= `amount` or `maxAmountRequired` from the 402 payload) |
| `authorization.validAfter`  | String | Authorization valid-after timestamp (Unix seconds)                                       |
| `authorization.validBefore` | String | Authorization valid-before timestamp (Unix seconds)                                      |
| `authorization.nonce`       | String | Random nonce (hex, 32 bytes), prevents replay attacks                                    |

**Return fields (aggr_deferred scheme)**:

| Field           | Type   | Description                                                                          |
|-----------------|--------|--------------------------------------------------------------------------------------|
| `signature`     | String | Base64-encoded Ed25519 session-key signature (no EOA signing)                        |
| `authorization` | Object | Standard x402 EIP-3009 `transferWithAuthorization` parameters (same fields as exact) |
| `sessionCert`   | String | Session certificate proving the session key's authority over the wallet              |

### 2. onchainos payment eip3009-sign

Sign an EIP-3009 `TransferWithAuthorization` locally using a hex private key (from `EVM_PRIVATE_KEY` env var or `~/.onchainos/.env`). No wallet session or TEE required. The payer address (`from`) is derived automatically from the private key. Uses the same `--accepts` interface as `x402-pay` — EIP-712 domain `name`/`version` are extracted from `accepts[].extra.name` / `extra.version`.

```bash
onchainos payment eip3009-sign \
  --accepts '<accepts array JSON>'
```

| Param             | Required | Default | Description                                                                                                             |
|-------------------|----------|---------|-------------------------------------------------------------------------------------------------------------------------|
| `EVM_PRIVATE_KEY` | Yes      | -       | Hex-encoded secp256k1 private key; read from env var, falls back to `~/.onchainos/.env`                                 |
| `--accepts`       | Yes      | -       | JSON `accepts` array from the 402 payload (same as `x402-pay`); `extra.name`/`extra.version` provide the EIP-712 domain |

The CLI derives the payer address from the private key, and extracts `network`, `amount`, `payTo`, `asset`, `maxTimeoutSeconds` from the accepts array (same logic as `x402-pay`), plus `extra.name` → EIP-712 domain name, `extra.version` → EIP-712 domain version (defaults to `"2"` if absent).

**Return fields**:

| Field                       | Type   | Description                                                     |
|-----------------------------|--------|-----------------------------------------------------------------|
| `signature`                 | String | EIP-3009 secp256k1 signature (hex, 0x-prefixed, 65 bytes r+s+v) |
| `authorization`             | Object | Standard x402 EIP-3009 `transferWithAuthorization` fields       |
| `authorization.from`        | String | Payer address                                                   |
| `authorization.to`          | String | Recipient address                                               |
| `authorization.value`       | String | Payment amount in minimal units                                 |
| `authorization.validAfter`  | String | `"0"`                                                           |
| `authorization.validBefore` | String | Computed expiry timestamp (Unix seconds)                        |
| `authorization.nonce`       | String | Random nonce (hex, 0x-prefixed, 32 bytes)                       |

## Input / Output Examples

**User says:** "Fetch https://api.example.com/data — it requires x402 payment"

**Step 1** — original request returns 402 (v2 example):

```
HTTP/1.1 402 Payment Required
PAYMENT-REQUIRED: eyJ4NDAyVmVyc2lvbiI6Miwi...   ← base64 in header
Content-Type: application/json

{}
```

Decoded `PAYMENT-REQUIRED` header:

```json
{
  "x402Version": 2,
  "error": "PAYMENT-SIGNATURE header is required",
  "resource": {
    "url": "https://api.example.com/data",
    "description": "Premium data",
    "mimeType": "application/json"
  },
  "accepts": [
    {
      "scheme": "aggr_deferred",
      "network": "eip155:196",
      "amount": "1000000",
      "payTo": "0xAbC...",
      "asset": "0x4ae46a509f6b1d9056937ba4500cb143933d2dc8",
      "maxTimeoutSeconds": 300,
      "extra": {
        "name": "USDG",
        "version": "1"
      }
    },
    {
      "scheme": "exact",
      "network": "eip155:196",
      "amount": "1000000",
      "payTo": "0xAbC...",
      "asset": "0x4ae46a509f6b1d9056937ba4500cb143933d2dc8",
      "maxTimeoutSeconds": 300,
      "extra": {
        "name": "USDG",
        "version": "1"
      }
    }
  ]
}
```

**Step 3–4** — check wallet + sign (CLI selects aggr_deferred automatically):

```bash
onchainos payment x402-pay \
  --accepts '<JSON.stringify(decoded.accepts)>'
# → { "signature": "base64...", "authorization": { ... }, "sessionCert": "..." }
# sessionCert present → CLI selected aggr_deferred scheme
```

**Step 5** — assemble v2 header (aggr_deferred, merge sessionCert into existing extra):

```
accepted       = decoded.accepts.find(a => a.scheme === "aggr_deferred")
accepted.extra = { ...accepted.extra, sessionCert }  // merge, keep name/version

paymentPayload = {
  x402Version: 2,
  resource:    decoded.resource,
  accepted:    accepted,
  payload:     { signature, authorization }
}
headerValue = btoa(JSON.stringify(paymentPayload))

GET https://api.example.com/data
PAYMENT-SIGNATURE: <headerValue>

→ HTTP 200  { "result": "..." }
```

## Local Signing Fallback (No Wallet)

> **⚠️ Security Notice**: This fallback uses your local private key for signing — the key stays on your machine but is **not** protected by TEE. Only use this path if you cannot log in to the wallet, and ensure your private key is stored securely (e.g., `~/.onchainos/.env` with `chmod 600`). The recommended path is always TEE signing via `onchainos payment x402-pay`.

If the user chose "Local private key" in Step 3, use the native `onchainos payment eip3009-sign` command to sign locally.

### Prerequisites

- A private key is available (via `EVM_PRIVATE_KEY` env var or `~/.onchainos/.env`) — Step 3 already verified this by reading the file
- The payer address must hold sufficient ERC-20 balance of the `asset` token on the target chain
- The `asset` token contract must support EIP-3009 `transferWithAuthorization`
- The 402 payload's `accepts[].extra` must include `name` (EIP-712 domain name); `version` is optional (defaults to `"2"`)

### Step 1: Decode the 402 Payload

Same as the main flow Step 2 — decode from `PAYMENT-REQUIRED` header (v2) or response body (v1):

```
// v2:
decoded = JSON.parse(atob(response.headers['PAYMENT-REQUIRED']))
// v1:
decoded = JSON.parse(response.body)

option = decoded.accepts[0]
```

Extract: `network`, `amount` (v2) or `maxAmountRequired` (v1), `payTo`, `asset`, `maxTimeoutSeconds`.

### Step 2: Sign with `onchainos payment eip3009-sign`

The CLI reads `EVM_PRIVATE_KEY` from env var or `~/.onchainos/.env` automatically, and derives the payer address from the private key:

```bash
onchainos payment eip3009-sign \
  --accepts '<JSON.stringify(decoded.accepts)>'
```

The CLI parses the accepts array (same as `x402-pay`), extracts `extra.name`/`extra.version` as the EIP-712 domain, and handles nonce generation, `validBefore` calculation, struct hashing, and secp256k1 signing internally. It returns `{ signature, authorization }` — same structure as the TEE path (exact scheme).

### Step 3: Assemble Header and Replay

Same as the main flow Step 5 — build the `paymentPayload` per version:

- **v2**: `{ x402Version: 2, resource: decoded.resource, accepted: option, payload: { signature, authorization } }` → header `PAYMENT-SIGNATURE`
- **v1**: `{ x402Version: 1, scheme: option.scheme, network: option.network, payload: { signature, authorization } }` → header `X-PAYMENT`

Base64-encode and replay the original request with the header attached.

### Important Notes for Local Signing

- The private key is read from the `EVM_PRIVATE_KEY` environment variable or `~/.onchainos/.env` — it **never** leaves the local machine
- The CLI generates a random 32-byte nonce and computes `validBefore = now + maxTimeoutSeconds` automatically
- The EIP-712 domain `name` must be present in `accepts[].extra.name`; if missing the CLI will error. `version` defaults to `"2"` if `extra.version` is absent
- The signed authorization only authorizes the **exact** `(from, to, value, nonce)` tuple — it cannot be modified or reused

## Edge Cases

- **Not logged in**: Ask user to choose between wallet login or local private key signing (see Step 3). If local: read `~/.onchainos/.env` to check for a key — the CLI derives the address automatically. If wallet: proceed with `onchainos wallet login`
- **Unsupported network**: Only EVM chains with CAIP-2 `eip155:<chainId>` format are supported
- **No wallet for chain**: The logged-in account must have an address on the requested chain; if not, inform the user
- **Amount in wrong units**: `amount` (v2) / `maxAmountRequired` (v1) must be in minimal units — remind user to convert (e.g., 1 USDG = `1000000` for 6 decimals)
- **Expired authorization**: If the server rejects the payment as expired, retry with a fresh signature
- **Network error**: Retry once, then prompt user to try again later

## Amount Display Rules

- `amount` (v2) / `maxAmountRequired` (v1) is always in minimal units (e.g., `1000000` for 1 USDG)
- When displaying to the user, convert to UI units: divide by `10^decimal`
- Show token symbol alongside (e.g., `1.00 USDG`)

Common token decimal reference:

| Token | Decimals | 1 unit in minimal     | Example                        |
|-------|----------|-----------------------|--------------------------------|
| USDC  | 6        | `1000000`             | `1000000` → 1.00 USDC          |
| USDG  | 6        | `1000000`             | `500000` → 0.50 USDG           |
| USDT  | 6        | `1000000`             | `2500000` → 2.50 USDT          |
| ETH   | 18       | `1000000000000000000` | `10000000000000000` → 0.01 ETH |

## Global Notes

- **Primary path** (`onchainos payment x402-pay`): requires an authenticated JWT session; signing is performed inside a TEE — the private key never leaves the secure enclave
- **Fallback path** (`onchainos payment eip3009-sign`): requires the user's own private key via `EVM_PRIVATE_KEY` env var or `~/.onchainos/.env`; signing is done entirely on the local machine — no JWT or TEE needed
- This skill only signs — it does **not** broadcast or deduct balance directly; payment settles when the recipient redeems the authorization on-chain
- The returned `authorization` object must be included alongside `signature` when building the payment header
