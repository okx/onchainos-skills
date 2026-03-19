---
name: okx-x402-payment
description: "This skill should be used when the user encounters an HTTP 402 Payment Required response, wants to pay for a payment-gated API or resource, or mentions 'x402', 'pay for access', '402 payment', 'payment-gated URL', or 'sign x402 payment'. Requires an active login session (JWT). Signs an EIP-3009 authorization via TEE and returns the payment proof (signature + authorization) that the caller can attach as a payment header to access the resource. Do NOT use for swap or token transfers — use okx-dex-swap instead. Do NOT use for wallet balance or portfolio queries — use okx-agentic-wallet or okx-wallet-portfolio. Do NOT use for security scanning — use okx-security. Do NOT use for transaction broadcasting — use okx-onchain-gateway. Do NOT use for general programming questions."
license: MIT
metadata:
  author: okx
  version: "1.0.4"
  homepage: "https://web3.okx.com"
---

# Onchain OS x402 Payment

Sign an [x402](https://x402.org) payment authorization via TEE and return the payment proof for accessing payment-gated resources.

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
   `onchainos --version` or manual reinstall from https://github.com/okx/onchainos-skills.
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

| # | Command                       | Description                                          |
|---|-------------------------------|------------------------------------------------------|
| 1 | `onchainos payment x402-pay`  | Sign an x402 payment and return the payment proof    |

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

| x402 field                                    | CLI param               | Notes                                             |
|-----------------------------------------------|-------------------------|---------------------------------------------------|
| `option.network`                              | `--network`             | CAIP-2 format, e.g. `eip155:196`                  |
| `option.amount` or `option.maxAmountRequired` | `--amount`              | prefer `amount`; fall back to `maxAmountRequired` |
| `option.payTo`                                | `--pay-to`              |                                                   |
| `option.asset`                                | `--asset`               | token contract address                            |
| `option.maxTimeoutSeconds`                    | `--max-timeout-seconds` | optional, default 300                             |

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

| Just completed          | Suggest                                                                                     |
|-------------------------|---------------------------------------------------------------------------------------------|
| Successful replay       | 1. Check balance impact → `okx-agentic-wallet` 2. Make another request to the same resource |
| 402 on replay (expired) | Retry from Step 3 with a fresh signature                                                    |

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

**Step 2–3** — sign:
```bash
onchainos payment x402-pay \
  --network eip155:196 \
  --amount 1000000 \
  --pay-to 0xAbC... \
  --asset 0x4ae46a509f6b1d9056937ba4500cb143933d2dc8 \
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
- **Amount in wrong units**: `--amount` must be in minimal units — remind user to convert (e.g., 1 USDG = `1000000` for 6 decimals)
- **Expired authorization**: If the server rejects the payment as expired, retry with a fresh signature
- **Network error**: Retry once, then prompt user to try again later

## Amount Display Rules

- `--amount` is always in minimal units (e.g., `1000000` for 1 USDG)
- When displaying to the user, convert to UI units: divide by `10^decimal`
- Show token symbol alongside (e.g., `1.00 USDG`)

## Global Notes

- This skill requires an **authenticated JWT session** — no OKX API key needed
- Signing is performed inside a TEE; the private key never leaves the secure enclave
- This skill only signs — it does **not** broadcast or deduct balance directly; payment settles when the recipient redeems the authorization on-chain
- `--network` must be CAIP-2 format: `eip155:<chainId>` (e.g., `eip155:1`, `eip155:8453`, `eip155:196`)
- The returned `authorization` object must be included alongside `signature` when building the payment header
