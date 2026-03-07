# onchainos Skills

onchainos skills for AI coding assistants. Provides token search, market data, wallet balance queries, swap execution, and transaction broadcasting across 20+ blockchains.

Contributing? See [CONTRIBUTING.md](./CONTRIBUTING.md).

## Available Skills

| Skill | Description |
|-------|-------------|
| `okx-wallet-portfolio` | Wallet balance, token holdings, portfolio value |
| `okx-dex-market` | Real-time prices, K-line charts, trade history, index prices, smart money signals, meme pump scanning |
| `okx-dex-swap` | Token swap via DEX aggregation (500+ liquidity sources) |
| `okx-dex-token` | Token search, metadata, market cap, rankings, holder analysis |
| `okx-onchain-gateway` | Gas estimation, transaction simulation, broadcasting, order tracking |

## Supported Chains

XLayer, Solana, Ethereum, Base, BSC, Arbitrum, Polygon, and 20+ other chains.

## Prerequisites

All skills require OKX API credentials. Apply at [OKX Developer Portal](https://web3.okx.com/onchain-os/dev-portal).

Recommended: copy `.env.example` to `.env` in your project root and fill in your own credentials:

```bash
cp .env.example .env
```

Then update:

```bash
OKX_API_KEY="your-api-key"
OKX_SECRET_KEY="your-secret-key"
OKX_PASSPHRASE="your-passphrase"
```

**Security warning**: Never commit `.env` to git (add it to `.gitignore`) and never expose credentials in logs, screenshots, or chat messages.

### Quick Start — Try It Now

Want to try the skills right away? Use sandbox credentials from your own OKX Developer account.

```bash
OKX_API_KEY="your-sandbox-api-key"
OKX_SECRET_KEY="your-sandbox-secret-key"
OKX_PASSPHRASE="your-sandbox-passphrase"
```

If you don't have sandbox credentials yet, apply in the [OKX Developer Portal](https://web3.okx.com/onchain-os/dev-portal).

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

Auto-detects your platform, downloads the matching binary, verifies SHA256 checksum, and installs to `~/.local/bin`:

```bash
curl -sSL https://raw.githubusercontent.com/okx/onchainos-skills/main/install.sh | sh
```

## API Key Security Notice & Disclaimer

This integration does **not** ship with production credentials. For testing and production, use credentials issued to your own OKX Developer account.

* Never commit API keys to source control.
* Never expose keys in logs, screenshots, issue comments, or chat messages.
* Use sandbox credentials only for testing and evaluation.
* Use dedicated production credentials for real assets and production traffic.

**Production Usage (Recommended)** For stable and reliable production usage, you must provide your own API credentials by setting the following environment variables:

* `OKX_API_KEY`
* `OKX_SECRET_KEY`
* `OKX_PASSPHRASE`

You are solely responsible for the security, confidentiality, and proper management of your own API keys. We shall not be liable for any unauthorized access, asset loss, or damages resulting from improper key management on your part.

## License

Apache-2.0
