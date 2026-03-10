---
name: okx-dex-market
description: "This skill should be used when the user asks about live on-chain market data: token prices, price charts (K-line, OHLC), trade history, or swap activity. Also covers on-chain signals — smart money, whale, and KOL wallet activity, large trades, and signal-supported chains. For meme tokens: scanning new launches (扫链/trenches，golden dog, alpha, pump fun), checking dev wallets, developer reputation, rug pull detection, tokens by same creator, bundle/sniper detection, bonding curves, and meme token safety checks. For token search, market cap, liquidity, trending tokens, or holder distribution, use okx-dex-token instead."
license: Apache-2.0
metadata:
  author: okx
  version: "1.0.2"
  homepage: "https://web3.okx.com"
---

# OKX DEX Market Data CLI

14 commands for on-chain prices, trades, candlesticks, index prices, smart money signals, and meme pump token scanning.

## Pre-flight Checks

Every time before running any `onchainos` command, always follow these steps in order. Do not echo routine command output to the user; only provide a brief status update when installing, updating, or handling a failure.

1. **Confirm installed**: Run `which onchainos`. If not found, install it:
   ```bash
   curl -sSL https://raw.githubusercontent.com/okx/onchainos-skills/main/install.sh | sh
   ```
   If the install script fails, ask the user to install manually following the instructions at: https://github.com/okx/onchainos-skills

2. **Check for updates**: Read `~/.onchainos/last_check` and compare it with the current timestamp:
   ```bash
   cached_ts=$(cat ~/.onchainos/last_check 2>/dev/null || true)
   now=$(date +%s)
   ```
   - If `cached_ts` is non-empty and `(now - cached_ts) < 43200` (12 hours), skip the update and proceed.
   - Otherwise (file missing or older than 12 hours), run the installer to check for updates:
     ```bash
     curl -sSL https://raw.githubusercontent.com/okx/onchainos-skills/main/install.sh | sh
     ```
     If a newer version is installed, tell the user and suggest updating their onchainos skills from https://github.com/okx/onchainos-skills to get the latest features.
3. If any `onchainos` command fails with an unexpected error during this
   session, try reinstalling before giving up:
   ```bash
   curl -sSL https://raw.githubusercontent.com/okx/onchainos-skills/main/install.sh | sh
   ```
4. Create a `.env` file in the project root to override the default API credentials (optional — skip this for quick start):
   ```
   OKX_API_KEY=          # or OKX_ACCESS_KEY
   OKX_SECRET_KEY=
   OKX_PASSPHRASE=
   ```

## Skill Routing

- For token search / metadata / rankings / holder analysis → use `okx-dex-token`
- For swap execution → use `okx-dex-swap`
- For transaction broadcasting → use `okx-onchain-gateway`
- For wallet balances / portfolio → use `okx-wallet-portfolio`
- Signal data (smart money / whale / KOL buy signals, signal-supported chains) → use `okx-dex-market`
- Meme pump scanning (token lists, dev info, bundle detection, aped wallets) → use `okx-dex-market`
- Meme token safety (rug pull check, dev reputation, bundler/sniper analysis, similar tokens by same dev) → use `okx-dex-market`
- **"Trenches" / "扫链"** (scanning for new meme tokens) → use `okx-dex-market` memepump commands (NOT signal commands)

## Keyword Glossary

Users may use Chinese crypto slang, English equivalents, or platform-specific terms. Map them to the correct commands:

| Chinese | English / Platform Terms | Maps To |
|---|---|---|
| 扫链 | trenches, memerush, 战壕, 打狗 | `memepump-tokens` |
| 同车 | aped, same-car, co-invested | `memepump-aped-wallet` |
| 牛人榜 | leaderboard, top traders, smart money ranking | `signal-list` (filter by `--wallet-type`) |
| 开发者信息 | dev info, developer reputation, rug check | `memepump-token-dev-info` |
| 捆绑/狙击 | bundler, sniper, bundle analysis | `memepump-token-bundle-info` |
| 行情 | market data, price, chart | `price`, `kline`, `trades` |
| 持仓分析 | holding analysis, holder distribution | `memepump-token-details` (tags fields) |
| 社媒筛选 | social filter | `memepump-tokens --has-x`, `--has-telegram`, etc. |
| 新盘 / 迁移中 / 已迁移 | NEW / MIGRATING / MIGRATED | `memepump-tokens --stage` |
| pumpfun / bonkers / bonk / believe / bags / mayhem | protocol names (launch platforms) | `memepump-tokens --protocol-id-list <id>` |

