---
name: okx-wallet-portfolio
description: "This skill should be used when the user asks to 'check my wallet balance', 'show my token holdings', 'how much OKB do I have', 'what tokens do I have', 'check my portfolio value', 'view my assets', 'how much is my portfolio worth', 'what\\'s in my wallet', 'show my PnL', 'what is my profit and loss', 'how much have I made', 'show my win rate', 'show my trading history', 'what did I buy or sell', 'my DEX transaction history', 'recent PnL by token', 'PnL for a specific token', or mentions checking wallet balance, total assets, token holdings, portfolio value, remaining funds, DeFi positions, multi-chain balance lookup, realized/unrealized PnL, trading win rate, DEX transaction history, or token-level profit and loss. Supports XLayer, Solana, Ethereum, Base, BSC, Arbitrum, Polygon, and 20+ other chains. Do NOT use for general programming questions about balance variables or API documentation. Do NOT use when the user is asking how to build or integrate a balance feature into code."
license: Apache-2.0
metadata:
  author: okx
  version: "1.1.0"
  homepage: "https://web3.okx.com"
---

# OKX Wallet Portfolio CLI

8 commands for supported chains, wallet total value, all token balances, specific token balances, portfolio PnL overview, DEX transaction history, recent PnL list, and per-token PnL snapshot.

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

- For token prices / K-lines → use `okx-dex-market`
- For token search / metadata → use `okx-dex-token`
- For swap execution → use `okx-dex-swap`
- For transaction broadcasting → use `okx-onchain-gateway`

## Quickstart

```bash
# Get supported chains for balance queries
onchainos portfolio chains

# Get total asset value on XLayer and Solana
onchainos portfolio total-value --address 0xYourWallet --chains "xlayer,solana"

# Get all token balances
onchainos portfolio all-balances --address 0xYourWallet --chains "xlayer,solana,ethereum"

# Check specific tokens (native OKB + USDC on XLayer)
onchainos portfolio token-balances --address 0xYourWallet --tokens "196:,196:0x74b7f16337b8972027f6196a17a631ac6de26d22"

# Portfolio PnL overview for the last 7 days
OKX_BASE_URL=https://web3pre.okex.org onchainos portfolio overview --address 0xYourWallet --chain ethereum --time-frame 7d

# DEX transaction history
OKX_BASE_URL=https://web3pre.okex.org onchainos portfolio dex-history --address 0xYourWallet --chain ethereum --limit 20

# Recent PnL by token
OKX_BASE_URL=https://web3pre.okex.org onchainos portfolio recent-pnl --address 0xYourWallet --chain ethereum

# Latest PnL for a specific token
OKX_BASE_URL=https://web3pre.okex.org onchainos portfolio token-pnl --address 0xYourWallet --chain ethereum --token 0xA0b86991c6218b36c1d19D4a2e9Eb0cE3606eB48
```

## Chain Name Support

The CLI accepts human-readable chain names and resolves them automatically.

| Chain | Name | chainIndex |
|---|---|---|
| XLayer | `xlayer` | `196` |
| Solana | `solana` | `501` |
| Ethereum | `ethereum` | `1` |
| Base | `base` | `8453` |
| BSC | `bsc` | `56` |
| Arbitrum | `arbitrum` | `42161` |

**Address format note**: EVM addresses (`0x...`) work across Ethereum/BSC/Polygon/Arbitrum/Base etc. Solana addresses (Base58) and Bitcoin addresses (UTXO) have different formats. Do NOT mix formats across chain types.

## Command Index

| # | Command | Description |
|---|---|---|
| 1 | `onchainos portfolio chains` | Get supported chains for balance queries |
| 2 | `onchainos portfolio total-value --address ... --chains ...` | Get total asset value for a wallet |
| 3 | `onchainos portfolio all-balances --address ... --chains ...` | Get all token balances for a wallet |
| 4 | `onchainos portfolio token-balances --address ... --tokens ...` | Get specific token balances |
| 5 | `onchainos portfolio overview --address ... --chain ... --time-frame ...` | Get wallet PnL summary, win rate, trading stats |
| 6 | `onchainos portfolio dex-history --address ... --chain ...` | Get DEX transaction history with pagination |
| 7 | `onchainos portfolio recent-pnl --address ... --chain ...` | Get recent token PnL list with pagination |
| 8 | `onchainos portfolio token-pnl --address ... --chain ... --token ...` | Get latest PnL snapshot for a specific token |

