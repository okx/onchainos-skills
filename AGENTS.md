# onchainos — Agent Instructions

This is an **onchainos skill + workflow collection** providing 14 skills and 8 pre-built workflows for on-chain operations across 20+ blockchains.

## Workflows (Primary Routing)

**For any of the following user intents, read `workflows/INDEX.md` before responding — do not call individual skills directly:**

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
| okx-x402-payment | x402 payment authorization | User encounters HTTP 402 or mentions x402 |
| okx-defi-invest | DeFi product discovery, deposit, withdraw, claim rewards | User wants to earn yield, stake, or manage DeFi positions |
| okx-defi-portfolio | DeFi positions and holdings overview | User wants to check DeFi positions across protocols |
| okx-audit-log | Audit log export and troubleshooting | User wants command history, debug info, or audit log |

## Architecture

- **skills/** — 14 onchainos CLI skill definitions (`SKILL.md` with YAML frontmatter + CLI command reference)
- **workflows/** — 8 pre-built workflow docs (`INDEX.md` for routing, W1–W8 as `*.md`)
- **cli/** — Rust CLI binary (`onchainos`), built with `clap`
- **cli/src/mcp/mod.rs** — MCP server implementation (rmcp v1.1.1)

## CLI Composite Commands

| Command | What it does |
|---------|-------------|
| `onchainos token report --address <addr>` | Token info + price + advanced-info + security scan in one parallel call |
| `onchainos workflow token-research --address <addr>` | Full W1 workflow: core data + holders + cluster + signals + optional launchpad |
| `onchainos workflow smart-money` | W3: signal list + per-token due diligence |
| `onchainos workflow new-tokens` | W4: MIGRATED token scan + safety enrichment |
| `onchainos workflow wallet-analysis --address <addr>` | W5: performance + behaviour + recent activity |
| `onchainos workflow portfolio --address <addr>` | W7: balances + total value + PnL overview |