**Protocol names are NOT token names.** When a user mentions pumpfun, bonkers, bonk, etc., look up their IDs via `onchainos market memepump-chains`, then pass to `--protocol-id-list`. Multiple protocols: comma-separate the IDs (e.g. `--protocol-id-list <bonkers_id>,<bonk_id>`).

When presenting `memepump-token-details` or `memepump-token-dev-info` responses, translate JSON field names (e.g., `top10HoldingsPercent` → "top-10 holder concentration", `rugPullCount` → "rug pull count / 跑路次数", `bondingPercent` → "bonding curve progress") into human-readable language. Never dump raw field names to the user.

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

# Get supported chains and protocols for meme pump
onchainos market memepump-chains

# List new meme pump tokens on Solana
onchainos market memepump-tokens solana --stage NEW

# Get meme pump token details
onchainos market memepump-token-details <address> --chain solana

# Check developer reputation for a meme token
onchainos market memepump-token-dev-info <address> --chain solana
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

### Meme Pump Commands

| # | Command | Description |
|---|---|---|
| 8 | `onchainos market memepump-chains` | Get supported chains and protocols for meme pump |
| 9 | `onchainos market memepump-tokens <chain>` | List meme pump tokens with advanced filtering |
| 10 | `onchainos market memepump-token-details <address>` | Get detailed info for a single meme pump token |
| 11 | `onchainos market memepump-token-dev-info <address>` | Get developer analysis and holding info |
| 12 | `onchainos market memepump-similar-tokens <address>` | Find similar tokens by same creator |
| 13 | `onchainos market memepump-token-bundle-info <address>` | Get bundle/sniper analysis |
| 14 | `onchainos market memepump-aped-wallet <address>` | Get aped (same-car) wallet list |

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
| Browse meme pump tokens by stage | `onchainos market memepump-tokens` | - |
| Meme token audit (top10, dev, insiders) | `onchainos market memepump-token-details` | - |
| Developer reputation / rug pull history | `onchainos market memepump-token-dev-info` | - |
| Similar tokens by same creator | `onchainos market memepump-similar-tokens` | - |
| Bundle/sniper detection | `onchainos market memepump-token-bundle-info` | - |
| Aped (same-car) wallet analysis | `onchainos market memepump-aped-wallet` | - |

**Rule of thumb**: `okx-dex-market` = raw price feeds, charts, smart money signals & meme pump scanning (including dev reputation, rug pull checks, bundler analysis). `okx-dex-token` = token discovery & enriched analytics (search, trending, holders, market cap).

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

### Workflow D: Meme Token Discovery & Analysis

> User: "Show me new meme tokens on Solana and check if any look safe"

```
1. okx-dex-market   onchainos market memepump-chains                          → discover supported chains & protocols
2. okx-dex-market   onchainos market memepump-tokens solana --stage NEW       → browse new tokens
       ↓ pick an interesting token
3. okx-dex-market   onchainos market memepump-token-details <address> --chain solana  → full token detail + audit tags
4. okx-dex-market   onchainos market memepump-token-dev-info <address> --chain solana → check dev reputation (rug pulls, migrations)
5. okx-dex-market   onchainos market memepump-token-bundle-info <address> --chain solana → check for bundlers/snipers
6. okx-dex-market   onchainos market kline <address> --chain solana           → view price chart
       ↓ user decides to buy
7. okx-dex-swap     onchainos swap quote --from ... --to <address> --amount ... --chain solana
8. okx-dex-swap     onchainos swap swap --from ... --to <address> --amount ... --chain solana --wallet <addr>
```

