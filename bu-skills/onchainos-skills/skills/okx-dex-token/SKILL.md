---
name: okx-dex-token
description: "This skill should be used when the user asks to 'find a token', 'search for a token', 'look up PEPE', 'what\\'s trending', 'top tokens', 'trending tokens on Solana', 'token rankings', 'who holds this token', 'holder distribution', 'is this token safe', 'token market cap', 'token liquidity', 'research a token', 'tell me about this token', 'token info', or mentions searching for tokens by name or address, discovering trending tokens, viewing token rankings, checking holder distribution, or analyzing token market cap and liquidity. Covers token search, metadata, market cap, liquidity, volume, trending token rankings, and holder analysis across XLayer, Solana, Ethereum, Base, BSC, Arbitrum, Polygon, and 20+ other chains. Do NOT use when the user says only a single generic word like 'tokens' or 'crypto' without specifying a token name, action, or question. For simple current price checks, price charts, candlestick data, or trade history, use okx-dex-market instead."
license: Apache-2.0
metadata:
  author: okx
  version: "1.0.0"
  homepage: "https://web3.okx.com"
---

# OKX DEX Token Info API

5 endpoints for token search, metadata, detailed pricing, rankings, and holder distribution.

**Base URL**: `https://web3.okx.com`

**Base path**: `/api/v6/dex/market`

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
//   GET  → body = "", requestPath includes query string (e.g., "/api/v6/dex/market/token/search?chains=196&search=xETH")
//   POST → body = JSON string of request body, requestPath is path only (e.g., "/api/v6/dex/market/price-info")
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
// Search token (GET)
const tokens = await okxFetch('GET', '/api/v6/dex/market/token/search?' + new URLSearchParams({
  chains: '196,501', search: 'xETH',
}));
// → tokens[].tokenContractAddress, price, communityRecognized

// Get detailed price info (POST — body is JSON array)
const path = '/api/v6/dex/market/price-info';
const prices = await okxFetch('POST', path, [
  { chainIndex: '196', tokenContractAddress: '0xe7b000003a45145decf8a28fc755ad5ec5ea025a' },
]);
// → prices[].price, marketCap, liquidity, volume24H, priceChange24H
```

## Common Chain IDs

| Chain | chainIndex | Chain | chainIndex |
|---|---|---|---|
| XLayer | `196` | Base | `8453` |
| Solana | `501` | BSC | `56` |
| Ethereum | `1` | Arbitrum | `42161` |

## Endpoint Index

| # | Method | Path | Docs |
|---|---|---|---|
| 1 | GET | `/api/v6/dex/market/token/search` | [market-token-search](https://web3.okx.com/onchain-os/dev-docs/market/market-token-search) |
| 2 | POST | `/api/v6/dex/market/token/basic-info` | [market-token-basic-info](https://web3.okx.com/onchain-os/dev-docs/market/market-token-basic-info) |
| 3 | POST | `/api/v6/dex/market/price-info` | [market-token-price-info](https://web3.okx.com/onchain-os/dev-docs/market/market-token-price-info) |
| 4 | GET | `/api/v6/dex/market/token/toplist` | [market-token-ranking](https://web3.okx.com/onchain-os/dev-docs/market/market-token-ranking) |
| 5 | GET | `/api/v6/dex/market/token/holder` | [market-token-holder](https://web3.okx.com/onchain-os/dev-docs/market/market-token-holder) |

Error Codes: [Token Error Codes](https://web3.okx.com/onchain-os/dev-docs/market/market-token-error-code)

## Boundary: token vs market skill

| Need | Use this skill (`okx-dex-token`) | Use `okx-dex-market` instead |
|---|---|---|
| Search token by name/symbol | `GET /market/token/search` | - |
| Token metadata (decimals, logo) | `POST /market/token/basic-info` | - |
| Price + market cap + liquidity + multi-timeframe change | `POST /market/price-info` | - |
| Token ranking (trending) | `GET /market/token/toplist` | - |
| Holder distribution | `GET /market/token/holder` | - |
| Raw real-time price (single value) | - | `POST /market/price` |
| K-line / candlestick chart | - | `GET /market/candles` |
| Trade history (buy/sell log) | - | `GET /market/trades` |
| Index price (multi-source aggregate) | - | `POST /index/current-price` |

**Rule of thumb**: `okx-dex-token` = token discovery & enriched analytics. `okx-dex-market` = raw price feeds & charts.

## Cross-Skill Workflows

This skill is the typical **entry point** — users often start by searching/discovering tokens, then proceed to check balance and swap.

### Workflow A: Search → Research → Buy

> User: "Find BONK token, analyze it, then buy some"

```
1. okx-dex-token   /market/token/search?search=BONK&chains=501             → get tokenContractAddress, chainIndex, price, communityRecognized
       ↓ tokenContractAddress + chainIndex
2. okx-dex-token   /market/price-info                                       → market cap, liquidity, volume24H, priceChange24H, holders
3. okx-dex-token   /market/token/holder                                     → top 20 holders distribution
4. okx-dex-market  /market/candles?bar=1H                                   → hourly price chart
       ↓ user decides to buy
