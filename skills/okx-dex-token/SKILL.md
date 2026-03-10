---
name: okx-dex-token
description: "This skill should be used when the user asks to 'find a token', 'search for a token', 'look up PEPE', 'what's trending', 'top tokens', 'trending tokens on Solana', 'token rankings', 'who holds this token', 'holder distribution', 'token market cap', 'token liquidity', 'research a token', 'tell me about this token', 'token info', or mentions searching for tokens, discovering trending tokens, viewing rankings, checking holder distribution, or analyzing market cap and liquidity. Covers token search, metadata, market cap, liquidity, volume, trending rankings, and holder analysis across XLayer, Solana, Ethereum, Base, BSC, Arbitrum, Polygon, and 20+ other chains. Do NOT use when the user says only a generic word like 'tokens' without a specific token name or action. For price charts, K-line, trades, or signals use okx-dex-market. For meme token safety, dev reputation, rug pulls, or bundle/sniper detection use okx-dex-market."
license: Apache-2.0
metadata:
  author: okx
  version: "1.0.2"
  homepage: "https://web3.okx.com"
---

# OKX DEX Token Info CLI

5 commands for token search, metadata, detailed pricing, rankings, and holder distribution.

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

- For real-time prices / K-lines / trade history → use `okx-dex-market`
- For swap execution → use `okx-dex-swap`
- For transaction broadcasting → use `okx-onchain-gateway`
- For wallet balances / portfolio → use `okx-wallet-portfolio`
- For meme token safety (dev reputation, rug pull, bundlers, similar tokens by same dev) → use `okx-dex-market`
- For smart money / whale / KOL signals → use `okx-dex-market`

## Quickstart

```bash
# Search token
onchainos token search xETH --chains "ethereum,solana"

# Get detailed price info
onchainos token price-info 0xe7b000003a45145decf8a28fc755ad5ec5ea025a --chain xlayer

# What's trending on Solana by volume?
onchainos token trending --chains solana --sort-by 5 --time-frame 4

# Check holder distribution
onchainos token holders 0xeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee --chain xlayer
```

## Chain Name Support

The CLI accepts human-readable chain names (e.g., `ethereum`, `solana`, `xlayer`) and resolves them automatically.

| Chain | Name | chainIndex |
|---|---|---|
| XLayer | `xlayer` | `196` |
| Solana | `solana` | `501` |
| Ethereum | `ethereum` | `1` |
| Base | `base` | `8453` |
| BSC | `bsc` | `56` |
| Arbitrum | `arbitrum` | `42161` |

## Command Index

| # | Command | Description |
|---|---|---|
| 1 | `onchainos token search <query>` | Search for tokens by name, symbol, or address |
| 2 | `onchainos token info <address>` | Get token basic info (name, symbol, decimals, logo) |
| 3 | `onchainos token price-info <address>` | Get detailed price info (price, market cap, liquidity, volume, 24h change) |
| 4 | `onchainos token trending` | Get trending / top tokens |
| 5 | `onchainos token holders <address>` | Get token holder distribution (top 20) |

## Boundary: token vs market skill

| Need | Use this skill (`okx-dex-token`) | Use `okx-dex-market` instead |
|---|---|---|
| Search token by name/symbol | `onchainos token search` | - |
| Token metadata (decimals, logo) | `onchainos token info` | - |
| Price + market cap + liquidity + multi-timeframe change | `onchainos token price-info` | - |
| Token ranking (trending) | `onchainos token trending` | - |
| Holder distribution | `onchainos token holders` | - |
| Raw real-time price (single value) | - | `onchainos market price` |
| K-line / candlestick chart | - | `onchainos market kline` |
| Trade history (buy/sell log) | - | `onchainos market trades` |
| Index price (multi-source aggregate) | - | `onchainos market index` |
| Meme token dev reputation / rug pull | - | `onchainos market memepump-token-dev-info` |
| Bundle/sniper detection | - | `onchainos market memepump-token-bundle-info` |
| Similar tokens by same creator | - | `onchainos market memepump-similar-tokens` |

