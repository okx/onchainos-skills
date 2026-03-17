# onchainos Skills

onchainos skills for AI coding assistants. Provides token search, market data, wallet balance queries, swap execution, and transaction broadcasting across 20+ blockchains.

## Available Skills

| Skill | Description |
|-------|-------------|
| `okx-wallet-portfolio` | Wallet balance, token holdings, portfolio value |
| `okx-dex-market` | Real-time prices, K-line charts, index prices, wallet PnL analysis |
| `okx-dex-signal` | Smart money / whale / KOL signal tracking |
| `okx-dex-trenches` | Meme pump/trenches token scanning, dev reputation, bundle detection, aped wallets |
| `okx-dex-swap` | Token swap via DEX aggregation (500+ liquidity sources) |
| `okx-dex-token` | Token search, metadata, market cap, rankings, liquidity pools, hot tokens, advanced info, holder analysis, top traders, trade history |
| `okx-onchain-gateway` | Gas estimation, transaction simulation, broadcasting, order tracking |

## Supported Chains

XLayer, Solana, Ethereum, Base, BSC, Arbitrum, Polygon, and 20+ other chains.

## Prerequisites

All skills require OKX API credentials. Apply at [OKX Developer Portal](https://web3.okx.com/onchain-os/dev-portal).

Recommended: create a `.env` file in your project root:

```bash
OKX_API_KEY="your-api-key"
OKX_SECRET_KEY="your-secret-key"
OKX_PASSPHRASE="your-passphrase"
```

**Security warning**: Never commit `.env` to git (add it to `.gitignore`) and never expose credentials in logs, screenshots, or chat messages.

## Installation

### Recommended

```bash
npx skills add okx/onchainos-skills
```

Works with Claude Code, Cursor, Codex CLI, and OpenCode. Auto-detects your environment and installs accordingly.

### Claude Code

```bash
# Run in Claude Code
/plugin marketplace add okx/onchainos-skills
/plugin install onchainos-skills
```

### Codex CLI

Tell Codex:

```plain
Fetch and follow instructions from https://raw.githubusercontent.com/okx/onchainos-skills/refs/heads/main/.codex/INSTALL.md
```

### OpenCode

Tell OpenCode:

```plain
Fetch and follow instructions from https://raw.githubusercontent.com/okx/onchainos-skills/refs/heads/main/.opencode/INSTALL.md
```

## Skill Workflows

The skills work together in typical DeFi flows:

**Search and Buy**: `okx-dex-token` (find token) -> `okx-wallet-portfolio` (check funds) -> `okx-dex-swap` (execute trade)

**Portfolio Overview**: `okx-wallet-portfolio` (holdings) -> `okx-dex-token` (enrich with analytics) -> `okx-dex-market` (price charts)

**Market Research**: `okx-dex-token` (trending/rankings) -> `okx-dex-market` (candles/history) -> `okx-dex-swap` (trade)

**Swap and Broadcast**: `okx-dex-swap` (get tx data) -> sign locally -> `okx-onchain-gateway` (broadcast) -> `okx-onchain-gateway` (track order)

**Pre-flight Check**: `okx-onchain-gateway` (estimate gas) -> `okx-onchain-gateway` (simulate tx) -> `okx-onchain-gateway` (broadcast) -> `okx-onchain-gateway` (track order)

**Full Trading Flow**: `okx-dex-token` (search) -> `okx-dex-market` (price/chart) -> `okx-wallet-portfolio` (check balance) -> `okx-dex-swap` (get tx) -> `okx-onchain-gateway` (simulate + broadcast + track)

## Install CLI

### Shell Script (macOS / Linux)

Auto-detects your platform, downloads the latest **stable** release, verifies SHA256 checksum, and installs to `~/.local/bin`:

```bash
curl -sSL https://raw.githubusercontent.com/okx/onchainos-skills/main/install.sh | sh
```

To install the latest **beta** version (includes pre-releases):

```bash
curl -sSL https://raw.githubusercontent.com/okx/onchainos-skills/main/install.sh | sh -s -- --beta
```

> **Note:** Beta versions (e.g., `v2.0.0-beta.0`) are opt-in only. The default installer and all skill auto-updates always use the latest stable release. Running without `--beta` will never downgrade a beta installation whose base version is ahead of the latest stable.

### PowerShell (Windows)

Auto-detects your platform, downloads the latest **stable** release, verifies SHA256 checksum, and installs to `%USERPROFILE%\.local\bin`:

```powershell
irm https://raw.githubusercontent.com/okx/onchainos-skills/main/install.ps1 | iex
```

To install the latest **beta** version (includes pre-releases):

```powershell
& ([scriptblock]::Create((irm https://raw.githubusercontent.com/okx/onchainos-skills/main/install.ps1))) --beta
```

> **Note:** The same beta/stable rules apply — default installs always use the latest stable release, and `--beta` is opt-in only.

## MCP Server

The `onchainos` CLI doubles as a native MCP server exposing tools to any MCP-compatible client.

### Claude Code

```bash
claude mcp add --scope user onchainos-cli onchainos mcp
```

## API Key Security Notice & Disclaimer

**Built-in Sandbox API Keys (Default)** This integration includes built-in sandbox API keys for testing purposes only. By using these keys, you acknowledge and accept the following:

* These keys are shared and may be subject to rate limiting, quota exhaustion, or unexpected behavior at any time without prior notice.
* Any Agent execution errors, failures, financial losses, or data inaccuracies arising from the use of built-in keys are solely your responsibility.
* We expressly disclaim all liability for any direct, indirect, incidental, or consequential damages resulting from the use of built-in sandbox keys in production or quasi-production environments.
* Built-in keys are strictly intended for local testing and evaluation only. Do not use them in production environments or with real assets.

**Production Usage (Recommended)** For stable and reliable production usage, you must provide your own API credentials by setting the following environment variables:

* `OKX_API_KEY`
* `OKX_SECRET_KEY`
* `OKX_PASSPHRASE`

You are solely responsible for the security, confidentiality, and proper management of your own API keys. We shall not be liable for any unauthorized access, asset loss, or damages resulting from improper key management on your part.

## License

MIT
