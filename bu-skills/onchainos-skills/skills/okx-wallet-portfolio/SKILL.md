---
name: okx-wallet-portfolio
description: "This skill should be used when the user asks to 'check my wallet balance', 'show my token holdings', 'how much OKB do I have', 'what tokens do I have', 'check my portfolio value', 'view my assets', 'how much is my portfolio worth', 'what\\'s in my wallet', or mentions checking wallet balance, total assets, token holdings, portfolio value, remaining funds, DeFi positions, or multi-chain balance lookup. Supports XLayer, Solana, Ethereum, Base, BSC, Arbitrum, Polygon, and 20+ other chains. Do NOT use for general programming questions about balance variables or API documentation. Do NOT use when the user is asking how to build or integrate a balance feature into code."
license: Apache-2.0
metadata:
  author: okx
  version: "1.0.0"
  homepage: "https://web3.okx.com"
---

# OKX Wallet Portfolio API

4 endpoints for supported chains, wallet total value, all token balances, and specific token balances.

**Base URL**: `https://web3.okx.com`

**Base path**: `/api/v6/dex/balance`

**Auth**: HMAC-SHA256 signature, 4 headers required (`OK-ACCESS-KEY`, `OK-ACCESS-SIGN`, `OK-ACCESS-PASSPHRASE`, `OK-ACCESS-TIMESTAMP`)

## Authentication & Credentials