**Rule of thumb**: `okx-dex-token` = token discovery & enriched analytics (search, trending, holders, market cap). `okx-dex-market` = raw price feeds, charts, smart money signals & meme pump scanning (including dev reputation, rug pull checks, bundler analysis).

## Cross-Skill Workflows

This skill is the typical **entry point** — users often start by searching/discovering tokens, then proceed to swap.

### Workflow A: Search → Research → Buy

> User: "Find BONK token, analyze it, then buy some"

```
1. okx-dex-token    onchainos token search BONK --chains solana              → get tokenContractAddress, chain, price
       ↓ tokenContractAddress
2. okx-dex-token    onchainos token price-info <address> --chain solana      → market cap, liquidity, volume24H, priceChange24H
3. okx-dex-token    onchainos token holders <address> --chain solana         → top 20 holders distribution
4. okx-dex-market   onchainos market kline <address> --chain solana --bar 1H → hourly price chart
       ↓ user decides to buy
5. okx-dex-swap     onchainos swap quote --from ... --to <address> --amount ... --chain solana
6. okx-dex-swap     onchainos swap swap --from ... --to <address> --amount ... --chain solana --wallet <addr>
```

**Data handoff**:
- `tokenContractAddress` from step 1 → reused in all subsequent steps
- `chain` from step 1 → reused in all subsequent steps
- `decimal` from step 1 or `onchainos token info` → needed for minimal unit conversion in swap

### Workflow B: Discover Trending → Investigate → Trade

> User: "What's trending on Solana?"

```
1. okx-dex-token    onchainos token trending --chains solana --sort-by 5 --time-frame 4  → top tokens by 24h volume
       ↓ user picks a token
2. okx-dex-token    onchainos token price-info <address> --chain solana                   → detailed analytics
3. okx-dex-token    onchainos token holders <address> --chain solana                      → check if whale-dominated
4. okx-dex-market   onchainos market kline <address> --chain solana                       → K-line for visual trend
       ↓ user decides to trade
5. okx-dex-swap     onchainos swap swap --from ... --to ... --amount ... --chain solana --wallet <addr>
```

### Workflow C: Token Verification Before Swap

Before swapping an unknown token, always verify:

```
1. okx-dex-token    onchainos token search <name>                            → find token
2. Check communityRecognized:
   - true → proceed with normal caution
   - false → warn user about risk
3. okx-dex-token    onchainos token price-info <address> → check liquidity:
   - liquidity < $10K → warn about high slippage risk
   - liquidity < $1K → strongly discourage trade
4. okx-dex-swap     onchainos swap quote ... → check isHoneyPot and taxRate
5. If all checks pass → proceed to swap
```

## Operation Flow

### Step 1: Identify Intent

- Search for a token → `onchainos token search`
- Get token metadata → `onchainos token info`
- Get price + market cap + liquidity → `onchainos token price-info`
- View rankings → `onchainos token trending`
- View holder distribution → `onchainos token holders`

### Step 2: Collect Parameters

- Missing chain → recommend XLayer (`--chain xlayer`, low gas, fast confirmation) as the default, then ask which chain the user prefers
- Only have token name, no address → use `onchainos token search` first
- For search, `--chains` defaults to `"1,501"` (Ethereum + Solana)
- For trending, `--sort-by` defaults to `5` (volume), `--time-frame` defaults to `4` (24h)

### Step 3: Call and Display

- Search results: show name, symbol, chain, price, 24h change
- Indicate `communityRecognized` status for trust signaling
- Price info: show market cap, liquidity, and volume together

### Step 4: Suggest Next Steps

After displaying results, suggest 2-3 relevant follow-up actions based on the command just executed:

| Just called | Suggest |
|---|---|
| `token search` | 1. View detailed analytics (market cap, liquidity) → `onchainos token price-info` (this skill) 2. View price chart → `okx-dex-market` 3. Buy/swap this token → `okx-dex-swap` |
| `token info` | 1. View price and market data → `onchainos token price-info` (this skill) 2. Check holder distribution → `onchainos token holders` (this skill) |
| `token price-info` | 1. View K-line chart → `okx-dex-market` 2. Check holder distribution → `onchainos token holders` (this skill) 3. Buy/swap this token → `okx-dex-swap` |
| `token trending` | 1. View details for a specific token → `onchainos token price-info` (this skill) 2. View price chart → `okx-dex-market` 3. Buy a trending token → `okx-dex-swap` |
| `token holders` | 1. View price trend → `okx-dex-market` 2. Buy/swap this token → `okx-dex-swap` |

