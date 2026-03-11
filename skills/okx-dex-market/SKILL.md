---
name: okx-dex-market
description: "Use this skill for on-chain market data: token prices/价格, K-line/OHLC charts, and wallet PnL/盈亏分析 (win rate, my DEX trade history, realized/unrealized PnL per token). Covers smart-money/whale/KOL/大户 signal/信号 tracking — monitoring what notable wallets are buying/selling across the market. For meme/打狗 token research: scanning new pump.fun launches, developer reputation/开发者信息 and past launch history, bundle/sniper detection, bonding curve status, tokens by the same dev, similar tokens, and wallets that co-invested or aped into a token. Do NOT use for token search, holder distribution of a specific token, liquidity pools, or honeypot checks — use okx-dex-token."
license: Apache-2.0
metadata:
  author: okx
  version: "1.0.3"
  homepage: "https://web3.okx.com"
---

# OKX DEX Market Data CLI

18 commands for on-chain prices, candlesticks, index prices, smart money signals, meme pump token scanning, and wallet PnL analysis.

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

- For token search / metadata / rankings / holder analysis / advanced token info / top traders → use `okx-dex-token`
- For per-token holder filtering by tag (whale, smart money, KOL, sniper) → use `okx-dex-token`
- For per-token risk analysis (holder concentration, dev rug pull count, creator info) → use `okx-dex-token`
- For swap execution → use `okx-dex-swap`
- For transaction broadcasting → use `okx-onchain-gateway`
- For wallet balances / token holdings → use `okx-wallet-portfolio`
- For wallet PnL analysis (realized/unrealized PnL, DEX history, recent PnL, per-token PnL) → use `okx-dex-market` portfolio commands
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
| 行情 | market data, price, chart | `price`, `kline` |
| 持仓分析 | holding analysis, holder distribution | `memepump-token-details` (tags fields) |
| 社媒筛选 | social filter | `memepump-tokens --has-x`, `--has-telegram`, etc. |
| 新盘 / 迁移中 / 已迁移 | NEW / MIGRATING / MIGRATED | `memepump-tokens --stage` |
| pumpfun / bonkers / bonk / believe / bags / mayhem | protocol names (launch platforms) | `memepump-tokens --protocol-id-list <id>` |
| 盈亏 / 收益 / PnL | PnL, profit and loss, realized/unrealized | `portfolio-overview`, `portfolio-recent-pnl`, `portfolio-token-pnl` |
| 已实现盈亏 | realized PnL, realized profit | `portfolio-token-pnl` (realizedPnlUsd) |
| 未实现盈亏 | unrealized PnL, paper profit, holding gain | `portfolio-token-pnl` (unrealizedPnlUsd) |
| 胜率 | win rate, success rate | `portfolio-overview` (winRate) |
| 历史交易 / 交易记录 | DEX transaction history, trade log | `portfolio-dex-history` |
| 清仓 | sold all, liquidated, sell off | `portfolio-recent-pnl` (unrealizedPnlUsd = "SELL_ALL") |
| 画像 / 钱包画像 / 持仓分析 | wallet profile, portfolio analysis | `portfolio-overview` |
| 近期收益 | recent PnL, latest earnings by token | `portfolio-recent-pnl` |

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

# Get wallet PnL overview (7D)
onchainos market portfolio-overview --address 0xd8dA6BF26964aF9D7eEd9e03E53415D37aA96045 --chain ethereum --time-frame 3

# Get wallet DEX transaction history
onchainos market portfolio-dex-history --address 0xd8dA6BF26964aF9D7eEd9e03E53415D37aA96045 --chain ethereum \
  --begin 1700000000000 --end 1710000000000

# Get recent PnL by token
onchainos market portfolio-recent-pnl --address 0xd8dA6BF26964aF9D7eEd9e03E53415D37aA96045 --chain ethereum

# Get per-token PnL snapshot
onchainos market portfolio-token-pnl --address 0xd8dA6BF26964aF9D7eEd9e03E53415D37aA96045 --chain ethereum \
  --token 0xa0b86991c6218b36c1d19d4a2e9eb0ce3606eb48
