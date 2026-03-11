---
name: okx-dex-memepump
description: "Use this skill for meme/打狗/alpha token research on pump.fun and similar launchpads: scanning new token launches, checking developer reputation/开发者信息 and past rug pull history, bundle/sniper detection/捆绑狙击, bonding curve status, finding similar tokens by the same dev, and wallets that co-invested (同车/aped) into a token. Use when the user asks about 'new meme coins', 'pump.fun launches', 'scan trenches/扫链', 'check dev reputation', 'bundler analysis', 'who else bought this token', '打狗', '新盘', or '开发者信息'. Do NOT use for market-wide whale/smart-money signals — use okx-dex-signal. Do NOT use for per-token holder distribution or honeypot checks — use okx-dex-token."
license: Apache-2.0
metadata:
  author: okx
  version: "1.0.0"
  homepage: "https://web3.okx.com"
---

# OKX DEX Memepump CLI

7 commands for meme token discovery, developer analysis, bundle detection, and co-investor tracking.

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

- For market-wide whale/smart-money/KOL signal alerts → use `okx-dex-signal`
- For per-token holder distribution filtered by tag (whale, sniper, KOL) → use `okx-dex-token`
- For honeypot / token safety checks → use `okx-dex-token`
- For real-time prices / K-line charts → use `okx-dex-market`
- For wallet PnL / DEX trade history → use `okx-dex-market`
- For swap execution → use `okx-dex-swap`
- For wallet balance / portfolio → use `okx-wallet-portfolio`

## Keyword Glossary

| Chinese | English / Platform Terms | Maps To |
|---|---|---|
| 扫链 | trenches, memerush, 战壕, 打狗 | `memepump-tokens` |
| 同车 | aped, same-car, co-invested | `memepump-aped-wallet` |
| 开发者信息 | dev info, developer reputation, rug check | `memepump-token-dev-info` |
| 捆绑/狙击 | bundler, sniper, bundle analysis | `memepump-token-bundle-info` |
| 持仓分析 | holding analysis (meme context) | `memepump-token-details` (tags fields) |
| 社媒筛选 | social filter | `memepump-tokens --has-x`, `--has-telegram`, etc. |
| 新盘 / 迁移中 / 已迁移 | NEW / MIGRATING / MIGRATED | `memepump-tokens --stage` |
| pumpfun / bonkers / bonk / believe / bags / mayhem | protocol names (launch platforms) | `memepump-tokens --protocol-id-list <id>` |

**Protocol names are NOT token names.** When a user mentions pumpfun, bonkers, bonk, believe, bags, mayhem, fourmeme, etc., look up their IDs via `onchainos memepump chains`, then pass to `--protocol-id-list`. Multiple protocols: comma-separate the IDs.

When presenting `memepump-token-details` or `memepump-token-dev-info` responses, translate JSON field names into human-readable language. Never dump raw field names to the user:
- `top10HoldingsPercent` → "top-10 holder concentration"
- `rugPullCount` → "rug pull count / 跑路次数"
- `bondingPercent` → "bonding curve progress"

## Quickstart

```bash
# Get supported chains and protocols for meme pump
onchainos memepump chains

# List new meme pump tokens on Solana
onchainos memepump tokens --chain solana --stage NEW

# Get meme pump token details
onchainos memepump token-details --address <address> --chain solana

# Check developer reputation for a meme token
onchainos memepump token-dev-info --address <address> --chain solana

# Get bundle/sniper analysis
onchainos memepump token-bundle-info --address <address> --chain solana

# Find similar tokens by same dev
onchainos memepump similar-tokens --address <address> --chain solana

# Get aped (same-car) wallet list
onchainos memepump aped-wallet --address <address> --chain solana
```

## Chain Name Support

Currently supports: Solana (501), BSC (56), X Layer (196), TRON (195). Always verify with `onchainos memepump chains` first.

## Command Index

| # | Command | Description |
|---|---|---|
| 1 | `onchainos memepump chains` | Get supported chains and protocols |
| 2 | `onchainos memepump tokens --chain <chain>` | List meme pump tokens with advanced filtering |
| 3 | `onchainos memepump token-details --address <address>` | Get detailed info for a single meme pump token |
| 4 | `onchainos memepump token-dev-info --address <address>` | Get developer analysis and holding info |
| 5 | `onchainos memepump similar-tokens --address <address>` | Find similar tokens by same creator |
| 6 | `onchainos memepump token-bundle-info --address <address>` | Get bundle/sniper analysis |
| 7 | `onchainos memepump aped-wallet --address <address>` | Get aped (same-car) wallet list |

## Operation Flow

### Step 1: Identify Intent

- Discover supported chains/protocols → `onchainos memepump chains`
- **Trenches / 扫链** / browse/filter meme tokens by stage → `onchainos memepump tokens`
- Deep-dive into a specific meme token → `onchainos memepump token-details`
- Check meme token developer reputation → `onchainos memepump token-dev-info`
- Find similar tokens by same creator → `onchainos memepump similar-tokens`
- Analyze bundler/sniper activity → `onchainos memepump token-bundle-info`
- View aped (same-car) wallet holdings → `onchainos memepump aped-wallet`