Present conversationally, e.g.: "Would you like to see the price chart or check the holder distribution?" — never expose skill names or endpoint paths to the user.

## CLI Command Reference

### 1. onchainos token search

Search for tokens by name, symbol, or contract address.

```bash
onchainos token search <query> [--chains <chains>]
```

| Param | Required | Default | Description |
|---|---|---|---|
| `<query>` | Yes | - | Keyword: token name, symbol, or contract address (positional) |
| `--chains` | No | `"1,501"` | Chain names or IDs, comma-separated (e.g., `"ethereum,solana"` or `"196,501"`) |

**Return fields**:

| Field | Type | Description |
|---|---|---|
| `tokenContractAddress` | String | Token contract address |
| `tokenSymbol` | String | Token symbol (e.g., `"ETH"`) |
| `tokenName` | String | Token full name |
| `tokenLogoUrl` | String | Token logo image URL |
| `chainIndex` | String | Chain identifier |
| `decimal` | String | Token decimals (e.g., `"18"`) |
| `price` | String | Current price in USD |
| `change` | String | 24-hour price change percentage |
| `marketCap` | String | Market capitalization in USD |
| `liquidity` | String | Liquidity in USD |
| `holders` | String | Number of token holders |
| `explorerUrl` | String | Block explorer URL for the token |
| `tagList.communityRecognized` | Boolean | `true` = listed on Top 10 CEX or community verified |

### 2. onchainos token info

Get token basic info (name, symbol, decimals, logo).

```bash
onchainos token info <address> [--chain <chain>]
```

| Param | Required | Default | Description |
|---|---|---|---|
| `<address>` | Yes | - | Token contract address (positional) |
| `--chain` | No | `ethereum` | Chain name |

**Return fields**:

| Field | Type | Description |
|---|---|---|
| `tokenName` | String | Token full name |
| `tokenSymbol` | String | Token symbol (e.g., `"ETH"`) |
| `tokenLogoUrl` | String | Token logo image URL |
| `decimal` | String | Token decimals (e.g., `"18"`) |
| `tokenContractAddress` | String | Token contract address |
| `tagList.communityRecognized` | Boolean | `true` = listed on Top 10 CEX or community verified |

### 3. onchainos token price-info

Get detailed price info including market cap, liquidity, volume, and multi-timeframe price changes.

```bash
onchainos token price-info <address> [--chain <chain>]
```

| Param | Required | Default | Description |
|---|---|---|---|
| `<address>` | Yes | - | Token contract address (positional) |
| `--chain` | No | `ethereum` | Chain name |

**Return fields**:

| Field | Type | Description |
|---|---|---|
| `price` | String | Current price in USD |
| `time` | String | Timestamp (Unix milliseconds) |
| `marketCap` | String | Market capitalization in USD |
| `liquidity` | String | Total liquidity in USD |
| `circSupply` | String | Circulating supply |
| `holders` | String | Number of token holders |
| `tradeNum` | String | 24-hour trade count |
| `priceChange5M` | String | Price change percentage — last 5 minutes |
| `priceChange1H` | String | Price change percentage — last 1 hour |
| `priceChange4H` | String | Price change percentage — last 4 hours |
| `priceChange24H` | String | Price change percentage — last 24 hours |
| `volume5M` | String | Trading volume (USD) — last 5 minutes |
| `volume1H` | String | Trading volume (USD) — last 1 hour |
| `volume4H` | String | Trading volume (USD) — last 4 hours |
| `volume24H` | String | Trading volume (USD) — last 24 hours |
| `txs5M` | String | Transaction count — last 5 minutes |
| `txs1H` | String | Transaction count — last 1 hour |
| `txs4H` | String | Transaction count — last 4 hours |
| `txs24H` | String | Transaction count — last 24 hours |
| `maxPrice` | String | 24-hour highest price |
| `minPrice` | String | 24-hour lowest price |