## Cross-Skill Workflows

This skill is often used **before swap** (to verify sufficient balance) or **as portfolio entry point**.

### Workflow A: Pre-Swap Balance Check

> User: "Swap 1 SOL for BONK"

```
1. okx-dex-token    onchainos token search BONK --chains solana               → get tokenContractAddress
       ↓ tokenContractAddress
2. okx-wallet-portfolio  onchainos portfolio all-balances --address <addr> --chains solana
       → verify SOL balance >= 1
       ↓ balance field (UI units) → convert to minimal units for swap
3. okx-dex-swap     onchainos swap quote --from 11111111111111111111111111111111 --to <BONK_address> --amount 1000000000 --chain solana
4. okx-dex-swap     onchainos swap swap --from ... --to <BONK_address> --amount 1000000000 --chain solana --wallet <addr>
```

**Data handoff**:
- `tokenContractAddress` from token search → feeds into swap `--from` / `--to`
- `balance` from portfolio is **UI units**; swap needs **minimal units** → multiply by `10^decimal`
- If balance < required amount → inform user, do NOT proceed to swap

### Workflow B: Portfolio Overview + Analysis

> User: "Show my portfolio"

```
1. okx-wallet-portfolio  onchainos portfolio total-value --address <addr> --chains "xlayer,solana,ethereum"
       → total USD value
2. okx-wallet-portfolio  onchainos portfolio all-balances --address <addr> --chains "xlayer,solana,ethereum"
       → per-token breakdown
       ↓ top holdings by USD value
3. okx-dex-token    onchainos token price-info <address> --chain <chain>  → enrich with 24h change, market cap
4. okx-dex-market   onchainos market kline <address> --chain <chain>      → price charts for tokens of interest
```

### Workflow C: Sell Underperforming Tokens

```
1. okx-wallet-portfolio  onchainos portfolio all-balances --address <addr> --chains "xlayer,solana,ethereum"
       → list all holdings
       ↓ tokenContractAddress + chainIndex for each
2. okx-dex-token    onchainos token price-info <address> --chain <chain>  → get priceChange24H per token
3. Filter by negative change → user confirms which to sell
4. okx-dex-swap     onchainos swap quote → onchainos swap swap → execute sell
```

**Key conversion**: `balance` (UI units) × `10^decimal` = `amount` (minimal units) for swap.

### Workflow D: PnL Performance Review

> User: "How has my Ethereum wallet performed this month?"

```
1. okx-wallet-portfolio  onchainos portfolio overview --address <addr> --chain ethereum --time-frame 1m
       → totalPnlUsd, winRate, buyTxCount, sellTxCount, preferredMarketCap
2. okx-wallet-portfolio  onchainos portfolio recent-pnl --address <addr> --chain ethereum --limit 20
       → pnlList: per-token realizedPnl, unrealizedPnl, totalPnl
       ↓ pick a top-performing or interesting token
3. okx-wallet-portfolio  onchainos portfolio token-pnl --address <addr> --chain ethereum --token <addr>
       → buyAvgPrice, sellAvgPrice, tokenBalance, lastActiveTimestamp
4. okx-dex-market   onchainos market kline <address> --chain ethereum  → price chart for context
```

**Data handoff**: `tokenContractAddress` from `recent-pnl` → `--token` in `token-pnl` and `market kline`.

### Workflow E: Paperhands Analysis

> User: "Did I sell too early? What are my sold tokens worth now?"