```

## Chain Name Support

The CLI accepts human-readable chain names (e.g., `ethereum`, `solana`, `xlayer`) or numeric chain indices (e.g., `1`, `501`, `196`).

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
| 3 | `onchainos market kline <address>` | Get K-line / candlestick data |

### Index Price Commands

| # | Command | Description |
|---|---|---|
| 4 | `onchainos market index <address>` | Get index price (aggregated from multiple sources) |

### Signal Commands

| # | Command | Description |
|---|---|---|
| 5 | `onchainos market signal-chains` | Get supported chains for market signals |
| 6 | `onchainos market signal-list <chain>` | Get latest signal list (smart money / KOL / whale activity) |

### Meme Pump Commands

| # | Command | Description |
|---|---|---|
| 7 | `onchainos market memepump-chains` | Get supported chains and protocols for meme pump |
| 8 | `onchainos market memepump-tokens <chain>` | List meme pump tokens with advanced filtering |
| 9 | `onchainos market memepump-token-details <address>` | Get detailed info for a single meme pump token |
| 10 | `onchainos market memepump-token-dev-info <address>` | Get developer analysis and holding info |
| 11 | `onchainos market memepump-similar-tokens <address>` | Find similar tokens by same creator |
| 12 | `onchainos market memepump-token-bundle-info <address>` | Get bundle/sniper analysis |
| 13 | `onchainos market memepump-aped-wallet <address>` | Get aped (same-car) wallet list |

### Portfolio PnL Commands

| # | Command | Description |
|---|---|---|
| 14 | `onchainos market portfolio-supported-chains` | Get chains supported by portfolio PnL endpoints |
| 15 | `onchainos market portfolio-overview` | Get wallet PnL overview (realized/unrealized PnL, win rate, Top 3 tokens) |
| 16 | `onchainos market portfolio-dex-history` | Get DEX transaction history for a wallet (paginated, up to 1000 records) |
| 17 | `onchainos market portfolio-recent-pnl` | Get recent PnL list by token for a wallet (paginated, up to 1000 records) |
| 18 | `onchainos market portfolio-token-pnl` | Get latest PnL snapshot for a specific token in a wallet |

## Boundary: market vs token skill

| Need | Use this skill (`okx-dex-market`) | Use `okx-dex-token` instead |
|---|---|---|
| Real-time price (single value) | `onchainos market price` | - |
| Price + market cap + liquidity + 24h change | - | `onchainos token price-info` |
| K-line / candlestick chart | `onchainos market kline` | - |
| Trade history with tag/wallet filter | - | `onchainos token trades` |
| Index price (multi-source aggregate) | `onchainos market index` | - |
| Token search by name/symbol | - | `onchainos token search` |
| Token metadata (decimals, logo) | - | `onchainos token info` |
| Token ranking (trending) | - | `onchainos token trending` |
| Holder distribution | - | `onchainos token holders` |
| Holders filtered by tag (KOL, whale, smart money) | - | `onchainos token holders --tag-filter` |
| Top 5 liquidity pools for a token | - | `onchainos token liquidity` |
| Hot tokens by trending score or X mentions | - | `onchainos token hot-tokens` |
| Advanced token info (risk, creator, dev stats) | - | `onchainos token advanced-info` |
| Top traders / profit addresses | - | `onchainos token top-trader` |
| Smart money / whale / KOL signals | `onchainos market signal-list` | - |
| Signal-supported chains | `onchainos market signal-chains` | - |
| Browse meme pump tokens by stage | `onchainos market memepump-tokens` | - |
| Meme token audit (top10, dev, insiders) | `onchainos market memepump-token-details` | - |
| Developer reputation / rug pull history | `onchainos market memepump-token-dev-info` | - |
| Similar tokens by same creator | `onchainos market memepump-similar-tokens` | - |
| Bundle/sniper detection | `onchainos market memepump-token-bundle-info` | - |
| Aped (same-car) wallet analysis | `onchainos market memepump-aped-wallet` | - |
| Wallet PnL overview (win rate, realized PnL, top tokens) | `onchainos market portfolio-overview` | - |
| Wallet DEX transaction history | `onchainos market portfolio-dex-history` | - |
| Recent PnL list by token | `onchainos market portfolio-recent-pnl` | - |
| Per-token latest PnL (realized/unrealized) | `onchainos market portfolio-token-pnl` | - |
| PnL-supported chain list | `onchainos market portfolio-supported-chains` | - |

**Rule of thumb**: `okx-dex-market` = raw price feeds, charts, smart money signals, meme pump scanning (dev reputation, rug pull checks, bundler analysis), and wallet PnL analysis. `okx-dex-token` = token discovery & enriched analytics (search, trending, holders, holder filtering, hot tokens, liquidity pools, market cap, advanced info, top traders, token risk).

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

### Workflow F: Wallet PnL Analysis

> User: "How is my wallet performing on Ethereum? Show me my PnL"

```
1. okx-dex-market   onchainos market portfolio-supported-chains                        → verify Ethereum is supported
2. okx-dex-market   onchainos market portfolio-overview --address <wallet> --chain ethereum --time-frame 3
                                                                                       → 7D PnL overview: realized PnL, win rate, top 3 tokens
       ↓ user wants to drill into a specific token
3. okx-dex-market   onchainos market portfolio-recent-pnl --address <wallet> --chain ethereum
                                                                                       → list recent PnL by token
       ↓ user picks a token
