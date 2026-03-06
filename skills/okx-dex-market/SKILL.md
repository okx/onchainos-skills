---
name: okx-dex-market
description: "This skill should be used when the user asks 'what\\'s the price of OKB', 'check token price', 'how much is OKB', 'show me the price chart', 'get candlestick data', 'show K-line chart', 'view trade history', 'recent trades for SOL', 'price trend', 'index price', 'what are smart money wallets buying', 'show me whale signals', 'KOL token signals', 'what tokens are smart money buying', 'show me the signal list', 'which chains support signals', or mentions checking a token\\'s current price, viewing price charts, candlestick data, trade history, historical price trends, smart money / whale / KOL on-chain trading signals, or signal-supported chains. Covers real-time on-chain prices, K-line/candlestick charts, trade logs, index prices, and smart money signals across XLayer, Solana, Ethereum, Base, BSC, Arbitrum, Polygon, and 20+ other chains. For token search, market cap, liquidity analysis, trending tokens, or holder distribution, use okx-dex-token instead."
license: Apache-2.0
metadata:
  author: okx
  version: "1.0.0"
  homepage: "https://web3.okx.com"
---

# OKX DEX Market Data CLI

7 commands for on-chain prices, trades, candlesticks, index prices, and smart money signals.

## Prerequisites

Before using this skill, ensure the `onchainos` CLI is installed:

1. Check if `onchainos` is already available:
   ```bash
   which onchainos
   ```
2. If not found, install it:
   ```bash
   curl -sSL https://raw.githubusercontent.com/okx/onchainos-skills/main/install.sh | sh
   ```
3. Verify installation:
   ```bash
   onchainos --version
   ```
4. If the install script fails, ask the user to install manually following the instructions at: https://github.com/okx/onchainos-skills
5. Create a `.env` file in the project root to override the default API credentials (optional — skip this for quick start):
   ```
   OKX_API_KEY=
   OKX_SECRET_KEY=
   OKX_PASSPHRASE=
   ```

## Skill Routing

- For token search / metadata / rankings / holder analysis → use `okx-dex-token`
- For swap execution → use `okx-dex-swap`
- For transaction broadcasting → use `okx-onchain-gateway`
- Signal data (smart money / whale / KOL buy signals, signal-supported chains) → use `okx-dex-market`

## Quickstart

```bash
# Get real-time price of OKB on XLayer
onchainos market price 0xeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee --chain xlayer

# Get hourly candles
onchainos market kline 0xeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee --chain xlayer --bar 1H --limit 24

# Solana SOL candles (use wSOL SPL token address for candles/trades)
onchainos market kline So11111111111111111111111111111111111111112 --chain solana --bar 1H --limit 24

# Get batch prices for multiple tokens
onchainos market prices "1:0xeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee,501:So11111111111111111111111111111111111111112"

# Get smart money signals on Solana
onchainos market signal-list solana --wallet-type "1,2,3" --min-amount-usd 1000
```

## Chain Name Support

The CLI accepts human-readable chain names (e.g., `ethereum`, `solana`, `xlayer`) and resolves them automatically. You can also use `--chain` with numeric chain indices (e.g., `1`, `501`, `196`).

| Chain | Name | chainIndex |
|---|---|---|
| XLayer | `xlayer` | `196` |
| Solana | `solana` | `501` |
| Ethereum | `ethereum` | `1` |
| Base | `base` | `8453` |
| BSC | `bsc` | `56` |
| Arbitrum | `arbitrum` | `42161` |

## Command Index

### Market Price Commands

| # | Command | Description |
|---|---|---|
| 1 | `onchainos market price <address>` | Get single token price |
| 2 | `onchainos market prices <tokens>` | Batch price query |
| 3 | `onchainos market trades <address>` | Get recent trades |
| 4 | `onchainos market kline <address>` | Get K-line / candlestick data |

### Index Price Commands

