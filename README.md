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

### Project-local development setup

Run one command after cloning the checkout, or on the first development session
after starting your machine:

```bash
npm run dev:init
```

It builds the checkout's debug CLI, links project-local skills, initializes
project-local runtime directories, and restarts the A2A daemon through the
already installed global `okx-a2a` command. No adjacent A2A repository is
required. The generated `onchainos` wrapper handles `preflight` locally, so
local builds do not trigger CLI self-update or integrity preflight actions and
the production CLI path remains unchanged.

For later work, choose the command that matches the development surface:

```bash
# Skills, workflows, or skill references only
npm run dev:skills

# CLI code: rebuild only
npm run dev:cli

# CLI code: rebuild, then execute the checkout-local CLI
npm run dev:cli -- wallet status
```

Run `npm run dev:init:test` to verify the first-time initialization flow using
only a globally installed `okx-a2a` substitute and no local A2A repository.

All runtime, wrapper, build, and skill-link files generated under `.codex/`
are ignored by Git. Do not commit its runtime state, logs, or credential files.
The wrappers set `TMPDIR` to `.codex/runtime/tmp` and `ONCHAINOS_A2A_SPOOL_DIR`
to `.codex/runtime/a2a-spool`, keeping temporary A2A payloads and validated
delivery-recovery files inside the checkout-local development runtime. Outside
local development, the spool variable is optional and falls back to the OS
temporary directory.
The setup script is endpoint-neutral: when `OKX_BASE_URL` is set, it is used at
both build and runtime; when it is unset, the CLI is built without a base-URL
override and uses its built-in production endpoint. In Codex development
sessions, the agent asks once whether to use `https://beta.okex.org` or the
production endpoint, then keeps that choice for subsequent CLI builds in the
same session unless explicitly told to switch.
They default `ONCHAINOS_SKIP_CLIENT_VERSION_GATE=true` for local development;
set it to `false` when verifying the production version gate.
The project-local A2A wrapper also recreates an ignored `.codex` mirror inside
its disposable AI workspace after every local daemon start or restart, so
daemon-spawned Codex sessions use the same relative wrappers and skills.

### OpenClaw

Tell OpenClaw:

```plain
Fetch and follow instructions from https://raw.githubusercontent.com/okx/onchainos-skills/refs/heads/main/.openclaw/INSTALL.md
```

### OpenCode

Tell OpenCode:

```plain
Fetch and follow instructions from https://raw.githubusercontent.com/okx/onchainos-skills/refs/heads/main/.opencode/INSTALL.md
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
