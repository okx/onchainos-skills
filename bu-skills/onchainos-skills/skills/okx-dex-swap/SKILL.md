---
name: okx-dex-swap
description: "This skill should be used when the user asks to 'swap tokens', 'trade OKB for USDC', 'buy tokens', 'sell tokens', 'exchange crypto', 'convert tokens', 'swap SOL for USDC', 'get a swap quote', 'execute a trade', 'find the best swap route', 'cheapest way to swap', 'optimal swap', 'compare swap rates', or mentions swapping, trading, buying, selling, or exchanging tokens on XLayer, Solana, Ethereum, Base, BSC, Arbitrum, Polygon, or any of 20+ supported chains. Aggregates liquidity from 500+ DEX sources for optimal routing and price. Supports slippage control, price impact protection, and cross-DEX route optimization. Do NOT use for general programming questions about swap code, or for analytical questions about historical swap volume."
license: Apache-2.0
metadata:
  author: okx
  version: "1.0.0"
  homepage: "https://web3.okx.com"
---

# OKX DEX Aggregator API

6 endpoints for multi-chain swap aggregation — quote, approve, and execute.

**Base URL**: `https://web3.okx.com`

**Base path**: `/api/v6/dex/aggregator`

**Auth**: HMAC-SHA256 signature, 4 headers required (`OK-ACCESS-KEY`, `OK-ACCESS-SIGN`, `OK-ACCESS-PASSPHRASE`, `OK-ACCESS-TIMESTAMP`)

**Note**: All aggregator endpoints are **GET** requests.

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

// Signature rule (all aggregator endpoints are GET):
//   GET  → body = "", requestPath includes query string (e.g., "/api/v6/dex/aggregator/quote?chainIndex=196&...")
//   POST → body = JSON string of request body, requestPath is path only (not used in this skill)
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

### EVM Swap (quote → approve → swap)

```typescript
// 1. Quote — sell 100 USDC for OKB on XLayer
const params = new URLSearchParams({
  chainIndex: '196', fromTokenAddress: '0x74b7f16337b8972027f6196a17a631ac6de26d22', // USDC
  toTokenAddress: '0xeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee',   // native OKB
  amount: '100000000', swapMode: 'exactIn', // 100 USDC (6 decimals)
});
const quote = await okxFetch('GET', `/api/v6/dex/aggregator/quote?${params}`);
console.log(`Expected: ${quote[0].toTokenAmount} OKB (minimal units)`);

// 2. Approve — ERC-20 tokens need approval before swap (skip for native OKB)
const approveParams = new URLSearchParams({
  chainIndex: '196', tokenContractAddress: '0x74b7f16337b8972027f6196a17a631ac6de26d22',
  approveAmount: '100000000',
});
const approve = await okxFetch('GET', `/api/v6/dex/aggregator/approve-transaction?${approveParams}`);
// → build tx: { to: tokenContractAddress, data: approve[0].data }, sign & send
// approve[0].dexContractAddress is the spender (already encoded in calldata), NOT the tx target

// 3. Swap
const swapParams = new URLSearchParams({
  chainIndex: '196', fromTokenAddress: '0x74b7f16337b8972027f6196a17a631ac6de26d22',
  toTokenAddress: '0xeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee',
  amount: '100000000', slippagePercent: '1',
  userWalletAddress: '0xYourWallet', swapMode: 'exactIn',
});
const swap = await okxFetch('GET', `/api/v6/dex/aggregator/swap?${swapParams}`);
// → sign & send swap[0].tx { from, to, data, value, gas }
```

### Solana Swap (swap-instruction)

```typescript
const params = new URLSearchParams({
  chainIndex: '501', fromTokenAddress: '11111111111111111111111111111111', // native SOL
  toTokenAddress: 'DezXAZ8z7PnrnRJjz3wXBoRgixCa6xjnB7YaB1pPB263', // BONK
  amount: '1000000000', slippagePercent: '1', userWalletAddress: 'YourSolanaWallet',
});
const result = await okxFetch('GET', `/api/v6/dex/aggregator/swap-instruction?${params}`);
// → result[0].instructionLists: assemble into VersionedTransaction, sign & send
```

## Common Chain IDs