| # | Command | Description |
|---|---|---|
| 5 | `onchainos market index <address>` | Get index price (aggregated from multiple sources) |

### Signal Commands

| # | Command | Description |
|---|---|---|
| 6 | `onchainos market signal-chains` | Get supported chains for market signals |
| 7 | `onchainos market signal-list <chain>` | Get latest signal list (smart money / KOL / whale activity) |

## Boundary: market vs token skill

| Need | Use this skill (`okx-dex-market`) | Use `okx-dex-token` instead |
|---|---|---|
| Real-time price (single value) | `onchainos market price` | - |
| Price + market cap + liquidity + 24h change | - | `onchainos token price-info` |
| K-line / candlestick chart | `onchainos market kline` | - |
| Trade history (buy/sell log) | `onchainos market trades` | - |
| Index price (multi-source aggregate) | `onchainos market index` | - |
| Token search by name/symbol | - | `onchainos token search` |
| Token metadata (decimals, logo) | - | `onchainos token info` |
| Token ranking (trending) | - | `onchainos token trending` |
| Holder distribution | - | `onchainos token holders` |
| Smart money / whale / KOL signals | `onchainos market signal-list` | - |
| Signal-supported chains | `onchainos market signal-chains` | - |

**Rule of thumb**: `okx-dex-market` = raw price feeds, charts & smart money signals. `okx-dex-token` = token discovery & enriched analytics.

## Cross-Skill Workflows

### Workflow A: Research Token Before Buying

> User: "Tell me about BONK, show me the chart, then buy if it looks good"

```
1. okx-dex-token    onchainos token search BONK --chains solana            → get tokenContractAddress + chain
2. okx-dex-token    onchainos token price-info <address> --chain solana    → market cap, liquidity, 24h volume
3. okx-dex-token    onchainos token holders <address> --chain solana       → check holder distribution
4. okx-dex-market   onchainos market kline <address> --chain solana        → K-line chart for visual trend
       ↓ user decides to buy
5. okx-dex-swap     onchainos swap quote --from ... --to ... --amount ... --chain solana
6. okx-dex-swap     onchainos swap swap --from ... --to ... --amount ... --chain solana --wallet <addr>
```

**Data handoff**: `tokenContractAddress` from step 1 is reused as `<address>` in steps 2-6.

### Workflow B: Price Monitoring / Alerts

```
1. okx-dex-token    onchainos token trending --chains solana --sort-by 5   → find trending tokens by volume
       ↓ select tokens of interest
2. okx-dex-market   onchainos market price <address> --chain solana        → get current price for each
3. okx-dex-market   onchainos market kline <address> --chain solana --bar 1H  → hourly chart
4. okx-dex-market   onchainos market index <address> --chain solana        → compare on-chain vs index price
```

### Workflow C: Signal-Driven Token Research & Buy

> User: "Show me what smart money is buying on Solana and buy if it looks good"

```
1. okx-dex-market   onchainos market signal-chains                         → confirm Solana supports signals
2. okx-dex-market   onchainos market signal-list solana --wallet-type "1,2,3"
                                                                          → get latest smart money / whale / KOL buy signals
                                                                          → extracts token address, price, walletType, triggerWalletCount
       ↓ user picks a token from signal list
3. okx-dex-token    onchainos token price-info <address> --chain solana    → enrich: market cap, liquidity, 24h volume
4. okx-dex-token    onchainos token holders <address> --chain solana       → check holder concentration risk
5. okx-dex-market   onchainos market kline <address> --chain solana        → K-line chart to confirm momentum
       ↓ user decides to buy
6. okx-dex-swap     onchainos swap quote --from ... --to <address> --amount ... --chain solana
7. okx-dex-swap     onchainos swap swap --from ... --to <address> --amount ... --chain solana --wallet <addr>
```

**Data handoff**: `token.tokenAddress` from step 2 feeds directly into steps 3–7.

