---
name: okx-dex-leaderboard
description: "Use this skill for smart money leaderboard / 牛人榜 data: ranking top traders by PnL, win rate, transaction count, volume, or ROI across chains. Covers filtering by wallet type (sniper, dev, fresh, pump, smart money, influencer) and time frame. Do NOT use for real-time price feeds, K-line charts, or wallet PnL analysis — use okx-dex-market for those."
license: Apache-2.0
metadata:
  author: okx
  version: "1.0.4"
  homepage: "https://web3.okx.com"
---

# OKX DEX Leaderboard CLI

2 commands for fetching smart money leaderboard rankings across chains.

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

- For real-time token price / K-line chart / index price → use `okx-dex-market`
- For smart money buy signals (signal-list) → use `okx-dex-market`
- For wallet PnL analysis (realized/unrealized PnL, win rate for a specific wallet) → use `okx-dex-market`
- For meme pump token scanning → use `okx-dex-market`
- For token search / metadata / holders / top traders for a specific token → use `okx-dex-token`
- For swap execution → use `okx-dex-swap`
- For wallet balances / token holdings → use `okx-wallet-portfolio`
- **Leaderboard / 牛人榜 / top traders ranked across the market** → use this skill (`okx-dex-leaderboard`)

## Keyword Glossary

| Chinese | English / Platform Terms | Maps To |
|---|---|---|
| 牛人榜 | leaderboard, top traders ranking, smart money ranking | `leaderboard list` |
| 聪明钱 | smart money | `--wallet-type smartMoney` |
| KOL / 网红 | influencer, KOL | `--wallet-type influencer` |
| 狙击手 | sniper | `--wallet-type sniper` |
| 开发者 | dev, developer | `--wallet-type dev` |
| 新钱包 | fresh wallet | `--wallet-type fresh` |
| 胜率 | win rate | `--sort-by 2` |
| 已实现盈亏 / PnL | realized PnL | `--sort-by 1` |
| 交易量 | volume, tx volume | `--sort-by 4` |
| 交易笔数 | tx count | `--sort-by 3` |
| ROI / 收益率 | ROI, profit rate | `--sort-by 5` |

## Quickstart

```bash
# Get supported chains for leaderboard
onchainos leaderboard supported-chains

# Top traders on Solana by PnL over last 7D
onchainos leaderboard list --chain solana --time-frame 3 --sort-by 1

# Top smart money on Ethereum by win rate over last 30D
onchainos leaderboard list --chain ethereum --time-frame 4 --sort-by 2 --wallet-type smartMoney

# Top snipers on BSC by volume over last 1D, min 10 txs
onchainos leaderboard list --chain bsc --time-frame 1 --sort-by 4 --wallet-type sniper --min-txs 10

# Filter by PnL range
onchainos leaderboard list --chain solana --time-frame 3 --sort-by 1 \
  --min-realized-pnl-usd 10000 --max-realized-pnl-usd 1000000
```

## Chain Name Support

The CLI accepts human-readable chain names (e.g., `ethereum`, `solana`) or numeric chain indices. Only single-chain queries are supported.

| Chain | Name | chainIndex |
|---|---|---|
| Solana | `solana` | `501` |
| Ethereum | `ethereum` | `1` |
| Base | `base` | `8453` |
| BSC | `bsc` | `56` |
| Arbitrum | `arbitrum` | `42161` |

## Command Index

| # | Command | Description |
|---|---|---|
| 1 | `onchainos leaderboard supported-chains` | Get chains supported by the leaderboard |
| 2 | `onchainos leaderboard list --chain <chain> --time-frame <tf> --sort-by <sort>` | Get top trader leaderboard (max 20 per request) |

## Operation Flow

### Step 1: Identify Intent

- User asks for leaderboard / 牛人榜 / top traders ranking → `onchainos leaderboard list`
- User wants to know which chains are supported → `onchainos leaderboard supported-chains`

### Step 2: Collect Parameters

- **Missing chain**: call `onchainos leaderboard supported-chains` to confirm support, then ask which chain. Default to `solana` if the user doesn't specify.
- **Missing `--time-frame`**: ask user for time frame preference. Map "today/1D" → `1`, "3 days/3D" → `2`, "7 days/1W/7D" → `3`, "1 month/30D" → `4`, "3 months/3M" → `5`.
- **Missing `--sort-by`**: ask user what to rank by. Map "PnL/盈亏" → `1`, "win rate/胜率" → `2`, "tx count/交易笔数" → `3`, "volume/交易量" → `4`, "ROI/收益率" → `5`.
- **`--wallet-type`**: optional single-select. If user mentions a type, map using the Keyword Glossary above.

### Step 3: Call and Display

- Returns at most 20 entries per request.
- Present as a ranked table: rank, wallet address (truncated), wallet type, PnL, win rate, tx count, volume.
- Translate field names — never dump raw JSON keys to the user.

### Step 4: Suggest Next Steps

| Just called | Suggest |
|---|---|
| `leaderboard supported-chains` | 1. Fetch the leaderboard → `onchainos leaderboard list` |
| `leaderboard list` | 1. Drill into a wallet's PnL → `okx-dex-market portfolio-overview` 2. Check a wallet's holdings → `okx-wallet-portfolio` 3. View price chart for a token they hold → `okx-dex-market kline` |

Present conversationally — never expose skill names or endpoint paths to the user.

## Additional Resources

For detailed parameter tables, return field schemas, and usage examples, consult:
- **`references/cli-reference.md`** — Full CLI command reference

## Region Restrictions (IP Blocking)

Some services are geo-restricted. When a command fails with error code `50125` or `80001`:

> {service_name} is not available in your region. Please switch to a supported region and try again.

Do not expose raw error codes or internal error messages to the user.

## Edge Cases

- **Unsupported chain**: always verify with `onchainos leaderboard supported-chains` first — not all chains are supported
- **Empty list**: no traders match the filter combination — suggest relaxing `--wallet-type`, PnL range, or win rate filters
- **Max 20 results per request**: inform user if they need more
- **`--wallet-type` is single select**: only one wallet type can be passed at a time; if omitted, all types are returned
- **Network error**: retry once, then prompt user to try again later
- **Region restriction (error code 50125 or 80001)**: display a friendly message — do NOT show raw error codes

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
