---
name: okx-onchain-gateway
description: "This skill should be used when the user asks to 'broadcast transaction', 'send tx', 'estimate gas', 'simulate transaction', 'check tx status', 'track my transaction', 'get gas price', 'gas limit', 'broadcast signed tx', or mentions broadcasting transactions, sending transactions on-chain, gas estimation, transaction simulation, tracking broadcast orders, or checking transaction status. Covers gas price, gas limit estimation, transaction simulation, transaction broadcasting, and order tracking across XLayer, Solana, Ethereum, Base, BSC, Arbitrum, Polygon, and 20+ other chains. Do NOT use for swap quote or execution — use okx-dex-swap instead. Do NOT use for general programming questions about transaction handling."
license: Apache-2.0
metadata:
  author: okx
  version: "1.0.0"
  homepage: "https://web3.okx.com"
---

# OKX Onchain Gateway API

6 endpoints for gas estimation, transaction simulation, broadcasting, and order tracking.

**Base URL**: `https://web3.okx.com`

**Base paths**: `/api/v6/dex/pre-transaction` and `/api/v6/dex/post-transaction`

**Auth**: HMAC-SHA256 signature, 4 headers required (`OK-ACCESS-KEY`, `OK-ACCESS-SIGN`, `OK-ACCESS-PASSPHRASE`, `OK-ACCESS-TIMESTAMP`)

**Note**: Endpoints #1-2 are **GET** requests, endpoints #3-5 are **POST** requests, endpoint #6 is **GET**.

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
//   GET  → body = "", requestPath includes query string (e.g., "/api/v6/dex/pre-transaction/gas-price?chainIndex=196")
//   POST → body = JSON string of request body, requestPath is path only (e.g., "/api/v6/dex/pre-transaction/gas-limit")
//   Note: Endpoints #1-2, #6 are GET; endpoints #3-5 are POST
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

### Get Gas Price

```typescript
// Get current gas price on XLayer
const gasPrice = await okxFetch('GET', '/api/v6/dex/pre-transaction/gas-price?' + new URLSearchParams({
  chainIndex: '196',
}));
// → gasPrice[].normal, gasPrice[].min, gasPrice[].max (legacy)
// → gasPrice[].eip1559Protocol.suggestBaseFee, .proposePriorityFee (EIP-1559)
console.log(`Gas price: ${gasPrice[0].eip1559Protocol?.suggestBaseFee ?? gasPrice[0].normal} wei`);
```

### Broadcast a Signed Transaction

```typescript
// Broadcast a signed EVM transaction
const result = await okxFetch('POST', '/api/v6/dex/pre-transaction/broadcast-transaction', {
  signedTx: '0xf86c...signed_hex',
  chainIndex: '196',
  address: '0xYourWallet',
});
// → result[].orderId — use with /post-transaction/orders to track
console.log(`Broadcast success, orderId: ${result[0].orderId}`);
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
| 1 | GET | `/api/v6/dex/pre-transaction/supported/chain` | [onchain-gateway-api-chains](https://web3.okx.com/onchain-os/dev-docs/trade/onchain-gateway-api-chains) |
| 2 | GET | `/api/v6/dex/pre-transaction/gas-price` | [onchain-gateway-api-gas-price](https://web3.okx.com/onchain-os/dev-docs/trade/onchain-gateway-api-gas-price) |
| 3 | POST | `/api/v6/dex/pre-transaction/gas-limit` | [onchain-gateway-api-gas-limit](https://web3.okx.com/onchain-os/dev-docs/trade/onchain-gateway-api-gas-limit) |
| 4 | POST | `/api/v6/dex/pre-transaction/simulate` | [onchain-gateway-api-simulate-transaction](https://web3.okx.com/onchain-os/dev-docs/trade/onchain-gateway-api-simulate-transaction) |
| 5 | POST | `/api/v6/dex/pre-transaction/broadcast-transaction` | [onchain-gateway-api-broadcast-transaction](https://web3.okx.com/onchain-os/dev-docs/trade/onchain-gateway-api-broadcast-transaction) |
| 6 | GET | `/api/v6/dex/post-transaction/orders` | [onchain-gateway-api-orders](https://web3.okx.com/onchain-os/dev-docs/trade/onchain-gateway-api-orders) |

Error Codes: [Onchain Gateway Error Codes](https://web3.okx.com/onchain-os/dev-docs/trade/onchain-gateway-error-code)

## Cross-Skill Workflows

This skill is the **final mile** — it takes a signed transaction and sends it on-chain. It pairs with swap (to get tx data) and balance (to verify funds).

### Workflow A: Swap → Broadcast → Track

> User: "Swap 1 ETH for USDC and broadcast it"

```
1. okx-dex-swap    /aggregator/swap                                → get tx calldata { from, to, data, value, gas }
       ↓ user signs the tx locally