5. okx-wallet-portfolio /balance/all-token-balances-by-address                   → verify wallet has enough SOL
6. okx-dex-swap    /aggregator/quote → /aggregator/swap-instruction → execute
```

**Data handoff**:
- `tokenContractAddress` from step 1 → reused in all subsequent steps
- `chainIndex` from step 1 → reused in all subsequent steps
- `decimal` from step 1 or `/market/token/basic-info` → needed for minimal unit conversion in swap

### Workflow B: Discover Trending → Investigate → Trade

> User: "What's trending on Solana?"

```
1. okx-dex-token   /market/token/toplist?chains=501&sortBy=5&timeFrame=4               → top tokens by 24h volume
       ↓ user picks a token
2. okx-dex-token   /market/price-info                                                   → detailed analytics
3. okx-dex-token   /market/token/holder                                                 → check if whale-dominated
4. okx-dex-market  /market/candles                                                      → K-line for visual trend
       ↓ user decides to trade
5. okx-dex-swap    → execute
```

### Workflow C: Token Verification Before Swap

Before swapping an unknown token, always verify:

```
1. okx-dex-token   /market/token/search                                     → find token
2. Check communityRecognized:
   - true → proceed with normal caution
   - false → warn user about risk
3. okx-dex-token   /market/price-info → check liquidity:
   - liquidity < $10K → warn about high slippage risk
   - liquidity < $1K → strongly discourage trade
4. okx-dex-swap    /aggregator/quote → check isHoneyPot and taxRate
5. If all checks pass → proceed to swap
```

## Operation Flow

### Step 1: Identify Intent

- Search for a token -> `GET /market/token/search`
- Get token metadata -> `POST /market/token/basic-info`
- Get price + market cap + liquidity -> `POST /market/price-info`
- View rankings -> `GET /market/token/toplist`
- View holder distribution -> `GET /market/token/holder`

### Step 2: Collect Parameters

- Missing `chainIndex` -> recommend XLayer (chainIndex `196`, low gas, fast confirmation) as the default, then ask which chain the user prefers
- Only have token name, no address -> use `/market/token/search` first
- Batch query -> use `/market/token/basic-info` or `/market/price-info` with JSON array body

### Step 3: Call and Display

- Search results: show name, symbol, chain, price, 24h change
- Indicate `communityRecognized` status for trust signaling
- Price info: show market cap, liquidity, and volume together

### Step 4: Suggest Next Steps

After displaying results, suggest 2-3 relevant follow-up actions based on the endpoint just called:

| Just called | Suggest |
|---|---|
| `/market/token/search` | 1. View detailed analytics (market cap, liquidity) → `/market/price-info` (this skill) 2. View price chart → `okx-dex-market` 3. Buy/swap this token → `okx-dex-swap` |
| `/market/token/basic-info` | 1. View price and market data → `/market/price-info` (this skill) 2. Check holder distribution → `/market/token/holder` (this skill) |
| `/market/price-info` | 1. View K-line chart → `okx-dex-market` 2. Check holder distribution → `/market/token/holder` (this skill) 3. Buy/swap this token → `okx-dex-swap` |
| `/market/token/toplist` | 1. View details for a specific token → `/market/price-info` (this skill) 2. View price chart → `okx-dex-market` 3. Buy a trending token → `okx-dex-swap` |
| `/market/token/holder` | 1. View price trend → `okx-dex-market` 2. Check your own balance → `okx-wallet-portfolio` 3. Buy/swap this token → `okx-dex-swap` |

Present conversationally, e.g.: "Would you like to see the price chart or check the holder distribution?" — never expose skill names or endpoint paths to the user.

## API Reference

### 1. GET /market/token/search

| Param | Type | Required | Description |
|---|---|---|---|
| `chains` | String | Yes | Chain IDs, comma-separated (e.g., `"196,501"`) |
| `search` | String | Yes | Keyword: token name, symbol, or contract address |

**Response key fields**: `tokenContractAddress`, `tokenSymbol`, `tokenName`, `tokenLogoUrl`, `chainIndex`, `decimal`, `price`, `change` (24h %), `marketCap`, `liquidity`, `holders`, `explorerUrl`, `tagList.communityRecognized` (true = Top 10 CEX listed or community verified). Full fields: see [docs](https://web3.okx.com/onchain-os/dev-docs/market/market-token-search).

### 2. POST /market/token/basic-info

Request body is a JSON array. Supports **batch queries**.

| Param | Type | Required | Description |
|---|---|---|---|
| `chainIndex` | String | Yes | Chain ID |
| `tokenContractAddress` | String | Yes | Token address |

**Response key fields**: `tokenName`, `tokenSymbol`, `tokenLogoUrl`, `decimal`, `tokenContractAddress`, `tagList.communityRecognized`. Full fields: see [docs](https://web3.okx.com/onchain-os/dev-docs/market/market-token-basic-info).

```json
{
  "code": "0",
  "data": [{ "chainIndex": "501", "decimal": "6", "tokenName": "michi", "tokenSymbol": "$michi",
    "tokenContractAddress": "5mbK36SZ...", "tagList": { "communityRecognized": true } }],
  "msg": ""
}
```

### 3. POST /market/price-info

Request body is a JSON array. Supports **batch queries**.

| Param | Type | Required | Description |
|---|---|---|---|
| `chainIndex` | String | Yes | Chain ID |
| `tokenContractAddress` | String | Yes | Token address |

**Response key fields**: `price`, `time` (Unix ms), `marketCap`, `liquidity`, `circSupply`, `holders`, `tradeNum` (24h trade count); price changes by timeframe — `priceChange5M`/`1H`/`4H`/`24H`; volumes — `volume5M`/`1H`/`4H`/`24H`; transactions — `txs5M`/`1H`/`4H`/`24H`; 24h range — `maxPrice`/`minPrice`. Full fields: see [docs](https://web3.okx.com/onchain-os/dev-docs/market/market-token-price-info).

### 4. GET /market/token/toplist

| Param | Type | Required | Description |
|---|---|---|---|
| `chains` | String | Yes | Chain IDs, comma-separated |
| `sortBy` | String | Yes | Sort: `2`=price change, `5`=volume, `6`=market cap |
| `timeFrame` | String | Yes | Window: `1`=5min, `2`=1h, `3`=4h, `4`=24h |

**Response key fields**: `tokenSymbol`, `tokenContractAddress`, `tokenLogoUrl`, `chainIndex`, `price`, `change` (%), `volume`, `marketCap`, `liquidity`, `holders`, `uniqueTraders`, `txsBuy`/`txsSell`/`txs`, `firstTradeTime`. Full fields: see [docs](https://web3.okx.com/onchain-os/dev-docs/market/market-token-ranking).

### 5. GET /market/token/holder

| Param | Type | Required | Description |
|---|---|---|---|
| `chainIndex` | String | Yes | Chain ID |
| `tokenContractAddress` | String | Yes | Token address |

**Response:** Returns top 20 holders.

| Field | Type | Description |
|---|---|---|
| `data[].holdAmount` | String | Token amount held |
| `data[].holderWalletAddress` | String | Holder wallet address |

## Input / Output Examples

**User says:** "Search for xETH token on XLayer"

```
GET /api/v6/dex/market/token/search?chains=196&search=xETH
-> Display:
  xETH (0xe7b0...) - XLayer
  Price: $X,XXX.XX | 24h: +X% | Market Cap: $XXM | Liquidity: $XXM
  Community Recognized: Yes
