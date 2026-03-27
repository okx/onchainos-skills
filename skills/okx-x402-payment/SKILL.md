---
name: okx-x402-payment
description: "This skill should be used when the user encounters an HTTP 402 Payment Required response, wants to pay for a payment-gated API or resource, or mentions 'x402', 'pay for access', '402 payment', 'payment-gated URL', or 'sign x402 payment'. Primary path signs via TEE with a wallet session (JWT); fallback path guides local EIP-3009 signing with the user's own private key if they have no wallet. Returns the payment proof (signature + authorization) that the caller can attach as a payment header to access the resource. Do NOT use for swap or token transfers — use okx-dex-swap instead. Do NOT use for wallet balance or portfolio queries — use okx-agentic-wallet or okx-wallet-portfolio. Do NOT use for security scanning — use okx-security. Do NOT use for transaction broadcasting — use okx-onchain-gateway. Do NOT use for general programming questions."
license: MIT
metadata:
  author: okx
  version: "2.2.2"
  homepage: "https://web3.okx.com"
---

# Onchain OS x402 Payment

Sign an [x402](https://x402.org) payment authorization and return the payment proof for accessing payment-gated resources. Supports TEE signing (via wallet session) or local signing (with user's own private key).

## Pre-flight Checks

Every time before running any `onchainos` command, always follow these steps in order. Do not echo routine command output to the user; only provide a brief status update when installing, updating, or handling a failure.

1. **Resolve latest stable version**: Fetch the latest stable release tag from the GitHub API:
   ```
   curl -sSL "https://api.github.com/repos/okx/onchainos-skills/releases/latest"
   ```
   Extract the `tag_name` field (e.g., `v1.0.5`) into `LATEST_TAG`.
   If the API call fails and `onchainos` is already installed locally, skip steps 2-3
   and proceed to run the command (the user may be offline or rate-limited; a stale
   binary is better than blocking). If `onchainos` is **not** installed, **stop** and
   tell the user to check their network connection or install manually from
   https://github.com/okx/onchainos-skills.

2. **Install or update**: If `onchainos` is not found, or if the cache at `~/.onchainos/last_check` (`$env:USERPROFILE\.onchainos\last_check` on Windows) is older than 12 hours:
   - Download the installer and its checksum file from the latest release tag:
     - **macOS/Linux**:
       `curl -sSL "https://raw.githubusercontent.com/okx/onchainos-skills/${LATEST_TAG}/install.sh" -o /tmp/onchainos-install.sh`
       `curl -sSL "https://github.com/okx/onchainos-skills/releases/download/${LATEST_TAG}/installer-checksums.txt" -o /tmp/installer-checksums.txt`
     - **Windows**:
       `Invoke-WebRequest -Uri "https://raw.githubusercontent.com/okx/onchainos-skills/${LATEST_TAG}/install.ps1" -OutFile "$env:TEMP\onchainos-install.ps1"`
       `Invoke-WebRequest -Uri "https://github.com/okx/onchainos-skills/releases/download/${LATEST_TAG}/installer-checksums.txt" -OutFile "$env:TEMP\installer-checksums.txt"`
   - Verify the installer's SHA256 against `installer-checksums.txt`. On mismatch, **stop** and warn — the installer may have been tampered with.
   - Execute: `sh /tmp/onchainos-install.sh` (or `& "$env:TEMP\onchainos-install.ps1"` on Windows).
     The installer handles version comparison internally and only downloads the binary if needed.
   - On other failures, point to https://github.com/okx/onchainos-skills.

3. **Verify binary integrity** (once per session): Run `onchainos --version` to get the installed
   version (e.g., `1.0.5` or `2.0.0-beta.0`). Construct the installed tag as `v<version>`.
   Download `checksums.txt` for the **installed version's tag** (not necessarily LATEST_TAG):
   `curl -sSL "https://github.com/okx/onchainos-skills/releases/download/v<version>/checksums.txt" -o /tmp/onchainos-checksums.txt`
   Look up the platform target and compare the installed binary's SHA256 against the checksum.
   On mismatch, reinstall (step 2) and re-verify. If still mismatched, **stop** and warn.
   - Platform targets — macOS: `arm64`->`aarch64-apple-darwin`, `x86_64`->`x86_64-apple-darwin`; Linux: `x86_64`->`x86_64-unknown-linux-gnu`, `aarch64`->`aarch64-unknown-linux-gnu`, `i686`->`i686-unknown-linux-gnu`, `armv7l`->`armv7-unknown-linux-gnueabihf`; Windows: `AMD64`->`x86_64-pc-windows-msvc`, `x86`->`i686-pc-windows-msvc`, `ARM64`->`aarch64-pc-windows-msvc`
   - Hash command — macOS/Linux: `shasum -a 256 ~/.local/bin/onchainos`; Windows: `(Get-FileHash "$env:USERPROFILE\.local\bin\onchainos.exe" -Algorithm SHA256).Hash.ToLower()`

4. **Check for skill version drift** (once per session): If `onchainos --version` is newer
   than this skill's `metadata.version`, display a one-time notice that the skill may be
   outdated and suggest the user re-install skills via their platform's method. Do not block.
5. **Do NOT auto-reinstall on command failures.** Report errors and suggest
   `onchainos --version` or manual reinstallation from https://github.com/okx/onchainos-skills.
6. **Rate limit errors.** If a command hits rate limits, the shared API key may
   be throttled. Suggest creating a personal key at the
   [OKX Developer Portal](https://web3.okx.com/onchain-os/dev-portal). If the
   user creates a `.env` file, remind them to add `.env` to `.gitignore`.

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

x402 is an HTTP payment protocol. When a server returns `HTTP 402 Payment Required`, it includes a base64-encoded JSON payload describing what payment is required. The full flow is:

1. Send request → receive `HTTP 402` with base64-encoded payment payload
2. Decode the payload, extract payment parameters from `accepts[0]`
3. Sign via TEE → `onchainos payment x402-pay` → obtain `{ signature, authorization }`
4. Assemble payment header and replay the original request

This skill owns **steps 2–4** end to end.

## Quickstart

```bash
# Sign an x402 payment for an X Layer USDG-gated resource
onchainos payment x402-pay \
  --network eip155:196 \
  --amount 1000000 \
  --pay-to 0xRecipientAddress \
  --asset 0x4ae46a509f6b1d9056937ba4500cb143933d2dc8 \
  --max-timeout-seconds 300
```

## Command Index

| # | Command                      | Description                                       |
|---|------------------------------|---------------------------------------------------|
| 1 | `onchainos payment x402-pay` | Sign an x402 payment and return the payment proof |

## Operation Flow

### Step 1: Send the Original Request

Make the HTTP request the user asked for. If the response status is **not 402**, return the result directly — **no payment needed, do not check wallet or attempt login**.

> **IMPORTANT**: Do NOT check wallet status or attempt login before sending the request. Only proceed to payment steps if the response is HTTP 402.

### Step 2: Decode the 402 Payload

If the response is `HTTP 402`, the body is a base64-encoded JSON string. Decode it:

```
rawBody  = response.body          // base64 string
decoded  = JSON.parse(atob(rawBody))
option   = decoded.accepts[0]
```

Extract these fields from `option`:

| x402 field                                    | CLI param               | Notes                                             |
|-----------------------------------------------|-------------------------|---------------------------------------------------|
| `option.network`                              | `--network`             | CAIP-2 format, e.g. `eip155:196`                  |
| `option.amount` or `option.maxAmountRequired` | `--amount`              | prefer `amount`; fall back to `maxAmountRequired` |
| `option.payTo`                                | `--pay-to`              |                                                   |
| `option.asset`                                | `--asset`               | token contract address                            |
| `option.maxTimeoutSeconds`                    | `--max-timeout-seconds` | optional, default 300                             |

**⚠️ MANDATORY: Display payment details and STOP to wait for user confirmation. Do NOT check wallet status, run `onchainos wallet status`, attempt login, or call any other tool until the user explicitly confirms.**

Present the following information to the user:

> This resource requires x402 payment:
> - **Network**: `<chain name>` (`<network>`)
> - **Token**: `<token symbol>` (`<asset>`)
> - **Amount**: `<human-readable amount>` (convert from minimal units using token decimals)
> - **Pay to**: `<payTo>`
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

- **Logged in** → proceed to Step 4 (Sign).
- **Not logged in** → ask the user:

> "This resource requires payment (x402). You need a wallet to sign the payment. Would you like to create one? (It's free and takes ~30 seconds.)"

- **User says yes** → run `onchainos wallet login` (AK login, no email) or `onchainos wallet login <email>` (OTP login), then proceed to Step 4.
- **User says no** → switch to the **Local Signing Fallback** (see below).

### Step 4: Sign

Run `onchainos payment x402-pay` with the extracted parameters. Returns `{ signature, authorization }`.

**If signing fails** (e.g., session expired, not logged in, AK re-login failed):
- Do NOT simply cancel or give up.
- Ask the user: "Signing failed because there is no active wallet session. Would you like to log in now, or sign locally with your own private key?"
  - **User wants to log in** → run `onchainos wallet login` or `onchainos wallet login <email>`, then retry this step.
  - **User wants local signing** → switch to the **Local Signing Fallback** (see below).
  - **User wants to cancel** → only then cancel the request.

### Step 5: Assemble Header and Replay

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
1. Send GET https://api.example.com/data                              → HTTP 402 with base64 payload
       ↓ decode payload, extract accepts[0]
2. okx-x402-payment   onchainos payment x402-pay \
                        --network eip155:196 --amount 1000000 \
                        --pay-to 0xAbC... \
                        --asset 0x4ae46a509f6b1d9056937ba4500cb143933d2dc8   → { signature, authorization }
       ↓ assemble payment header
3. Replay GET https://api.example.com/data with PAYMENT-SIGNATURE header  → HTTP 200
```

**Data handoff**:
- `accepts[0].network` → `--network`
- `accepts[0].amount` (or `maxAmountRequired`) → `--amount`
- `accepts[0].payTo` → `--pay-to`
- `accepts[0].asset` → `--asset`

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

| Param                   | Required | Default          | Description                                                                         |
|-------------------------|----------|------------------|-------------------------------------------------------------------------------------|
| `--network`             | Yes      | -                | CAIP-2 network identifier (e.g., `eip155:196` for X Layer, `eip155:1` for Ethereum) |
| `--amount`              | Yes      | -                | Payment amount in minimal units (e.g., `1000000` = 1 USDG with 6 decimals)          |
| `--pay-to`              | Yes      | -                | Recipient address (from x402 `payTo` field)                                         |
| `--asset`               | Yes      | -                | Token contract address (from x402 `asset` field)                                    |
| `--from`                | No       | selected account | Payer address; if omitted, uses the currently selected account                      |
| `--max-timeout-seconds` | No       | `300`            | Authorization validity window in seconds                                            |

**Return fields**:

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
    "network": "eip155:196",
    "amount": "1000000",
    "payTo": "0xAbC...",
    "asset": "0x4ae46a509f6b1d9056937ba4500cb143933d2dc8",
    "maxTimeoutSeconds": 300
  }]
}
```

**Step 3–4** — check wallet + sign:
```bash
onchainos payment x402-pay \
  --network eip155:196 \
  --amount 1000000 \
  --pay-to 0xAbC... \
  --asset 0x4ae46a509f6b1d9056937ba4500cb143933d2dc8 \
  --max-timeout-seconds 300
# → { "signature": "0x...", "authorization": { ... } }
```

**Step 5** — assemble header and replay:
```
paymentPayload = { ...decoded, payload: { signature, authorization } }
headerValue    = btoa(JSON.stringify(paymentPayload))

GET https://api.example.com/data
PAYMENT-SIGNATURE: <headerValue>

→ HTTP 200  { "result": "..." }
```

## Local Signing Fallback (No Wallet)

If the user does not have a wallet and chooses not to create one, guide them through local EIP-3009 signing with their own private key.

### Prerequisites

- User has a local private key (e.g., in a `.env` file, hardware wallet, or MetaMask export)
- The payer address must hold sufficient ERC-20 balance of the `asset` token on the target chain
- The `asset` token contract must support EIP-3009 `transferWithAuthorization`

### Step 1: Decode the 402 Payload

Same as the main flow — decode the base64 body and extract `accepts[0]`:

```
rawBody  = response.body
decoded  = JSON.parse(atob(rawBody))
option   = decoded.accepts[0]
```

Extract: `network`, `amount` (or `maxAmountRequired`), `payTo`, `asset`, `maxTimeoutSeconds`.

### Step 2: Construct EIP-3009 Parameters and Sign

Build the `TransferWithAuthorization` message and sign it with `eth_signTypedData_v4`. Key fields:

| Field         | Value                                    |
|---------------|------------------------------------------|
| `from`        | Payer address                            |
| `to`          | `option.payTo`                           |
| `value`       | `option.amount`                          |
| `validAfter`  | `"0"`                                    |
| `validBefore` | `now + maxTimeoutSeconds` (Unix seconds) |
| `nonce`       | Random 32 bytes (hex)                    |

EIP-712 domain: query the token contract's `name()`, `version` (often `"1"` or `"2"`), `chainId` from the CAIP-2 network, and `verifyingContract` = `option.asset`.

**Sign with ethers.js**:

```javascript
const wallet = new ethers.Wallet('<PRIVATE_KEY>');
const signature = await wallet.signTypedData(domain, types, message);
```

> See [EIP-3009](https://eips.ethereum.org/EIPS/eip-3009) for the full typed data spec. `domain.name`/`version` vary per
> token (e.g. USDC uses `"USD Coin"` / `"2"`) — query the contract to confirm.

### Step 3: Assemble Header and Replay

Same as the main flow Step 5 — build `authorization` from the signed fields, determine header name from `x402Version`, assemble `paymentPayload = { ...decoded, payload: { signature, authorization } }`, base64-encode, and replay the original request with the payment header attached.

### Important Notes for Local Signing

- The private key **never** leaves the local machine — signing is done entirely offline
- The `nonce` must be a random 32-byte hex value; reusing a nonce will cause the transaction to be rejected
- `validBefore` is a Unix timestamp in seconds — set it to `now + maxTimeoutSeconds` (default 300s / 5 minutes)
- If the token uses a non-standard EIP-712 domain (e.g., different `version` string), the signature will be invalid — always query the contract first
- The signed authorization only authorizes the **exact** `(from, to, value, nonce)` tuple — it cannot be modified or reused

## Edge Cases

- **Not logged in**: Ask user if they want to create a wallet (`onchainos wallet login` or `onchainos wallet login <email>`). If not, guide them through the Local Signing Fallback above
- **Unsupported network**: Only EVM chains with CAIP-2 `eip155:<chainId>` format are supported
- **No wallet for chain**: The logged-in account must have an address on the requested chain; if not, inform the user
- **Amount in wrong units**: `--amount` must be in minimal units — remind user to convert (e.g., 1 USDG = `1000000` for 6 decimals)
- **Expired authorization**: If the server rejects the payment as expired, retry with a fresh signature
- **Network error**: Retry once, then prompt user to try again later

## Amount Display Rules

- `--amount` is always in minimal units (e.g., `1000000` for 1 USDG)
- When displaying to the user, convert to UI units: divide by `10^decimal`
- Show token symbol alongside (e.g., `1.00 USDG`)

## Global Notes

- **Primary path** (`onchainos payment x402-pay`): requires an authenticated JWT session; signing is performed inside a TEE — the private key never leaves the secure enclave
- **Fallback path** (local signing): requires the user's own private key; signing is done entirely on the local machine — no JWT or TEE needed
- This skill only signs — it does **not** broadcast or deduct balance directly; payment settles when the recipient redeems the authorization on-chain
- `--network` must be CAIP-2 format: `eip155:<chainId>` (e.g., `eip155:1`, `eip155:8453`, `eip155:196`)
- The returned `authorization` object must be included alongside `signature` when building the payment header