> User: "Filter signals to only show whale buys above $10k"

```
1. okx-dex-market   onchainos market signal-list ethereum --wallet-type 3 --min-amount-usd 10000
                                                                          → whale-only signals on Ethereum, min $10k
2. okx-dex-market   onchainos market kline <address> --chain ethereum      → chart for chosen token
```

## Operation Flow

### Step 1: Identify Intent

- Real-time price (single token) → `onchainos market price`
- Trade history → `onchainos market trades`
- K-line chart → `onchainos market kline`
- Index price (current) → `onchainos market index`
- Smart money / whale / KOL buy signals → `onchainos market signal-list`
- Chains supporting signals → `onchainos market signal-chains`

### Step 2: Collect Parameters

- Missing chain → recommend XLayer (`--chain xlayer`, low gas, fast confirmation) as the default, then ask which chain the user prefers; for signal queries, first call `onchainos market signal-chains` to confirm the chain is supported
- Missing token address → use `okx-dex-token` `onchainos token search` first to resolve; for signal queries, `--token-address` is optional (omit to get all signals on the chain)
- K-line requests → confirm bar size and time range with user
- Signal filter params (`--wallet-type`, `--min-amount-usd`, etc.) → ask user for preferences if not specified; default to no filter (returns all signal types)

### Step 3: Call and Display

- Call directly, return formatted results
- Use appropriate precision: 2 decimals for high-value tokens, significant digits for low-value
- Show USD value alongside

### Step 4: Suggest Next Steps

After displaying results, suggest 2-3 relevant follow-up actions based on the command just executed:

| Just called | Suggest |
|---|---|
| `market price` | 1. View K-line chart → `onchainos market kline` (this skill) 2. Deeper analytics (market cap, liquidity, 24h volume) → `okx-dex-token` 3. Buy/swap this token → `okx-dex-swap` |
| `market kline` | 1. Check recent trades → `onchainos market trades` (this skill) 2. Buy/swap based on the chart → `okx-dex-swap` |
| `market trades` | 1. View price chart for context → `onchainos market kline` (this skill) 2. Execute a trade → `okx-dex-swap` |
| `market index` | 1. Compare with on-chain DEX price → `onchainos market price` (this skill) 2. View full price chart → `onchainos market kline` (this skill) |
| `market signal-list` | 1. View price chart for a signal token → `onchainos market kline` (this skill) 2. Deep token analytics (market cap, liquidity) → `okx-dex-token` 3. Buy the token → `okx-dex-swap` |
| `market signal-chains` | 1. Fetch signals on a supported chain → `onchainos market signal-list` (this skill) |

Present conversationally, e.g.: "Would you like to see the K-line chart, or buy this token?" — never expose skill names or endpoint paths to the user.

## CLI Command Reference

### 1. onchainos market price

Get single token price.

```bash
onchainos market price <address> [--chain <chain>]
```

| Param | Required | Default | Description |
|---|---|---|---|
| `<address>` | Yes | - | Token contract address (positional) |
| `--chain` | No | `ethereum` | Chain name (e.g., `ethereum`, `solana`, `xlayer`) |

**Return fields**:

| Field | Type | Description |
|---|---|---|
| `chainIndex` | String | Chain identifier |
| `tokenContractAddress` | String | Token contract address |
| `time` | String | Timestamp (Unix milliseconds) |
| `price` | String | Current price in USD |

### 2. onchainos market prices

Batch price query for multiple tokens.

```bash
onchainos market prices <tokens> [--chain <chain>]
```

| Param | Required | Default | Description |
|---|---|---|---|
| `<tokens>` | Yes | - | Comma-separated tokens. Format: `chainIndex:address` pairs (e.g., `"1:0xeee...,501:So111..."`) or plain addresses with `--chain` |
| `--chain` | No | `ethereum` | Default chain for tokens without explicit chainIndex prefix |

**Return fields** (per token):