### 4. onchainos token trending

Get trending / top tokens by various criteria.

```bash
onchainos token trending [--chains <chains>] [--sort-by <sort>] [--time-frame <frame>]
```

| Param | Required | Default | Description |
|---|---|---|---|
| `--chains` | No | `"1,501"` | Chain names or IDs, comma-separated |
| `--sort-by` | No | `"5"` | Sort: `2`=price change, `5`=volume, `6`=market cap |
| `--time-frame` | No | `"4"` | Window: `1`=5min, `2`=1h, `3`=4h, `4`=24h |

**Return fields**:

| Field | Type | Description |
|---|---|---|
| `tokenSymbol` | String | Token symbol |
| `tokenContractAddress` | String | Token contract address |
| `tokenLogoUrl` | String | Token logo image URL |
| `chainIndex` | String | Chain identifier |
| `price` | String | Current price in USD |
| `change` | String | Price change percentage (for selected time frame) |
| `volume` | String | Trading volume in USD (for selected time frame) |
| `marketCap` | String | Market capitalization in USD |
| `liquidity` | String | Total liquidity in USD |
| `holders` | String | Number of token holders |
| `uniqueTraders` | String | Number of unique traders (for selected time frame) |
| `txsBuy` | String | Buy transaction count (for selected time frame) |
| `txsSell` | String | Sell transaction count (for selected time frame) |
| `txs` | String | Total transaction count (for selected time frame) |
| `firstTradeTime` | String | First trade timestamp (Unix milliseconds) |

### 5. onchainos token holders

Get token holder distribution (top 20).

```bash
onchainos token holders <address> [--chain <chain>]
```

| Param | Required | Default | Description |
|---|---|---|---|
| `<address>` | Yes | - | Token contract address (positional) |
| `--chain` | No | `ethereum` | Chain name |

**Return fields** (top 20 holders):

| Field | Type | Description |
|---|---|---|
| `data[].holdAmount` | String | Token amount held |
| `data[].holderWalletAddress` | String | Holder wallet address |

## Input / Output Examples

**User says:** "Search for xETH token on XLayer"

```bash
onchainos token search xETH --chains xlayer
# → Display:
#   xETH (0xe7b0...) - XLayer
#   Price: $X,XXX.XX | 24h: +X% | Market Cap: $XXM | Liquidity: $XXM
#   Community Recognized: Yes
```

**User says:** "What's trending on Solana by volume?"

```bash
onchainos token trending --chains solana --sort-by 5 --time-frame 4
# → Display top tokens sorted by 24h volume:
#   #1 SOL  - Vol: $1.2B | Change: +3.5% | MC: $80B
#   #2 BONK - Vol: $450M | Change: +12.8% | MC: $1.5B
#   ...
```

**User says:** "Who are the top holders of this token?"

```bash
onchainos token holders 0xeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee --chain xlayer
# → Display top 20 holders with amounts and addresses
```

## Edge Cases

- **Token not found**: suggest verifying the contract address (symbols can collide)
- **Same symbol on multiple chains**: show all matches with chain names
- **Unverified token**: `communityRecognized = false` — warn user about risk
- **Too many results**: name/symbol search caps at 100 — suggest using exact contract address
- **Network error**: retry once
- **Region restriction (error code 50125 or 80001)**: do NOT show the raw error code to the user. Instead, display a friendly message: `⚠️ Service is not available in your region. Please switch to a supported region and try again.`

## Amount Display Rules

- Use appropriate precision: 2 decimals for high-value, significant digits for low-value
- Market cap / liquidity in shorthand ($1.2B, $45M)
- 24h change with sign and color hint (+X% / -X%)

## Global Notes

- Use contract address as **primary identity** — symbols can collide across tokens
- `communityRecognized = true` means listed on Top 10 CEX or community verified
- The CLI resolves chain names automatically (e.g., `ethereum` → `1`, `solana` → `501`)
- EVM addresses must be **all lowercase**
- The CLI handles authentication internally via environment variables — see Prerequisites step 4 for default values