2. okx-onchain-gateway /pre-transaction/broadcast-transaction      → broadcast signed tx → orderId
3. okx-onchain-gateway /post-transaction/orders                    → track order status until confirmed
```

**Data handoff**:
- `tx.data`, `tx.to`, `tx.value`, `tx.gas` from swap → user builds & signs → `signedTx` for broadcast
- `orderId` from broadcast → `orderId` param in orders query

### Workflow B: Simulate → Broadcast → Track

> User: "Simulate this transaction first, then broadcast if safe"

```
1. okx-onchain-gateway /pre-transaction/simulate                   → check if tx will succeed
       ↓ simulation passes (no revert)
2. okx-onchain-gateway /pre-transaction/broadcast-transaction      → broadcast signed tx
3. okx-onchain-gateway /post-transaction/orders                    → track order status
```

### Workflow C: Balance Check → Swap → Broadcast

> User: "Check if I have enough ETH, swap for USDC, then send it"

```
1. okx-wallet-portfolio /balance/token-balances-by-address         → verify ETH balance
       ↓ sufficient balance confirmed
2. okx-dex-swap    /aggregator/swap                                → get swap tx calldata
       ↓ user signs
3. okx-onchain-gateway /pre-transaction/broadcast-transaction      → broadcast
4. okx-onchain-gateway /post-transaction/orders                    → track
```

## Operation Flow

### Step 1: Identify Intent

- Estimate gas for a chain → `GET /pre-transaction/gas-price`
- Estimate gas limit for a specific tx → `POST /pre-transaction/gas-limit`
- Test if a tx will succeed → `POST /pre-transaction/simulate`
- Broadcast a signed tx → `POST /pre-transaction/broadcast-transaction`
- Track a broadcast order → `GET /post-transaction/orders`
- Check supported chains → `GET /pre-transaction/supported/chain`

### Step 2: Collect Parameters

- Missing `chainIndex` → recommend XLayer (chainIndex `196`, low gas, fast confirmation) as the default, then ask which chain the user prefers
- Missing `signedTx` → remind user to sign the transaction first (this API does NOT sign)
- Missing wallet `address` → ask user
- For gas-limit / simulate → need `fromAddress`, `toAddress`, `txAmount`, `extJson.inputData`
- For orders query → need `address` and `chainIndex`, optionally `orderId`

### Step 3: Execute

- **Gas estimation**: call `/gas-price` or `/gas-limit`, display results
- **Simulation**: call `/simulate`, check for revert or success
- **Broadcast**: call `/broadcast-transaction` with signed tx, return `orderId`
- **Tracking**: call `/orders`, display order status

### Step 4: Suggest Next Steps

After displaying results, suggest 2-3 relevant follow-up actions based on the endpoint just called:

| Just completed | Suggest |
|---|---|
| `/pre-transaction/gas-price` | 1. Estimate gas limit for a specific tx → `/pre-transaction/gas-limit` (this skill) 2. Get a swap quote → `okx-dex-swap` 3. Check wallet balance → `okx-wallet-portfolio` |
| `/pre-transaction/gas-limit` | 1. Simulate the transaction → `/pre-transaction/simulate` (this skill) 2. Proceed to broadcast → `/pre-transaction/broadcast-transaction` (this skill) |
| `/pre-transaction/simulate` | 1. Broadcast the transaction → `/pre-transaction/broadcast-transaction` (this skill) 2. Adjust and re-simulate if failed → `/pre-transaction/simulate` (this skill) |
| `/pre-transaction/broadcast-transaction` | 1. Track order status → `/post-transaction/orders` (this skill) 2. Check updated wallet balance → `okx-wallet-portfolio` |
| `/post-transaction/orders` | 1. Check wallet balance after confirmation → `okx-wallet-portfolio` 2. View price of received token → `okx-dex-market` 3. Execute another swap → `okx-dex-swap` |

Present conversationally, e.g.: "Transaction broadcast! Would you like to track the order status?" — never expose skill names or endpoint paths to the user.

## API Reference

### 1. GET /pre-transaction/supported/chain

No request parameters.

**Response:**

| Field | Type | Description |
|---|---|---|
| `data[].chainIndex` | String | Chain unique identifier (e.g., "196") |
| `data[].name` | String | Chain name (e.g., "XLayer") |
| `data[].logoUrl` | String | Chain logo URL |
| `data[].shortName` | String | Chain short name (e.g., "ETH") |

### 2. GET /pre-transaction/gas-price

| Param | Type | Required | Description |
|---|---|---|---|
| `chainIndex` | String | Yes | Chain ID (e.g., `"196"`) |

**Response:**

| Field | Type | Description |
|---|---|---|
| `data[].normal` | String | Normal gas price (legacy) |
| `data[].min` | String | Minimum gas price |
| `data[].max` | String | Maximum gas price |
| `data[].supporteip1559` | Boolean | Whether EIP-1559 is supported |
| `data[].eip1559Protocol.suggestBaseFee` | String | Suggested base fee |
| `data[].eip1559Protocol.baseFee` | String | Current base fee |
| `data[].eip1559Protocol.proposePriorityFee` | String | Proposed priority fee |
| `data[].eip1559Protocol.safePriorityFee` | String | Safe (slow) priority fee |
| `data[].eip1559Protocol.fastPriorityFee` | String | Fast priority fee |

For Solana chains, response includes: `proposePriorityFee`, `safePriorityFee`, `fastPriorityFee`, `extremePriorityFee`.

```json
{
  "code": "0",
  "data": [{
    "normal": "20000000000",
    "min": "15000000000",
    "max": "30000000000",
    "supporteip1559": true,
    "eip1559Protocol": {
      "suggestBaseFee": "18000000000",
      "baseFee": "18000000000",
      "proposePriorityFee": "2000000000",
      "safePriorityFee": "1000000000",
      "fastPriorityFee": "3000000000"
    }
  }],
  "msg": ""
}
```

### 3. POST /pre-transaction/gas-limit

| Param | Type | Required | Description |
|---|---|---|---|
| `chainIndex` | String | Yes | Chain ID |
| `fromAddress` | String | Yes | Sender address |
| `toAddress` | String | Yes | Recipient / contract address |
| `txAmount` | String | No | Transfer value in minimal units (default "0") |
| `extJson` | Object | No | Extended parameters |
| `extJson.inputData` | String | No | Encoded calldata (for contract interactions) |

**Response:**

| Field | Type | Description |
|---|---|---|
| `data[].gasLimit` | String | Estimated gas limit |

```json
{
  "code": "0",
  "data": [{ "gasLimit": "21000" }],
  "msg": ""
}
```

### 4. POST /pre-transaction/simulate

| Param | Type | Required | Description |
|---|---|---|---|
| `chainIndex` | String | Yes | Chain ID |
| `fromAddress` | String | Yes | Sender address |
| `toAddress` | String | Yes | Recipient / contract address |
| `txAmount` | String | No | Transfer value in minimal units |
| `gasPrice` | String | No | Gas price in wei (for legacy EVM txs) |
| `priorityFee` | String | No | Priority fee in micro-lamports (Solana only) |
| `extJson` | Object | Yes | Extended parameters |
| `extJson.inputData` | String | Yes | Encoded calldata |

**Response:**

| Field | Type | Description |
|---|---|---|
| `data[].intention` | String | Transaction intent description |
| `data[].assetChange[]` | Array | Asset changes from the simulation |
| `data[].assetChange[].assetType` | String | Asset type |
| `data[].assetChange[].name` | String | Token name |
| `data[].assetChange[].symbol` | String | Token symbol |
| `data[].assetChange[].decimals` | Number | Token decimals |
| `data[].assetChange[].address` | String | Token contract address |
| `data[].assetChange[].rawValue` | String | Raw amount change |
| `data[].gasUsed` | String | Gas consumed in simulation |
| `data[].failReason` | String | Failure reason (empty string = success) |
| `data[].risks[]` | Array | Risk information (address, addressType) |

```json
{
  "code": "0",
  "data": [{
    "intention": "Token Swap",
    "assetChange": [{"assetType": "token", "name": "USDC", "symbol": "USDC", "decimals": 6, "address": "0xa0b8...", "rawValue": "-100000000"}],
    "gasUsed": "145000",
    "failReason": "",
    "risks": []
  }],
  "msg": ""
}
```

### 5. POST /pre-transaction/broadcast-transaction

| Param | Type | Required | Description |
|---|---|---|---|
| `signedTx` | String | Yes | Fully signed transaction (hex for EVM, base58 for Solana) |
| `chainIndex` | String | Yes | Chain ID |
| `address` | String | Yes | Sender wallet address |
| `extraData` | String | No | JSON string of extra options (must `JSON.stringify` before sending) |

`extraData` JSON fields:

| Field | Type | Description |
|---|---|---|
| `enableMevProtection` | Boolean | Enable MEV protection (ETH/BSC/SOL/BASE) |
| `jitoSignedTx` | String | Jito signed transaction (Solana) |

**Response:**

| Field | Type | Description |
|---|---|---|
| `data[].orderId` | String | OKX order tracking ID |
| `data[].txHash` | String | On-chain transaction hash |

```json
{
  "code": "0",
  "data": [{ "orderId": "123456789", "txHash": "0xabc...def" }],
  "msg": ""
}
```

### 6. GET /post-transaction/orders

| Param | Type | Required | Description |
|---|---|---|---|
| `address` | String | Yes | Wallet address |
| `chainIndex` | String | Yes | Chain ID |
| `orderId` | String | No | Specific order ID (from broadcast response) |
| `txStatus` | String | No | Filter by status: `1`=Pending, `2`=Success, `3`=Failed |
| `cursor` | String | No | Pagination cursor |
| `limit` | String | No | Max results per page (default 20) |

**Response:**

| Field | Type | Description |
|---|---|---|
| `data[].cursor` | String | Pagination cursor for next page |
| `data[].orders[]` | Array | Order list (nested under each data element) |
| `data[].orders[].orderId` | String | Order ID |
| `data[].orders[].txHash` | String | On-chain transaction hash |
| `data[].orders[].chainIndex` | String | Chain ID |
| `data[].orders[].address` | String | Sender address |
| `data[].orders[].txStatus` | String | Order status: `1`=Pending, `2`=Success, `3`=Failed |
| `data[].orders[].failReason` | String | Failure reason (if failed) |

```json
{
  "code": "0",
  "data": [{
    "cursor": "1",
    "orders": [{
      "orderId": "123456789",
      "txHash": "0xabc...def",
      "chainIndex": "196",
      "address": "0x...",
      "txStatus": "2",
      "failReason": ""
    }]
  }],
  "msg": ""
}
```

## Input / Output Examples

**User says:** "What's the current gas price on XLayer?"

```
GET /api/v6/dex/pre-transaction/gas-price?chainIndex=196
-> Display:
  Base fee: 0.05 Gwei
  Max fee: 0.1 Gwei
  Priority fee: 0.01 Gwei