**Data handoff**: `tokenAddress` from step 2 is reused as `<address>` in steps 3–8.

### Workflow E: Meme Token Due Diligence

> User: "Check if this meme token is safe before I buy"

```
1. okx-dex-market   onchainos market memepump-token-details <address> --chain solana   → basic info + audit tags
2. okx-dex-market   onchainos market memepump-token-dev-info <address> --chain solana  → dev history + holding
3. okx-dex-market   onchainos market memepump-similar-tokens <address> --chain solana  → other tokens by same dev
4. okx-dex-market   onchainos market memepump-token-bundle-info <address> --chain solana → bundler analysis
5. okx-dex-market   onchainos market memepump-aped-wallet <address> --chain solana     → who else is holding
```

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
- Discover meme pump supported chains/protocols → `onchainos market memepump-chains`
- **Trenches / 扫链** / browse/filter meme tokens by stage → `onchainos market memepump-tokens`
- Deep-dive into a specific meme token → `onchainos market memepump-token-details`
- Check meme token developer reputation → `onchainos market memepump-token-dev-info`
- Find similar tokens by same creator → `onchainos market memepump-similar-tokens`
- Analyze bundler/sniper activity → `onchainos market memepump-token-bundle-info`
- View aped (same-car) wallet holdings → `onchainos market memepump-aped-wallet`

### Step 2: Collect Parameters

- Missing chain → recommend XLayer (`--chain xlayer`, low gas, fast confirmation) as the default, then ask which chain the user prefers; for signal queries, first call `onchainos market signal-chains` to confirm the chain is supported; for meme pump queries, default to Solana (`--chain solana`)
- Missing token address → use `okx-dex-token` `onchainos token search` first to resolve; for signal queries, `--token-address` is optional (omit to get all signals on the chain); for meme pump, use `onchainos market memepump-tokens` first to discover tokens
- Missing `--stage` for memepump-tokens → ask user which stage (NEW / MIGRATING / MIGRATED)
- User mentions a protocol name (pumpfun, bonkers, bonk, believe, bags, mayhem, fourmeme, etc.) → first call `onchainos market memepump-chains` to get the protocol ID, then pass `--protocol-id-list <id>` to `memepump-tokens`. Do NOT use `okx-dex-token` to search for protocol names as tokens.
- K-line requests → confirm bar size and time range with user
- Signal filter params (`--wallet-type`, `--min-amount-usd`, etc.) → ask user for preferences if not specified; default to no filter (returns all signal types)

### Step 3: Call and Display

- Call directly, return formatted results
- Use appropriate precision: 2 decimals for high-value tokens, significant digits for low-value
- Show USD value alongside
- Translate field names per the Keyword Glossary — never dump raw JSON keys. For `memepump-token-dev-info`, present as a developer reputation report. For `memepump-token-details`, present as a token safety summary highlighting red/green flags.
- When listing tokens from `memepump-tokens`, never merge or deduplicate entries that share the same symbol. Different tokens can have identical symbols but different contract addresses — each is a distinct token and must be shown separately. Always include the contract address to distinguish them.

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
| `market memepump-chains` | 1. Browse tokens → `onchainos market memepump-tokens` (this skill) |
| `market memepump-tokens` | 1. Pick a token for details → `onchainos market memepump-token-details` (this skill) 2. Check dev → `onchainos market memepump-token-dev-info` (this skill) |
| `market memepump-token-details` | 1. Dev analysis → `onchainos market memepump-token-dev-info` (this skill) 2. Similar tokens → `onchainos market memepump-similar-tokens` (this skill) 3. Bundle check → `onchainos market memepump-token-bundle-info` (this skill) |
| `market memepump-token-dev-info` | 1. Check bundle activity → `onchainos market memepump-token-bundle-info` (this skill) 2. View price chart → `onchainos market kline` (this skill) |
| `market memepump-similar-tokens` | 1. Compare with details → `onchainos market memepump-token-details` (this skill) |
| `market memepump-token-bundle-info` | 1. Check aped wallets → `onchainos market memepump-aped-wallet` (this skill) |
| `market memepump-aped-wallet` | 1. View price chart → `onchainos market kline` (this skill) 2. Buy the token → `okx-dex-swap` |

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

