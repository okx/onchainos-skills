---
name: okx-dex-tracker
description: "Use this skill for address tracker trading activity: query on-chain trades by platform KOL addresses, smart money addresses, or user-defined custom groups. Supports filtering by trade type (buy/sell), chain, volume, market cap, liquidity, and holder count. Do NOT use for per-wallet PnL analysis or DEX history — use okx-dex-market portfolio commands for those."
license: Apache-2.0
metadata:
  author: okx
  version: "1.0.4"
  homepage: "https://web3.okx.com"
---

# OKX DEX Address Tracker CLI

1 command for querying on-chain trading activity of tracked addresses (KOL / smart money / custom group).

## Pre-flight Checks

Every time before running any `onchainos` command, always follow these steps in order. Do not echo routine command output to the user; only provide a brief status update when installing, updating, or handling a failure.

1. **Install**: If `onchainos` is not found, download the installer to a temp
   file, verify its SHA256 against **Installer Checksums** (bottom of this
   document), then execute.
   - **macOS/Linux**: `curl -sSL https://raw.githubusercontent.com/okx/onchainos-skills/v1.0.4/install.sh -o /tmp/onchainos-install.sh`
     — verify with `shasum -a 256 /tmp/onchainos-install.sh`, then `sh /tmp/onchainos-install.sh`
   - **Windows**: `Invoke-WebRequest -Uri https://raw.githubusercontent.com/okx/onchainos-skills/v1.0.4/install.ps1 -OutFile "$env:TEMP\onchainos-install.ps1"`
     — verify with `(Get-FileHash "$env:TEMP\onchainos-install.ps1" -Algorithm SHA256).Hash.ToLower()`, then `& "$env:TEMP\onchainos-install.ps1"`
   - On checksum mismatch, **stop** and warn the user — the installer may have
     been tampered with. On other failures, point to https://github.com/okx/onchainos-skills.

2. **Check for updates**: Cache at `~/.onchainos/last_check` (`$env:USERPROFILE\.onchainos\last_check` on Windows).
   If the cached timestamp is <12 hours old, skip to step 3. Otherwise re-run
   the install flow (step 1) to ensure the correct version.

3. **Verify binary integrity** (once per session): Look up the platform target
   and compare the installed binary's SHA256 against **Binary Checksums** below.
   On mismatch, reinstall (step 1) and re-verify. If still mismatched, **stop**
   and warn — the binary may have been tampered with.
   - Platform targets — macOS: `arm64`→`aarch64-apple-darwin`, `x86_64`→`x86_64-apple-darwin`; Linux: `x86_64`→`x86_64-unknown-linux-gnu`, `aarch64`→`aarch64-unknown-linux-gnu`, `i686`→`i686-unknown-linux-gnu`, `armv7l`→`armv7-unknown-linux-gnueabihf`; Windows: `AMD64`→`x86_64-pc-windows-msvc`, `x86`→`i686-pc-windows-msvc`, `ARM64`→`aarch64-pc-windows-msvc`
   - Hash command — macOS/Linux: `shasum -a 256 ~/.local/bin/onchainos`; Windows: `(Get-FileHash "$env:USERPROFILE\.local\bin\onchainos.exe" -Algorithm SHA256).Hash.ToLower()`

4. **Do NOT auto-reinstall on command failures.** Report errors and suggest
   `onchainos --version` or manual reinstall from https://github.com/okx/onchainos-skills.
