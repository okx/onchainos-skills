---
name: onchainos-router
description: "Auto-loaded routing table for onchainos skills and workflows. Maps user intents to the correct skill or workflow. Do not invoke directly — this skill provides background context for intent routing."
user-invocable: false
---

# onchainos — Intent Router

This plugin provides 15 skills and pre-built workflows for on-chain operations across 20+ blockchains.

## Workflow Routing

For any of the following user intents, read `workflows/INDEX.md` before responding:

| Intent | Trigger examples | Workflow file |
|--------|-----------------|---------------|
| Token research | "analyse token", "research [address]", "is this token safe" | `workflows/token-research.md` |
| Market overview | "daily brief", "market overview", "what's the market doing" | `workflows/daily-brief.md` |
| Smart money | "what are whales buying", "copy trading signals", "smart money" | `workflows/smart-money-signals.md` |
| New token scan | "scan new tokens", "pump.fun tokens", "meme scan" | `workflows/new-token-screening.md` |
| Wallet analysis | "analyse wallet", "check this address", "is this wallet worth following" | `workflows/wallet-analysis.md` |
| Portfolio | "check my holdings", "my portfolio", "my wallet" | `workflows/portfolio-check.md` |
| Wallet monitor | "watch wallet", "monitor address" | `workflows/wallet-monitor.md` |
| Background monitor | "background monitor", "offline monitor", "WebSocket monitor" | `workflows/wallet-monitor-ws.md` |

For Chinese queries, read `workflows/references/keyword-glossary.md` first to resolve the intent.

Safety: follow token risk controls defined in `okx-security` SKILL.md.
For script requests, append `--format json` to all CLI commands.

## Skill Routing

| Skill | When to Use |
|-------|-------------|
| okx-agentic-wallet | User wants to log in, check balance, send tokens, view tx history, or call contracts |
| okx-wallet-portfolio | User asks about wallet holdings or token balances for a specific address |
| okx-security | User asks about DApp/URL safety, tx scan, signature safety, honeypot check, or token approvals |
| okx-dex-market | User asks for token prices, K-line data, index prices, or wallet PnL analysis |
| okx-dex-signal | User asks what smart money/whales/KOLs are buying or wants signal alerts |
| okx-dex-trenches | User asks about new meme launches, dev reputation, bundle analysis, or pump.fun tokens |
| okx-dex-ws | User wants a WebSocket script or real-time on-chain data stream |
| okx-dex-swap | User wants to swap, trade, buy, or sell tokens |
| okx-dex-token | User searches for tokens, wants rankings, holder info, liquidity, or cluster analysis |
| okx-dex-bridge | User wants to bridge tokens or do a cross-chain swap/transfer |
| okx-onchain-gateway | User wants to broadcast a tx, estimate gas, simulate tx, or check tx status |
| okx-x402-payment | User encounters HTTP 402 or mentions x402 payment |
| okx-defi-invest | User wants to earn yield, stake, deposit, withdraw, or claim DeFi rewards |
| okx-defi-portfolio | User wants to check DeFi positions across protocols |
| okx-audit-log | User wants command history, debug info, or audit log export |

## CLI Composite Commands

| Command | What it does |
|---------|-------------|
| `onchainos token report --address <addr>` | Token info + price + advanced-info + security scan in one parallel call |
| `onchainos workflow token-research --address <addr>` | Full token research: core data + holders + cluster + signals + optional launchpad |
| `onchainos workflow smart-money` | Smart money signals: signal list + per-token due diligence |
| `onchainos workflow new-tokens` | New token screening: MIGRATED token scan + safety enrichment |
| `onchainos workflow wallet-analysis --address <addr>` | Wallet analysis: performance + behaviour + recent activity |
| `onchainos workflow portfolio --address <addr>` | Portfolio check: balances + total value + PnL overview |
