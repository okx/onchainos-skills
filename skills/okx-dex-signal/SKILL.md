---
name: okx-dex-signal
description: "Use this skill for smart-money/whale/KOL/大户 signal/信号 tracking — monitoring what notable wallets are buying across the market. Covers: real-time buy signals from smart money, KOL/influencers, and whales; filtering by wallet type, trade size, market cap, liquidity; listing supported chains for signals. Use when the user asks 'what are smart money/whales/KOLs buying', '大户在买什么', '牛人榜', 'show me whale signals', 'smart money alerts', or wants to follow notable wallet activity. Do NOT use for meme/pump.fun token scanning — use okx-dex-memepump. Do NOT use for individual token holder distribution — use okx-dex-token."
license: Apache-2.0
metadata:
  author: okx
  version: "1.0.0"
  homepage: "https://web3.okx.com"
---

# OKX DEX Signal CLI

2 commands for tracking smart money, KOL, and whale buy signals across supported chains.

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

- For meme/pump.fun token scanning (dev reputation, bundle detection, new launches) → use `okx-dex-memepump`
- For per-token holder distribution filtered by wallet tag → use `okx-dex-token`
- For token search / metadata / rankings → use `okx-dex-token`
- For real-time prices / K-line charts → use `okx-dex-market`
- For wallet PnL / DEX trade history → use `okx-dex-market`
- For swap execution → use `okx-dex-swap`
- For wallet balance / portfolio → use `okx-wallet-portfolio`

## Keyword Glossary

| Chinese | English / Platform Terms | Maps To |
|---|---|---|
| 牛人榜 | leaderboard, top traders, smart money ranking | `signal-list` (filter by `--wallet-type`) |
| 大户 / 巨鲸 | whale, big player | `signal-list --wallet-type 3` |
| 聪明钱 / 聪明资金 | smart money | `signal-list --wallet-type 1` |
| KOL / 网红 | influencer, KOL | `signal-list --wallet-type 2` |
| 信号 | signal, alert | `signal-list` |
| 在买什么 | what are they buying | `signal-list` |

## Quickstart

```bash
# Check which chains support signals
onchainos market signal-chains

# Get smart money buy signals on Solana
onchainos market signal-list --chain solana --wallet-type 1

# Get whale buy signals above $10k on Ethereum
onchainos market signal-list --chain ethereum --wallet-type 3 --min-amount-usd 10000

# Get all signal types on Base
onchainos market signal-list --chain base
```

## Command Index

| # | Command | Description |
|---|---|---|
| 1 | `onchainos market signal-chains` | Get supported chains for signals |
| 2 | `onchainos market signal-list --chain <chain>` | Get latest buy-direction signals (smart money / KOL / whale) |

## Operation Flow

### Step 1: Identify Intent

- Supported chains for signals → `onchainos market signal-chains`
- Smart money / whale / KOL buy signals → `onchainos market signal-list`

### Step 2: Collect Parameters

- Missing chain → always call `onchainos market signal-chains` first to confirm the chain is supported
- Signal filter params (`--wallet-type`, `--min-amount-usd`, etc.) → ask user for preferences if not specified; default to no filter (returns all signal types)
- `--token-address` is optional — omit to get all signals on the chain; include to filter for a specific token

### Step 3: Call and Display

- Present signals in a readable table: token symbol, wallet type, amount USD, trigger wallet count, price at signal time
- Translate `walletType` values: `SMART_MONEY` → "Smart Money", `WHALE` → "Whale", `INFLUENCER` → "KOL/Influencer"
- Show `soldRatioPercent` — lower means the wallet is still holding (bullish signal)

### Step 4: Suggest Next Steps

| Just called | Suggest |
|---|---|
| `signal-chains` | 1. Fetch signals on a supported chain → `onchainos market signal-list` (this skill) |
| `signal-list` | 1. View price chart for a signal token → `okx-dex-market` (`onchainos market kline`) 2. Deep token analytics (market cap, liquidity, holders) → `okx-dex-token` 3. Buy the token → `okx-dex-swap` |

Present conversationally — never expose skill names or endpoint paths to the user.

## Cross-Skill Workflow: Signal-Driven Token Research & Buy

> User: "Show me what smart money is buying on Solana and buy if it looks good"

```
1. okx-dex-signal   onchainos market signal-chains                         → confirm Solana supports signals
2. okx-dex-signal   onchainos market signal-list --chain solana --wallet-type "1,2,3"
                                                                          → get latest smart money / whale / KOL buy signals
                                                                          → extracts token address, price, walletType, triggerWalletCount
       ↓ user picks a token from signal list
3. okx-dex-token    onchainos token price-info --address <address> --chain solana    → enrich: market cap, liquidity, 24h volume
4. okx-dex-token    onchainos token holders --address <address> --chain solana       → check holder concentration risk
5. okx-dex-market   onchainos market kline --address <address> --chain solana        → K-line chart to confirm momentum
       ↓ user decides to buy
6. okx-dex-swap     onchainos swap quote --from ... --to <address> --amount ... --chain solana
7. okx-dex-swap     onchainos swap swap --from ... --to <address> --amount ... --chain solana --wallet <addr>
```

**Data handoff**: `token.tokenAddress` from step 2 feeds directly into steps 3–7.

## Additional Resources

For detailed parameter tables and return field schemas, consult:
- **`references/cli-reference.md`** — Full CLI command reference for signal commands

## Edge Cases

- **Unsupported chain for signals**: not all chains support signals — always verify with `onchainos market signal-chains` first
- **Empty signal list**: no signals on this chain for the given filters — suggest relaxing `--wallet-type`, `--min-amount-usd`, or `--min-address-count`, or try a different chain

## Region Restrictions (IP Blocking)

When a command fails with error code `50125` or `80001`, display:

> DEX is not available in your region. Please switch to a supported region and try again.

Do not expose raw error codes or internal error messages to the user.