### 8. onchainos market memepump-chains

Get supported chains and protocols for meme pump. No parameters required.

```bash
onchainos market memepump-chains
```

**Return fields**:

| Field | Type | Description |
|---|---|---|
| `data[].chainIndex` | String | Chain identifier (e.g., `"501"` for Solana, `"56"` for BSC) |
| `data[].chainName` | String | Human-readable chain name |
| `data[].protocolList[].protocolId` | String | Protocol unique ID |
| `data[].protocolList[].protocolName` | String | Protocol display name (e.g., `pumpfun`, `fourmeme`) |

> Currently supports: Solana (501), BSC (56), X Layer (196), TRON (195).

### 9. onchainos market memepump-tokens

List meme pump tokens with advanced filtering. Returns up to 30 tokens per request.

```bash
onchainos market memepump-tokens <chain> --stage <stage> [options]
```

| Param | Required | Default | Description |
|---|---|---|---|
| `<chain>` | Yes | - | Chain name (e.g., `solana`, `bsc`) (positional) |
| `--stage` | Yes | - | Token stage: `NEW`, `MIGRATING`, or `MIGRATED` |
| `--wallet-address` | No | - | Wallet address for position-specific data |
| `--protocol-id-list` | No | - | Comma-separated protocol IDs to filter (get IDs from `memepump-chains`) |
| `--quote-token-address-list` | No | - | Comma-separated quote token addresses |
| `--min-top10-holdings-percent` | No | - | Min top 10 holders percentage (0-100) |
| `--max-top10-holdings-percent` | No | - | Max top 10 holders percentage (0-100) |
| `--min-dev-holdings-percent` | No | - | Min developer holdings percentage |
| `--max-dev-holdings-percent` | No | - | Max developer holdings percentage |
| `--min-insiders-percent` | No | - | Min insider wallet percentage |
| `--max-insiders-percent` | No | - | Max insider wallet percentage |
| `--min-bundlers-percent` | No | - | Min bundler wallet percentage |
| `--max-bundlers-percent` | No | - | Max bundler wallet percentage |
| `--min-snipers-percent` | No | - | Min sniper wallet percentage |
| `--max-snipers-percent` | No | - | Max sniper wallet percentage |
| `--min-fresh-wallets-percent` | No | - | Min newly-created wallet percentage |
| `--max-fresh-wallets-percent` | No | - | Max newly-created wallet percentage |
| `--min-suspected-phishing-wallet-percent` | No | - | Min phishing wallet percentage |
| `--max-suspected-phishing-wallet-percent` | No | - | Max phishing wallet percentage |
| `--min-bot-traders` | No | - | Min bot trader wallet count |
| `--max-bot-traders` | No | - | Max bot trader wallet count |
| `--min-dev-migrated` | No | - | Min tokens migrated by developer |
| `--max-dev-migrated` | No | - | Max tokens migrated by developer |
| `--min-market-cap` | No | - | Minimum market cap in USD |
| `--max-market-cap` | No | - | Maximum market cap in USD |
| `--min-volume` | No | - | Minimum 24h volume in USD |
| `--max-volume` | No | - | Maximum 24h volume in USD |
| `--min-tx-count` | No | - | Minimum transaction count |
| `--max-tx-count` | No | - | Maximum transaction count |
| `--min-bonding-percent` | No | - | Min bonding curve completion (0-100) |
| `--max-bonding-percent` | No | - | Max bonding curve completion (0-100) |
| `--min-holders` | No | - | Minimum unique holder count |
| `--max-holders` | No | - | Maximum unique holder count |
| `--min-token-age` | No | - | Minimum token age in minutes |
| `--max-token-age` | No | - | Maximum token age in minutes. When stage=MIGRATED, age counts from migration time |
| `--min-buy-tx-count` | No | - | Min buy transactions (last 1 hour) |
| `--max-buy-tx-count` | No | - | Max buy transactions (last 1 hour) |
| `--min-sell-tx-count` | No | - | Min sell transactions (last 1 hour) |
| `--max-sell-tx-count` | No | - | Max sell transactions (last 1 hour) |
| `--min-token-symbol-length` | No | - | Min ticker symbol length |
| `--max-token-symbol-length` | No | - | Max ticker symbol length |
| `--has-at-least-one-social-link` | No | - | Require at least one social link (true/false) |
| `--has-x` | No | - | Require X (Twitter) link (true/false) |
| `--has-telegram` | No | - | Require Telegram link (true/false) |
| `--has-website` | No | - | Require website link (true/false) |
| `--website-type-list` | No | - | Website types: 0=official, 1=YouTube, 2=Twitch, etc. |
| `--dex-screener-paid` | No | - | Filter by DexScreener promotion status (true/false) |
| `--live-on-pump-fun` | No | - | Filter by PumpFun live stream status (true/false) |
| `--dev-sell-all` | No | - | Filter by developer liquidation status (true/false) |
| `--dev-still-holding` | No | - | Filter by developer holding status (true/false) |
| `--community-takeover` | No | - | Filter by community takeover status (true/false) |
| `--bags-fee-claimed` | No | - | Filter by fee claim status (true/false) |
| `--min-fees-native` | No | - | Minimum fees in native currency |
| `--max-fees-native` | No | - | Maximum fees in native currency |
| `--keywords-include` | No | - | Include tokens matching keyword (case-insensitive) |
| `--keywords-exclude` | No | - | Exclude tokens matching keyword (case-insensitive) |