```
1. okx-wallet-portfolio  onchainos portfolio dex-history --address <addr> --chain ethereum --tx-type 2
       → list of all sell transactions: tokenContractAddress, price (sell price), amount, value (USD at sell time)
       ↓ tokenContractAddress + amount for each sold token
2. okx-dex-market   onchainos market prices "<chainIndex>:<addr>,..." → current price for each sold token
       ↓ compare current price vs sell price
3. Compute: currentValue = currentPrice × soldAmount
   leftOnTable = currentValue - soldValue
   - leftOnTable > 0 → paperhands: token went up after you sold (missed gains)
   - leftOnTable < 0 → smart exit: token went down after you sold (dodged losses)
```

**Data handoff**: `tokenContractAddress` + `amount` from `dex-history` sells → batch `market prices` → compute delta.

**Interpretation**:
- Token up after sell → user sold too early ("paperhands")
- Token down after sell → user timed exit well ("smart money")

### Workflow F: Audit Trading History

> User: "Show me all my buys and sells on Ethereum"

```
1. okx-wallet-portfolio  onchainos portfolio dex-history --address <addr> --chain ethereum --tx-type 1,2
       → historyList of buy/sell transactions
       ↓ if more pages: use returned cursor
2. okx-wallet-portfolio  onchainos portfolio dex-history --address <addr> --chain ethereum --tx-type 1,2 --cursor <cursor>
       → next page
       ↓ filter by a specific token if needed
3. okx-wallet-portfolio  onchainos portfolio dex-history --address <addr> --chain ethereum --token <addr> --tx-type 1,2
       → all trades for that token
```

**Pagination**: pass `--cursor <value>` from the previous response's `cursor` field to get the next page. Stop when `cursor` is empty.

## Operation Flow

### Step 1: Identify Intent

- Check total assets → `onchainos portfolio total-value`
- View all token holdings → `onchainos portfolio all-balances`
- Check specific token balance → `onchainos portfolio token-balances`
- Unsure which chains are supported → `onchainos portfolio chains` first
- Get PnL summary / win rate / trading stats → `onchainos portfolio overview`
- View DEX trade + transfer history → `onchainos portfolio dex-history`
- See recent PnL by token (all tokens) → `onchainos portfolio recent-pnl`
- Get detailed PnL for one token → `onchainos portfolio token-pnl`

### Step 2: Collect Parameters

- Missing wallet address → ask user
- Missing target chains → recommend XLayer (`--chains xlayer`, low gas, fast confirmation) as the default, then ask which chain the user prefers. Common set: `"xlayer,solana,ethereum,base,bsc"`
- Need to filter risky tokens → set `--exclude-risk 0` (only works on ETH/BSC/SOL/BASE)

### Step 3: Call and Display

- Total value: display USD amount
- Token balances: show token name, amount (UI units), USD value
- Sort by USD value descending

### Step 4: Suggest Next Steps

After displaying results, suggest 2-3 relevant follow-up actions:

| Just completed | Suggest |
|---|---|
| `portfolio total-value` | 1. View token-level breakdown → `onchainos portfolio all-balances` (this skill) 2. Check price trend for top holdings → `okx-dex-market` |
| `portfolio all-balances` | 1. View detailed analytics (market cap, 24h change) for a token → `okx-dex-token` 2. Swap a token → `okx-dex-swap` 3. View price chart for a token → `okx-dex-market` |
| `portfolio token-balances` | 1. View full portfolio across all tokens → `onchainos portfolio all-balances` (this skill) 2. Swap this token → `okx-dex-swap` |
| `portfolio overview` | 1. See per-token PnL breakdown → `onchainos portfolio recent-pnl` (this skill) 2. Audit trade history → `onchainos portfolio dex-history` (this skill) |
| `portfolio dex-history` | 1. Get PnL for a traded token → `onchainos portfolio token-pnl` (this skill) 2. View price chart for a token → `okx-dex-market` |
| `portfolio recent-pnl` | 1. Drill into a token's PnL → `onchainos portfolio token-pnl` (this skill) 2. Swap an underperforming token → `okx-dex-swap` |
| `portfolio token-pnl` | 1. View price chart → `okx-dex-market` 2. Swap this token → `okx-dex-swap` |

Present conversationally, e.g.: "Would you like to see the price chart for your top holding, or swap any of these tokens?" — never expose skill names or endpoint paths to the user.

