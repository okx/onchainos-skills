# onchainos — Agent Instructions

This is an **onchainos skill + workflow collection** providing 15 skills and pre-built workflows for on-chain operations across 20+ blockchains.

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

Safety: follow token risk controls defined in `okx-security` SKILL.md.
For script requests, append `--format json` to all CLI commands.

## Available Skills

| Skill | Purpose | When to Use |
|-------|---------|-------------|
| okx-agentic-wallet | Wallet auth, authenticated balance, send tokens, tx history, contract call | User wants to log in, check balance, send tokens, or view tx history |
| okx-wallet-portfolio | Public address balance, token holdings, portfolio value | User asks about wallet holdings or token balances for a specific address |
| okx-security | DApp/URL phishing detection, tx pre-execution scan, signature safety, approval management | User asks about DApp/URL safety, tx scan, signature safety, or token approvals |
| okx-dex-market | Prices, charts, index prices, wallet PnL | User asks for token prices, K-line data, or wallet PnL analysis |
| okx-dex-signal | Smart money / KOL / whale tracking, buy signals, leaderboard | User asks what smart money/whales/KOLs are buying or wants signal alerts |
| okx-dex-trenches | Meme/pump.fun token scanning, dev reputation, bundle detection | User asks about new meme launches, dev reputation, or bundle analysis |
| okx-dex-ws | Real-time WebSocket monitoring and scripting | User wants a WS script or real-time on-chain data stream |
| okx-dex-swap | DEX swap execution | User wants to swap, trade, buy, or sell tokens |
| okx-dex-token | Token search, metadata, rankings, liquidity, holders, top traders, cluster analysis | User searches for tokens, wants rankings, holder info, or cluster analysis |
| okx-onchain-gateway | Gas estimation, tx simulation, broadcasting | User wants to broadcast a tx, estimate gas, or check tx status |
| okx-x402-payment | Dual-protocol HTTP 402 dispatcher (x402 + MPP) | User encounters HTTP 402, mentions x402, or mentions any MPP channel/voucher/session/charge operation |
| okx-defi-invest | DeFi product discovery, deposit, withdraw, claim rewards | User wants to earn yield, stake, or manage DeFi positions |
| okx-defi-portfolio | DeFi positions and holdings overview | User wants to check DeFi positions across protocols |
| okx-audit-log | Audit log export and troubleshooting | User wants command history, debug info, or audit log |
| okx-dapp-discovery | Third-party DApp discovery + direct plugin routing | User names a specific third-party DApp/protocol (Polymarket, Aave, Hyperliquid, PancakeSwap, Morpho, …) or asks "what dapps are available" — installs the matching plugin on demand and forwards the prompt to its quickstart |

## DApp routing — `okx-dapp-discovery`

When the user names a specific third-party DApp/protocol as the destination of an action, route through `okx-dapp-discovery`. That skill applies a confidence framework to identify the matching plugin, installs it on demand via `npx skills add okx/plugin-store --skill <plugin-name> --yes --global`, then reads the installed plugin's `SKILL.md` and forwards the user's original request to it.

Onchainos-skills intentionally does **not** enumerate which DApps are supported in this file or in `CLAUDE.md`. The supported set lives in `okx-dapp-discovery/SKILL.md` (currently Polymarket, Aave V3, Hyperliquid, PancakeSwap V3 AMM, Morpho V1 Optimizer) and the per-DApp behavior lives in each installed plugin's own `SKILL.md`.

**Quick tiebreaker vs `okx-defi-invest`**: if removing the DApp/protocol name from the request still leaves a coherent generic-yield question ("deposit USDC for yield", "find best APY"), prefer `okx-defi-invest` (OKX-aggregated DeFi). If the DApp name carries the intent ("place a bet on Polymarket", "use Hyperliquid for perps"), route via `okx-dapp-discovery`.

## Architecture

- **skills/** — 15 onchainos CLI skill definitions (`SKILL.md` with YAML frontmatter + CLI command reference)
- **workflows/** — Pre-built workflow docs (`INDEX.md` for routing, `TEMPLATE.md` for authoring guide)
- **cli/** — Rust CLI binary (`onchainos`), built with `clap`
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