**Return fields**: Array of token objects (same structure as `memepump-token-details` response).

### 10. onchainos market memepump-token-details

Get detailed information for a specific meme pump token.

```bash
onchainos market memepump-token-details <address> [--chain <chain>] [--wallet <wallet>]
```

| Param | Required | Default | Description |
|---|---|---|---|
| `<address>` | Yes | - | Token contract address (positional) |
| `--chain` | No | `solana` | Chain name |
| `--wallet` | No | - | User wallet address (for position and P&L data) |

**Return fields**:

| Field | Type | Description |
|---|---|---|
| `chainIndex` | String | Chain identifier |
| `protocolId` | String | Protocol numeric ID (e.g., `"120596"` for pumpfun) |
| `quoteTokenAddress` | String | Quote token contract address |
| `tokenAddress` | String | Token contract address |
| `symbol` | String | Token symbol |
| `name` | String | Token name |
| `logoUrl` | String | Token logo URL |
| `creatorAddress` | String | Token creator wallet address |
| `createdTimestamp` | String | Creation timestamp (Unix ms) |
| `migratedBeginTimestamp` | String | Migration start timestamp (Unix ms, empty if not migrating) |
| `migratedEndTimestamp` | String | Migration end timestamp (Unix ms, empty if not migrated) |
| `market.marketCapUsd` | String | Market cap in USD |
| `market.volumeUsd1h` | String | 1-hour volume in USD |
| `market.txCount1h` | String | 1-hour transaction count |
| `market.buyTxCount1h` | String | 1-hour buy transaction count |
| `market.sellTxCount1h` | String | 1-hour sell transaction count |
| `bondingPercent` | String | Bonding curve progress (0-100) |
| `tags.top10HoldingsPercent` | String | Top 10 holders percentage (0-100) |
| `tags.devHoldingsPercent` | String | Dev holdings percentage (0-100) |
| `tags.insidersPercent` | String | Insiders percentage (0-100) |
| `tags.bundlersPercent` | String | Bundlers percentage (0-100) |
| `tags.snipersPercent` | String | Snipers percentage (0-100) |
| `tags.freshWalletsPercent` | String | Fresh wallets percentage (0-100) |
| `tags.suspectedPhishingWalletPercent` | String | Phishing wallet percentage (0-100) |
| `tags.totalHolders` | String | Total holder count |
| `social.x` | String | X (Twitter) URL |
| `social.telegram` | String | Telegram URL |
| `social.website` | String | Website URL |
| `social.dexScreenerPaid` | Boolean | Paid on DexScreener |
| `social.communityTakeover` | Boolean | Community takeover flag |
| `social.liveOnPumpFun` | Boolean | Currently live on Pump.fun |
| `bagsFeeClaimed` | Boolean | Bags fee claimed |
| `mayhemModeTimeRemaining` | String | Pump.fun Mayhem Mode time remaining (seconds, empty if inactive) |
| `aped` | String | Same-car wallet count |