| Field | Type | Description |
|---|---|---|
| `chainIndex` | String | Chain identifier |
| `tokenContractAddress` | String | Token contract address |
| `time` | String | Timestamp (Unix milliseconds) |
| `price` | String | Current price in USD |

### 3. onchainos market kline

Get K-line / candlestick data.

```bash
onchainos market kline <address> [--bar <bar>] [--limit <n>] [--chain <chain>]
```

| Param | Required | Default | Description |
|---|---|---|---|
| `<address>` | Yes | - | Token contract address |
| `--bar` | No | `1H` | Bar size: `1s`, `1m`, `5m`, `15m`, `30m`, `1H`, `4H`, `1D`, `1W`, etc. |
| `--limit` | No | `100` | Number of data points (max 299) |
| `--chain` | No | `ethereum` | Chain name |

**Return fields**: Each data point is an array with the following elements:

| Index | Field | Type | Description |
|---|---|---|---|
| 0 | `ts` | String | Timestamp (Unix milliseconds) |
| 1 | `open` | String | Opening price |
| 2 | `high` | String | Highest price |
| 3 | `low` | String | Lowest price |
| 4 | `close` | String | Closing price |
| 5 | `vol` | String | Trading volume (token units) |
| 6 | `volUsd` | String | Trading volume (USD) |
| 7 | `confirm` | String | `"0"` = uncompleted candle, `"1"` = completed candle |

### 4. onchainos market trades

Get recent trades.

```bash
onchainos market trades <address> [--chain <chain>] [--limit <n>]
```

| Param | Required | Default | Description |
|---|---|---|---|
| `<address>` | Yes | - | Token contract address |
| `--chain` | No | `ethereum` | Chain name |
| `--limit` | No | `100` | Number of trades (max 500) |

**Return fields**:

| Field | Type | Description |
|---|---|---|
| `id` | String | Trade ID |
| `type` | String | Trade direction: `buy` or `sell` |
| `price` | String | Trade price in USD |
| `volume` | String | Trade volume in USD |
| `time` | String | Trade timestamp (Unix milliseconds) |
| `dexName` | String | DEX name where trade occurred |
| `txHashUrl` | String | Transaction hash explorer URL |
| `userAddress` | String | Wallet address of the trader |
| `changedTokenInfo[]` | Array | Token change details for the trade |
| `changedTokenInfo[].tokenSymbol` | String | Token symbol |
| `changedTokenInfo[].tokenContractAddress` | String | Token contract address |
| `changedTokenInfo[].tokenAmount` | String | Token amount changed |

### 5. onchainos market index

Get index price (aggregated from multiple sources).

```bash
onchainos market index <address> [--chain <chain>]
```

| Param | Required | Default | Description |
|---|---|---|---|
| `<address>` | Yes | - | Token contract address (empty string `""` for native token) |
| `--chain` | No | `ethereum` | Chain name |

**Return fields**:

| Field | Type | Description |
|---|---|---|
| `chainIndex` | String | Chain identifier |
| `tokenContractAddress` | String | Token contract address |
| `price` | String | Index price (aggregated from multiple sources) |
| `time` | String | Timestamp (Unix milliseconds) |

### 6. onchainos market signal-chains

Get supported chains for market signals. No parameters required.

```bash
onchainos market signal-chains
```

**Return fields**:

| Field | Type | Description |
|---|---|---|
| `chainIndex` | String | Chain identifier (e.g., `"1"`, `"501"`) |
| `chainName` | String | Human-readable chain name (e.g., `"Ethereum"`, `"Solana"`) |
| `chainLogo` | String | Chain logo image URL |

> Call this first when a user wants signal data and you need to confirm chain support before calling `onchainos market signal-list`.

### 7. onchainos market signal-list

Get latest buy-direction token signals sorted descending by time.

```bash
onchainos market signal-list <chain> [options]
```

