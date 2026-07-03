# onchainos — Agent Instructions

This is an **onchainos skill + workflow collection** providing 21 skills and pre-built workflows for on-chain operations across 20+ blockchains.

## Workflows (Primary Routing)

**For any of the following user intents, read `workflows/INDEX.md` before responding:**

| Intent | Trigger examples |
|--------|-----------------|
| Token research | "analyse token", "research [address]", "is this token safe" |
| Market overview | "daily brief", "market overview", "what's the market doing" |
| Smart money | "what are whales buying", "copy trading signals", "smart money" |
| New token scan | "scan new tokens", "pump.fun tokens", "meme scan" |
| Wallet analysis | "analyse wallet", "check this address", "is this wallet worth following" |
| Portfolio | "check my holdings", "my portfolio", "my wallet" |
| Wallet monitor | "watch wallet", "monitor address", "background monitor" |

`workflows/INDEX.md` maps each intent to the correct workflow file.
For Chinese queries, read `workflows/references/keyword-glossary.md` first.

Safety: follow token risk controls defined in `okx-agentic-wallet` SKILL.md.
For script requests, append `--format json` to all CLI commands.

## Available Skills

| Skill | Purpose | When to Use |
|-------|---------|-------------|
| okx-agentic-wallet | Wallet lifecycle (auth, balance, portfolio PnL, send, history, contract call), Gas Station, DEX swap, cross-chain bridge, limit-order strategy, transaction gateway (gas / simulate / broadcast / track), public-address portfolio, security scanning, audit log | User wants to operate their wallet or execute on-chain: log in, check balance/PnL, send tokens, call contracts, swap/trade/buy/sell, bridge across chains, place limit orders, broadcast/simulate/track a tx, look up a public address's holdings, run a token/DApp/tx/signature safety check, or export the audit log |
| okx-dex-market | Prices, charts, index prices, wallet PnL | User asks for token prices, K-line data, index/aggregate prices, wallet PnL analysis |
| okx-dex-signal | Smart money / KOL / whale tracking, buy signals, leaderboard | User asks what smart money/whales/KOLs are buying, wants signal alerts (信号), top traders (牛人榜) |
| okx-dex-trenches | Meme/pump.fun token scanning, trenches | User asks about new meme launches, dev reputation, bundle detection, 打狗/扫链/新盘, or mentions trench/trenches |
| okx-dex-ws | WebSocket scripting for all DEX channels | User wants to write a WebSocket script/脚本/bot for real-time on-chain data |
| okx-dex-token | Token search, metadata, rankings, liquidity, hot tokens, advanced info, holders, top traders, trade history, holder cluster analysis | User searches for tokens, wants rankings, liquidity pools, holder info, top traders, filtered trade history, or holder cluster concentration |
| okx-dex-social | Crypto news, sentiment, KOL / vibe analytics | User asks for news, market sentiment, top KOLs discussing a token, or token vibe score |
| okx-agent-payments-protocol | Unified payment dispatcher: x402 (`exact` / `aggr_deferred`), MPP (`charge` / `session`), and a2a-pay (paymentId). | User encounters HTTP 402, mentions x402 / MPP channel/voucher/session/charge, or a paymentId / `a2a_...` link / payment status |
| okx-defi-invest | DeFi product discovery, deposit, withdraw, claim rewards | User wants to earn yield, stake, provide liquidity, deposit/withdraw from DeFi protocols |
| okx-defi-portfolio | DeFi positions and holdings overview | User wants to check DeFi positions, view DeFi portfolio across protocols and chains |
| okx-agent-identity | ERC-8004 on-chain Agent identity: register / update / search / rate / service-list on XLayer | User wants to register/create/update/deactivate/activate/search agents, submit or view feedback, or list agent services |
| okx-guide | Onboarding & guide hub (merges former okx-how-to-play + okx-ai-guide + okx-ai-support): Onchain OS onboarding + welcome banner, OKX.AI intro & role-registration routing, customer-support / Help Center guidance — routes via its `## Intent Routing` table | First-time user ("what is onchainos", "how do I use/play this", "getting started", "I just installed"); OKX.AI questions (是什么/能做什么/怎么用/怎么开始, "OKX.AI 快速开始", spelling variants); or customer service / talk to a human / complaint / feedback / help center / FAQ |
| okx-agent-chat | Agent-to-agent communication: XMTP plugin management, encrypted file attachments | Agent needs to communicate with another agent, upload/download file attachments, install/update XMTP plugin |
| okx-agent-task | Agent task marketplace: publish, accept, deliver, dispute, AI-evaluate jobs | User wants to publish a task / accept a job / deliver work / confirm or reject completion / open a dispute |
| okx-dapp-discovery | Third-party DApp discovery + direct plugin routing | User names a specific third-party DApp/protocol (Polymarket, Aave, Hyperliquid, PancakeSwap, Morpho, …) or asks "what dapps are available" — installs the matching plugin on demand and forwards the prompt to its quickstart |
| okx-growth-competition | Agentic Wallet exclusive trading competitions: list, join, rank, claim rewards | User asks about trading competitions, wants to join/register for a competition, check leaderboard ranking, or claim competition rewards |

## DApp routing — `okx-dapp-discovery`

When the user names a specific third-party DApp/protocol as the destination of an action, route through `okx-dapp-discovery`. That skill applies a confidence framework to identify the matching plugin, installs it on demand via `npx skills add okx/plugin-store --skill <plugin-name> --yes --global`, then reads the installed plugin's `SKILL.md` and forwards the user's original request to it.

Onchainos-skills intentionally does **not** enumerate which DApps are supported in this file or in `CLAUDE.md`. The supported set lives in `okx-dapp-discovery/SKILL.md` (currently Polymarket, Aave V3, Hyperliquid, PancakeSwap V3 AMM, Morpho V1 Optimizer) and the per-DApp behavior lives in each installed plugin's own `SKILL.md`.

**Quick tiebreaker vs `okx-defi-invest`**: if removing the DApp/protocol name from the request still leaves a coherent generic-yield question ("deposit USDC for yield", "find best APY"), prefer `okx-defi-invest` (OKX-aggregated DeFi). If the DApp name carries the intent ("place a bet on Polymarket", "use Hyperliquid for perps"), route via `okx-dapp-discovery`.

## Architecture

- **skills/** — 21 onchainos CLI skill definitions (each is a `SKILL.md` with YAML frontmatter + CLI command reference)
- **workflows/** — Pre-built workflow docs (`INDEX.md` for routing, `TEMPLATE.md` for authoring guide)
- **cli/** — Rust CLI binary (`onchainos`), built with `clap`; source in `cli/src/`, config in `cli/Cargo.toml`
- **cli/src/mcp/mod.rs** — MCP server implementation (rmcp v1.1.1)

## CLI Composite Commands

| Command | What it does |
|---------|-------------|
| `onchainos token report --address <addr>` | Token info + price + advanced-info + security scan in one parallel call |
| `onchainos workflow token-research --address <addr>` | Full token research: core data + holders + cluster + signals + optional launchpad |
| `onchainos workflow smart-money` | Smart money signals: signal list + per-token due diligence |
| `onchainos workflow new-tokens` | New token screening: MIGRATED token scan + safety enrichment |
| `onchainos workflow wallet-analysis --address <addr>` | Wallet analysis: performance + behaviour + recent activity |
| `onchainos workflow portfolio --address <addr>` | Portfolio check: balances + total value + PnL overview |
