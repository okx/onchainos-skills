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

9 commands for supported chains, wallet total value, all token balances, specific token balances, portfolio PnL overview, DEX transaction history, recent PnL list, and per-token PnL snapshot.

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

# Get supported chains for portfolio PnL endpoints
onchainos portfolio supported-chains

# Get total asset value on XLayer and Solana
onchainos portfolio total-value --address 0xYourWallet --chains "xlayer,solana"

# Get all token balances
onchainos portfolio all-balances --address 0xYourWallet --chains "xlayer,solana,ethereum"

# Check specific tokens (native OKB + USDC on XLayer)
onchainos portfolio token-balances --address 0xYourWallet --tokens "196:,196:0x74b7f16337b8972027f6196a17a631ac6de26d22"

# Portfolio PnL overview for the last 7 days
onchainos portfolio overview --address 0xYourWallet --chain ethereum --time-frame 7d

# DEX transaction history
onchainos portfolio dex-history --address 0xYourWallet --chain ethereum --limit 20

# Recent PnL by token
onchainos portfolio recent-pnl --address 0xYourWallet --chain ethereum

# Latest PnL for a specific token
onchainos portfolio token-pnl --address 0xYourWallet --chain ethereum --token 0xA0b86991c6218b36c1d19D4a2e9Eb0cE3606eB48
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
| 2 | `onchainos portfolio supported-chains` | Get supported chains for portfolio PnL endpoints |
| 3 | `onchainos portfolio total-value --address ... --chains ...` | Get total asset value for a wallet |
| 4 | `onchainos portfolio all-balances --address ... --chains ...` | Get all token balances for a wallet |
| 5 | `onchainos portfolio token-balances --address ... --tokens ...` | Get specific token balances |
| 6 | `onchainos portfolio overview --address ... --chain ... --time-frame ...` | Get wallet PnL summary, win rate, trading stats |
| 7 | `onchainos portfolio dex-history --address ... --chain ...` | Get DEX transaction history with pagination |
| 8 | `onchainos portfolio recent-pnl --address ... --chain ...` | Get recent token PnL list with pagination |
| 9 | `onchainos portfolio token-pnl --address ... --chain ... --token ...` | Get latest PnL snapshot for a specific token |

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
   - leftOnTable > 0 → paperhands: token appreciated after the sell (missed gains)
   - leftOnTable < 0 → smart exit: token depreciated after the sell (avoided losses)
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
- Unsure which chains are supported for balance queries → `onchainos portfolio chains` first
- Unsure which chains are supported for PnL endpoints → `onchainos portfolio supported-chains` first
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

## Additional Resources

For detailed parameter tables, return field schemas, and usage examples for all 9 commands, consult:
- **`references/cli-reference.md`** — Full CLI command reference with params, return fields, and examples

To search for specific command details: `grep -n "onchainos portfolio <command>" references/cli-reference.md`

## Edge Cases

- **Zero balance**: valid state — display `$0.00`, not an error
- **Unsupported chain**: call `onchainos portfolio chains` first to confirm
- **chains exceeds 50**: split into batches, max 50 per request
- **`--exclude-risk` not working**: only supported on ETH/BSC/SOL/BASE
- **DeFi positions**: use `--asset-type 2` to query DeFi holdings separately
- **Address format mismatch**: EVM address on Solana chain will return empty data — do NOT mix
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