### 11. onchainos market memepump-token-dev-info

Get developer analysis including rug pull history, migration stats, and holding info.

```bash
onchainos market memepump-token-dev-info <address> [--chain <chain>]
```

| Param | Required | Default | Description |
|---|---|---|---|
| `<address>` | Yes | - | Token contract address (positional) |
| `--chain` | No | `solana` | Chain name |

**Return fields**:

| Field | Type | Description |
|---|---|---|
| `devLaunchedInfo.totalTokens` | String | Total tokens created by this dev |
| `devLaunchedInfo.rugPullCount` | String | Number of rug pulls |
| `devLaunchedInfo.migratedCount` | String | Number of successfully migrated tokens |
| `devLaunchedInfo.goldenGemCount` | String | Number of golden gem tokens |
| `devHoldingInfo.devHoldingPercent` | String | Dev holding percentage (0-100) |
| `devHoldingInfo.devAddress` | String | Developer wallet address |
| `devHoldingInfo.fundingAddress` | String | Funding source address |
| `devHoldingInfo.devBalance` | String | Dev's current balance |
| `devHoldingInfo.lastFundedTimestamp` | String | Last funded timestamp (Unix ms) |

> **Note**: `devHoldingInfo` may be `null` if the creator address is unavailable.

### 12. onchainos market memepump-similar-tokens

Find similar tokens created by the same developer. Returns at most 2 results.

```bash
onchainos market memepump-similar-tokens <address> [--chain <chain>]
```

| Param | Required | Default | Description |
|---|---|---|---|
| `<address>` | Yes | - | Token contract address (positional) |
| `--chain` | No | `solana` | Chain name |

**Return fields**:

| Field | Type | Description |
|---|---|---|
| `data[].tokenAddress` | String | Similar token contract address |
| `data[].tokenSymbol` | String | Token symbol |
| `data[].tokenLogo` | String | Token logo URL |
| `data[].marketCapUsd` | String | Market cap in USD |
| `data[].lastTxTimestamp` | String | Last transaction timestamp (Unix ms) |
| `data[].createdTimestamp` | String | Creation timestamp (Unix ms) |

### 13. onchainos market memepump-token-bundle-info

Get bundle/sniper analysis for a token.

```bash
onchainos market memepump-token-bundle-info <address> [--chain <chain>]
```

| Param | Required | Default | Description |
|---|---|---|---|
| `<address>` | Yes | - | Token contract address (positional) |
| `--chain` | No | `solana` | Chain name |

**Return fields**:

| Field | Type | Description |
|---|---|---|
| `bundlerAthPercent` | String | Bundler all-time-high percentage (0-100) |
| `totalBundlers` | String | Total number of bundlers |
| `bundledValueNative` | String | Total bundled value in native token |
| `bundledTokenAmount` | String | Total bundled token amount |

### 14. onchainos market memepump-aped-wallet

Get the aped (same-car) wallet list for a token.

```bash
onchainos market memepump-aped-wallet <address> [--chain <chain>] [--wallet <wallet>]
```

| Param | Required | Default | Description |
|---|---|---|---|
| `<address>` | Yes | - | Token contract address (positional) |
| `--chain` | No | `solana` | Chain name |
| `--wallet` | No | - | User wallet address (to highlight if present in aped wallets) |