```

**User says:** "Simulate this swap transaction before I send it"

```
POST /api/v6/dex/pre-transaction/simulate
Body: {
  "chainIndex": "196",
  "fromAddress": "0xYourWallet",
  "toAddress": "0xDexContract",
  "txAmount": "1000000000000000000",
  "extJson": { "inputData": "0x..." }
}
-> Display:
  Simulation: SUCCESS (failReason is empty)
  Estimated gas: 145,000
  Intent: Token Swap
```

**User says:** "Broadcast my signed transaction"

```
POST /api/v6/dex/pre-transaction/broadcast-transaction
Body: {
  "signedTx": "0xf86c...signed",
  "chainIndex": "196",
  "address": "0xYourWallet"
}
-> Display:
  Broadcast successful!
  Order ID: 123456789
  Tx Hash: 0xabc...def
```

**User says:** "Check the status of my broadcast order"

```
GET /api/v6/dex/post-transaction/orders?address=0xYourWallet&chainIndex=196&orderId=123456789
-> Response: data[0].orders[0] contains order details
-> Display:
  Order 123456789: Success (txStatus=2)
  Tx Hash: 0xabc...def
  Confirmed on-chain
```

## Edge Cases

- **MEV protection**: Broadcasting through OKX nodes may offer MEV protection on supported chains — confirm with OKX documentation for chain-specific behavior.
- **Solana special handling**: Solana signed transactions use **base58** encoding (not hex). Ensure the `signedTx` format matches the chain.
- **Chain not supported (81104)**: call `/pre-transaction/supported/chain` first to verify. Not all chains support all endpoints.
- **Node return failed (81451)**: the underlying blockchain node rejected the transaction. Common causes: insufficient gas, nonce too low, contract revert. Retry with corrected parameters.
- **Wallet type mismatch (81108)**: the address format does not match the chain (e.g., EVM address on Solana chain).
- **429 rate limit**: exponential backoff with jitter. See [Rate Limit & Fee Docs](https://web3.okx.com/onchain-os/dev-docs/home/api-fee) for tier-specific RPS limits (Trial: 1 RPS, Start-up: 2-50 RPS, Enterprise: custom).
- **Cross-skill pipeline rate limit**: when chaining calls across multiple skills (e.g., swap → simulate → broadcast), add 300-500ms delay between requests to avoid triggering rate limit (error code `50011`).
- **Network error**: retry once, then prompt user to try again later
- **Request timeout**: all API calls must set a **10-second** timeout limit
- **Transaction already broadcast**: if the same `signedTx` is broadcast twice, the API may return an error or the same `txHash` — handle idempotently.

## Amount Display Rules

- Gas prices in Gwei for EVM chains (`18.5 Gwei`), never raw wei
- Gas limit as integer (`21000`, `145000`)
- USD gas cost estimate when possible
- Transaction values in UI units (`1.5 ETH`), never base units

## Global Notes

- **This skill does NOT sign transactions** — it only broadcasts pre-signed transactions
- Amounts in API params use **minimal units** (wei/lamports)
- Gas price fields: use `eip1559Protocol.suggestBaseFee` + `eip1559Protocol.proposePriorityFee` for EIP-1559 chains, `normal` for legacy
- EVM contract addresses must be **all lowercase**
- For swap quote and execution → use `okx-dex-swap`
- For market prices → use `okx-dex-market`
- For balance queries → use `okx-wallet-portfolio`
- For token search → use `okx-dex-token`
- For transaction broadcasting → use this skill (`okx-onchain-gateway`)