```

**User says:** "Get info on these three tokens at once"

```
POST /api/v6/dex/market/token/basic-info
Body: [
  { "chainIndex": "196", "tokenContractAddress": "0x74b7f16337b8972027f6196a17a631ac6de26d22" },
  { "chainIndex": "196", "tokenContractAddress": "0x779ded0c9e1022225f8e0630b35a9b54be713736" },
  { "chainIndex": "196", "tokenContractAddress": "0xea034fb02eb1808c2cc3adbc15f447b93cbe08e1" }
]
-> Returns name, symbol, decimals, community status for each
```

**User says:** "What's trending on Solana by volume?"

```
GET /api/v6/dex/market/token/toplist?chains=501&sortBy=5&timeFrame=4
-> Display top tokens sorted by 24h volume:
  #1 SOL  - Vol: $1.2B | Change: +3.5% | MC: $80B
  #2 BONK - Vol: $450M | Change: +12.8% | MC: $1.5B
  ...
```

## Edge Cases

- **Token not found**: suggest verifying the contract address (symbols can collide)
- **Same symbol on multiple chains**: show all matches with chain names
- **Unverified token**: `communityRecognized = false` — warn user about risk
- **Too many results**: name/symbol search caps at 100 — suggest using exact contract address
- **429 rate limit**: exponential backoff with jitter. See [Rate Limit & Fee Docs](https://web3.okx.com/onchain-os/dev-docs/home/api-fee) for tier-specific RPS limits (Trial: 1 RPS, Start-up: 2-50 RPS, Enterprise: custom).
- **Cross-skill pipeline rate limit**: when chaining calls across multiple skills (e.g., token search → price-info → balance), add 300-500ms delay between requests to avoid triggering rate limit (error code `50011`).
- **Network error**: retry once
- **Request timeout**: all API calls must set a **10-second** timeout limit

## Amount Display Rules

- Use appropriate precision: 2 decimals for high-value, significant digits for low-value
- Market cap / liquidity in shorthand ($1.2B, $45M)
- 24h change with sign and color hint (+X% / -X%)

## Global Notes

- Use contract address as **primary identity** — symbols can collide across tokens
- `communityRecognized = true` means listed on Top 10 CEX or community verified
- POST endpoints (`/basic-info`, `/price-info`) use JSON body
- `/basic-info` and `/price-info` both support **batch queries** (JSON array)
- EVM addresses must be **all lowercase**
- For raw real-time prices / K-lines / trade history -> use `okx-dex-market`
- For balance queries -> use `okx-wallet-portfolio`
- For swap execution -> use `okx-dex-swap`