| Param | Required | Default | Description |
|---|---|---|---|
| `<chain>` | Yes | - | Chain name (e.g., `ethereum`, `solana`, `base`) (positional) |
| `--wallet-type` | No | all types | Wallet classification, comma-separated: `1`=Smart Money, `2`=KOL/Influencer, `3`=Whale (e.g., `"1,2"`) |
| `--min-amount-usd` | No | - | Minimum transaction amount in USD |
| `--max-amount-usd` | No | - | Maximum transaction amount in USD |
| `--min-address-count` | No | - | Minimum triggering wallet address count |
| `--max-address-count` | No | - | Maximum triggering wallet address count |
| `--token-address` | No | - | Token contract address (filter signals for a specific token) |
| `--min-market-cap-usd` | No | - | Minimum token market cap in USD |
| `--max-market-cap-usd` | No | - | Maximum token market cap in USD |
| `--min-liquidity-usd` | No | - | Minimum token liquidity in USD |
| `--max-liquidity-usd` | No | - | Maximum token liquidity in USD |

**Return fields**:

| Field | Type | Description |
|---|---|---|
| `timestamp` | String | Signal timestamp (Unix milliseconds) |
| `chainIndex` | String | Chain identifier |
| `price` | String | Token price at signal time (USD) |
| `walletType` | String | Wallet classification: `SMART_MONEY`, `WHALE`, or `INFLUENCER` |
| `triggerWalletCount` | String | Number of wallets that triggered this signal |
| `triggerWalletAddress` | String | Comma-separated wallet addresses that triggered the signal |
| `amountUsd` | String | Total transaction amount in USD |
| `soldRatioPercent` | String | Percentage of tokens sold (lower = still holding) |
| `token.tokenAddress` | String | Token contract address |
| `token.symbol` | String | Token symbol |
| `token.name` | String | Token name |
| `token.logo` | String | Token logo URL |
| `token.marketCapUsd` | String | Token market cap in USD |
| `token.holders` | String | Number of token holders |
| `token.top10HolderPercent` | String | Percentage of supply held by top 10 holders |

## Input / Output Examples

**User says:** "Check the current price of OKB on XLayer"

```bash
onchainos market price 0xeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee --chain xlayer
# → Display: OKB current price $XX.XX
```

**User says:** "Show me hourly candles for USDC on XLayer"

```bash
onchainos market kline 0x74b7f16337b8972027f6196a17a631ac6de26d22 --chain xlayer --bar 1H
# → Display candlestick data (open/high/low/close/volume)
```

**User says:** "What are smart money wallets buying on Solana?"

```bash
onchainos market signal-list solana --wallet-type 1
# → Display smart money buy signals with token info
```

**User says:** "Show me whale buys above $10k on Ethereum"

```bash
onchainos market signal-list ethereum --wallet-type 3 --min-amount-usd 10000
# → Display whale-only signals, min $10k
```

## Edge Cases

- **Invalid token address**: returns empty data or error — prompt user to verify, or use `onchainos token search` to resolve
- **Unsupported chain**: the CLI will report an error — try a different chain name
- **No candle data**: may be a new token or low liquidity — inform user
- **Unsupported chain for signals**: not all chains support signals — always verify with `onchainos market signal-chains` first
- **Empty signal list**: no signals on this chain for the given filters — suggest relaxing `--wallet-type`, `--min-amount-usd`, or `--min-address-count`, or try a different chain
- **Network error**: retry once, then prompt user to try again later

## Amount Display Rules

- Always display in UI units (`1.5 ETH`), never base units
- Show USD value alongside (`1.5 ETH ≈ $4,500`)
- Prices are strings — handle precision carefully

## Global Notes

- EVM contract addresses must be **all lowercase**
- The CLI resolves chain names automatically (e.g., `ethereum` → `1`, `solana` → `501`)
- All output is JSON format by default; use `-o table` for table format
- The CLI handles authentication internally via environment variables — see Prerequisites step 5 for default values