| Chain | chainIndex | Chain | chainIndex |
|---|---|---|---|
| XLayer | `196` | Base | `8453` |
| Solana | `501` | BSC | `56` |
| Ethereum | `1` | Arbitrum | `42161` |

## Native Token Addresses

> **CRITICAL**: Each chain has a specific native token address for use in OKX DEX API. Using the wrong address (e.g., wSOL SPL token address instead of the Solana system program address) will cause swap transactions to fail. Reference: [DEX Aggregation FAQ](https://web3.okx.com/onchain-os/dev-docs/trade/dex-aggregation-faq)

| Chain | Native Token Address |
|---|---|
| EVM (Ethereum, BSC, Polygon, Arbitrum, Base, etc.) | `0xeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee` |
| Solana | `11111111111111111111111111111111` |
| Sui | `0x2::sui::SUI` |
| Tron | `T9yD14Nj9j7xAB4dbGeiX9h8unkKHxuWwb` |
| Ton | `EQAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAM9c` |

> **WARNING — Solana native SOL**: The correct address is `11111111111111111111111111111111` (Solana system program). Do **NOT** use `So11111111111111111111111111111111111111112` (wSOL SPL token) — it is a different token and will cause swap failures (`custom program error: 0xb`).

## Endpoint Index

| # | Method | Path | Docs |
|---|---|---|---|
| 1 | GET | `/api/v6/dex/aggregator/supported/chain` | [dex-get-aggregator-supported-chains](https://web3.okx.com/onchain-os/dev-docs/trade/dex-get-aggregator-supported-chains) |
| 2 | GET | `/api/v6/dex/aggregator/get-liquidity` | [dex-get-liquidity](https://web3.okx.com/onchain-os/dev-docs/trade/dex-get-liquidity) |
| 3 | GET | `/api/v6/dex/aggregator/approve-transaction` | [dex-approve-transaction](https://web3.okx.com/onchain-os/dev-docs/trade/dex-approve-transaction) |
| 4 | GET | `/api/v6/dex/aggregator/quote` | [dex-get-quote](https://web3.okx.com/onchain-os/dev-docs/trade/dex-get-quote) |
| 5 | GET | `/api/v6/dex/aggregator/swap-instruction` | [dex-solana-swap-instruction](https://web3.okx.com/onchain-os/dev-docs/trade/dex-solana-swap-instruction) |
| 6 | GET | `/api/v6/dex/aggregator/swap` | [dex-swap](https://web3.okx.com/onchain-os/dev-docs/trade/dex-swap) |

Error Codes: [DEX Error Codes](https://web3.okx.com/onchain-os/dev-docs/trade/dex-error-code)

## Cross-Skill Workflows

This skill is the **execution endpoint** of most user trading flows. It almost always needs input from other skills first.

### Workflow A: Full Swap by Token Name (most common)

> User: "Swap 1 SOL for BONK on Solana"

```
1. okx-dex-token    /market/token/search?search=BONK&chains=501              → get BONK tokenContractAddress
       ↓ tokenContractAddress
2. okx-wallet-portfolio  /balance/all-token-balances-by-address                    → verify SOL balance >= 1
       ↓ sufficient balance confirmed
3. okx-dex-swap     /aggregator/quote                                         → get quote (show expected output, gas, price impact)
       ↓ user confirms
4. okx-dex-swap     /aggregator/swap-instruction                              → get serialized instruction (Solana)
5. User signs & sends tx (or use `okx-onchain-gateway` to broadcast via OKX nodes)
```

**Data handoff**:
- `tokenContractAddress` from step 1 → `toTokenAddress` in steps 3-4
- SOL native address = `"11111111111111111111111111111111"` (Solana system program) → `fromTokenAddress`. Do NOT use `So11111111111111111111111111111111111111112` (wSOL) — see [Native Token Addresses](#native-token-addresses).
- Amount `1 SOL` = `"1000000000"` (9 decimals) → `amount` param
- `balance` from step 2 is UI units; swap needs **minimal units** → multiply by `10^decimal`

### Workflow B: EVM Swap with Approval

> User: "Swap 100 USDC for OKB on XLayer"

```
1. okx-dex-token    /market/token/search?search=USDC&chains=196              → get USDC address
2. okx-wallet-portfolio  /balance/token-balances-by-address                        → verify USDC balance >= 100
3. okx-dex-swap     /aggregator/quote                                         → get quote
       ↓ check isHoneyPot, taxRate, priceImpactPercent
4. okx-dex-swap     /aggregator/approve-transaction                           → get ERC-20 approval calldata
5. User signs & sends approval tx
6. okx-dex-swap     /aggregator/swap                                          → get swap calldata
7. User signs & sends swap tx (or use `okx-onchain-gateway` to broadcast via OKX nodes)
```

**Key**: EVM tokens (not native OKB) require an **approve** step. Skip it if user is selling native OKB.

### Workflow C: Compare Quote Then Execute

```
1. okx-dex-swap     /aggregator/quote                                         → get quote with route info
2. Display to user: expected output, gas, price impact, route
3. If price impact > 5% → warn user
4. If isHoneyPot = true → block trade, warn user
5. User confirms → proceed to approve (if EVM) → swap
```

## Swap Flow

### EVM Chains (XLayer, Ethereum, BSC, Base, etc.)

```
1. GET /aggregator/quote               -> Get price and route
2. GET /aggregator/approve-transaction  -> Get approval calldata (if needed)
3. User signs & sends approval tx
4. GET /aggregator/swap                 -> Get swap calldata
5. User signs & sends swap tx
```

### Solana

```
1. GET /aggregator/quote               -> Get price and route
2. GET /aggregator/swap-instruction     -> Get serialized instruction
3. User signs & sends tx
```

## Operation Flow

### Step 1: Identify Intent

- View a quote -> `GET /aggregator/quote`
- Execute a swap -> full swap flow (quote -> approve -> swap)
- List available DEXes -> `GET /aggregator/get-liquidity`
- Approve a token -> `GET /aggregator/approve-transaction`

### Step 2: Collect Parameters

- Missing `chainIndex` -> recommend XLayer (chainIndex `196`, low gas, fast confirmation) as the default, then ask which chain the user prefers
- Missing token addresses -> use `okx-dex-token` `/market/token/search` to resolve name → address
- Missing amount -> ask user, remind to convert to minimal units
- Missing slippage -> suggest 1% default, 3-5% for volatile tokens
- Missing wallet address -> ask user

### Step 3: Execute

- **Quote phase**: call `/quote`, display estimated results
  - Expected output, gas estimate, price impact, routing path
  - Check `isHoneyPot` and `taxRate` — surface safety info to users
- **Confirmation phase**: wait for user approval before proceeding
- **Approval phase** (EVM only): check/execute approve if selling non-native token
- **Execution phase**: call `/swap` (EVM) or `/swap-instruction` (Solana), return tx data for signing

### Step 4: Suggest Next Steps

After displaying results, suggest 2-3 relevant follow-up actions based on the swap phase just completed:

| Just completed | Suggest |
|---|---|
| `/aggregator/quote` (quote only, not yet confirmed) | 1. Check wallet balance first → `okx-wallet-portfolio` 2. View price chart before deciding → `okx-dex-market` 3. Proceed with swap → continue approve + swap (this skill) |
| Swap executed successfully | 1. Verify updated balance → `okx-wallet-portfolio` 2. Check price of the token just received → `okx-dex-market` 3. Swap another token → new swap flow (this skill) |
| `/aggregator/get-liquidity` | 1. Get a swap quote → `/aggregator/quote` (this skill) |

Present conversationally, e.g.: "Swap complete! Would you like to check your updated balance?" — never expose skill names or endpoint paths to the user.

## API Reference

### 1. GET /aggregator/supported/chain

| Param | Type | Required | Description |
|---|---|---|---|
| `chainIndex` | String | No | Filter to a specific chain (e.g., `"196"`) |

**Response:**

| Field | Type | Description |
|---|---|---|
| `data[].chainIndex` | String | Chain unique identifier (e.g., "196") |
| `data[].chainName` | String | Chain name (e.g., "XLayer") |
| `data[].dexTokenApproveAddress` | String | OKX DEX token approve contract address |

### 2. GET /aggregator/get-liquidity

| Param | Type | Required | Description |
|---|---|---|---|
| `chainIndex` | String | Yes | Chain ID |

**Response:**

| Field | Type | Description |
|---|---|---|
| `data[].id` | String | Liquidity pool ID |
| `data[].name` | String | Pool name (e.g., "Uniswap V3") |
| `data[].logo` | String | Pool logo URL |

### 3. GET /aggregator/approve-transaction

| Param | Type | Required | Description |
|---|---|---|---|
| `chainIndex` | String | Yes | Chain ID |
| `tokenContractAddress` | String | Yes | Token to approve |
| `approveAmount` | String | Yes | Amount in minimal units |

**Response:**

| Field | Type | Description |
|---|---|---|
| `data[].data` | String | Approval calldata (`approve(dexContractAddress, amount)`) |
| `data[].dexContractAddress` | String | DEX router address (the spender, already encoded in calldata). **NOT the tx `to`** — send tx to the token contract |
| `data[].gasLimit` | String | Gas limit. May underestimate — use simulation or ×1.5 |
| `data[].gasPrice` | String | Gas price in wei |

### 4. GET /aggregator/quote

| Param | Type | Required | Description |
|---|---|---|---|
| `chainIndex` | String | Yes | Chain ID |
| `amount` | String | Yes | Amount in minimal units (sell amount if exactIn, buy amount if exactOut) |
| `swapMode` | String | Yes | `exactIn` (default) or `exactOut` |
| `fromTokenAddress` | String | Yes | Token to sell |
| `toTokenAddress` | String | Yes | Token to buy |

Optional params: `dexIds`, `directRoute`, `priceImpactProtectionPercent` (default 90%), `feePercent` (max 10 Solana, 3 others).

**Response key fields**: `toTokenAmount` (output minimal units), `fromTokenAmount`, `estimateGasFee`, `tradeFee` (USD estimate), `priceImpactPercent`, `router`, `dexRouterList`, `fromToken`/`toToken` (each has `isHoneyPot`, `taxRate`, `decimal`, `tokenUnitPrice`). Full fields: see [docs](https://web3.okx.com/onchain-os/dev-docs/trade/dex-get-quote).

### 5. GET /aggregator/swap-instruction (Solana only)

| Param | Type | Required | Description |
|---|---|---|---|
| `chainIndex` | String | Yes | Must be `"501"` (Solana) |
| `amount` | String | Yes | Amount in minimal units |
| `fromTokenAddress` | String | Yes | Token to sell |
| `toTokenAddress` | String | Yes | Token to buy |
| `userWalletAddress` | String | Yes | User's wallet |
| `slippagePercent` | String | Yes | 0 to <100 |

Optional params: `autoSlippage`, `computeUnitPrice`, `computeUnitLimit`, `dexIds`, `swapReceiverAddress`, `feePercent`, `priceImpactProtectionPercent`.

**Response key fields**: `instructionLists[]` (each has `data`, `accounts`, `programId`), `addressLookupTableAccount`, `routerResult` (same as /quote), `tx` (`minReceiveAmount`, `slippagePercent`), `wsolRentFee`. Full fields: see [docs](https://web3.okx.com/onchain-os/dev-docs/trade/dex-solana-swap-instruction).

### 6. GET /aggregator/swap (EVM + Solana)

> Note: This endpoint works for **all chains** including Solana. `/swap-instruction` is a Solana-specific alternative that returns deserialized instructions instead of a serialized transaction.

| Param | Type | Required | Description |
|---|---|---|---|
| `chainIndex` | String | No | Chain ID. Technically optional but **strongly recommended** — always pass it. |
| `amount` | String | Yes | Amount in minimal units |
| `swapMode` | String | Yes | `exactIn` (default) or `exactOut` |
| `fromTokenAddress` | String | Yes | Token to sell |
| `toTokenAddress` | String | Yes | Token to buy |
| `slippagePercent` | String | Yes | 0-100 (EVM), 0-<100 (Solana) |
| `userWalletAddress` | String | Yes | User's wallet |

Optional params: `gasLevel` (`average`/`fast`/`slow`), `computeUnitPrice`, `computeUnitLimit`, `tips` (Jito, 0.0000000001-2 SOL; use `0` if `computeUnitPrice` is set), `dexIds`, `autoSlippage`, `maxAutoSlippagePercent`, `swapReceiverAddress`, `feePercent`, `priceImpactProtectionPercent` (default 90%), `approveTransaction` (Boolean, combine approve+swap in one call), `approveAmount` (used with `approveTransaction`).

**Response key fields**: `routerResult` (same as /quote), `tx` — `from`, `to`, `data` (hex-encoded for EVM, base58-encoded for Solana), `gas`, `gasPrice`, `maxPriorityFeePerGas` (EIP-1559), `value`, `minReceiveAmount`, `maxSpendAmount`, `slippagePercent`, `signatureData`. Full fields: see [docs](https://web3.okx.com/onchain-os/dev-docs/trade/dex-swap).

## Input / Output Examples

**User says:** "Swap 100 USDC for OKB on XLayer"

```
1. GET /api/v6/dex/aggregator/quote?chainIndex=196&fromTokenAddress=0x74b7...&toTokenAddress=0xeeee...&amount=100000000&swapMode=exactIn
-> Display:
  Expected output: 3.2 OKB
  Gas fee: ~$0.001
  Price impact: 0.05%
  Route: USDC -> OKB (CurveNG)

2. User confirms

3. GET /api/v6/dex/aggregator/approve-transaction?chainIndex=196&tokenContractAddress=0x74b7...&approveAmount=100000000
-> Returns approval calldata and spender address
-> Build tx: { to: "0x74b7..." (token contract), data: response.data } — sign & send

4. GET /api/v6/dex/aggregator/swap?chainIndex=196&...&slippagePercent=1&userWalletAddress=0x...
-> Returns tx: { from, to, data, gas, gasPrice, value, minReceiveAmount }
-> User signs and broadcasts
```

**User says:** "What DEXes are available on XLayer?"

```
GET /api/v6/dex/aggregator/get-liquidity?chainIndex=196
-> Display: CurveNG, XLayer DEX, ... (DEX sources on XLayer)
```

## Edge Cases

- **High slippage (>5%)**: warn user, suggest splitting the trade or adjusting slippage
- **Large price impact (>10%)**: strongly warn, suggest reducing amount
- **Honeypot token**: `isHoneyPot = true` — block trade and warn user
- **Tax token**: `taxRate` non-zero — display to user (e.g. 5% buy tax)
- **Insufficient balance**: use `okx-wallet-portfolio` to check first, show current balance, suggest adjusting amount
- **exactOut not supported**: only Ethereum/Base/BSC/Arbitrum — prompt user to use `exactIn`
- **Solana chain**: `/swap-instruction` returns deserialized instructions (more control); `/swap` also works but returns a serialized transaction
- **Solana native SOL address**: Must use `11111111111111111111111111111111` (system program), NOT `So11111111111111111111111111111111111111112` (wSOL). Using wSOL address causes `custom program error: 0xb` on-chain failures. See [DEX Aggregation FAQ](https://web3.okx.com/onchain-os/dev-docs/trade/dex-aggregation-faq).
- **429 rate limit**: exponential backoff with jitter. See [Rate Limit & Fee Docs](https://web3.okx.com/onchain-os/dev-docs/home/api-fee) for tier-specific RPS limits (Trial: 1 RPS, Start-up: 2-50 RPS, Enterprise: custom).
- **Cross-skill pipeline rate limit**: when chaining calls across multiple skills (e.g., token search → balance check → swap), add 300-500ms delay between requests to avoid triggering rate limit (error code `50011`).
- **Network error**: retry once, then prompt user to try again later
- **Request timeout**: all API calls must set a **10-second** timeout limit

## Amount Display Rules

- Input/output amounts in UI units (`1.5 ETH`, `3,200 USDC`)
- Internal API params use minimal units (`1 USDC` = `"1000000"`, `1 ETH` = `"1000000000000000000"`)
- Gas fees in USD
- `minReceiveAmount` in both UI units and USD
- Price impact as percentage

## Global Notes

- **All endpoints are GET** — no POST in the aggregator family
- Amounts must be in **minimal units** (wei/lamports)
- `exactOut` only on Ethereum(`1`)/Base(`8453`)/BSC(`56`)/Arbitrum(`42161`)
- `/swap-instruction` is **Solana-only**
- Check `isHoneyPot` and `taxRate` — surface safety info to users
- Solana referrer wallets require SOL deposit activation
- TON chain has limited commission pool support (excludes Stonfi V1)
- EVM contract addresses must be **all lowercase**
- For token search -> use `okx-dex-token`
- For market prices -> use `okx-dex-market`
- For balance queries -> use `okx-wallet-portfolio`
- For transaction broadcasting -> use `okx-onchain-gateway`