4. okx-dex-market   onchainos market portfolio-token-pnl --address <wallet> --chain ethereum --token <address>
                                                                                       → latest realized/unrealized PnL for that token
5. okx-dex-token    onchainos token price-info <address> --chain ethereum              → current market context
```

**Data handoff**: `--address` (wallet) is reused across all portfolio steps; `--token` from step 3 feeds into step 4.

### Workflow G: Wallet Trade History Review

> User: "Show me my recent DEX trades on Ethereum"

```
1. okx-dex-market   onchainos market portfolio-dex-history --address <wallet> --chain ethereum
                    --begin <start_ms> --end <end_ms>
                                                                                       → paginated DEX tx list (buy/sell/transfer)
       ↓ filter by specific token
2. okx-dex-market   onchainos market portfolio-dex-history --address <wallet> --chain ethereum
                    --begin <start_ms> --end <end_ms> --token <address> --tx-type 1,2
                                                                                       → buy+sell history for one token
       ↓ check PnL for that token
3. okx-dex-market   onchainos market portfolio-token-pnl --address <wallet> --chain ethereum --token <address>
                                                                                       → realized/unrealized PnL snapshot
```

**Note**: `--begin` and `--end` are required Unix millisecond timestamps. For "last 30 days", compute: `end = now * 1000`, `begin = (now - 2592000) * 1000`.

> User: "Filter signals to only show whale buys above $10k"

```
1. okx-dex-market   onchainos market signal-list ethereum --wallet-type 3 --min-amount-usd 10000
                                                                          → whale-only signals on Ethereum, min $10k
2. okx-dex-market   onchainos market kline <address> --chain ethereum      → chart for chosen token
```

## Operation Flow

### Step 1: Identify Intent

- Real-time price (single token) → `onchainos market price`
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
- Wallet PnL overview (win rate, realized PnL, top 3 tokens) → `onchainos market portfolio-overview`
- Wallet DEX transaction history → `onchainos market portfolio-dex-history`
- Recent token PnL list for a wallet → `onchainos market portfolio-recent-pnl`
- Per-token latest PnL (realized/unrealized) → `onchainos market portfolio-token-pnl`
- Chains supported for PnL → `onchainos market portfolio-supported-chains`

### Step 2: Collect Parameters

- Missing chain → recommend XLayer (`--chain xlayer`, low gas, fast confirmation) as the default, then ask which chain the user prefers; for signal queries, first call `onchainos market signal-chains` to confirm the chain is supported; for meme pump queries, default to Solana (`--chain solana`); for portfolio PnL queries, first call `onchainos market portfolio-supported-chains` to confirm the chain is supported
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
| `market kline` | 1. Check filtered trade history → `onchainos token trades` (okx-dex-token) 2. Buy/swap based on the chart → `okx-dex-swap` |
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
| `market portfolio-supported-chains` | 1. Get PnL overview → `onchainos market portfolio-overview` (this skill) |
| `market portfolio-overview` | 1. Drill into trade history → `onchainos market portfolio-dex-history` (this skill) 2. Check recent PnL by token → `onchainos market portfolio-recent-pnl` (this skill) 3. Buy/sell a top-PnL token → `okx-dex-swap` |
| `market portfolio-dex-history` | 1. Check PnL for a specific traded token → `onchainos market portfolio-token-pnl` (this skill) 2. View token price chart → `onchainos market kline` (this skill) |
| `market portfolio-recent-pnl` | 1. Get detailed PnL for a specific token → `onchainos market portfolio-token-pnl` (this skill) 2. View token analytics → `okx-dex-token` |
| `market portfolio-token-pnl` | 1. View full trade history for this token → `onchainos market portfolio-dex-history` (this skill) 2. View token price chart → `onchainos market kline` (this skill) |

Present conversationally, e.g.: "Would you like to see the K-line chart, or buy this token?" — never expose skill names or endpoint paths to the user.

## Additional Resources

For detailed parameter tables, return field schemas, and usage examples for all 18 commands, consult:
- **`references/cli-reference.md`** — Full CLI command reference with params, return fields, and examples

To search for specific command details: `grep -n "onchainos market <command>" references/cli-reference.md`

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
- **Unsupported chain for portfolio PnL**: not all chains support PnL — always verify with `onchainos market portfolio-supported-chains` first
- **`portfolio-dex-history` requires `--begin` and `--end`**: both timestamps (Unix milliseconds) are mandatory; if the user says "last 30 days" compute them before calling
- **`portfolio-recent-pnl` `unrealizedPnlUsd` returns `SELL_ALL`**: this means the address has sold all its holdings of that token
- **`portfolio-token-pnl` `isPnlSupported = false`**: PnL calculation is not supported for this token/chain combination
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