## CLI Command Reference

### 1. onchainos portfolio chains

Get supported chains for balance queries. No parameters required.

```bash
onchainos portfolio chains
```

**Return fields**:

| Field | Type | Description |
|---|---|---|
| `name` | String | Chain name (e.g., `"XLayer"`) |
| `logoUrl` | String | Chain logo URL |
| `shortName` | String | Chain short name (e.g., `"OKB"`) |
| `chainIndex` | String | Chain unique identifier (e.g., `"196"`) |

### 2. onchainos portfolio total-value

Get total asset value for a wallet address.

```bash
onchainos portfolio total-value --address <address> --chains <chains> [--asset-type <type>] [--exclude-risk <bool>]
```

| Param | Required | Default | Description |
|---|---|---|---|
| `--address` | Yes | - | Wallet address |
| `--chains` | Yes | - | Chain names or IDs, comma-separated (e.g., `"xlayer,solana"` or `"196,501"`) |
| `--asset-type` | No | `"0"` | `0`=all, `1`=tokens only, `2`=DeFi only |
| `--exclude-risk` | No | `true` | `true`=filter risky tokens, `false`=include. Only ETH/BSC/SOL/BASE |

**Return fields**:

| Field | Type | Description |
|---|---|---|
| `totalValue` | String | Total asset value in USD |

### 3. onchainos portfolio all-balances

Get all token balances for a wallet address.

```bash
onchainos portfolio all-balances --address <address> --chains <chains> [--exclude-risk <value>]
```

| Param | Required | Default | Description |
|---|---|---|---|
| `--address` | Yes | - | Wallet address |
| `--chains` | Yes | - | Chain names or IDs, comma-separated, max 50 |
| `--exclude-risk` | No | `"0"` | `0`=filter out risky tokens (default), `1`=include. Only ETH/BSC/SOL/BASE |

**Return fields** (per token in `tokenAssets[]`):

| Field | Type | Description |
|---|---|---|
| `chainIndex` | String | Chain identifier |
| `tokenContractAddress` | String | Token contract address |
| `symbol` | String | Token symbol (e.g., `"OKB"`) |
| `balance` | String | Token balance in UI units (e.g., `"10.5"`) |
| `rawBalance` | String | Token balance in base units (e.g., `"10500000000000000000"`) |
| `tokenPrice` | String | Token price in USD |
| `isRiskToken` | Boolean | `true` if flagged as risky |

### 4. onchainos portfolio token-balances

Get specific token balances for a wallet address.

```bash
onchainos portfolio token-balances --address <address> --tokens <tokens> [--exclude-risk <value>]
```

| Param | Required | Default | Description |
|---|---|---|---|
| `--address` | Yes | - | Wallet address |
| `--tokens` | Yes | - | Token list: `"chainIndex:tokenAddress"` pairs, comma-separated. Use empty address for native token (e.g., `"196:"` for native OKB). Max 20 items. |
| `--exclude-risk` | No | `"0"` | `0`=filter out (default), `1`=include |

**Return fields**: Same schema as `all-balances` (`tokenAssets[]`).

### 5. onchainos portfolio overview

Get wallet-level PnL summary and trading behaviour metrics. *(Requires `OKX_BASE_URL=https://web3pre.okex.org`)*

```bash
onchainos portfolio overview --address <address> --chain <chain> [--time-frame <frame>]
```

| Param | Required | Default | Description |
|---|---|---|---|
| `--address` | Yes | - | Wallet address |
| `--chain` | Yes | - | Chain name or ID (e.g., `ethereum`, `solana`, `xlayer`) |
| `--time-frame` | No | `7d` | `1d`, `3d`, `7d`, `1m`, `3m` |

**Return fields**:

