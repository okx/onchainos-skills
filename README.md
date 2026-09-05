# onchainos Skills

onchainos skills for AI coding assistants. Provides token search, market data, wallet balance queries, swap execution, transaction broadcasting, leaderboard rankings, token cluster analysis, and direct third-party DApp routing across 20+ blockchains.

## Available Skills

| Skill | Description |
|-------|-------------|
| `okx-agentic-wallet` | Wallet lifecycle (auth, balance, portfolio PnL, send, tx history, contract call), Gas Station, DEX swap, cross-chain bridge, limit-order strategy, transaction gateway (gas / simulate / broadcast / track order), public-address portfolio, security scanning (token risk, DApp phishing, tx & signature checks, approvals), audit log |
| `okx-dex-market` | Read-only on-chain DEX data: real-time prices/K-line/index/wallet PnL, address tracker activities, smart money/whale/KOL signal tracking + leaderboard rankings, token search/metadata/market cap/rankings/liquidity/hot tokens/holder & cluster analysis/top traders/trade history, crypto news/sentiment/vibe/KOL leaderboard, meme pump/trenches scanning/dev reputation/bundle detection (read-only), and WS/script real-time streaming |
| `okx-agent-payments-protocol` | Unified payment dispatcher across x402 (`exact` / `aggr_deferred` schemes — TEE or local-key sign), MPP (`charge` / `session` intents — open / voucher / topUp / close, transaction or hash mode), and a2a-pay (paymentId-based create / pay / status). Routes to per-scheme/intent references. |
| `okx-defi` | OKX-aggregated DeFi: product discovery, deposit, withdraw, claim rewards across Aave, Lido, PancakeSwap, Kamino, NAVI and more, plus positions and holdings overview across protocols and chains |
| `okx-ai` | ERC-8004 on-chain Agent identity (register/update/search/rate/service-list) + agent task marketplace (publish/accept/deliver/dispute) + live task-progress monitor |
| `okx-guide` | Onboarding & guide hub (merges former okx-how-to-play + okx-ai-guide + okx-ai-support): Onchain OS onboarding + welcome banner, OKX.AI intro & role-registration routing, customer-support / Help Center guidance |
| `okx-dapp-discovery` | Third-party DApp discovery and direct plugin routing — currently supports Polymarket, Aave V3, Hyperliquid, PancakeSwap V3 AMM, Morpho V1 Optimizer |

## Supported Chains

XLayer, Solana, Ethereum, Base, BSC, Arbitrum, Polygon, and 20+ other chains.

## Prerequisites

- Git
- Node.js (includes `npx`)

## Installation

Install the CLI, skills, and A2A runtime together:

```bash
npx -y oc-onchainos install
```

Install the beta channel:

```bash
npx -y oc-onchainos install --beta
```

## Skill Workflows

The skills work together in typical DeFi flows:

**Search and Buy**: `okx-dex-market` (find token) -> `okx-agentic-wallet` (check funds + execute trade)

**Portfolio Overview**: `okx-agentic-wallet` (holdings) -> `okx-dex-market` (enrich with analytics + price charts)

**Market Research**: `okx-dex-market` (trending/rankings + candles/history) -> `okx-agentic-wallet` (trade)

**Swap and Broadcast**: `okx-agentic-wallet` (get quote -> swap -> broadcast -> track order)

**Full Trading Flow**: `okx-dex-market` (search + price/chart) -> `okx-agentic-wallet` (check balance -> swap -> simulate + broadcast + track)

**Leaderboard → Research → Trade**: `okx-dex-market` (top traders by PnL/win rate + token analytics) -> `okx-agentic-wallet` (execute trade)

**Follow Smart Money**: `okx-dex-market` (KOL/smart money buys + token details + holder cluster + price chart) -> `okx-agentic-wallet` (trade)

## Workflows

Pre-built workflow orchestrations in `workflows/` compose multiple skills into complete operations. The agent reads `workflows/INDEX.md` to route requests, then follows the step-by-step instructions in the matched workflow file.

| Workflow | What it does | CLI command |
|----------|-------------|-------------|
| **Token Research** | Price, security, holders, cluster, smart money signals, optional launchpad deep-dive | `onchainos workflow token-research --address <addr>` |
| **Daily Brief** | Market pulse + smart money + new token activity + portfolio alerts | — |
| **Smart Money Signals** | SM signal list aggregated by token + per-token due diligence | `onchainos workflow smart-money` |
| **New Token Screening** | MIGRATED launchpad scan + safety & dev enrichment for top 10 | `onchainos workflow new-tokens` |
| **Wallet Analysis** | 7d/30d PnL, trading behaviour, recent on-chain activity | `onchainos workflow wallet-analysis --address <addr>` |
| **Portfolio Check** | Balances, total value, 30d PnL overview | `onchainos workflow portfolio --address <addr>` |
| **Wallet Monitor** | In-session polling — alert on new trades from watched wallets | — |
| **Wallet Monitor (WS)** | Background WebSocket session for offline wallet monitoring | — |

### Composite CLI commands

Single commands that replace multiple individual tool calls:

```bash
# Token report: info + price + advanced-info + security scan (parallel)
onchainos token report --address <addr> --chain solana

# Full workflow commands
onchainos workflow token-research --address <addr> [--chain solana]
onchainos workflow smart-money [--chain solana]
onchainos workflow new-tokens [--chain solana] [--stage MIGRATED]
onchainos workflow wallet-analysis --address <addr> [--chain ethereum]
onchainos workflow portfolio --address <addr> [--chains ethereum,solana]
```

## MCP Server

The `onchainos` CLI doubles as a native MCP server exposing tools to any MCP-compatible client.

### Claude Code

```bash
claude mcp add --scope user onchainos-cli onchainos mcp
```

## License

MIT
