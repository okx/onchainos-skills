---
name: okx-dex-token
description: "This skill should be used when the user asks to 'find a token', 'search for BONK', 'look up PEPE', 'what is trending', 'top tokens on Solana', 'who holds this token', 'show whale holders', 'filter holders by smart money', 'is this token risky', 'show advanced info', 'show top traders', 'profit addresses for this token', 'show hot tokens', 'what tokens are trending on Twitter', 'show liquidity pools', 'top pools for this token', or mentions token search, discovery, trending rankings, hot token lists (trending score or X/Twitter mentions), liquidity pool analysis, holder distribution, holder filtering by tag (whale, smart money, KOL, sniper), advanced token info (risk level, creator, dev stats, holder concentration), or top trader/profit address analysis. Covers search, metadata, market cap, liquidity pools, volume, trending, hot tokens, holders, advanced info, and top traders across 20+ chains. Do NOT use for a single generic word like 'tokens' without context. For price charts, candlestick data, or trade history, use okx-dex-market. For memepump safety analysis, use okx-dex-market."

license: Apache-2.0
metadata:
  author: okx
  version: "1.0.2"
  homepage: "https://web3.okx.com"
---

# OKX DEX Token Info CLI

9 commands for token search, metadata, detailed pricing, rankings, liquidity pools, hot token lists, holder distribution, advanced token info, and top trader analysis.

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
- For meme token safety via memepump (dev reputation, rug pull history, bundlers, similar tokens by same dev) → use `okx-dex-market`
- For market-wide smart money / whale / KOL signal alerts → use `okx-dex-market`
- For per-token holder filtering by tag (whale, smart money, KOL, sniper) → use this skill (`holders --tag-filter`)
- For per-token risk analysis (dev rug pull count, holder concentration, creator info) → use this skill (`advanced-info`)


## Quickstart

```bash
# Search token
onchainos token search xETH --chains "ethereum,solana"

# Get top 5 liquidity pools for a token
onchainos token liquidity 0x1f16e03c1a5908818f47f6ee7bb16690b40d0671 --chain base

# Get hot tokens (trending by score, all chains)
onchainos token hot-tokens --ranking-type 4

# Get X-mentioned hot tokens on Solana
onchainos token hot-tokens --ranking-type 5 --chain solana

# Get detailed price info
onchainos token price-info 0xe7b000003a45145decf8a28fc755ad5ec5ea025a --chain xlayer

# What's trending on Solana by volume?
onchainos token trending --chains solana --sort-by 5 --time-frame 4

# Check holder distribution
onchainos token holders 0xeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee --chain xlayer

# Filter holders by smart money
onchainos token holders 0xeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee --chain xlayer --tag-filter 3

# Get advanced token info (risk, creator, dev stats)
onchainos token advanced-info EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v --chain solana

# Get top traders / profit addresses
onchainos token top-trader EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v --chain solana

# Top KOL traders
onchainos token top-trader EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v --chain solana --tag-filter 1
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
| 5 | `onchainos token holders <address>` | Get token holder distribution (top 20, with optional tag filter) |
| 6 | `onchainos token liquidity <address>` | Get top 5 liquidity pools for a token |
| 7 | `onchainos token hot-tokens` | Get hot token list ranked by trending score or X mentions (max 200) |
| 8 | `onchainos token advanced-info <address>` | Get advanced token info (risk level, creator, dev stats, holder concentration) |
| 9 | `onchainos token top-trader <address>` | Get top traders / profit addresses for a token |

## Boundary: token vs market skill

| Need | Use this skill (`okx-dex-token`) | Use `okx-dex-market` instead |
|---|---|---|
| Search token by name/symbol | `onchainos token search` | - |
| Token metadata (decimals, logo) | `onchainos token info` | - |
| Price + market cap + liquidity + multi-timeframe change | `onchainos token price-info` | - |
| Token ranking (trending) | `onchainos token trending` | - |
| Holder distribution | `onchainos token holders` | - |
| Holders filtered by tag (KOL, whale, smart money) | `onchainos token holders --tag-filter` | - |
| Top 5 liquidity pools for a token | `onchainos token liquidity` | - |
| Hot tokens by trending score or X mentions | `onchainos token hot-tokens` | - |
| Advanced token info (risk, creator, dev stats) | `onchainos token advanced-info` | - |
| Top traders / profit addresses | `onchainos token top-trader` | - |
| Raw real-time price (single value) | - | `onchainos market price` |
| K-line / candlestick chart | - | `onchainos market kline` |
| Trade history (buy/sell log) | - | `onchainos market trades` |
| Index price (multi-source aggregate) | - | `onchainos market index` |
| Token risk analysis (dev rug pull count, holder %) | `onchainos token advanced-info` | - |
| Meme token dev reputation / rug pull history | - | `onchainos market memepump-token-dev-info` |
| Bundle/sniper detection | - | `onchainos market memepump-token-bundle-info` |
| Similar tokens by same creator | - | `onchainos market memepump-similar-tokens` |
| Market-wide smart money / whale / KOL alerts | - | `onchainos market signal-list` |

**Rule of thumb**: `okx-dex-token` = token discovery & enriched analytics (search, trending, holders, holder filtering, market cap, advanced info, top traders, token risk). `okx-dex-market` = raw price feeds, charts, market-wide smart money signal alerts & meme pump scanning (including dev reputation, rug pull history, bundler analysis).

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
- Filter holders by tag (KOL, whale, smart money) → `onchainos token holders --tag-filter`
- View top liquidity pools → `onchainos token liquidity`
- View hot/trending tokens (by score or X mentions) → `onchainos token hot-tokens`
- Get advanced token info (risk, creator, dev stats) → `onchainos token advanced-info`
- View top traders / profit addresses → `onchainos token top-trader`

### Step 2: Collect Parameters

- Missing chain → recommend XLayer (`--chain xlayer`, low gas, fast confirmation) as the default, then ask which chain the user prefers
- Only have token name, no address → use `onchainos token search` first
- For hot-tokens, `--ranking-type` defaults to `4` (Trending); use `5` for X-mentioned rankings
- For hot-tokens without chain → defaults to all chains; specify `--chain` to narrow
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
| `token holders` | 1. View price trend → `okx-dex-market` 2. Buy/swap this token → `okx-dex-swap` 3. Check advanced info → `onchainos token advanced-info` (this skill) |
| `token liquidity` | 1. View price chart → `okx-dex-market` 2. Buy/swap this token → `okx-dex-swap` 3. Check holders → `onchainos token holders` (this skill) |
| `token hot-tokens` | 1. View details for a hot token → `onchainos token price-info` (this skill) 2. Check liquidity pools → `onchainos token liquidity` (this skill) 3. Buy a hot token → `okx-dex-swap` |
| `token advanced-info` | 1. View holders → `onchainos token holders` (this skill) 2. View top traders → `onchainos token top-trader` (this skill) 3. Buy/swap this token → `okx-dex-swap` |
| `token top-trader` | 1. View advanced info → `onchainos token advanced-info` (this skill) 2. View holder distribution → `onchainos token holders` (this skill) 3. Buy/swap this token → `okx-dex-swap` |

Present conversationally, e.g.: "Would you like to see the price chart or check the holder distribution?" — never expose skill names or endpoint paths to the user.

## Additional Resources

For detailed parameter tables, return field schemas, and usage examples for all 9 commands, consult:
- **`references/cli-reference.md`** — Full CLI command reference with params, return fields, and examples

To search for specific command details: `grep -n "onchainos token <command>" references/cli-reference.md`

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