### Step 2: Collect Parameters

- Missing chain → default to Solana (`--chain solana`); verify support with `onchainos memepump chains` first
- Missing `--stage` for memepump-tokens → ask user which stage (NEW / MIGRATING / MIGRATED)
- User mentions a protocol name → first call `onchainos memepump chains` to get the protocol ID, then pass `--protocol-id-list <id>` to `memepump-tokens`. Do NOT use `okx-dex-token` to search for protocol names as tokens.

### Step 3: Call and Display

- Translate field names per the Keyword Glossary — never dump raw JSON keys
- For `memepump-token-dev-info`, present as a developer reputation report
- For `memepump-token-details`, present as a token safety summary highlighting red/green flags
- When listing tokens from `memepump-tokens`, never merge or deduplicate entries that share the same symbol. Different tokens can have identical symbols but different contract addresses — each is a distinct token and must be shown separately. Always include the contract address to distinguish them.

### Step 4: Suggest Next Steps

| Just called | Suggest |
|---|---|
| `memepump-chains` | 1. Browse tokens → `onchainos memepump tokens` (this skill) |
| `memepump-tokens` | 1. Pick a token for details → `onchainos memepump token-details` (this skill) 2. Check dev → `onchainos memepump token-dev-info` (this skill) |
| `memepump-token-details` | 1. Dev analysis → `onchainos memepump token-dev-info` (this skill) 2. Similar tokens → `onchainos memepump similar-tokens` (this skill) 3. Bundle check → `onchainos memepump token-bundle-info` (this skill) |
| `memepump-token-dev-info` | 1. Check bundle activity → `onchainos memepump token-bundle-info` (this skill) 2. View price chart → `okx-dex-market` (`onchainos market kline`) |
| `memepump-similar-tokens` | 1. Compare with details → `onchainos memepump token-details` (this skill) |
| `memepump-token-bundle-info` | 1. Check aped wallets → `onchainos memepump aped-wallet` (this skill) |
| `memepump-aped-wallet` | 1. View price chart → `okx-dex-market` (`onchainos market kline`) 2. Buy the token → `okx-dex-swap` |

Present conversationally — never expose skill names or endpoint paths to the user.

## Cross-Skill Workflows

### Workflow A: Meme Token Discovery & Analysis

> User: "Show me new meme tokens on Solana and check if any look safe"

```
1. okx-dex-memepump onchainos memepump chains                          → discover supported chains & protocols
2. okx-dex-memepump onchainos memepump tokens --chain solana --stage NEW       → browse new tokens
       ↓ pick an interesting token
3. okx-dex-memepump onchainos memepump token-details --address <address> --chain solana  → full token detail + audit tags
4. okx-dex-memepump onchainos memepump token-dev-info --address <address> --chain solana → check dev reputation (rug pulls, migrations)
5. okx-dex-memepump onchainos memepump token-bundle-info --address <address> --chain solana → check for bundlers/snipers
6. okx-dex-market   onchainos market kline --address <address> --chain solana           → view price chart
       ↓ user decides to buy
7. okx-dex-swap     onchainos swap quote --from ... --to <address> --amount ... --chain solana
8. okx-dex-swap     onchainos swap swap --from ... --to <address> --amount ... --chain solana --wallet <addr>
```

**Data handoff**: `tokenAddress` from step 2 is reused as `<address>` in steps 3–8.

### Workflow B: Meme Token Due Diligence

> User: "Check if this meme token is safe before I buy"

```
1. okx-dex-memepump onchainos memepump token-details --address <address> --chain solana   → basic info + audit tags
2. okx-dex-memepump onchainos memepump token-dev-info --address <address> --chain solana  → dev history + holding
3. okx-dex-memepump onchainos memepump similar-tokens --address <address> --chain solana  → other tokens by same dev
4. okx-dex-memepump onchainos memepump token-bundle-info --address <address> --chain solana → bundler analysis
5. okx-dex-memepump onchainos memepump aped-wallet --address <address> --chain solana     → who else is holding
```

## Additional Resources

For detailed parameter tables, return field schemas, and usage examples, consult:
- **`references/cli-reference.md`** — Full CLI command reference for memepump commands

## Edge Cases

- **Unsupported chain for meme pump**: only Solana (501), BSC (56), X Layer (196), TRON (195) are supported — verify with `onchainos memepump chains` first
- **Invalid stage**: must be exactly `NEW`, `MIGRATING`, or `MIGRATED`
- **Token not found in meme pump**: `memepump-token-details` returns null data if the token doesn't exist in meme pump ranking data — it may be on a standard DEX
- **No dev holding info**: `memepump-token-dev-info` returns `devHoldingInfo` as `null` if the creator address is unavailable
- **Empty similar tokens**: `memepump-similar-tokens` may return empty array if no similar tokens are found
- **Empty aped wallets**: `memepump-aped-wallet` returns empty array if no co-holders found

## Region Restrictions (IP Blocking)

When a command fails with error code `50125` or `80001`, display:

> DEX is not available in your region. Please switch to a supported region and try again.

Do not expose raw error codes or internal error messages to the user.