| Field | Type | Description |
|---|---|---|
| `realizedPnlUsd` | String | Realized PnL in USD |
| `unrealizedPnlUsd` | String | Unrealized PnL in USD |
| `totalPnlUsd` | String | Total PnL in USD |
| `totalPnlPercent` | String | Total PnL as a percentage |
| `winRate` | String | Ratio of profitable sells (e.g., `"0.65"` = 65%) |
| `buyTxCount` | String | Number of buy transactions |
| `sellTxCount` | String | Number of sell transactions |
| `preferredMarketCap` | String | Most-traded market cap bucket (`1`–`5`, small→large) |
| `topPnlTokenList[]` | Array | Top performing tokens in the period |

### 6. onchainos portfolio dex-history

Get wallet DEX transaction history with cursor pagination. *(Requires `OKX_BASE_URL=https://web3pre.okex.org`)*

```bash
onchainos portfolio dex-history --address <address> --chain <chain> [--limit <n>] [--cursor <cursor>] [--token <address>] [--tx-type <types>]
```

| Param | Required | Default | Description |
|---|---|---|---|
| `--address` | Yes | - | Wallet address |
| `--chain` | Yes | - | Chain name or ID |
| `--limit` | No | `20` | Page size (1–100) |
| `--cursor` | No | - | Pagination cursor from previous response (omit for first page) |
| `--token` | No | - | Filter by token contract address |
| `--tx-type` | No | all | Transaction type(s), comma-separated: `1`=buy, `2`=sell, `3`=transfer-in, `4`=transfer-out, `0`=all |

**Return fields**:

| Field | Type | Description |
|---|---|---|
| `cursor` | String | Next-page cursor (empty when no more pages) |
| `historyList[]` | Array | Transaction records |
| `historyList[].type` | String | Transaction type (`1`–`4`) |
| `historyList[].timestamp` | String | Transaction time (Unix ms) |
| `historyList[].tokenContractAddress` | String | Token involved |

### 7. onchainos portfolio recent-pnl

Get paginated list of recent per-token PnL records. *(Requires `OKX_BASE_URL=https://web3pre.okex.org`)*

```bash
onchainos portfolio recent-pnl --address <address> --chain <chain> [--limit <n>] [--cursor <cursor>]
```

| Param | Required | Default | Description |
|---|---|---|---|
| `--address` | Yes | - | Wallet address |
| `--chain` | Yes | - | Chain name or ID |
| `--limit` | No | `20` | Page size (1–100) |
| `--cursor` | No | - | Pagination cursor from previous response |

**Return fields**:

| Field | Type | Description |
|---|---|---|
| `cursor` | String | Next-page cursor (empty when no more pages) |
| `pnlList[]` | Array | Token PnL records |
| `pnlList[].tokenSymbol` | String | Token symbol |
| `pnlList[].tokenContractAddress` | String | Token contract address |
| `pnlList[].realizedPnl` | String | Realized PnL in USD |
| `pnlList[].unrealizedPnl` | String | Unrealized PnL in USD |
| `pnlList[].totalPnl` | String | Total PnL in USD |
| `pnlList[].buyTxCount` | String | Buy transaction count |
| `pnlList[].sellTxCount` | String | Sell transaction count |
| `pnlList[].tokenBalanceAmount` | String | Current token amount held |
| `pnlList[].lastActiveTimestamp` | String | Last activity timestamp (Unix ms) |

### 8. onchainos portfolio token-pnl

Get latest PnL snapshot for a specific token in a wallet. *(Requires `OKX_BASE_URL=https://web3pre.okex.org`)*

```bash
onchainos portfolio token-pnl --address <address> --chain <chain> --token <token>
```

| Param | Required | Default | Description |
|---|---|---|---|
| `--address` | Yes | - | Wallet address |
| `--chain` | Yes | - | Chain name or ID |
| `--token` | Yes | - | Token contract address |

**Return fields**:

| Field | Type | Description |
|---|---|---|
| `tokenSymbol` | String | Token symbol |
| `tokenContractAddress` | String | Token contract address |
| `realizedPnl` | String | Realized PnL in USD |
| `unrealizedPnl` | String | Unrealized PnL in USD |
| `totalPnl` | String | Total PnL in USD |
| `buyAvgPrice` | String | Average buy price in USD |
| `sellAvgPrice` | String | Average sell price in USD |
| `buyTxCount` | String | Buy transaction count |
| `sellTxCount` | String | Sell transaction count |
| `tokenBalance` | String | Current position value in USD |
| `tokenBalanceAmount` | String | Current token amount (`"0"` = fully closed position) |
| `lastActiveTimestamp` | String | Last activity timestamp (Unix ms) |

