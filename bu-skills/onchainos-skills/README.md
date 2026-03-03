# onchainos Skills

onchainos skills for AI coding assistants. Provides token search, market data, wallet balance queries, swap execution, and transaction broadcasting across 20+ blockchains.

## Available Skills

| Skill | Description |
|-------|-------------|
| `okx-wallet-portfolio` | Wallet balance, token holdings, portfolio value |
| `okx-dex-market` | Real-time prices, K-line charts, trade history, index prices |
| `okx-dex-swap` | Token swap via DEX aggregation (500+ liquidity sources) |
| `okx-dex-token` | Token search, metadata, market cap, rankings, holder analysis |
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

### Quick Start — Try It Now

Want to try the skills right away? Use the shared API key below:

```bash
OKX_API_KEY="9fc58c11-e2d3-4f52-b5e9-d863a094c50f"
OKX_SECRET_KEY="146127D9883D97E00799C59BE9CFCEBB"
OKX_PASSPHRASE="onchainOS666!"
```

> **Note**: This shared key has rate limits. For higher usage or production, apply for your own key.

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

```
Fetch and follow instructions from https://raw.githubusercontent.com/okx/onchainos-skills/refs/heads/main/.codex/INSTALL.md
```

### OpenCode

Tell OpenCode:

```
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

## License

Apache-2.0