**Return fields**:

| Field | Type | Description |
|---|---|---|
| `data[].walletAddress` | String | Wallet address |
| `data[].walletType` | String | Wallet type label (e.g., Smart Money, KOL, Whale) |
| `data[].holdingUsd` | String | Holding value in USD |
| `data[].holdingPercent` | String | Holding percentage (0-100) |
| `data[].totalPnl` | String | Total PnL in USD |
| `data[].pnlPercent` | String | PnL percentage |

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

**User says:** "Show me new meme tokens on Solana"

```bash
onchainos market memepump-tokens solana --stage NEW
# → Display list of new meme pump tokens with market data and audit tags
```

**User says:** "Find Solana meme tokens matching a phrase and exclude another keyword"

```bash
onchainos market memepump-tokens solana --stage NEW --keywords-include "dog wif" --keywords-exclude "cat"
# → Filter meme pump tokens by case-insensitive keyword phrases
```

**User says:** "Is this meme token safe? Check the developer"

```bash
onchainos market memepump-token-dev-info <address> --chain solana
# → Display dev rug pull count, migration count, golden gems, dev holding info
```

**User says:** "Check if this token has bundler activity"

```bash
onchainos market memepump-token-bundle-info <address> --chain solana
# → Display bundler count, bundled value, bundled token amount
```

## Region Restrictions (IP Blocking)

Some services are geo-restricted. When a command fails with error code `50125` or `80001`, return a friendly message without exposing the raw error code:

| Service | Restricted Regions | Blocking Method |
|---|---|---|
| DEX | United Kingdom | API key auth |
| DeFi | Hong Kong | API key auth + backend |
| Wallet | None | None |
| Global | Sanctioned countries | Gateway (403) |

**Error handling**: When the CLI returns error `50125` or `80001`, display:

> {service_name} is not available in your region. Please switch to a supported region and try again.

Examples:
- "DEX is not available in your region. Please switch to a supported region and try again."
- "DeFi is not available in your region. Please switch to a supported region and try again."

Do not expose raw error codes or internal error messages to the user.

## Edge Cases

- **Invalid token address**: returns empty data or error — prompt user to verify, or use `onchainos token search` to resolve
- **Unsupported chain**: the CLI will report an error — try a different chain name
- **No candle data**: may be a new token or low liquidity — inform user
- **Unsupported chain for signals**: not all chains support signals — always verify with `onchainos market signal-chains` first
- **Empty signal list**: no signals on this chain for the given filters — suggest relaxing `--wallet-type`, `--min-amount-usd`, or `--min-address-count`, or try a different chain
- **Unsupported chain for meme pump**: only Solana (501), BSC (56), X Layer (196), TRON (195) are supported — verify with `onchainos market memepump-chains` first
- **Invalid stage**: must be exactly `NEW`, `MIGRATING`, or `MIGRATED`
- **Token not found in meme pump**: `memepump-token-details` returns null data if the token doesn't exist in meme pump ranking data — it may be on a standard DEX
- **No dev holding info**: `memepump-token-dev-info` returns `devHoldingInfo` as `null` if the creator address is unavailable
- **Empty similar tokens**: `memepump-similar-tokens` may return empty array if no similar tokens are found
- **Empty aped wallets**: `memepump-aped-wallet` returns empty array if no co-holders found
- **Network error**: retry once, then prompt user to try again later
- **Region restriction (error code 50125 or 80001)**: do NOT show the raw error code to the user. Instead, display a friendly message: `⚠️ Service is not available in your region. Please switch to a supported region and try again.`

## Amount Display Rules

- Always display in UI units (`1.5 ETH`), never base units
- Show USD value alongside (`1.5 ETH ≈ $4,500`)
- Prices are strings — handle precision carefully

## Global Notes

- EVM contract addresses must be **all lowercase**
- The CLI resolves chain names automatically (e.g., `ethereum` → `1`, `solana` → `501`)
- The CLI handles authentication internally via environment variables — see Prerequisites step 4 for default values