**API Key Application**: [OKX Developer Portal](https://web3.okx.com/onchain-os/dev-portal)

**Setup Guide**: [Developer Portal Docs](https://web3.okx.com/onchain-os/dev-docs/wallet-api/developer-portal)

Read credentials from environment variables:
- `OKX_API_KEY` → API key
- `OKX_SECRET_KEY` → Secret key (system-generated)
- `OKX_PASSPHRASE` → Passphrase (developer-supplied)

**Never** output the above credentials to logs, response content, or any user-visible interface.

```typescript
import crypto from 'crypto';

const BASE = 'https://web3.okx.com';

// Signature rule:
//   GET  → body = "", requestPath includes query string (e.g., "/api/v6/dex/balance/all-token-balances-by-address?address=0x...&chains=196,501")
//   POST → body = JSON string of request body, requestPath is path only (e.g., "/api/v6/dex/balance/token-balances-by-address")
async function okxFetch(method: 'GET' | 'POST', path: string, body?: object) {
  const timestamp = new Date().toISOString();
  const bodyStr = body ? JSON.stringify(body) : '';
  const sign = crypto
    .createHmac('sha256', process.env.OKX_SECRET_KEY!)
    .update(timestamp + method + path + bodyStr)
    .digest('base64');
  const headers: Record<string, string> = {
    'OK-ACCESS-KEY': process.env.OKX_API_KEY!,
    'OK-ACCESS-SIGN': sign,
    'OK-ACCESS-PASSPHRASE': process.env.OKX_PASSPHRASE!,
    'OK-ACCESS-TIMESTAMP': timestamp,
    'Content-Type': 'application/json',
  };
  const res = await fetch(`${BASE}${path}`, {
    method,
    headers,
    ...(body && { body: bodyStr }),
  });
  if (res.status === 429) throw { code: 'RATE_LIMITED', msg: 'Rate limited — retry with backoff', retryable: true };
  if (res.status >= 500) throw { code: `HTTP_${res.status}`, msg: 'Server error', retryable: true };
  const json = await res.json();
  if (json.code !== '0') throw { code: json.code, msg: json.msg || 'API error', retryable: false };
  return json.data;
}
```

Response envelope: `{ "code": "0", "data": [...], "msg": "" }`. `code` = `"0"` means success.

## Developer Quickstart

```typescript
// Get all token balances (GET)
const balances = await okxFetch('GET', '/api/v6/dex/balance/all-token-balances-by-address?' + new URLSearchParams({
  address: '0xYourWallet', chains: '196,501,1',
}));
// → balances[].tokenAssets[]: { symbol, balance (UI units), tokenPrice, rawBalance }

// Check specific tokens (POST)
const specific = await okxFetch('POST', '/api/v6/dex/balance/token-balances-by-address', {
  address: '0xYourWallet',
  tokenContractAddresses: [
    { chainIndex: '196', tokenContractAddress: '' },  // native OKB
    { chainIndex: '196', tokenContractAddress: '0x74b7f16337b8972027f6196a17a631ac6de26d22' },  // USDC
  ],
});
```

## Common Chain IDs

| Chain | chainIndex | Chain | chainIndex |
|---|---|---|---|
| XLayer | `196` | Base | `8453` |
| Solana | `501` | BSC | `56` |
| Ethereum | `1` | Arbitrum | `42161` |

**Address format note**: The same `address` parameter is only valid across chains of the same type. EVM addresses (`0x...`) work on Ethereum/BSC/Polygon/Arbitrum/Base etc. Solana addresses (Base58) and Bitcoin addresses (UTXO) have different formats. Do NOT mix — e.g., passing an EVM address with `chains="1,501"` will return empty data for the Solana portion.

## Endpoint Index

| # | Method | Path | Docs |
|---|---|---|---|
| 1 | GET | `/api/v6/dex/balance/supported/chain` | [supported-chain](https://web3.okx.com/onchain-os/dev-docs/market/balance-chains) |
| 2 | GET | `/api/v6/dex/balance/total-value-by-address` | [total-value-by-address](https://web3.okx.com/onchain-os/dev-docs/market/balance-total-value) |
| 3 | GET | `/api/v6/dex/balance/all-token-balances-by-address` | [all-token-balances-by-address](https://web3.okx.com/onchain-os/dev-docs/market/balance-total-token-balances) |
| 4 | POST | `/api/v6/dex/balance/token-balances-by-address` | [token-balances-by-address](https://web3.okx.com/onchain-os/dev-docs/market/balance-specific-token-balance) |

Error Codes: [Balance Error Codes](https://web3.okx.com/onchain-os/dev-docs/market/balance-error-code)

## Cross-Skill Workflows

This skill is often used **before swap** (to verify sufficient balance) or **as portfolio entry point**. Below are typical end-to-end flows showing how data flows between skills.

### Workflow A: Pre-Swap Balance Check

> User: "Swap 1 SOL for BONK"

```
1. okx-dex-token   /market/token/search?search=BONK                     → get tokenContractAddress
2. okx-wallet-portfolio /balance/all-token-balances-by-address                → verify SOL balance >= 1
       ↓ balance field (UI units) → convert to minimal units for swap
3. okx-dex-swap    /aggregator/quote                                     → get quote
4. okx-dex-swap    /aggregator/swap-instruction                          → execute (Solana)
```

**Data handoff**:
- `tokenContractAddress` from token search → feeds into swap `fromTokenAddress` / `toTokenAddress`
- `balance` from balance API is **UI units**; swap API needs **minimal units** → multiply by `10^decimal`
- If balance < required amount → inform user, do NOT proceed to swap

### Workflow B: Portfolio Overview + Analysis

> User: "Show my portfolio"

```
1. okx-wallet-portfolio /balance/total-value-by-address                        → total USD value
2. okx-wallet-portfolio /balance/all-token-balances-by-address                 → per-token breakdown
       ↓ top holdings by USD value
3. okx-dex-token   /market/price-info                                     → enrich with 24h change, market cap, liquidity
4. okx-dex-market  /market/candles (optional)                             → price charts for tokens of interest
```

### Workflow C: Sell Underperforming Tokens

```
1. okx-wallet-portfolio /balance/all-token-balances-by-address                 → list all holdings
       ↓ tokenContractAddress + chainIndex for each
2. okx-dex-token   /market/price-info                                     → get priceChange24H per token
3. Filter by negative change → user confirms which to sell
4. okx-dex-swap    /aggregator/quote → /aggregator/swap → execute sell
```

**Key conversion**: `balance` (UI units) × `10^decimal` = `amount` (minimal units) for swap API.

## Operation Flow

### Step 1: Identify Intent

- Check total assets -> `GET /balance/total-value-by-address`
- View all token holdings -> `GET /balance/all-token-balances-by-address`
- Check specific token balance -> `POST /balance/token-balances-by-address`
- Unsure which chains are supported -> `GET /balance/supported/chain` first

### Step 2: Collect Parameters

- Missing wallet address -> ask user
- Missing target chains -> recommend XLayer (chainIndex `196`, low gas, fast confirmation) as the default, then ask which chain the user prefers. Common set: `"196,501,1,8453,56"`
- Need to filter risky tokens -> set `excludeRiskToken=true` (only works on ETH/BSC/SOL/BASE)

### Step 3: Call and Display

- Total value: display USD amount
- Token balances: show token name, amount (UI units), USD value
- Sort by USD value descending

### Step 4: Suggest Next Steps

After displaying results, suggest 2-3 relevant follow-up actions based on the endpoint just called:

| Just called | Suggest |
|---|---|
| `/balance/total-value-by-address` | 1. View token-level breakdown → `/balance/all-token-balances-by-address` (this skill) 2. Check price trend for top holdings → `okx-dex-market` |
| `/balance/all-token-balances-by-address` | 1. View detailed analytics (market cap, 24h change) for a token → `okx-dex-token` 2. Swap a token → `okx-dex-swap` 3. View price chart for a token → `okx-dex-market` |
| `/balance/token-balances-by-address` | 1. View full portfolio across all tokens → `/balance/all-token-balances-by-address` (this skill) 2. Swap this token → `okx-dex-swap` |

Present conversationally, e.g.: "Would you like to see the price chart for your top holding, or swap any of these tokens?" — never expose skill names or endpoint paths to the user.

## API Reference

### 1. GET /balance/supported/chain

No request parameters.

**Response:**

| Field | Type | Description |
|---|---|---|
| `data[].name` | String | Chain name (e.g., "XLayer") |
| `data[].logoUrl` | String | Chain logo URL |
| `data[].shortName` | String | Chain short name (e.g., "OKB") |
| `data[].chainIndex` | String | Chain unique identifier (e.g., "196") |

```json
{ "code": "0", "data": [{ "name": "XLayer", "logoUrl": "...", "shortName": "OKB", "chainIndex": "196" }], "msg": "" }
```

### 2. GET /balance/total-value-by-address

> **`excludeRiskToken` type warning**: This endpoint uses `Boolean` (`true`/`false`), but endpoints #3 and #4 use `String` (`"0"`/`"1"`). This is an OKX API inconsistency — pay attention to the type per endpoint.

| Param | Type | Required | Description |
|---|---|---|---|
| `address` | String | Yes | Wallet address |
| `chains` | String | Yes | Chain IDs, comma-separated, max 50. e.g., `"196,501"` |
| `assetType` | String | No | `0`=all (default), `1`=tokens only, `2`=DeFi only |
| `excludeRiskToken` | Boolean | No | `true`=filter risky tokens (default), `false`=include. Only ETH/BSC/SOL/BASE |

**Response:**

| Field | Type | Description |
|---|---|---|
| `data[].totalValue` | String | Total asset value in USD |

```json
{ "code": "0", "data": [{ "totalValue": "1172.895057177065864522" }], "msg": "success" }
```

### 3. GET /balance/all-token-balances-by-address

| Param | Type | Required | Description |
|---|---|---|---|
| `address` | String | Yes | Wallet address |
| `chains` | String | Yes | Chain IDs, comma-separated, max 50 |
| `excludeRiskToken` | String | No | `0`=filter out (default), `1`=do not filter. Only ETH/BSC/SOL/BASE |

**Response key fields**: `tokenAssets[]` — `chainIndex`, `tokenContractAddress`, `symbol`, `balance` (UI units), `rawBalance` (base units), `tokenPrice` (USD), `isRiskToken`. Full fields: see [docs](https://web3.okx.com/onchain-os/dev-docs/market/balance-total-token-balances).

```json
{
  "code": "0",
  "data": [{
    "tokenAssets": [{
      "chainIndex": "196", "tokenContractAddress": "0xea034fb02eb1808c2cc3adbc15f447b93cbe08e1",
      "symbol": "WBTC", "balance": "0.5", "rawBalance": "50000000",
      "tokenPrice": "65000.00", "isRiskToken": false,
      "address": "0xEd0C6079229E2d407672a117c22b62064f4a4312"
    }]
  }],
  "msg": "success"
}
```

### 4. POST /balance/token-balances-by-address

| Param | Type | Required | Description |
|---|---|---|---|
| `address` | String | Yes | Wallet address |
| `tokenContractAddresses` | Array | Yes | Max 20 items |
| `tokenContractAddresses[].chainIndex` | String | Yes | Chain ID (e.g., `"196"`) |
| `tokenContractAddresses[].tokenContractAddress` | String | Yes | Token address (`""` for native token) |
| `excludeRiskToken` | String | No | `0`=filter out (default), `1`=do not filter |

**Response:** Same schema as endpoint #3.

```json
{
  "code": "0",
  "data": [{
    "tokenAssets": [{
      "chainIndex": "196", "tokenContractAddress": "",
      "symbol": "OKB", "balance": "10.5", "tokenPrice": "48.50",
      "isRiskToken": false, "rawBalance": "1500000000000000000", "address": "0x..."
    }]
  }],
  "msg": "success"
}
```

## Input / Output Examples

**User says:** "Check my wallet 0xABC... total assets on XLayer and Solana"

```
GET /api/v6/dex/balance/total-value-by-address?address=0xabc...&chains=196,501
-> Display: Total assets $12,345.67
```

**User says:** "Show all tokens in my wallet"

```
GET /api/v6/dex/balance/all-token-balances-by-address?address=0xabc...&chains=196,501
-> Display:
  OKB:  10.5 ($509.25)
  USDC: 2,000 ($2,000.00)
  USDT: 1,500 ($1,500.00)
  ...
```

**User says:** "Only check USDC and USDT balances"

```
POST /api/v6/dex/balance/token-balances-by-address
Body: {
  "address": "0xabc...",
  "tokenContractAddresses": [
    { "chainIndex": "196", "tokenContractAddress": "0x74b7f16337b8972027f6196a17a631ac6de26d22" },
    { "chainIndex": "196", "tokenContractAddress": "0x779ded0c9e1022225f8e0630b35a9b54be713736" }
  ]
}
-> Display: USDC: 2,000 ($2,000.00), USDT: 1,500 ($1,500.00)
```

## Edge Cases

- **Zero balance**: valid state — display `$0.00`, not an error
- **Unsupported chain**: call `/balance/supported/chain` first to confirm
- **chains exceeds 50**: split into batches, max 50 per request
- **`excludeRiskToken` not working**: only supported on ETH/BSC/SOL/BASE
- **DeFi positions**: use `assetType=2` to query DeFi holdings separately
- **429 rate limit**: exponential backoff with jitter. See [Rate Limit & Fee Docs](https://web3.okx.com/onchain-os/dev-docs/home/api-fee) for tier-specific RPS limits (Trial: 1 RPS, Start-up: 2-50 RPS, Enterprise: custom).
- **Cross-skill pipeline rate limit**: when chaining calls across multiple skills (e.g., token search → balance → swap), add 300-500ms delay between requests to avoid triggering rate limit (error code `50011`).
- **Network error**: retry once
- **Request timeout**: all API calls must set a **10-second** timeout limit

## Amount Display Rules

- Token amounts in UI units (`1.5 ETH`), never base units (`1500000000000000000`)
- USD values with 2 decimal places
- Large amounts in shorthand (`$1.2M`)
- Sort by USD value descending

## Global Notes

- `chains` supports up to **50** chain IDs (comma-separated strings)
- `assetType`: `0`=all `1`=tokens only `2`=DeFi only
- `excludeRiskToken` only works on ETH(`1`)/BSC(`56`)/SOL(`501`)/BASE(`8453`)
- `/token-balances-by-address` is the only **POST** endpoint in this family
- Total value is returned as a USD string — handle type conversion
- For token prices / K-lines -> use `okx-dex-market`
- For token search / metadata -> use `okx-dex-token`
- For swap execution -> use `okx-dex-swap`