5. **Rate limit errors.** If a command hits rate limits, the shared API key may
   be throttled. Suggest creating a personal key at the
   [OKX Developer Portal](https://web3.okx.com/onchain-os/dev-portal). If the
   user creates a `.env` file, remind them to add `.env` to `.gitignore`.

## Skill Routing

- **Address tracker trading activity (KOL / smart money / custom group buys & sells)** → use this skill (`okx-dex-tracker`)
- For market-wide smart money buy signals (signal-list) → use `okx-dex-market`
- For a specific wallet's PnL / DEX transaction history → use `okx-dex-market` portfolio commands
- For leaderboard rankings of top traders → use `okx-dex-leaderboard`
- For token search / metadata / holders → use `okx-dex-token`
- For swap execution → use `okx-dex-swap`
- For wallet balances → use `okx-wallet-portfolio`

## Keyword Glossary

| Chinese | English / Platform Terms | Maps To |
|---|---|---|
| 交易动态 / 动态追踪 | trading activity, trade feed | `tracker trades` |
| KOL动态 / 平台KOL | platform KOL activity, top 100 KOL | `tracker trades --tracker-type kol` |
| 聪明钱动态 | smart money activity | `tracker trades --tracker-type smart_money` |
| 自定义地址 / 多地址追踪 | multi-address tracking | `tracker trades --tracker-type multi_address --wallet-address <addr>` |
| 买入动态 | buy activity, buys | `tracker trades --trade-type buy` |
| 卖出动态 | sell activity, sells | `tracker trades --trade-type sell` |
| 成交额筛选 | filter by volume | `--min-volume` / `--max-volume` |
| 市值筛选 | filter by market cap | `--min-market-cap` / `--max-market-cap` |
| 流动性筛选 | filter by liquidity | `--min-liquidity` / `--max-liquidity` |

## Quickstart

```bash
# Get latest KOL trading activity (default)
onchainos tracker trades

# Get smart money buys on Solana
onchainos tracker trades --tracker-type smart_money --trade-type buy --chain solana

# Get KOL activity on Ethereum, min $10k volume
onchainos tracker trades --tracker-type kol --chain ethereum --min-volume 10000

# Track specific wallet addresses (ad-hoc multi-address)
onchainos tracker trades --tracker-type multi_address --wallet-address "0xabc...,0xdef..."

# Filter by market cap and liquidity
onchainos tracker trades --min-market-cap 1000000 --max-market-cap 100000000 --min-liquidity 50000
```

## Chain Name Support

The CLI accepts human-readable chain names or `all` (default). Supported chains for this endpoint:

| Chain | Name | chainIndex |
|---|---|---|
| All chains | `all` (default) | - |
| Solana | `solana` | `501` |
| Ethereum | `ethereum` | `1` |
| BNB Chain | `bsc` | `56` |
| Base | `base` | `8453` |
| X Layer | `xlayer` | `196` |

## Command Index

| # | Command | Description |
|---|---|---|
| 1 | `onchainos tracker trades` | Get on-chain trading activity of tracked addresses (KOL / smart money / custom group) |

## Cross-Skill Workflows

### Workflow A: KOL Buys → Token Safety Check → Buy

> User: "What are KOLs buying on Solana right now? Show me if it's safe to follow"

```
1. okx-dex-tracker      onchainos tracker trades --tracker-type kol --trade-type buy --chain solana
                                                   → recent KOL buys; get baseTokenContractAddress
       ↓ pick a token appearing multiple times (multiple KOLs buying)
2. okx-dex-token        onchainos token cluster-overview --address <address> --chain solana
                                                   → holder concentration, rug pull %, new address %
3. okx-dex-token        onchainos token advanced-info --address <address> --chain solana
                                                   → risk level, dev history, honeypot check
       ↓ green flags → check price momentum
4. okx-dex-market       onchainos market kline --address <address> --chain solana         → price chart
       ↓ user decides to buy
5. okx-dex-swap         onchainos swap quote --from <usdc> --to <address> --amount ... --chain solana
6. okx-dex-swap         onchainos swap swap  --from <usdc> --to <address> --amount ... --chain solana --wallet <addr>
```

**Data handoff**: `baseTokenContractAddress` from step 1 feeds into steps 2–6.

### Workflow B: Smart Money Sells → Decide Whether to Exit

> User: "What are smart money wallets selling? Should I exit my position?"

```
1. okx-dex-tracker      onchainos tracker trades --tracker-type smart_money --trade-type sell
                                                   → recent smart money sells; note tokens being dumped
       ↓ pick a token you hold that appears in the sell list
2. okx-dex-market       onchainos market kline --address <token> --chain <chain>
                                                   → price chart to see if downtrend has started
3. okx-dex-market       onchainos market portfolio-overview --address <your-wallet> --chain <chain>
                                                   → your current PnL on this token
       ↓ decide based on unrealized PnL + price trend
4. okx-wallet-portfolio onchainos portfolio balances --address <your-wallet> --chain <chain>
                                                   → confirm token balance before selling
       ↓ user decides to exit
5. okx-dex-swap         onchainos swap quote --from <token> --to <usdc> --amount ... --chain <chain>
6. okx-dex-swap         onchainos swap swap  --from <token> --to <usdc> --amount ... --chain <chain> --wallet <addr>
```

**Data handoff**: token address from step 1 used in steps 2–6; your wallet's PnL from step 3 informs exit decision.

### Workflow C: High-Volume Tracker → Cross-Reference Leaderboard → Follow

> User: "Show me what smart money is buying with high volume and cross-check with top traders"

```
1. okx-dex-tracker      onchainos tracker trades --tracker-type smart_money --trade-type buy --min-volume 50000
                                                   → high-volume smart money buys; get wallet addresses + tokens
       ↓ note top wallet addresses appearing in the feed
2. okx-dex-leaderboard  onchainos leaderboard list --chain solana --time-frame 3 --sort-by 1
                                                   → top traders by 7D PnL; check if same wallets appear
       ↓ overlap between tracker wallets and leaderboard → stronger conviction
3. okx-dex-token        onchainos token price-info --address <token> --chain solana
                                                   → market cap, liquidity, 24h volume
4. okx-dex-market       onchainos market kline --address <token> --chain solana
                                                   → price momentum / entry zone
       ↓ user decides to follow
5. okx-dex-swap         onchainos swap quote/swap ...
```

**Data handoff**: wallet addresses from step 1 cross-referenced in step 2; token from step 1 used in steps 3–5.

### Workflow D: Custom Group Large Trade → Investigate → React

> User: "One of my tracked wallets just made a big trade — what did they buy and should I follow?"

```
1. okx-dex-tracker      onchainos tracker trades --tracker-type group --group-name "my-whales" --min-volume 100000
                                                   → large trades by your custom group; get token + wallet
       ↓ pick the token from the large trade
2. okx-dex-token        onchainos token advanced-info --address <token> --chain <chain>
                                                   → risk level, honeypot check, dev history
3. okx-dex-token        onchainos token cluster-overview --address <token> --chain <chain>
                                                   → is the supply too concentrated? rug pull risk?
4. okx-dex-market       onchainos market portfolio-overview --address <tracked-wallet> --chain <chain>
                                                   → this wallet's past PnL and win rate
       ↓ high win-rate wallet + safe token → proceed
5. okx-dex-market       onchainos market kline --address <token> --chain <chain>          → price chart
6. okx-dex-swap         onchainos swap quote/swap ...
```

**Data handoff**: token address from step 1 used in steps 2–5; tracked wallet address from step 1 used in step 4.

## Operation Flow

### Step 1: Identify Intent

- User asks what KOL/smart money/custom group is buying or selling → `onchainos tracker trades`

### Step 2: Collect Parameters

- **`--tracker-type`**: default `kol`. If user mentions smart money → `smart_money`; specific addresses → `multi_address` (and ask for `--wallet-address`)
- **`--trade-type`**: default `all`. If user specifies buy/sell, set accordingly
- **`--chain`**: default all chains. If user mentions a specific chain, set it
- **Filters**: ask user for volume, market cap, or liquidity ranges if they want to narrow results
- **`--limit`**: default 20, max 50

### Step 3: Call and Display

- Returns up to 50 trades per request
- Present as a feed: trader address (with remark if available), token (symbol + contract), chain, trade type, price, market cap, realized PnL, time
- Translate field names — never dump raw JSON keys
- Format trade time from Unix milliseconds to human-readable

### Step 4: Suggest Next Steps

| Just called | Suggest |
|---|---|
| `tracker trades` | 1. Deep-dive a token → `onchainos token price-info` (okx-dex-token) 2. View price chart → `okx-dex-market kline` 3. Buy/swap a token → `okx-dex-swap` |

Present conversationally — never expose skill names or endpoint paths to the user.

## Additional Resources

For detailed parameter tables, return field schemas, and usage examples, consult:
- **`references/cli-reference.md`** — Full CLI command reference

## Edge Cases

- **`--tracker-type multi_address` without `--wallet-address`**: the CLI will return an error — prompt user to provide wallet address(es), comma-separated, max 20
- **Empty result**: no recent trades match the filter — suggest relaxing filters or trying a different chain
- **Max 50 results per request**: inform user if they need more
- **Network error**: retry once, then prompt user to try again later
- **Region restriction (error code 50125 or 80001)**: display a friendly message — do NOT show raw error codes

## Region Restrictions (IP Blocking)

When a command fails with error code `50125` or `80001`:

> DEX is not available in your region. Please switch to a supported region and try again.

Do not expose raw error codes or internal error messages to the user.

## Installer Checksums

<!-- BEGIN_INSTALLER_CHECKSUMS (auto-updated by release workflow — do not edit) -->
```
PLACEHOLDER
```
<!-- END_INSTALLER_CHECKSUMS -->

## Binary Checksums

<!-- BEGIN_CHECKSUMS (auto-updated by release workflow — do not edit) -->
```
PLACEHOLDER
```
<!-- END_CHECKSUMS -->