## Input / Output Examples

**User says:** "Check my wallet total assets on XLayer and Solana"

```bash
onchainos portfolio total-value --address 0xYourWallet --chains "xlayer,solana"
# → Display: Total assets $12,345.67
```

**User says:** "Show all tokens in my wallet"

```bash
onchainos portfolio all-balances --address 0xYourWallet --chains "xlayer,solana,ethereum"
# → Display:
#   OKB:  10.5 ($509.25)
#   USDC: 2,000 ($2,000.00)
#   USDT: 1,500 ($1,500.00)
#   ...
```

**User says:** "Only check USDC and native OKB balances on XLayer"

```bash
onchainos portfolio token-balances --address 0xYourWallet --tokens "196:,196:0x74b7f16337b8972027f6196a17a631ac6de26d22"
# → Display: OKB: 10.5 ($509.25), USDC: 2,000 ($2,000.00)
```

**User says:** "Show my PnL on Ethereum for the last month"

```bash
OKX_BASE_URL=https://web3pre.okex.org onchainos portfolio overview --address 0xYourWallet --chain ethereum --time-frame 1m
# → Display: Total PnL $+1,234.56 | Win rate: 65% | Buys: 42 | Sells: 28
```

**User says:** "What tokens did I buy on Ethereum recently?"

```bash
OKX_BASE_URL=https://web3pre.okex.org onchainos portfolio dex-history --address 0xYourWallet --chain ethereum --tx-type 1 --limit 20
# → Display: list of buy transactions with token, amount, timestamp
```

**User says:** "How much profit have I made on USDC on Ethereum?"

```bash
OKX_BASE_URL=https://web3pre.okex.org onchainos portfolio token-pnl \
  --address 0xYourWallet \
  --chain ethereum \
  --token 0xa0b86991c6218b36c1d19d4a2e9eb0ce3606eb48
# → Display: Realized PnL $+500.00 | Unrealized $+12.50 | Avg buy $1.00 | Avg sell $1.001
```

## Edge Cases

- **Zero balance**: valid state — display `$0.00`, not an error
- **Unsupported chain**: call `onchainos portfolio chains` first to confirm
- **chains exceeds 50**: split into batches, max 50 per request
- **`--exclude-risk` not working**: only supported on ETH/BSC/SOL/BASE
- **DeFi positions**: use `--asset-type 2` to query DeFi holdings separately
- **Address format mismatch**: EVM address on Solana chain will return empty data — do NOT mix
- **Commands 5–8 returning "Not Found"**: these endpoints require `OKX_BASE_URL=https://web3pre.okex.org`
- **`Invalid Authority` on pre-production**: the API key does not have access to `web3pre.okex.org` — use your own credentials
- **`tokenBalanceAmount = "0"`** in `token-pnl`: position is fully closed (sold)
- **Empty `cursor`** in `dex-history` / `recent-pnl`: no more pages — stop pagination
- **Network error**: retry once, then prompt user to try again later

## Amount Display Rules

- Token amounts in UI units (`1.5 ETH`), never base units (`1500000000000000000`)
- USD values with 2 decimal places
- Large amounts in shorthand (`$1.2M`)
- Sort by USD value descending

## Global Notes

- `--chains` supports up to **50** chain IDs (comma-separated, names or numeric)
- `--asset-type`: `0`=all `1`=tokens only `2`=DeFi only (only for `total-value`)
- `--exclude-risk` only works on ETH(`1`)/BSC(`56`)/SOL(`501`)/BASE(`8453`)
- `token-balances` supports max **20** token entries
- The CLI resolves chain names automatically (e.g., `ethereum` → `1`, `solana` → `501`)
- All output is JSON format by default; use `-o table` for table format
- The CLI handles authentication internally via environment variables — see Prerequisites step 5 for default values
